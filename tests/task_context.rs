mod support;
use jtasks::{
    domain::types::{Outcome, Receipt},
    read::recovery_facts::{self, MAX_ATTEMPTS, MAX_TEXT},
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use serde_json::json;
use std::fs;
use support::{Fixture, task};

fn record(store: &mut Store, run: &str, task: &str, attempt: &str, summary: &str) {
    store.intent(run, task, attempt).unwrap();
    store.launched(attempt, 42).unwrap();
    store
        .accept(
            run,
            attempt,
            &Receipt {
                receipt_id: format!("receipt-{attempt}"),
                attempt_id: attempt.into(),
                task_id: task.into(),
                outcome: Outcome::Blocked,
                summary: summary.into(),
            },
        )
        .unwrap();
    store.retire(run, task, attempt, Some(7), true).unwrap();
}

#[test]
fn launched_retry_gets_common_context_and_only_its_own_recorded_facts() {
    let f = Fixture::new();
    let mut a = task("A", &[], "completed", "new result");
    a["context"] = json!({"brief":"Current task brief", "requirements":["Keep preserved edits"],"dependency_artifacts":["design/schema.json"],"operations":["cargo check after edits"],"configuration":["mode=judgment"],"limits":["one attempt"],"resources":["src/module.rs"]});
    let mut plan = json!({"schema_version":1,"completion":"judgment","context":{"brief":"Original common project brief","references":["spec.json"]},"tasks":[a,task("B", &[], "completed", "B result")]});
    let plan_path = f.root.join("plan.json");
    fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(
        f.call(&["import", plan_path.to_str().unwrap(), "--run", "demo"])
            .status
            .success()
    );
    plan["tasks"] = json!([task("A", &[], "completed", "different run")]);
    let other = f.root.join("other.json");
    fs::write(&other, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(
        f.call(&["import", other.to_str().unwrap(), "--run", "other"])
            .status
            .success()
    );
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    {
        let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
        let mut store = Store::open(&workspace, &guard).unwrap();
        record(
            &mut store,
            "demo",
            "A",
            "old-a",
            "Same task interrupted work",
        );
        record(
            &mut store,
            "demo",
            "B",
            "old-b",
            "OTHER_TASK_PRIVATE_HISTORY",
        );
        record(
            &mut store,
            "other",
            "A",
            "other-run-a",
            "OTHER_RUN_PRIVATE_HISTORY",
        );
        // Test setup represents an explicitly approved retry; the renderer never mutates task state.
        store
            .conn
            .execute(
                "UPDATE tasks SET state='interrupted' WHERE run_id='demo'",
                [],
            )
            .unwrap();
        store.conn.execute_batch("CREATE TABLE IF NOT EXISTS managed_commands(id TEXT PRIMARY KEY,request_id TEXT UNIQUE,run_id TEXT,task_id TEXT,attempt_id TEXT,spec TEXT,state TEXT,pid INTEGER,exit_code INTEGER,reason TEXT)").unwrap();
        for (id, task, attempt) in [
            ("cmd-a", "A", "old-a"),
            ("OTHER_TASK_COMMAND", "B", "old-b"),
        ] {
            store.conn.execute("INSERT INTO managed_commands(id,request_id,run_id,task_id,attempt_id,spec,state,exit_code,reason) VALUES(?1,?1,'demo',?2,?3,'{}','exited',9,'test failed')",[id,task,attempt]).unwrap();
        }
    }
    let old = f.data.join("attempts/old-a");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("recovery-facts.json"), serde_json::to_vec(&json!({
        "findings":["Parser fails on empty field"], "decisions":["Preserve explicit null"],
        "obstacles":["Required fixture missing"], "remaining_work":["Add empty fixture"],
        "artifacts":["src/parser.rs"], "commands":[{"command":"cargo test parser","exit_code":7,"outcome":"failed assertion","receipt":"command-c1"}]
    })).unwrap()).unwrap();
    fs::write(
        old.join("stdout.log"),
        "WHOLE_TRANSCRIPT_MUST_NOT_BE_INJECTED",
    )
    .unwrap();
    let run = f.call(&["run", "--run", "demo"]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let snapshot = f.snapshot();
    let attempt = snapshot["tasks"][0]["attempts"][1]["id"].as_str().unwrap();
    let ticket =
        fs::read_to_string(f.data.join("attempts").join(attempt).join("ticket.md")).unwrap();
    for text in [
        "Original common project brief",
        "Current task brief",
        "design/schema.json",
        "cargo check after edits",
        "mode=judgment",
        "one attempt",
        "src/module.rs",
        "Parser fails on empty field",
        "Preserve explicit null",
        "Required fixture missing",
        "Add empty fixture",
        "src/parser.rs",
        "command-c1",
        "receipt-old-a",
        "exit=Some(7)",
        "Same task interrupted work",
        "cmd-a",
        "exit=Some(9)",
        "runtime managed_commands",
    ] {
        assert!(ticket.contains(text), "missing {text}: {ticket}");
    }
    for forbidden in [
        "OTHER_TASK_PRIVATE_HISTORY",
        "OTHER_RUN_PRIVATE_HISTORY",
        "WHOLE_TRANSCRIPT_MUST_NOT_BE_INJECTED",
        "OTHER_TASK_COMMAND",
    ] {
        assert!(!ticket.contains(forbidden));
    }
    assert!(ticket.contains("Inspect the current checkout"));
}

#[test]
fn recovery_reads_are_bounded_read_only_and_missing_logs_are_uncertain() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "fresh")]));
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    {
        let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
        let mut store = Store::open(&workspace, &guard).unwrap();
        for index in 0..7 {
            record(
                &mut store,
                "demo",
                "A",
                &format!("old-{index}"),
                &"é".repeat(MAX_TEXT * 2),
            );
        }
    }
    let before = fs::read(workspace.database()).unwrap();
    let facts =
        recovery_facts::read(&workspace.database(), &workspace.data, "demo", "A", "new").unwrap();
    assert_eq!(facts.attempts.len(), MAX_ATTEMPTS);
    assert_eq!(facts.omitted_attempts, 3);
    assert_eq!(facts.attempts[0].attempt, "old-3");
    for attempt in facts.attempts {
        assert!(attempt.summary.unwrap().chars().count() <= MAX_TEXT + 12);
        assert!(
            attempt
                .warnings
                .iter()
                .any(|w| w.contains("unavailable or invalid"))
        );
    }
    assert_eq!(fs::read(workspace.database()).unwrap(), before);
    assert!(!workspace.data.join("attempts/new").exists());
}

