mod support;
use anyhow::Result;
use jtasks::{
    controller::cleanup::{self, Preview, Removal},
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use rusqlite::{Connection, params};
use std::{
    fs,
    path::{Path, PathBuf},
};
use support::{Fixture, task};

struct Setup {
    fixture: Fixture,
    workspace: Workspace,
    conn: Connection,
    guard: ControllerGuard,
}
impl Setup {
    fn new() -> Self {
        let mut fixture = Fixture::new();
        fixture.data = fixture.root.join(".git/jtasks-v2");
        fixture.import(serde_json::json!([task("T1", &[], "completed", "done")]));
        let workspace = Workspace::resolve(&fixture.root, Some(&fixture.data)).unwrap();
        let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
        // Construct the deletion target exclusively in memory. On-disk fixture
        // data is inventoried/read but is never deleted by this test suite.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(jtasks::storage::schema::SQL).unwrap();
        let mut store = Store { conn };
        jtasks::controller::graph::initialize(&store.conn).unwrap();
        jtasks::controller::recovery::initialize(&store.conn).unwrap();
        jtasks::controller::cooldowns::initialize(&store.conn).unwrap();
        jtasks::controller::activity::initialize(&store.conn).unwrap();
        jtasks::controller::clock::initialize(&store.conn).unwrap();
        jtasks::controller::due::initialize(&store.conn).unwrap();
        jtasks::controller::reviews::initialize(&store.conn).unwrap();
        jtasks::domain::autonomy::initialize(&store.conn).unwrap();
        jtasks::controller::questions::initialize(&store.conn).unwrap();
        let plan: jtasks::domain::types::Plan =
            serde_json::from_slice(&fs::read(fixture.root.join("plan.json")).unwrap()).unwrap();
        store.import(&workspace, "demo", &plan).unwrap();
        store.import(&workspace, "other", &plan).unwrap();
        store
            .conn
            .execute("UPDATE runs SET state='completed' WHERE id='demo'", [])
            .unwrap();
        store
            .conn
            .execute("UPDATE tasks SET state='completed' WHERE run_id='demo'", [])
            .unwrap();
        Self {
            fixture,
            workspace,
            // Store switches its database out of WAL when dropped, so its connection is swapped out, not moved.
            conn: std::mem::replace(&mut store.conn, Connection::open_in_memory().unwrap()),
            guard,
        }
    }
    fn preview(&self) -> Preview {
        cleanup::preview_with(&self.conn, &self.workspace, &["demo".into()], false, false).unwrap()
    }
    fn output(&self) -> PathBuf {
        let dir = self.workspace.data.join("runs/demo");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("decisions.md");
        fs::write(&file, "owned projection").unwrap();
        file
    }
    fn attempt(&self, state: &str, empty: bool) -> PathBuf {
        self.conn
            .execute(
                "INSERT INTO attempts(id,run_id,task_id,state) VALUES('a-one','demo','T1',?1)",
                [state],
            )
            .unwrap();
        let dir = self.workspace.data.join("attempts/a-one");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ticket.md"), "owned ticket").unwrap();
        fs::write(dir.join("stdout.log"), "owned output").unwrap();
        fs::write(
            dir.join("custody.json"),
            serde_json::to_vec(&jtasks::execution::custody::Record {
                platform: std::env::consts::OS.into(),
                scope: format!("jtasks-{}", uuid::Uuid::new_v4()),
                pid: Some(1234),
                start: Some("identity".into()),
                bound: true,
                empty,
            })
            .unwrap(),
        )
        .unwrap();
        self.conn.execute("INSERT INTO receipts(id,attempt_id,payload) VALUES('receipt-one','a-one','private-receipt')", []).unwrap();
        dir
    }
    fn count(&self, run: &str) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM runs WHERE id=?1", [run], |r| r.get(0))
            .unwrap()
    }
}
#[derive(Default)]
struct Recording {
    paths: Vec<PathBuf>,
    fail: bool,
}
impl Removal for Recording {
    fn remove_file(&mut self, path: &Path) -> Result<()> {
        self.paths.push(path.into());
        anyhow::ensure!(!self.fail, "injected removal failure");
        Ok(())
    }
    fn remove_directory(&mut self, path: &Path) -> Result<()> {
        self.paths.push(path.into());
        Ok(())
    }
}

