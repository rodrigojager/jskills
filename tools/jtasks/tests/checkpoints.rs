mod support;
use serde_json::{Value, json};
use std::{fs, process::Command};
use support::{Fixture, task};

fn git(f: &Fixture, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(&f.root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}

fn setup() -> Fixture {
    let mut f = Fixture::new();
    f.data = f.root.join(".git/checkpoint-runtime");
    git(&f, &["config", "user.name", "Checkpoint Fixture"]);
    git(&f, &["config", "user.email", "checkpoint@example.invalid"]);
    git(&f, &["config", "core.autocrlf", "false"]);
    for path in ["owned.txt", "deleted.txt", "unrelated.txt"] {
        fs::write(f.root.join(path), "baseline\n").unwrap();
    }
    git(
        &f,
        &["add", "--", "owned.txt", "deleted.txt", "unrelated.txt"],
    );
    git(&f, &["commit", "--quiet", "-m", "Fixture baseline"]);
    f
}

fn op(paths: Value) -> Value {
    json!({"id":"checkpoint","stage":{"kind":"final","tasks":["A"]},"required":true,"revision":"reviewed-r1","inputs":["owned.txt","deleted.txt","new.txt"],"initial":{"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent"],"cwd":".","deadline_ms":5000},"repair_task":"A","checkpoint":{"paths":paths}})
}

