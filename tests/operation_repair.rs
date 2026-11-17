mod support;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

fn shell(script: &str) -> Value {
    if cfg!(windows) {
        json!({"program":"powershell.exe","args":["-NoProfile","-NonInteractive","-Command"],"script":script})
    } else {
        json!({"program":"sh","args":["-c"],"script":script})
    }
}
fn check(id: &str, input: &str, fail: bool) -> Value {
    let mut initial = if fail {
        shell(if cfg!(windows) {
            "if (Test-Path -LiteralPath 'ready.flag') { exit 0 } else { exit 7 }"
        } else {
            "test -f ready.flag"
        })
    } else {
        json!({"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent"]})
    };
    initial["cwd"] = json!(".");
    json!({"id":id,"stage":{"kind":"final","tasks":["A"]},"required":true,"revision":"r1","inputs":[input],"initial":initial,"repair_task":"A"})
}
fn import(f: &Fixture, operations: Value) {
    let path = f.root.join("plan.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[task("A",&[],"completed","implementation finished")],"operations":operations})).unwrap()).unwrap();
    let result = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn call(f: &Fixture, args: &[&str]) -> Value {
    let result = f.call(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn refused(f: &Fixture, args: &[&str], error: &str) {
    let result = f.call(args);
    assert!(!result.status.success(), "unexpected success: {args:?}");
    assert!(
        String::from_utf8_lossy(&result.stderr).contains(error),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn serve(f: &Fixture) -> Server {
    let old = fs::read(f.data.join("commands-control.json")).ok();
    let server = Server(
        f.command()
            .args(["exec", "serve"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    loop {
        let new = fs::read(f.data.join("commands-control.json")).ok();
        if new.is_some() && new != old {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    server
}
fn shutdown(f: &Fixture, server: &mut Server) {
    call(f, &["exec", "shutdown"]);
    assert!(server.0.wait().unwrap().success());
}
fn begin(f: &Fixture, op: &str, request: &str) -> Value {
    call(
        f,
        &[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            op,
            "--worker",
            "A",
            "--request-id",
            request,
        ],
    )
}
fn command(f: &Fixture, op: &str, request: &str, invocation: Value) -> Value {
    let mut spec = invocation;
    spec["request_id"] = json!(request);
    spec["run"] = json!("demo");
    spec["operation"] = json!(op);
    spec["cwd"] = json!(f.root);
    let path = f.root.join("command.json");
    fs::write(&path, serde_json::to_vec(&spec).unwrap()).unwrap();
    let receipt = call(f, &["exec", "start", "--request", path.to_str().unwrap()]);
    let id = receipt["command_id"].as_str().unwrap();
    call(f, &["exec", "wait", id, "--wait-seconds", "10"])
}
fn report(f: &Fixture, op: &str, cycle: u32, command: &Value, revision: &str) -> String {
    let path = f.root.join("report.json");
    fs::write(&path,serde_json::to_vec(&json!({"run":"demo","operation":op,"cycle":cycle,"worker":"A","command_id":command["command_id"],"revision":revision,"summary":"Agent selected this exact invocation as the original check recheck."})).unwrap()).unwrap();
    path.to_str().unwrap().into()
}
fn finish(f: &Fixture, op: &str, cycle: u32, command: &Value, revision: &str) -> Value {
    let path = report(f, op, cycle, command, revision);
    call(f, &["operations", "repair-finish", "--report", &path])
}

#[test]
fn agent_selects_multiple_commands_but_only_exact_recheck_certifies_failure() {
    let f = Fixture::new();
    let op = check("check", "src/", true);
    import(&f, json!([op]));
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let before = f.snapshot();
    assert_eq!(before["operations"][0]["repair_task"], "A");
    assert_eq!(
        before["operations"][0]["repair"]["commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut server = serve(&f);
    refused(
        &f,
        &[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            "check",
            "--worker",
            "unrelated",
            "--request-id",
            "wrong-worker",
        ],
        "responsibility",
    );
    assert_eq!(begin(&f, "check", "cycle-one")["cycle"], 1);
    assert_eq!(begin(&f, "check", "cycle-one")["duplicate"], true);
    let unrelated = command(
        &f,
        "check",
        "unrelated-success",
        json!({"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent"]}),
    );
    let path = report(&f, "check", 1, &unrelated, "r1");
    refused(
        &f,
        &["operations", "repair-finish", "--report", &path],
        "original check specification",
    );
    let correction = command(
        &f,
        "check",
        "agent-chosen-correction",
        shell(if cfg!(windows) {
            "Set-Content -LiteralPath 'ready.flag' -Value 'ready'"
        } else {
            "touch ready.flag"
        }),
    );
    assert_eq!(correction["exit_code"], 0);
    assert_eq!(f.snapshot()["operations"][0]["state"], "repairing");
    let recheck = command(
        &f,
        "check",
        "actual-recheck",
        check("check", "src/", true)["initial"].clone(),
    );
    assert_eq!(finish(&f, "check", 1, &recheck, "r1")["state"], "passed");
    assert_eq!(finish(&f, "check", 1, &recheck, "r1")["duplicate"], true);
    let repaired = f.snapshot();
    assert_eq!(
        repaired["operations"][0]["repair"]["work_stages_charged"],
        1
    );
    assert_eq!(
        repaired["operations"][0]["repair"]["commands"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        repaired["operations"][0]["repair"]["commands"][0]["exit_code"],
        7
    );
    shutdown(&f, &mut server);
    assert!(f.call(&["run", "--run", "demo"]).status.success());
}

#[test]
fn cycles_and_command_membership_survive_new_handles_and_controller_restart() {
    let f = Fixture::new();
    import(&f, json!([check("check", "src/", true)]));
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let mut server = serve(&f);
    let mut early = check("check", "src/", true)["initial"].clone();
    early["request_id"] = json!("bypass");
    early["run"] = json!("demo");
    early["operation"] = json!("check");
    early["cwd"] = json!(f.root);
    let path = f.root.join("bypass.json");
    fs::write(&path, serde_json::to_vec(&early).unwrap()).unwrap();
    refused(
        &f,
        &["exec", "start", "--request", path.to_str().unwrap()],
        "active repair stage",
    );
    begin(&f, "check", "first");
    shutdown(&f, &mut server);
    let mut server = serve(&f);
    assert_eq!(begin(&f, "check", "first")["duplicate"], true);
    refused(
        &f,
        &[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            "check",
            "--worker",
            "A",
            "--request-id",
            "replacement-request",
        ],
        "repair stage already active",
    );
    let first = command(
        &f,
        "check",
        "new-command-one",
        check("check", "src/", true)["initial"].clone(),
    );
    assert_eq!(finish(&f, "check", 1, &first, "r1")["state"], "failed");
    shutdown(&f, &mut server);
    let mut server = serve(&f);
    assert_eq!(begin(&f, "check", "second")["cycle"], 2);
    let path = report(&f, "check", 2, &first, "r1");
    refused(
        &f,
        &["operations", "repair-finish", "--report", &path],
        "this operation correction stage",
    );
    let second = command(
        &f,
        "check",
        "new-command-two",
        check("check", "src/", true)["initial"].clone(),
    );
    assert_ne!(first["command_id"], second["command_id"]);
    assert_eq!(finish(&f, "check", 2, &second, "r1")["state"], "failed");
    shutdown(&f, &mut server);
    let mut server = serve(&f);
    refused(
        &f,
        &[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            "check",
            "--worker",
            "A",
            "--request-id",
            "third",
        ],
        "allowance exhausted",
    );
    assert_eq!(
        f.snapshot()["operations"][0]["repair"]["work_stages_charged"],
        2
    );
    shutdown(&f, &mut server);
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
}

#[test]
fn task_local_failure_defaults_to_its_own_worker_without_relaunching_development() {
    let f = Fixture::new();
    let mut op = check("local", "src/", true);
    op["stage"] = json!({"kind":"task","task":"A"});
    op.as_object_mut().unwrap().remove("repair_task");
    let path = f.root.join("plan.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"task","tasks":[task("A",&[],"completed","candidate")],"operations":[op]})).unwrap()).unwrap();
    call(&f, &["import", path.to_str().unwrap(), "--run", "demo"]);
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let before = f.snapshot();
    assert_eq!(before["tasks"][0]["state"], "verification_pending");
    assert_eq!(before["operations"][0]["repair_task"], "A");
    let mut server = serve(&f);
    begin(&f, "local", "local-stage");
    command(
        &f,
        "local",
        "local-correction",
        shell(if cfg!(windows) {
            "Set-Content -LiteralPath 'ready.flag' -Value 'ready'"
        } else {
            "touch ready.flag"
        }),
    );
    let recheck = command(
        &f,
        "local",
        "local-recheck",
        check("local", "src/", true)["initial"].clone(),
    );
    finish(&f, "local", 1, &recheck, "r1");
    shutdown(&f, &mut server);
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"],
        before["tasks"][0]["attempts"]
    );
}

#[test]
fn declared_input_change_invalidates_only_affected_checks_and_transitive_dependents() {
    let f = Fixture::new();
    let mut dependent = check("downstream", "generated/", false);
    dependent["depends_on"] = json!([{"kind":"operation","id":"changed"}]);
    import(
        &f,
        json!([
            check("changed", "src/", false),
            dependent,
            check("unrelated", "tests/", false)
        ]),
    );
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    let original = f.snapshot();
    let mut server = serve(&f);
    let change = call(
        &f,
        &[
            "operations",
            "inputs-changed",
            "--run",
            "demo",
            "--request-id",
            "input-change",
            "--revision",
            "r2",
            "--input",
            "src/lib.rs",
            "--reason",
            "agent corrected a declared producer",
        ],
    );
    assert_eq!(change["invalidated"], json!(["changed", "downstream"]));
    let snapshot = f.snapshot();
    assert_eq!(snapshot["operations"][0]["state"], "invalidated");
    assert_eq!(snapshot["operations"][1]["state"], "invalidated");
    assert_eq!(snapshot["operations"][2]["state"], "passed");
    assert_eq!(
        snapshot["operations"][2]["command_id"],
        original["operations"][2]["command_id"]
    );
    refused(
        &f,
        &[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            "downstream",
            "--worker",
            "A",
            "--request-id",
            "too-soon",
        ],
        "dependency",
    );
    begin(&f, "changed", "changed-stage");
    let recheck = command(
        &f,
        "changed",
        "changed-recheck",
        check("changed", "src/", false)["initial"].clone(),
    );
    let stale = report(&f, "changed", 1, &recheck, "r1");
    refused(
        &f,
        &["operations", "repair-finish", "--report", &stale],
        "stale check input revision",
    );
    finish(&f, "changed", 1, &recheck, "r2");
    begin(&f, "downstream", "dependent-stage");
    let recheck = command(
        &f,
        "downstream",
        "dependent-recheck",
        check("downstream", "generated/", false)["initial"].clone(),
    );
    finish(&f, "downstream", 1, &recheck, "r2");
    shutdown(&f, &mut server);
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    let after = f.snapshot();
    assert_eq!(
        after["operations"][2]["repair"]["commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        after["operations"][2]["command_id"],
        original["operations"][2]["command_id"]
    );
}

#[test]
fn ambiguous_declared_inputs_are_rejected_before_runtime_or_selective_invalidation() {
    for input in [
        "src/../tests",
        "../src",
        "/src",
        "C:\\src",
        "src/**",
        " src",
    ] {
        let f = Fixture::new();
        let op = check("check", input, false);
        let path = f.root.join("invalid-plan.json");
        fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[task("A",&[],"completed","done")],"operations":[op]})).unwrap()).unwrap();
        refused(
            &f,
            &["validate", path.to_str().unwrap()],
            "checkout-relative paths",
        );
        assert!(!f.data.exists());
    }
    let f = Fixture::new();
    import(&f, json!([check("check", "src/", false)]));
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    let mut server = serve(&f);
    refused(
        &f,
        &[
            "operations",
            "inputs-changed",
            "--run",
            "demo",
            "--request-id",
            "traversal",
            "--revision",
            "r2",
            "--input",
            "src/../tests",
            "--reason",
            "changed",
        ],
        "checkout-relative paths",
    );
    assert_eq!(f.snapshot()["operations"][0]["state"], "passed");
    shutdown(&f, &mut server);
}