#[test]
fn inventory_owns_only_selected_rows_and_output_and_keeps_shared_storage() {
    let s = Setup::new();
    let projection = s.output();
    let attempt = s.attempt("retired", true);
    fs::write(s.fixture.root.join("source.rs"), "source").unwrap();
    fs::create_dir_all(s.workspace.data.join("config")).unwrap();
    fs::write(s.workspace.data.join("config/profiles.json"), "profile").unwrap();
    let p = s.preview();
    assert!(p.runs[0].finished);
    assert!(!p.runs[0].discards_resumability);
    assert!(p.runs[0].blockers.is_empty(), "{:?}", p.runs[0].blockers);
    assert!(
        p.runs[0]
            .entries
            .iter()
            .any(|e| e.path == fs::canonicalize(&projection).unwrap())
    );
    assert!(
        p.runs[0]
            .entries
            .iter()
            .any(|e| e.path == fs::canonicalize(attempt.join("ticket.md")).unwrap())
    );
    assert!(
        !p.runs[0]
            .entries
            .iter()
            .any(|e| e.path.ends_with("profiles.json")
                || e.path.ends_with("state.sqlite3")
                || e.path.ends_with("source.rs"))
    );
    assert!(p.retained_shared.contains(&s.workspace.database()));
    assert!(
        p.retained_shared
            .iter()
            .any(|p| p.to_string_lossy().ends_with("state.sqlite3-wal"))
    );
    assert!(
        !serde_json::to_string(&p)
            .unwrap()
            .contains("private-receipt")
    );
    assert_eq!(
        p.bytes,
        p.runs[0].entries.iter().map(|e| e.bytes).sum::<u64>()
    );
}

#[test]
fn fake_removal_retires_only_selected_sql_ownership_without_deleting_any_files() {
    let mut s = Setup::new();
    let file = s.output();
    let dir = s.attempt("retired", true);
    s.conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let approval = s.preview();
    let mut recording = Recording::default();
    let receipt = cleanup::execute_with(
        &mut s.conn,
        &s.workspace,
        &s.guard,
        &approval,
        false,
        &mut recording,
    )
    .unwrap();
    assert_eq!(receipt.removed_runs, ["demo"]);
    assert_eq!(receipt.contract_version, 1);
    assert_eq!(receipt.approval_sha256.len(), 64);
    assert_eq!(receipt.removed_paths, recording.paths);
    assert_eq!(s.count("demo"), 0);
    assert_eq!(s.count("other"), 1);
    assert_eq!(
        s.conn
            .query_row("SELECT count(*) FROM receipts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        s.conn
            .query_row(
                "SELECT count(*) FROM recovery_lineages WHERE run_id='other'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert!(file.is_file() && dir.join("ticket.md").is_file());
    assert!(s.workspace.database().is_file());
}

#[test]
fn changed_bytes_or_sql_require_new_approval_before_any_removal() {
    let mut s = Setup::new();
    let file = s.output();
    let approval = s.preview();
    fs::write(&file, "edited projection").unwrap();
    let mut recording = Recording::default();
    let error = cleanup::execute_with(
        &mut s.conn,
        &s.workspace,
        &s.guard,
        &approval,
        false,
        &mut recording,
    )
    .unwrap_err();
    assert!(error.to_string().contains("changed"));
    assert!(recording.paths.is_empty());
    let approval = s.preview();
    s.conn
        .execute(
            "UPDATE journal SET detail='new decision' WHERE run_id='demo'",
            [],
        )
        .unwrap();
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &approval,
            false,
            &mut recording
        )
        .is_err()
    );
    assert!(recording.paths.is_empty());
    assert_eq!(s.count("demo"), 1);
}

#[test]
fn inactive_unfinished_history_is_never_automatically_selected_or_discarded() {
    let mut s = Setup::new();
    s.conn
        .execute("UPDATE runs SET state='paused' WHERE id='demo'", [])
        .unwrap();
    s.conn
        .execute("UPDATE tasks SET state='blocked' WHERE run_id='demo'", [])
        .unwrap();
    assert!(
        cleanup::preview_with(&s.conn, &s.workspace, &[], true, true)
            .unwrap()
            .runs
            .is_empty()
    );
    let p = s.preview();
    assert!(p.runs[0].discards_resumability);
    assert!(p.runs[0].blockers.is_empty());
    let mut recording = Recording::default();
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &p,
            false,
            &mut recording
        )
        .unwrap_err()
        .to_string()
        .contains("discard-resume")
    );
    let strict =
        cleanup::preview_with(&s.conn, &s.workspace, &["demo".into()], false, true).unwrap();
    assert!(!strict.runs[0].blockers.is_empty());
    cleanup::execute_with(
        &mut s.conn,
        &s.workspace,
        &s.guard,
        &p,
        true,
        &mut recording,
    )
    .unwrap();
    assert_eq!(s.count("demo"), 0);
}

