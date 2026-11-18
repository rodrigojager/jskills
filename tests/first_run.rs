mod support;
use jtasks::{
    domain::types::{Outcome, Receipt},
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use serde_json::json;
use std::fs;
use support::{Fixture, task};

#[test]
fn two_dependent_tasks_complete_from_durable_designated_results() {
    let f = Fixture::new();
    let summary = "Judgment: spaces, quotes \" and $(shell); literal arguments";
    f.import(json!([
        task("A", &[], "completed", summary),
        task("B", &["A"], "completed", "Dependency input accepted")
    ]));
    let validation = f.call(&["validate", f.root.join("plan.json").to_str().unwrap()]);
    assert!(validation.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&validation.stdout).unwrap()["tasks"],
        2
    );
    let output = f.call(&["run", "--run", "demo"]);
    assert!(
        output.status.success(),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = f.snapshot();
    assert_eq!(snapshot["state"], "completed");
    assert_eq!(snapshot["tasks"][0]["attempts"][0]["summary"], summary);
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM receipts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let events: Vec<String> = conn
        .prepare("SELECT event FROM journal ORDER BY sequence")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        events,
        [
            "imported",
            "launch_intent",
            "result_accepted",
            "process_retired",
            "launch_intent",
            "result_accepted",
            "process_retired"
        ]
    );
    let rerun = f.call(&["run", "--run", "demo"]);
    assert!(rerun.status.success());
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn prose_helper_json_exit_zero_and_blank_summaries_never_complete() {
    for (outcome, summary) in [
        ("silent", ""),
        ("prose", "all done"),
        ("helper_json", ""),
        ("malformed", ""),
        ("completed", "   "),
    ] {
        let f = Fixture::new();
        f.import(json!([
            task("A", &[], outcome, summary),
            task("B", &["A"], "completed", "must not start")
        ]));
        assert_eq!(
            f.call(&["run", "--run", "demo"]).status.code(),
            Some(3),
            "{outcome}"
        );
        let snapshot = f.snapshot();
        assert_eq!(
            snapshot["tasks"][0]["state"], "retry_exhausted",
            "{outcome}"
        );
        assert!(snapshot["tasks"][0]["attempts"][0]["receipt_id"].is_null());
        assert_eq!(snapshot["tasks"][1]["state"], "pending");
    }
}

#[test]
fn blocked_and_failed_are_attributable_but_run_remains_incomplete() {
    for outcome in ["blocked", "failed"] {
        let f = Fixture::new();
        f.import(json!([
            task("A", &[], outcome, "Needs an explicit intervention"),
            task("B", &["A"], "completed", "must not run")
        ]));
        assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
        let snapshot = f.snapshot();
        assert_eq!(snapshot["state"], "incomplete");
        assert_eq!(
            snapshot["tasks"][0]["state"],
            if outcome == "failed" {
                "retry_exhausted"
            } else {
                outcome
            }
        );
        assert_eq!(snapshot["tasks"][0]["attempts"][0]["outcome"], outcome);
        assert!(!snapshot["tasks"][0]["attempts"][0]["receipt_id"].is_null());
        assert_eq!(snapshot["tasks"][1]["attempts"], json!([]));
    }
}

#[test]
fn offline_status_does_not_create_or_change_database_files() {
    let f = Fixture::new();
    assert!(!f.call(&["status", "--run", "demo"]).status.success());
    assert!(!f.data.exists());
    f.import(json!([task(
        "A",
        &[],
        "completed",
        "judgment needs no test evidence"
    )]));
    let files = || {
        let mut entries: Vec<_> = fs::read_dir(&f.data)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (
                    p.file_name().unwrap().to_owned(),
                    fs::read(&p).unwrap(),
                    fs::metadata(&p).unwrap().modified().unwrap(),
                )
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    };
    let before = files();
    assert_eq!(f.call(&["status", "--run", "demo"]).status.code(), Some(3));
    assert_eq!(files(), before);
}

#[test]
fn validation_rejects_implicit_policy_unknown_fields_and_dependency_cycles() {
    let f = Fixture::new();
    let path = f.root.join("invalid.json");
    for plan in [
        json!({"schema_version":1,"tasks":[task("A",&[],"completed","x")]}),
        json!({"schema_version":1,"completion":"judgment","tasks":[task("A",&["B"],"completed","x"),task("B",&["A"],"completed","x")]}),
        json!({"schema_version":1,"completion":"judgment","tasks":[task("A",&["missing"],"completed","x")]}),
        json!({"schema_version":1,"completion":"judgment","tasks":[task("A",&[],"completed","x")],"helper":true}),
    ] {
        fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
        assert!(
            !f.call(&["validate", path.to_str().unwrap()])
                .status
                .success()
        );
        assert!(!f.data.exists());
    }
}

#[test]
fn receipts_are_idempotent_immutable_attributable_and_durable() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "x")]));
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
    let mut store = Store::open(&workspace, &guard).unwrap();
    store.intent("demo", "A", "attempt-1").unwrap();
    store.launched("attempt-1", 42).unwrap();
    let receipt = Receipt {
        receipt_id: "receipt-1".into(),
        attempt_id: "attempt-1".into(),
        task_id: "A".into(),
        outcome: Outcome::Completed,
        summary: "Attributable judgment".into(),
    };
    let mut wrong = receipt.clone();
    wrong.task_id = "B".into();
    assert!(store.accept("demo", "attempt-1", &wrong).is_err());
    assert!(store.accept("demo", "stale-attempt", &receipt).is_err());
    assert!(store.accept("demo", "attempt-1", &receipt).unwrap());
    drop(store);
    let mut store = Store::open(&workspace, &guard).unwrap();
    assert!(!store.accept("demo", "attempt-1", &receipt).unwrap());
    let mut conflicting = receipt.clone();
    conflicting.summary = "Different".into();
    assert!(store.accept("demo", "attempt-1", &conflicting).is_err());
    conflicting.receipt_id = "receipt-2".into();
    assert!(store.accept("demo", "attempt-1", &conflicting).is_err());
    assert!(
        store
            .conn
            .execute("UPDATE attempts SET id='changed' WHERE id='attempt-1'", [])
            .is_err()
    );
}