#[test]
fn absent_history_does_not_claim_a_pristine_checkout() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "first attempt")]));
    fs::write(f.root.join("preserved.txt"), "existing edit").unwrap();
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    let snapshot = f.snapshot();
    let id = snapshot["tasks"][0]["attempts"][0]["id"].as_str().unwrap();
    let ticket = fs::read_to_string(f.data.join("attempts").join(id).join("ticket.md")).unwrap();
    assert!(ticket.contains("Missing facts never mean a pristine checkout"));
    assert!(ticket.contains("existing edits and output may still be present"));
    assert_eq!(
        fs::read_to_string(f.root.join("preserved.txt")).unwrap(),
        "existing edit"
    );
}

#[test]
fn oversized_and_malformed_explicit_logs_do_not_inject_transcripts() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "new")]));
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    {
        let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
        let mut store = Store::open(&workspace, &guard).unwrap();
        record(&mut store, "demo", "A", "malformed", "result");
        record(&mut store, "demo", "A", "oversized", "result");
    }
    for (id, content) in [
        ("malformed", "unstructured conversation".into()),
        ("oversized", "x".repeat(33000)),
    ] {
        let dir = f.data.join("attempts").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("recovery-facts.json"), content).unwrap();
    }
    let facts =
        recovery_facts::read(&workspace.database(), &workspace.data, "demo", "A", "new").unwrap();
    for attempt in facts.attempts {
        assert!(attempt.worker.findings.is_empty());
        assert!(!attempt.warnings.is_empty());
    }
}

#[test]
fn explicit_fact_lists_and_utf8_strings_have_independent_bounds() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "new")]));
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    {
        let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
        let mut store = Store::open(&workspace, &guard).unwrap();
        record(&mut store, "demo", "A", "old", "result");
    }
    let dir = f.data.join("attempts/old");
    fs::create_dir_all(&dir).unwrap();
    let findings: Vec<_> = (0..12).map(|_| "é".repeat(1000)).collect();
    fs::write(
        dir.join("recovery-facts.json"),
        serde_json::to_vec(&json!({"findings":findings})).unwrap(),
    )
    .unwrap();
    let facts =
        recovery_facts::read(&workspace.database(), &workspace.data, "demo", "A", "new").unwrap();
    assert_eq!(
        facts.attempts[0].worker.findings.len(),
        recovery_facts::MAX_FACTS
    );
    assert!(
        facts.attempts[0]
            .worker
            .findings
            .iter()
            .all(|s| s.chars().count() <= MAX_TEXT + 12 && s.contains("[truncated]"))
    );
    assert!(
        facts.attempts[0]
            .warnings
            .iter()
            .any(|s| s.contains("omitted"))
    );
}