#[test]
fn terminal_result_does_not_prove_empty_custody_and_preview_never_reconciles() {
    let mut s = Setup::new();
    let dir = s.attempt("retired", false);
    let before = fs::read(dir.join("custody.json")).unwrap();
    let p = s.preview();
    assert!(
        p.runs[0]
            .blockers
            .iter()
            .any(|s| s.contains("unresolved custody"))
    );
    assert_eq!(before, fs::read(dir.join("custody.json")).unwrap());
    let mut recording = Recording::default();
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &p,
            false,
            &mut recording
        )
        .is_err()
    );
    assert!(recording.paths.is_empty());
    s.conn
        .execute("UPDATE attempts SET state='uncertain' WHERE id='a-one'", [])
        .unwrap();
    assert!(
        s.preview().runs[0]
            .blockers
            .iter()
            .any(|s| s.contains("live/unresolved attempts"))
    );
}

#[test]
fn traversal_forged_approval_and_live_checkout_controller_are_refused() {
    let mut s = Setup::new();
    assert!(
        cleanup::preview_with(&s.conn, &s.workspace, &["../demo".into()], false, false).is_err()
    );
    s.output();
    let mut p = s.preview();
    p.runs[0].entries[0].path = s.fixture.root.join("source.rs");
    let mut recording = Recording::default();
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &p,
            false,
            &mut recording
        )
        .is_err()
    );
    assert!(recording.paths.is_empty());
    assert!(
        cleanup::execute(&s.workspace, &s.preview(), false)
            .unwrap_err()
            .to_string()
            .contains("another foreground controller")
    );
}

#[test]
fn failed_filesystem_action_retains_database_rows_for_explicit_recovery() {
    let mut s = Setup::new();
    let file = s.output();
    let p = s.preview();
    let mut recording = Recording {
        fail: true,
        ..Default::default()
    };
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &p,
            false,
            &mut recording
        )
        .unwrap_err()
        .to_string()
        .contains("injected")
    );
    assert_eq!(s.count("demo"), 1);
    assert!(file.is_file());
}