#[test]
fn result_acceptance_precedes_retirement_and_lock_applies_across_data_roots() {
    let f = Fixture::new();
    let mut first = task("A", &[], "completed", "accepted before process retires");
    first["executor"]["linger_ms"] = json!(800);
    f.import(json!([
        first,
        task("B", &["A"], "completed", "after retirement")
    ]));
    let mut controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let snapshot = f.await_status(|s| {
        s["tasks"][0]["attempts"]
            .as_array()
            .is_some_and(|a| !a.is_empty() && !a[0]["receipt_id"].is_null())
    });
    assert_eq!(snapshot["tasks"][0]["state"], "running");
    assert_eq!(
        snapshot["tasks"][0]["attempts"][0]["process_state"],
        "running"
    );
    assert_eq!(snapshot["tasks"][1]["attempts"], json!([]));
    let plan = f.root.join("plan.json");
    let other = f.root.join("different-runtime");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jtasks"))
        .arg("--root")
        .arg(&f.root)
        .arg("--data-dir")
        .arg(&other)
        .arg("import")
        .arg(plan)
        .args(["--run", "another"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("another foreground controller"));
    assert!(!other.exists());
    assert!(controller.wait().unwrap().success());
}

#[test]
fn explicit_stop_retires_process_and_explicit_run_resumes_with_new_identity() {
    let f = Fixture::new();
    let mut first = task("A", &[], "completed", "resumed");
    first["executor"]["delay_ms"] = json!(3000);
    f.import(json!([first]));
    let mut controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    f.await_status(|s| {
        s["tasks"][0]["attempts"]
            .as_array()
            .is_some_and(|a| !a.is_empty() && a[0]["process_state"] == "running")
    });
    let stop = f.call(&["stop", "--run", "demo"]);
    assert!(
        stop.status.success(),
        "stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert_eq!(controller.wait().unwrap().code(), Some(3));
    let snapshot = f.snapshot();
    assert_eq!(snapshot["state"], "stopped");
    assert_eq!(snapshot["tasks"][0]["state"], "pending");
    assert_eq!(
        snapshot["tasks"][0]["attempts"][0]["process_state"],
        "interrupted"
    );
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    let attempts = f.snapshot()["tasks"][0]["attempts"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(attempts.len(), 2);
    assert_ne!(attempts[0]["id"], attempts[1]["id"]);
}

#[test]
fn stop_records_the_actor_and_a_stopped_run_answers_exit_3() {
    let f = Fixture::new();
    let mut first = task("A", &[], "completed", "resumed");
    first["executor"]["delay_ms"] = json!(3000);
    f.import(json!([first]));
    let mut controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    f.await_status(|s| {
        s["tasks"][0]["attempts"]
            .as_array()
            .is_some_and(|a| !a.is_empty() && a[0]["process_state"] == "running")
    });
    let stop = f.call(&["stop", "--run", "demo", "--actor", "web"]);
    assert!(stop.status.success());
    controller.wait().unwrap();
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    let detail: String = conn
        .query_row(
            "SELECT detail FROM journal WHERE event='stop_requested'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&detail).unwrap()["actor"],
        "web"
    );
    let again = f.call(&["stop", "--run", "demo"]);
    assert_eq!(again.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&again.stdout).contains("already stopped"));
}

#[test]
fn container_and_product_resolve_the_same_configured_runtime_root() {
    let f = Fixture::new();
    let container = f.root.join("container");
    let product = container.join("app");
    fs::create_dir_all(container.join("_work")).unwrap();
    fs::create_dir_all(&product).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&product)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        container.join("_work/WORKSPACE.md"),
        "---\njproject: 1\nproduct_dir: app\nwork_dir: _work\njtasks_data: _work/runtime\n---\n",
    )
    .unwrap();
    let from_container = Workspace::resolve(&container, None).unwrap();
    let from_product = Workspace::resolve(&product, None).unwrap();
    assert_eq!(from_container.checkout, from_product.checkout);
    assert_eq!(from_container.data, from_product.data);
    assert_eq!(
        from_container.data,
        container.canonicalize().unwrap().join("_work/runtime/v2")
    );
    assert!(!from_container.data.exists());
    let plan = f.plan(json!([task("A", &[], "completed", "container intent")]));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jtasks"))
        .arg("--root")
        .arg(&container)
        .arg("import")
        .arg(&plan)
        .args(["--run", "container-run"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(from_product.database().is_file());
    assert!(!product.join(".scratch").exists());
}
