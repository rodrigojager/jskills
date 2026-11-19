mod support;
use serde_json::{Value, json};
use std::{
    fs,
    process::Stdio,
    time::{Duration, Instant},
};
use support::{Fixture, task};

fn write(f: &Fixture, name: &str, value: &Value) -> String {
    let path = f.root.join(name);
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path.to_str().unwrap().into()
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
fn plan(tasks: Value) -> Value {
    json!({"schema_version":1,"completion":"judgment","operation_placement":"group",
        "requirements":[{"id":"R","text":"Preserve original batch requirement"}],"tasks":tasks,
        "operations":[{"id":"check","stage":{"kind":"group","tasks":["A"]},"required":true,"revision":"r1","inputs":["ready.flag"],"repair_task":"A",
            "initial":{"program":"python","args":["-c","from pathlib import Path; import sys; sys.exit(0 if Path('ready.flag').exists() else 7)"],"cwd":"."}}]})
}
fn mapped(id: &str) -> Value {
    let mut t = task(id, &[], "completed", "Designated batch result");
    t["requirements"] = json!(["R"]);
    t["admission"] = json!({"resources":["batch/shared"]});
    t
}

/// One public workflow joins graph lineage, stopped infrastructure recovery,
/// reservations, independent work, managed repairs and the canonical allowance.
#[test]
fn split_infrastructure_session_and_operation_repair_share_one_lineage() {
    let f = Fixture::new();
    let p = plan(json!([mapped("A")]));
    let path = write(&f, "plan.json", &p);
    call(&f, &["import", &path, "--run", "demo"]);
    let script = f.root.join("native.py");
    fs::write(&script, r#"import sys,json,pathlib
args=sys.argv[1:];sys.stdin.read()
p=pathlib.Path('calls.json');calls=json.loads(p.read_text()) if p.exists() else [];calls.append(args);p.write_text(json.dumps(calls))
print(json.dumps({'type':'thread.started','thread_id':'split-B-session'}),flush=True)
print(json.dumps({'type':'turn.started'}),flush=True)
if len(calls)==1:
 print(json.dumps({'type':'turn.failed','error':{'message':'connection lost'}}),flush=True)
else:
 schema=json.loads(pathlib.Path(args[args.index('--output-schema')+1]).read_text());result={k:v['const'] for k,v in schema['properties'].items() if 'const' in v};result.update(outcome='completed',summary='Same split task resumed')
 text=json.dumps(result);pathlib.Path(args[args.index('--output-last-message')+1]).write_text(text)
 print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':text}}),flush=True);print(json.dumps({'type':'turn.completed'}),flush=True)
"#).unwrap();
    let mut b = mapped("B");
    b["executor"] = json!({"kind":"codex","executable":"python","launcher_args":[script]});
    b["unknown_capabilities"] =
        json!({"actor":"batch-acceptance","reason":"Synthetic native protocol, no provider calls"});
    let mut revised = plan(json!([
        b,
        mapped("C"),
        task("D", &[], "completed", "Independent work")
    ]));
    revised["operations"][0]["stage"]["tasks"] = json!(["B", "C"]);
    revised["operations"][0]["repair_task"] = json!("B");
    // D is explicit additional scope, rather than silently assigning ancestry.
    revised["requirements"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"D","text":"Independent batch work"}));
    revised["tasks"][2]["requirements"] = json!(["D"]);
    let request = json!({"request_id":"split","base_revision":0,"reason":"Split while preserving checks","plan":revised,
        "ancestry":{"B":"A","C":"A"},"scope_additions":{"D":"Independent work requested by acceptance fixture"}});
    let path = write(&f, "proposal.json", &request);
    call(&f, &["graph", "propose", "--run", "demo", "--file", &path]);
    let mut controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    f.await_status(|s| s["tasks"][1]["cooldown"]["latest"]["state"] == "waiting");
    call(
        &f,
        &[
            "retry",
            "B",
            "--run",
            "demo",
            "--reason",
            "Fixture connection restored",
        ],
    );
    assert_eq!(controller.wait().unwrap().code(), Some(3));
    let before = f.snapshot();
    assert_eq!(before["tasks"][0]["state"], "superseded");
    assert_eq!(before["tasks"][1]["state"], "completed");
    assert_eq!(before["tasks"][2]["state"], "completed");
    assert_eq!(before["tasks"][3]["state"], "completed");
    assert_eq!(before["operations"][0]["state"], "failed");
    let calls: Vec<Vec<String>> =
        serde_json::from_slice(&fs::read(f.root.join("calls.json")).unwrap()).unwrap();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1]
            .windows(2)
            .any(|w| w == ["resume", "split-B-session"])
    );
    for attempt in before["tasks"][1]["attempts"].as_array().unwrap() {
        let custody: Value = serde_json::from_slice(
            &fs::read(
                f.data
                    .join("attempts")
                    .join(attempt["id"].as_str().unwrap())
                    .join("custody.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(custody["empty"], true);
    }
    let mut server = f
        .command()
        .args(["exec", "serve"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if f.call(&[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            "check",
            "--worker",
            "B",
            "--request-id",
            "repair",
        ])
        .status
        .success()
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let repair = f.snapshot();
    assert_eq!(
        repair["operations"][0]["repair"]["shared_ledger"]["infrastructure"],
        1
    );
    assert_eq!(
        repair["operations"][0]["repair"]["shared_ledger"]["work"],
        2
    );
    let mut command = json!({"request_id":"correction","run":"demo","operation":"check","cwd":f.root,
        "program":"python","args":["-c","from pathlib import Path; Path('ready.flag').write_text('corrected')"]});
    let path = write(&f, "command.json", &command);
    let receipt = call(&f, &["exec", "start", "--request", &path]);
    call(
        &f,
        &[
            "exec",
            "wait",
            receipt["command_id"].as_str().unwrap(),
            "--wait-seconds",
            "10",
        ],
    );
    command["request_id"] = json!("recheck");
    command["args"] = p["operations"][0]["initial"]["args"].clone();
    let path = write(&f, "command.json", &command);
    let receipt = call(&f, &["exec", "start", "--request", &path]);
    let terminal = call(
        &f,
        &[
            "exec",
            "wait",
            receipt["command_id"].as_str().unwrap(),
            "--wait-seconds",
            "10",
        ],
    );
    assert_eq!(terminal["exit_code"], 0);
    let path = write(
        &f,
        "report.json",
        &json!({"run":"demo","operation":"check","cycle":1,"worker":"B","command_id":receipt["command_id"],"revision":f.snapshot()["operations"][0]["revision"],"summary":"Exact admitted check passed after correction"}),
    );
    call(&f, &["operations", "repair-finish", "--report", &path]);
    call(&f, &["exec", "shutdown"]);
    assert!(server.wait().unwrap().success());
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    let status = f.snapshot();
    assert_eq!(status["complete"], true);
    assert_eq!(status["graph"]["revision"], 1);
    assert!(
        status["graph"]["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["satisfied"] == true)
    );
}
