mod support;
use serde_json::{Value, json};
use std::fs;
use support::{Fixture, task};

fn run(f: &Fixture) -> Value {
    let output = f.call(&["run", "--run", "demo"]);
    assert!(
        matches!(output.status.code(), Some(0 | 3)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    f.snapshot()
}
fn db(f: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        f.data.join("state.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

#[test]
fn initial_and_two_work_retries_survive_restart_preserving_files() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    fs::write(f.root.join("preserved.txt"), "existing edits").unwrap();
    let status = run(&f);
    assert_eq!(status["tasks"][0]["state"], "retry_exhausted");
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(
        run(&f)["tasks"][0]["attempts"],
        status["tasks"][0]["attempts"]
    );
    assert_eq!(
        fs::read_to_string(f.root.join("preserved.txt")).unwrap(),
        "existing edits"
    );
    let ledger = jtasks::controller::recovery::ledger(&db(&f), "demo", "A").unwrap();
    assert_eq!(
        (ledger.initial, ledger.work, ledger.infrastructure),
        (1, 2, 0)
    );
}

const NATIVE: &str = r#"
import json,sys,os
from pathlib import Path
mode=sys.argv[1]
args=sys.argv[2:]
sys.stdin.read()
path=Path('calls.json')
calls=json.loads(path.read_text()) if path.exists() else []
calls.append(args)
path.write_text(json.dumps(calls))
print(json.dumps({'type':'thread.started','thread_id':'exact-task-thread'}))
print(json.dumps({'type':'turn.started'}))
if mode=='transient' and len(calls)<=5:
    print(json.dumps({'type':'turn.failed','error':{'message':'connection lost'}}))
elif mode=='unknown' or (mode=='work-once' and len(calls)==1):
    print(json.dumps({'type':'turn.failed','error':{'message':'unclassified evidence'}}))
else:
    schema=json.loads(Path(args[args.index('--output-schema')+1]).read_text())
    result={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
    result.update(outcome='completed',summary='exact task session completed')
    text=json.dumps(result)
    print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':text}}))
    Path(args[args.index('--output-last-message')+1]).write_text(text)
    print(json.dumps({'type':'turn.completed'}))
"#;
fn native(mode: &str) -> Fixture {
    let f = Fixture::new();
    let script = f.root.join("native.py");
    fs::write(&script, NATIVE).unwrap();
    f.import(json!([{"id":"A","title":"Synthetic native recovery","unknown_capabilities":{"actor":"recovery-test","reason":"Synthetic protocol emitter; no native catalog or inference account"},"executor":{"kind":"codex","executable":"python","launcher_args":[script,mode]}}]));
    f
}
#[test]
fn five_infrastructure_retries_resume_only_exact_task_session() {
    let f = native("transient");
    let mut controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for invocation in 1..=5 {
        f.await_status(|v| {
            v["tasks"][0]["attempts"].as_array().unwrap().len() == invocation
                && v["tasks"][0]["cooldown"]["latest"]["state"] == "waiting"
        });
        assert!(
            f.call(&[
                "retry",
                "A",
                "--run",
                "demo",
                "--reason",
                "simulated availability changed"
            ])
            .status
            .success()
        );
    }
    assert!(controller.wait().unwrap().success());
    assert_eq!(f.snapshot()["complete"], true);
    let calls: Vec<Vec<String>> =
        serde_json::from_slice(&fs::read(f.root.join("calls.json")).unwrap()).unwrap();
    assert_eq!(calls.len(), 6);
    assert!(!calls[0].iter().any(|v| v == "resume"));
    for args in &calls[1..] {
        assert!(
            args.windows(2)
                .any(|pair| pair == ["resume", "exact-task-thread"]),
            "{args:?}"
        );
    }
    let ledger = jtasks::controller::recovery::ledger(&db(&f), "demo", "A").unwrap();
    assert_eq!((ledger.work, ledger.infrastructure), (0, 5));
    let status = run(&f);
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 6);
}
#[test]
fn unknown_failure_uses_bounded_fresh_context_instead_of_infrastructure() {
    let f = native("unknown");
    assert_eq!(run(&f)["tasks"][0]["state"], "retry_exhausted");
    let calls: Vec<Vec<String>> =
        serde_json::from_slice(&fs::read(f.root.join("calls.json")).unwrap()).unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|args| !args.iter().any(|v| v == "resume")));
    let ledger = jtasks::controller::recovery::ledger(&db(&f), "demo", "A").unwrap();
    assert_eq!((ledger.work, ledger.infrastructure), (2, 0));
}
#[test]
fn task_policy_can_disable_automatic_retries() {
    let f = Fixture::new();
    let mut task = task("A", &[], "failed", "unfinished");
    task["recovery"] = json!({"work_retries":0,"infrastructure_retries":0});
    f.import(json!([task]));
    let status = run(&f);
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(status["tasks"][0]["state"], "retry_exhausted");
}
#[test]
fn definite_prelaunch_failure_records_no_actual_invocation() {
    let f = Fixture::new();
    f.import(json!([{"id":"A","title":"Definite prelaunch","unknown_capabilities":{"actor":"recovery-test","reason":"Intentionally missing synthetic executable"},"executor":{"kind":"codex","executable":"missing-jtasks-synthetic-program"}}]));
    let output = f.call(&["run", "--run", "demo"]);
    assert_eq!(output.status.code(), Some(3));
    let conn = db(&f);
    let state: String = conn
        .query_row("SELECT state FROM recovery_invocations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "prelaunch_failed");
    assert_eq!(
        jtasks::controller::recovery::ledger(&conn, "demo", "A")
            .unwrap()
            .initial,
        0
    );
    assert_eq!(f.snapshot()["tasks"][0]["state"], "needs_intervention");
}

#[test]
fn definite_task_prelaunch_failure_preserves_independent_active_worker() {
    let f = Fixture::new();
    let mut worker = task("B", &[], "completed", "independent active worker completed");
    worker["executor"]["delay_ms"] = json!(250);
    let path = f.root.join("plan.json");
    fs::write(&path, serde_json::to_vec(&json!({
        "schema_version":1,"completion":"judgment","operation_placement":"none",
        "scheduling":{"task_capacity":2},"tasks":[worker,
        {"id":"A","title":"Task-local prelaunch failure","unknown_capabilities":{"actor":"recovery-test","reason":"Intentionally missing synthetic executable"},"executor":{"kind":"codex","executable":"missing-jtasks-synthetic-program"}}]
    })).unwrap()).unwrap();
    assert!(
        f.call(&["import", path.to_str().unwrap(), "--run", "demo"])
            .status
            .success()
    );
    let status = run(&f);
    assert_eq!(status["tasks"][0]["state"], "completed");
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(status["tasks"][0]["attempts"][0]["exit_code"], 0);
    assert_eq!(status["tasks"][1]["state"], "needs_intervention");
    assert_eq!(status["tasks"][1]["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        run(&f)["tasks"][0]["attempts"],
        status["tasks"][0]["attempts"]
    );
}

fn synthetic() -> (Fixture, jtasks::storage::Workspace, jtasks::storage::Store) {
    let f = Fixture::new();
    let workspace = jtasks::storage::Workspace {
        checkout: f.root.clone(),
        git_dir: f.root.join(".git"),
        data: f.data.clone(),
    };
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(jtasks::storage::schema::SQL).unwrap();
    jtasks::controller::recovery::initialize(&conn).unwrap();
    let mut store = jtasks::storage::Store { conn };
    let plan=serde_json::from_value(json!({"schema_version":1,"completion":"judgment","operation_placement":"none","tasks":[task("A",&[],"silent","")]})).unwrap();
    store.import(&workspace, "demo", &plan).unwrap();
    (f, workspace, store)
}
#[test]
fn uncertain_launch_retains_reservation_and_blocks_duplicate_admission() {
    let (_f, workspace, mut store) = synthetic();
    store.intent("demo", "A", "uncertain").unwrap();
    let dir = workspace.data.join("attempts/uncertain");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("custody.json"),
        serde_json::to_vec(&jtasks::execution::custody::Record {
            platform: std::env::consts::OS.into(),
            scope: format!("jtasks-{}", uuid::Uuid::new_v4()),
            pid: None,
            start: None,
            bound: false,
            empty: false,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(
        !jtasks::controller::recovery::launch_failure(
            &workspace,
            &store,
            "demo",
            "A",
            "uncertain",
            "raw launch uncertainty"
        )
        .unwrap()
    );
    assert!(store.intent("demo", "A", "duplicate").is_err());
    assert_eq!(
        jtasks::controller::recovery::ledger(&store.conn, "demo", "A")
            .unwrap()
            .initial,
        1
    );
    let state: String = store
        .conn
        .query_row("SELECT state FROM recovery_invocations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "uncertain");
}
#[test]
fn operation_stage_events_share_work_allowance_across_operation_identities() {
    let f = Fixture::new();
    let script = f.root.join("native.py");
    fs::write(&script, NATIVE).unwrap();
    let operation = |id: &str| json!({"id":id,"stage":{"kind":"final","tasks":["A"]},"required":true,"revision":"r1","inputs":["src/"],"repair_task":"A","initial":{"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent","--exit-code=7"],"cwd":"."}});
    let path = f.root.join("plan.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[{"id":"A","title":"One worker repair then checks","unknown_capabilities":{"actor":"recovery-test","reason":"Synthetic protocol emitter"},"executor":{"kind":"codex","executable":"python","launcher_args":[script,"work-once"]}}],"operations":[operation("original-check"),operation("new-check-identity")]})).unwrap()).unwrap();
    assert!(
        f.call(&["import", path.to_str().unwrap(), "--run", "demo"])
            .status
            .success()
    );
    let status = run(&f);
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 2);
    assert!(
        status["operations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|op| op["state"] == "failed")
    );
    assert_eq!(
        jtasks::controller::recovery::ledger(&db(&f), "demo", "A")
            .unwrap()
            .work,
        1
    );
    let mut server = command_server(&f);
    let begin = |operation: &str, request: &str| {
        f.call(&[
            "operations",
            "repair-begin",
            "--run",
            "demo",
            "--operation",
            operation,
            "--worker",
            "A",
            "--request-id",
            request,
        ])
    };
    assert!(begin("original-check", "first-stage").status.success());
    // Replaying the request and any number of commands stay within this stage.
    assert!(begin("original-check", "first-stage").status.success());
    for request in ["first-command", "second-command"] {
        let command = f.root.join("command.json");
        fs::write(&command,serde_json::to_vec(&json!({"request_id":request,"run":"demo","operation":"original-check","program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent"],"cwd":f.root})).unwrap()).unwrap();
        let receipt = f.call(&["exec", "start", "--request", command.to_str().unwrap()]);
        assert!(
            receipt.status.success(),
            "{}",
            String::from_utf8_lossy(&receipt.stderr)
        );
        let receipt: Value = serde_json::from_slice(&receipt.stdout).unwrap();
        assert!(
            f.call(&[
                "exec",
                "wait",
                receipt["command_id"].as_str().unwrap(),
                "--wait-seconds",
                "10"
            ])
            .status
            .success()
        );
    }
    let refused = begin("new-check-identity", "another-operation");
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("task lineage work allowance exhausted")
    );
    assert_eq!(
        jtasks::controller::recovery::ledger(&db(&f), "demo", "A")
            .unwrap()
            .work,
        2
    );
    assert!(f.call(&["exec", "shutdown"]).status.success());
    assert!(server.wait().unwrap().success());
    // A new controller and operation request still use the persisted common ledger.
    let mut server = command_server(&f);
    assert!(
        !begin("new-check-identity", "after-restart")
            .status
            .success()
    );
    let status = f.snapshot();
    assert_eq!(status["operations"][0]["repair"]["work_stages_charged"], 1);
    assert_eq!(status["operations"][1]["repair"]["work_stages_charged"], 0);
    assert_eq!(
        status["operations"][0]["repair"]["allowance_scope"],
        "task_lineage"
    );
    assert_eq!(
        status["operations"][0]["repair"]["shared_ledger"]["work"],
        2
    );
    assert!(f.call(&["exec", "shutdown"]).status.success());
    assert!(server.wait().unwrap().success());
}

fn command_server(f: &Fixture) -> std::process::Child {
    let previous = fs::read(f.data.join("commands-control.json")).ok();
    let child = f
        .command()
        .args(["exec", "serve"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        let current = fs::read(f.data.join("commands-control.json")).ok();
        if current.is_some() && current != previous {
            return child;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "command server readiness deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn reading_preledger_schema_three_reports_unavailable_without_initializing() {
    let f = Fixture::new();
    fs::create_dir_all(&f.data).unwrap();
    // Represent the supported schema before writer-only ledger initialization.
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    conn.execute_batch(jtasks::storage::schema::SQL).unwrap();
    let task = task("A", &[], "completed", "candidate");
    let operation = json!({"id":"check","stage":{"kind":"task","task":"A"},"required":true,"revision":"r1","inputs":["src/"],"initial":{"program":"synthetic-unlaunched-check","cwd":"."}});
    let plan = json!({"schema_version":1,"completion":"judgment","operation_placement":"task","tasks":[task.clone()],"operations":[operation.clone()]});
    conn.execute(
        "INSERT INTO runs(id,checkout,plan,state) VALUES('demo',?1,?2,'ready')",
        rusqlite::params![f.root.to_string_lossy(), plan.to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks(run_id,id,ordinal,spec,state) VALUES('demo','A',0,?1,'pending')",
        [task.to_string()],
    )
    .unwrap();
    conn.execute("INSERT INTO operations(run_id,id,ordinal,spec,required,revision,state,request_id) VALUES('demo','check',0,?1,1,'r1','pending','original-operation')", [operation.to_string()]).unwrap();
    let before = conn.total_changes();
    conn.pragma_update(None, "query_only", true).unwrap();
    let status = f.snapshot();
    let repair = &status["operations"][0]["repair"];
    assert_eq!(repair["ledger_state"], "not_initialized");
    assert_eq!(repair["shared_ledger"], Value::Null);
    assert_eq!(repair["limit"], Value::Null);
    assert_eq!(conn.total_changes(), before);
    let tables: u32 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('recovery_lineages','recovery_invocations')", [], |r|r.get(0)).unwrap();
    assert_eq!(tables, 0);
}