fn import(f: &Fixture, operation: Value, tasks: Value) {
    let plan = f.root.join("checkpoint-plan.json");
    fs::write(&plan, serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":tasks,"operations":[operation]})).unwrap()).unwrap();
    let out = f.call(&["import", plan.to_str().unwrap(), "--run", "demo"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run(f: &Fixture) {
    let out = f.call(&["--json", "run", "--run", "demo"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn review(f: &Fixture) -> std::process::Output {
    f.call(&[
        "checkpoint",
        "review",
        "--run",
        "demo",
        "--operation",
        "checkpoint",
        "--reviewer",
        "fixture-reviewer",
        "--attribution",
        "Reviewed changes attributable to task A",
        "--message",
        "Apply explicit scoped checkpoint",
    ])
}

fn save_review(f: &Fixture) -> std::path::PathBuf {
    let out = review(f);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let path = f.root.join("checkpoint-review.json");
    fs::write(&path, out.stdout).unwrap();
    path
}

fn apply(f: &Fixture, path: &std::path::Path) -> std::process::Output {
    f.call(&["checkpoint", "apply", "--review", path.to_str().unwrap()])
}

#[test]
fn ordinary_completion_preserves_head_work_and_existing_index() {
    let f = setup();
    fs::write(f.root.join("unrelated.txt"), "already staged\n").unwrap();
    git(&f, &["add", "--", "unrelated.txt"]);
    fs::write(f.root.join("unrelated.txt"), "concurrent work\n").unwrap();
    let head = git(&f, &["rev-parse", "HEAD"]);
    let index = fs::read(f.root.join(".git/index")).unwrap();
    f.import(json!([task(
        "A",
        &[],
        "completed",
        "Complete without source-control operations"
    )]));
    run(&f);
    assert_eq!(git(&f, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(f.root.join("unrelated.txt")).unwrap(),
        "concurrent work\n"
    );
    assert!(!review(&f).status.success());
}

#[test]
fn explicit_reviewed_checkpoint_commits_only_scope_preserving_partial_staged_work() {
    let f = setup();
    fs::write(f.root.join("unrelated.txt"), "pre-existing staged\n").unwrap();
    git(&f, &["add", "--", "unrelated.txt"]);
    fs::write(f.root.join("unrelated.txt"), "pre-existing unstaged\n").unwrap();
    let staged = git(&f, &["show", ":unrelated.txt"]);
    let work = fs::read(f.root.join("unrelated.txt")).unwrap();
    let head = git(&f, &["rev-parse", "HEAD"]);
    import(
        &f,
        op(json!(["owned.txt", "new.txt"])),
        json!([task("A", &[], "completed", "Reviewable task")]),
    );
    fs::write(f.root.join("owned.txt"), "reviewed task output\n").unwrap();
    fs::write(f.root.join("new.txt"), "reviewed new file\n").unwrap();
    run(&f);
    assert_eq!(
        git(&f, &["rev-parse", "HEAD"]),
        head,
        "declared check passing must not itself commit"
    );
    assert_eq!(
        f.snapshot()["operations"][0]["checkpoint"]["application_state"],
        "awaiting_explicit_review_and_apply"
    );
    let path = save_review(&f);
    let out = apply(&f, &path);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        git(&f, &["show", "--format=", "--name-only", "HEAD"]),
        "new.txt\nowned.txt"
    );
    assert_eq!(git(&f, &["show", "HEAD:owned.txt"]), "reviewed task output");
    assert_eq!(git(&f, &["show", ":unrelated.txt"]), staged);
    assert_eq!(fs::read(f.root.join("unrelated.txt")).unwrap(), work);
    assert_eq!(
        git(&f, &["diff", "--cached", "--name-only"]),
        "unrelated.txt"
    );
    assert!(git(&f, &["diff", "--name-only", "--", "owned.txt", "new.txt"]).is_empty());
    let replay = apply(&f, &path);
    assert!(replay.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.stdout).unwrap()["replayed"],
        true
    );
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(
        f.snapshot()["operations"][0]["checkpoint"]["application_state"],
        "applied"
    );
    let db = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM journal WHERE event='checkpoint_applied'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn preexisting_or_newly_staged_scope_is_ambiguous_and_remains_untouched() {
    for staged_after_import in [false, true] {
        let f = setup();
        if !staged_after_import {
            fs::write(f.root.join("owned.txt"), "preexisting edit\n").unwrap();
        }
        import(
            &f,
            op(json!(["owned.txt"])),
            json!([task(
                "A",
                &[],
                "completed",
                "No attribution for prior edits"
            )]),
        );
        fs::write(f.root.join("owned.txt"), "current output\n").unwrap();
        if staged_after_import {
            git(&f, &["add", "--", "owned.txt"]);
        }
        run(&f);
        let head = git(&f, &["rev-parse", "HEAD"]);
        let index = fs::read(f.root.join(".git/index")).unwrap();
        let out = review(&f);
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(if staged_after_import {
                "ambiguous staged"
            } else {
                "ambiguous pre-existing"
            })
        );
        assert_eq!(git(&f, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
        assert_eq!(
            fs::read_to_string(f.root.join("owned.txt")).unwrap(),
            "current output\n"
        );
    }
}

#[test]
fn changed_reviewed_content_or_index_refuses_apply_without_staging() {
    for stage_scope in [false, true] {
        let f = setup();
        import(
            &f,
            op(json!(["owned.txt"])),
            json!([task("A", &[], "completed", "Review first")]),
        );
        fs::write(f.root.join("owned.txt"), "reviewed\n").unwrap();
        run(&f);
        let path = save_review(&f);
        if stage_scope {
            git(&f, &["add", "--", "owned.txt"]);
        } else {
            fs::write(f.root.join("owned.txt"), "changed after review\n").unwrap();
        }
        let head = git(&f, &["rev-parse", "HEAD"]);
        let index = fs::read(f.root.join(".git/index")).unwrap();
        let out = apply(&f, &path);
        assert!(!out.status.success());
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(
            error.contains("stale") || error.contains("ambiguous staged"),
            "{error}"
        );
        assert_eq!(git(&f, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    }
}

#[test]
fn checkpoint_review_waits_for_declared_check_and_retired_producer() {
    let f = setup();
    let mut producer = task("A", &[], "completed", "Result precedes process retirement");
    producer["executor"]["linger_ms"] = json!(700);
    import(&f, op(json!(["owned.txt"])), json!([producer]));
    fs::write(f.root.join("owned.txt"), "output\n").unwrap();
    let mut child = f
        .command()
        .args(["--json", "run", "--run", "demo"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    f.await_status(|value| value["tasks"][0]["state"] != "pending");
    let out = review(&f);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("waiting:"));
    assert!(child.wait().unwrap().success());
    assert!(review(&f).status.success());
}

#[test]
fn failed_relevant_checks_block_checkpoint_even_after_its_own_check_passes() {
    let f = setup();
    let mut check = op(json!(["owned.txt"]));
    check["id"] = json!("other-check");
    check.as_object_mut().unwrap().remove("checkpoint");
    check["initial"]["args"] = json!(["fixture", "--outcome", "silent", "--exit-code", "2"]);
    let path = f.root.join("checkpoint-plan.json");
    fs::write(&path, json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[task("A", &[], "completed", "Task")],"operations":[op(json!(["owned.txt"])),check]}).to_string()).unwrap();
    assert!(
        f.call(&["import", path.to_str().unwrap(), "--run", "demo"])
            .status
            .success()
    );
    fs::write(f.root.join("owned.txt"), "output\n").unwrap();
    assert!(!f.call(&["--json", "run", "--run", "demo"]).status.success());
    let out = review(&f);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("relevant check other-check is failed"));
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "1");
}

#[test]
fn declarations_reject_implicit_or_out_of_operation_scopes() {
    let f = setup();
    for paths in [
        json!(["."]),
        json!(["../owned.txt"]),
        json!(["owned.txt", "owned.txt"]),
        json!([".git/config"]),
        json!(["unrelated.txt"]),
        json!(["owned.txt/"]),
    ] {
        let plan = f.root.join("invalid-checkpoint.json");
        fs::write(&plan, json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[task("A", &[], "completed", "Task")],"operations":[op(paths)]}).to_string()).unwrap();
        let out = f.call(&["validate", plan.to_str().unwrap()]);
        assert!(!out.status.success(), "invalid scope must not validate");
    }
    assert!(!f.data.exists());
}

#[test]
fn bytes_changed_after_check_cannot_inherit_its_pass() {
    let f = setup();
    import(
        &f,
        op(json!(["owned.txt"])),
        json!([task("A", &[], "completed", "Task")]),
    );
    fs::write(f.root.join("owned.txt"), "bytes checked by operation\n").unwrap();
    run(&f);
    fs::write(f.root.join("owned.txt"), "unverified later bytes\n").unwrap();
    let out = review(&f);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("stale passed check"));
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "1");
    assert!(git(&f, &["diff", "--cached", "--name-only"]).is_empty());
}

#[test]
fn terminal_looking_rows_and_missing_or_nonempty_custody_never_authorize_checkpoint() {
    let f = setup();
    import(
        &f,
        op(json!(["owned.txt"])),
        json!([task("A", &[], "completed", "Task")]),
    );
    fs::write(f.root.join("owned.txt"), "checked bytes\n").unwrap();
    run(&f);
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    let attempt: String = conn
        .query_row("SELECT id FROM attempts", [], |r| r.get(0))
        .unwrap();
    let index = fs::read(f.root.join(".git/index")).unwrap();
    assert!(
        review(&f).status.success(),
        "real retired attempt and check proofs permit review"
    );
    conn.execute("UPDATE attempts SET state='completed'", [])
        .unwrap();
    let out = review(&f);
    assert!(String::from_utf8_lossy(&out.stderr).contains("live/unresolved attempts"));
    conn.execute("UPDATE attempts SET state='retired'", [])
        .unwrap();
    let path = f.data.join("attempts").join(&attempt).join("custody.json");
    let original = fs::read(&path).unwrap();
    let mut proof: Value = serde_json::from_slice(&original).unwrap();
    proof["empty"] = json!(false);
    fs::write(&path, proof.to_string()).unwrap();
    let out = review(&f);
    assert!(String::from_utf8_lossy(&out.stderr).contains("unresolved custody proof"));
    fs::write(&path, &original).unwrap();
    // A terminal-looking review with invoked=0 still requires both process scopes.
    conn.execute("INSERT OR IGNORE INTO execution_reviews(attempt_id,run_id,task_id,policy,state) VALUES(?1,'demo','A','{}','completed')", [&attempt]).unwrap();
    conn.execute("INSERT INTO reviewer_calls(id,run_id,task_id,attempt_id,generation,state,invoked,evidence) VALUES('fixture-review','demo','A',?1,1,'completed',0,'{}')", [&attempt]).unwrap();
    let out = review(&f);
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing custody proof"));
    let root = f.data.join("reviews/fixture-review");
    fs::create_dir_all(root.join("snapshot")).unwrap();
    fs::write(root.join("custody.json"), &original).unwrap();
    let out = review(&f);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("snapshot/custody.json")
            || String::from_utf8_lossy(&out.stderr).contains("snapshot\\custody.json")
    );
    fs::write(root.join("snapshot/custody.json"), proof.to_string()).unwrap();
    let out = review(&f);
    assert!(String::from_utf8_lossy(&out.stderr).contains("unresolved custody proof"));
    fs::write(root.join("snapshot/custody.json"), &original).unwrap();
    assert!(review(&f).status.success());
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "1");
}

#[test]
fn explicit_apply_waits_for_checkout_writer_lock_before_any_commit() {
    let f = setup();
    import(
        &f,
        op(json!(["owned.txt"])),
        json!([task("A", &[], "completed", "Task")]),
    );
    fs::write(f.root.join("owned.txt"), "checked bytes\n").unwrap();
    run(&f);
    let path = save_review(&f);
    let guard = jtasks::resources::ControllerGuard::acquire(&f.root.join(".git")).unwrap();
    let out = apply(&f, &path);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("foreground writer still owns checkout"));
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "1");
    drop(guard);
    let out = apply(&f, &path);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn checkpoint_tables_have_run_cleanup_ownership_and_uncertain_intents_are_preserved() {
    use jtasks::controller::cleanup::{self, Removal};
    #[derive(Default)]
    struct FakeRemoval(Vec<std::path::PathBuf>);
    impl Removal for FakeRemoval {
        fn remove_file(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
            self.0.push(path.into());
            Ok(())
        }
        fn remove_directory(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
            self.0.push(path.into());
            Ok(())
        }
    }
    let f = setup();
    import(
        &f,
        op(json!(["owned.txt"])),
        json!([task("A", &[], "completed", "Task")]),
    );
    fs::write(f.root.join("owned.txt"), "checked bytes\n").unwrap();
    run(&f);
    let path = save_review(&f);
    let out = apply(&f, &path);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let workspace = jtasks::storage::Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let guard = jtasks::resources::ControllerGuard::acquire(&workspace.git_dir).unwrap();
    let disk = rusqlite::Connection::open(workspace.database()).unwrap();
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    // The historical cleanup target is exclusively in-memory. Copy the real
    // fixture state using SQLite's backup API; never remove its persisted rows.
    // SAFETY: both connections outlive the backup, the handles are used only in
    // this thread, and the backup is finished before either is queried again.
    unsafe {
        let backup = rusqlite::ffi::sqlite3_backup_init(
            conn.handle(),
            c"main".as_ptr(),
            disk.handle(),
            c"main".as_ptr(),
        );
        assert!(!backup.is_null());
        let step = rusqlite::ffi::sqlite3_backup_step(backup, -1);
        let finish = rusqlite::ffi::sqlite3_backup_finish(backup);
        assert_eq!(step, rusqlite::ffi::SQLITE_DONE);
        assert_eq!(finish, rusqlite::ffi::SQLITE_OK);
    }
    let approval = cleanup::preview_with(&conn, &workspace, &["demo".into()], false, true).unwrap();
    assert!(
        approval.runs[0].blockers.is_empty(),
        "{:?}",
        approval.runs[0].blockers
    );
    for table in [
        "checkpoint_baselines",
        "checkpoint_check_inputs",
        "checkpoint_receipts",
    ] {
        assert!(
            approval.runs[0]
                .rows
                .iter()
                .any(|row| row.table == table && row.count == 1),
            "missing owned checkpoint table {table}"
        );
    }
    conn.execute("UPDATE checkpoint_receipts SET state='intent'", [])
        .unwrap();
    let uncertain =
        cleanup::preview_with(&conn, &workspace, &["demo".into()], false, true).unwrap();
    assert!(
        uncertain.runs[0]
            .blockers
            .iter()
            .any(|blocker| blocker.contains("checkpoint_receipts"))
    );
    conn.execute("UPDATE checkpoint_receipts SET state='applied'", [])
        .unwrap();
    let mut removal = FakeRemoval::default();
    cleanup::execute_with(
        &mut conn,
        &workspace,
        &guard,
        &approval,
        false,
        &mut removal,
    )
    .unwrap();
    assert!(!removal.0.is_empty());
    assert!(
        removal.0.iter().all(|path| path.exists()),
        "fake cleanup must leave every fixture file in place"
    );
    for table in [
        "checkpoint_baselines",
        "checkpoint_check_inputs",
        "checkpoint_receipts",
    ] {
        assert_eq!(
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(
        disk.query_row("SELECT count(*) FROM runs WHERE id='demo'", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        disk.query_row("SELECT count(*) FROM checkpoint_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
