mod support;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

fn worker(id: &str, resources: &[&str], delay: u64) -> Value {
    let mut task = task(id, &[], "completed", "designated completion");
    task["executor"]["delay_ms"] = json!(delay);
    task["admission"] = json!({"resources":resources});
    task
}
fn import(
    f: &Fixture,
    tasks: Value,
    operations: Value,
    task_capacity: usize,
    heavy_capacity: usize,
) {
    let path = f.root.join("parallel.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"mixed","scheduling":{"task_capacity":task_capacity,"heavy_capacity":heavy_capacity},"tasks":tasks,"operations":operations})).unwrap()).unwrap();
    let result = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
struct Controller(Child);
impl Drop for Controller {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn run(f: &Fixture) -> Controller {
    Controller(
        f.command()
            .args(["run", "--run", "demo"])
            .env("JTASKS_SHUTDOWN_GRACE_MS", "20")
            .env("JTASKS_SHUTDOWN_FORCE_MS", "1500")
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn call(f: &Fixture, args: &[&str]) -> Value {
    let output = f.call(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn command(
    f: &Fixture,
    request: &str,
    task: Option<(&str, &str)>,
    resources: &[&str],
    delay: u64,
) -> Value {
    let path = f.root.join(format!("{request}.json"));
    fs::write(&path,serde_json::to_vec(&json!({"request_id":request,"run":"demo","task":task.map(|v|v.0),"attempt":task.map(|v|v.1),"operation":if task.is_none(){Some("manual")}else{None},"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent","--delay-ms",delay.to_string()],"cwd":f.root,"resources":resources})).unwrap()).unwrap();
    call(f, &["exec", "start", "--request", path.to_str().unwrap()])
}
fn db(f: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap()
}

#[test]
fn resource_conflict_is_skipped_without_partial_holds_or_consuming_coding_capacity() {
    let f = Fixture::new();
    import(
        &f,
        json!([
            worker("A", &["path:src"], 1200),
            worker("B", &["path:src/file.rs", "tools/build"], 0),
            worker("C", &["tools/build"], 1200)
        ]),
        json!([]),
        2,
        1,
    );
    let mut controller = run(&f);
    let snapshot = f.await_status(|s| {
        s["tasks"][0]["state"] == "running" && s["tasks"][2]["state"] == "running"
    });
    assert_eq!(snapshot["tasks"][1]["attempts"], json!([]));
    let held = jtasks::resources::registry::list(&f.data.join("advisory.sqlite3")).unwrap();
    assert_eq!(held.len(), 2);
    assert!(held.iter().all(|r| r.owner.task != "B"));
    assert!(controller.0.wait().unwrap().success());
    assert!(
        jtasks::resources::registry::list(&f.data.join("advisory.sqlite3"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.snapshot()["tasks"][1]["state"], "completed");
}

#[test]
fn heavy_capacity_is_separate_and_queued_operations_keep_stable_handles() {
    let f = Fixture::new();
    let operation = |id| json!({"id":id,"stage":{"kind":"prerequisite","consumers":["D"]},"required":true,"revision":"r1","inputs":["src/"],"initial":{"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent","--delay-ms","1200"],"cwd":"."}});
    import(
        &f,
        json!([
            worker("A", &[], 1800),
            worker("B", &[], 1800),
            worker("D", &[], 0)
        ]),
        json!([operation("first"), operation("second")]),
        2,
        1,
    );
    let mut controller = run(&f);
    let snapshot = f.await_status(|s| {
        s["tasks"][0]["state"] == "running"
            && s["tasks"][1]["state"] == "running"
            && s["operations"][1]["state"] == "running"
    });
    assert_eq!(snapshot["tasks"][2]["attempts"], json!([]));
    let conn = db(&f);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM managed_commands WHERE state='running'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM managed_commands WHERE state='queued'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let second = snapshot["operations"][1]["command_id"].clone();
    assert!(controller.0.wait().unwrap().success());
    assert_eq!(f.snapshot()["operations"][1]["command_id"], second);
}

#[test]
fn surviving_command_retains_inherited_resources_after_worker_retires() {
    let f = Fixture::new();
    import(
        &f,
        json!([
            worker("A", &["tools/build"], 1000),
            worker("B", &["tools/build"], 0),
            worker("C", &[], 0)
        ]),
        json!([]),
        1,
        1,
    );
    let mut controller = run(&f);
    let snapshot = f.await_status(|s| s["tasks"][0]["state"] == "running");
    let attempt = snapshot["tasks"][0]["attempts"][0]["id"].as_str().unwrap();
    let receipt = command(&f, "survivor", Some(("A", attempt)), &[], 2500);
    assert_eq!(receipt["state"], "running");
    let snapshot = f.await_status(|s| {
        s["tasks"][0]["state"] == "completed" && s["tasks"][2]["state"] == "completed"
    });
    assert_eq!(snapshot["tasks"][1]["attempts"], json!([]));
    let held = jtasks::resources::registry::list(&f.data.join("advisory.sqlite3")).unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].owner.task, "A");
    assert!(controller.0.wait().unwrap().success());
    assert_eq!(f.snapshot()["tasks"][1]["state"], "completed");
}

#[test]
fn stopped_queued_waits_free_capacity_and_admit_independent_commands() {
    let f = Fixture::new();
    f.import(json!([worker("A", &[], 15000)]));
    let mut controller = run(&f);
    f.await_status(|s| s["tasks"][0]["state"] == "running");
    let first = command(&f, "first", None, &["tools/build"], 15000);
    let second = command(&f, "second", None, &["tools/other"], 4000);
    assert_eq!(second["state"], "queued");
    let second_id = second["command_id"].as_str().unwrap();
    assert_eq!(
        call(&f, &["exec", "wait", second_id, "--wait-seconds", "0"])["wait_expired"],
        true
    );
    assert_eq!(
        call(
            &f,
            &["exec", "stop", second_id, "--reason", "cancel queued wait"]
        )["state"],
        "stopped"
    );
    call(
        &f,
        &[
            "exec",
            "stop",
            first["command_id"].as_str().unwrap(),
            "--reason",
            "release heavy slot",
        ],
    );
    call(
        &f,
        &[
            "exec",
            "wait",
            first["command_id"].as_str().unwrap(),
            "--wait-seconds",
            "5",
        ],
    );
    let third = command(&f, "third", None, &["tools/other"], 0);
    assert_ne!(third["state"], "queued");
    call(&f, &["stop", "--run", "demo"]);
    assert_eq!(controller.0.wait().unwrap().code(), Some(3));
    assert!(
        jtasks::resources::registry::list(&f.data.join("advisory.sqlite3"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn admission_rechecks_allowance_configuration_dependencies_and_cooldown() {
    let f = Fixture::new();
    let mut disabled = worker("disabled", &[], 0);
    disabled["admission"]["allowance"] = json!(false);
    let mut invalid = worker("invalid", &[], 0);
    invalid["executor"] = json!({"kind":"profile","alias":"missing-profile"});
    let mut dependent = worker("dependent", &[], 0);
    dependent["depends_on"] = json!(["disabled"]);
    let mut cooling = worker("cooling", &[], 0);
    cooling["admission"]["cooldown_until_ms"] =
        json!(jtasks::controller::admission::now_ms() + 5000);
    import(
        &f,
        json!([
            disabled,
            invalid,
            dependent,
            cooling,
            worker("independent", &[], 0)
        ]),
        json!([]),
        2,
        1,
    );
    let start = Instant::now();
    let mut controller = run(&f);
    let snapshot = f.await_status(|s| s["tasks"][4]["state"] == "completed");
    assert_eq!(snapshot["tasks"][0]["attempts"], json!([]));
    assert_eq!(snapshot["tasks"][1]["state"], "needs_intervention");
    assert_eq!(snapshot["tasks"][2]["attempts"], json!([]));
    assert_eq!(snapshot["tasks"][3]["attempts"], json!([]));
    assert_eq!(controller.0.wait().unwrap().code(), Some(3));
    assert!(start.elapsed() >= Duration::from_millis(700));
    assert_eq!(f.snapshot()["tasks"][3]["state"], "completed");
}

#[test]
fn explicit_shared_scope_matches_public_registry_across_checkouts() {
    let authority = Fixture::new();
    let consumer = Fixture::new();
    let registry = authority.root.join("shared.sqlite3");
    let registry_path = registry.to_str().unwrap();
    let held = call(
        &authority,
        &[
            "resources",
            "--registry",
            registry_path,
            "reserve",
            "--run",
            "external",
            "--task",
            "writer",
            "--resource",
            "database/integration",
            "--shared-scope",
            "host/dev",
            "--request",
            "external-hold",
            "--actor",
            "fixture",
            "--reason",
            "cross checkout writer",
        ],
    );
    import(
        &consumer,
        json!([
            worker("shared", &["shared:host/dev:database/integration"], 0),
            worker("local", &["database/integration"], 0)
        ]),
        json!([]),
        2,
        1,
    );
    let output = consumer
        .command()
        .args(["run", "--run", "demo"])
        .env("JTASKS_ADVISORY_REGISTRY", &registry)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let snapshot = consumer.snapshot();
    assert_eq!(snapshot["tasks"][0]["attempts"], json!([]));
    assert_eq!(snapshot["tasks"][1]["state"], "completed");
    let reservation = held["current"][0]["id"].as_str().unwrap();
    call(
        &authority,
        &[
            "resources",
            "--registry",
            registry_path,
            "release",
            reservation,
            "--request",
            "external-release",
            "--actor",
            "fixture",
            "--reason",
            "explicit release",
        ],
    );
    let output = consumer
        .command()
        .args(["run", "--run", "demo"])
        .env("JTASKS_ADVISORY_REGISTRY", &registry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        jtasks::resources::registry::list(&registry)
            .unwrap()
            .is_empty()
    );
    for malformed in [
        "shared:",
        "shared::db/name",
        "shared:host:db/",
        "shared:host:name",
    ] {
        let workspace =
            jtasks::storage::Workspace::resolve(&consumer.root, Some(&consumer.data)).unwrap();
        assert!(jtasks::controller::admission::resources(&workspace, &[malformed.into()]).is_err());
    }
}