#[test]
fn cli_preview_is_read_only_and_requires_reviewed_inventory_for_execution() {
    let mut fixture = Fixture::new();
    fixture.data = fixture.root.join(".git/jtasks-v2");
    fixture.import(serde_json::json!([task("T1", &[], "completed", "done")]));
    let before = fs::read(fixture.data.join("state.sqlite3")).unwrap();
    let output = fixture.call(&["clean", "--run", "demo", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let p: Preview = serde_json::from_slice(&output.stdout).unwrap();
    assert!(p.runs[0].discards_resumability);
    assert_eq!(
        before,
        fs::read(fixture.data.join("state.sqlite3")).unwrap()
    );
    assert!(
        !fixture
            .call(&["clean", "--run", "demo", "--yes"])
            .status
            .success()
    );
    assert!(!fixture.call(&["clean"]).status.success());
    assert!(
        !fixture
            .call(&["clean", "--run", "demo", "--discard-resume"])
            .status
            .success()
    );
}

#[test]
fn advisory_ownership_blocks_cleanup_without_releasing_shared_records() {
    let s = Setup::new();
    let path = s.workspace.data.join("advisory.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE reservations(id TEXT PRIMARY KEY, record TEXT NOT NULL); CREATE TABLE receipts(id TEXT PRIMARY KEY, request TEXT NOT NULL, receipt TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    let checkout_key = jtasks::resources::identity::checkout_key(&s.workspace.checkout).unwrap();
    let reservation = serde_json::json!({"id":"reservation","resource":{"identity":{"kind":"named","scope":{"kind":"shared","identity":"fixture"},"namespace":"cpu","name":"one"},"mode":"exclusive"},"owner":{"checkout":checkout_key,"run":"demo","task":"T1","attempt":null},"owner_observation":"unknown","overridden":false});
    conn.execute(
        "INSERT INTO reservations VALUES('reservation',?1)",
        params![reservation.to_string()],
    )
    .unwrap();
    let before = fs::read(path).unwrap();
    let p = s.preview();
    assert!(
        p.runs[0]
            .blockers
            .iter()
            .any(|s| s.contains("advisory reservations"))
    );
    assert_eq!(
        before,
        fs::read(s.workspace.data.join("advisory.sqlite3")).unwrap()
    );
}

#[test]
fn linked_owned_directory_is_refused_even_if_target_stays_inside_runtime_data() {
    let s = Setup::new();
    let target = s.workspace.data.join("shared-output");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(s.workspace.data.join("runs")).unwrap();
    let link = s.workspace.data.join("runs/demo");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    {
        let native = |path: &Path| {
            path.to_string_lossy()
                .replace('/', "\\")
                .trim_start_matches("\\\\?\\")
                .to_owned()
        };
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(native(&link))
            .arg(native(&target))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(cleanup::preview_with(&s.conn, &s.workspace, &["demo".into()], false, false).is_err());
    assert!(target.is_dir());
}

#[test]
fn terminal_review_requires_positive_empty_proof_for_both_scopes() {
    let mut s = Setup::new();
    let attempt = s.attempt("retired", true);
    let record = fs::read(attempt.join("custody.json")).unwrap();
    s.conn.execute("INSERT INTO execution_reviews(attempt_id,run_id,task_id,policy,state) VALUES('a-one','demo','T1','{}','completed')", []).unwrap();
    s.conn.execute("INSERT INTO reviewer_calls(id,run_id,task_id,attempt_id,generation,state,invoked,evidence) VALUES('review-one','demo','T1','a-one',1,'completed',1,'{}')", []).unwrap();
    let dir = s.workspace.data.join("reviews/review-one");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("custody.json"), &record).unwrap();
    let p = s.preview();
    assert!(
        p.runs[0]
            .blockers
            .iter()
            .any(|s| s.contains("missing custody proof") && s.contains("snapshot"))
    );
    let mut recording = Recording::default();
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &p,
            false,
            &mut recording
        )
        .is_err()
    );
    assert!(recording.paths.is_empty());
    fs::create_dir_all(dir.join("snapshot")).unwrap();
    fs::write(dir.join("snapshot/custody.json"), &record).unwrap();
    assert!(s.preview().runs[0].blockers.is_empty());

    s.conn.execute("INSERT INTO reviewer_calls(id,run_id,task_id,attempt_id,generation,state,invoked,evidence) VALUES('review-two','demo','T1','a-one',2,'stale',0,'{}')", []).unwrap();
    let dir = s.workspace.data.join("reviews/review-two");
    fs::create_dir_all(dir.join("snapshot")).unwrap();
    fs::write(dir.join("snapshot/custody.json"), &record).unwrap();
    let p = s.preview();
    assert!(
        p.runs[0]
            .blockers
            .iter()
            .any(|s| s.contains("missing custody proof") && s.contains("review-two"))
    );
    assert!(
        cleanup::execute_with(
            &mut s.conn,
            &s.workspace,
            &s.guard,
            &p,
            false,
            &mut recording
        )
        .is_err()
    );
    assert!(recording.paths.is_empty());
    // Explicit positive no-launch evidence is accepted; absence is never proof.
    let mut no_launch: jtasks::execution::custody::Record =
        serde_json::from_slice(&record).unwrap();
    no_launch.bound = false;
    no_launch.pid = None;
    no_launch.start = None;
    fs::write(
        dir.join("custody.json"),
        serde_json::to_vec(&no_launch).unwrap(),
    )
    .unwrap();
    assert!(s.preview().runs[0].blockers.is_empty());
}
