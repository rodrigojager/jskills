mod support;
use jtasks::{
    controller::questions,
    domain::{
        autonomy::{self, Answer, Decision, Question},
        profiles::EffectiveProfile,
        types::{Outcome, Receipt, Task},
    },
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use serde_json::{Value, json};
use std::fs;
use support::{Fixture, task};

fn workspace(f: &Fixture) -> Workspace {
    Workspace::resolve(&f.root, Some(&f.data)).unwrap()
}
fn question() -> Question {
    Question {
        request_id: "q1".into(),
        question: "Which documented endpoint is authorized?".into(),
        blocked_requirement: "External endpoint is unspecified".into(),
        options: vec!["staging".into(), "production".into()],
    }
}
fn decision() -> Decision {
    Decision {
        request_id: "d1".into(),
        situation: "Two equivalent local routines".into(),
        decision: "Use the existing implementation".into(),
        deviation: "none".into(),
    }
}
fn snapshot(mode: &str) -> EffectiveProfile {
    serde_json::from_value(json!({"alias":null,"values":{"autonomy":mode},"origins":{},"executor_version":null,"catalog_reference":null,"discovery":"fixture"})).unwrap()
}

#[test]
fn run_restriction_and_native_permissions_survive_resume_and_profile_changes() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "x")]));
    let w = workspace(&f);
    let guard = ControllerGuard::acquire(&w.git_dir).unwrap();
    let mut store = Store::open(&w, &guard).unwrap();
    autonomy::launch(&store.conn, "demo", true).unwrap();
    autonomy::launch(&store.conn, "demo", false).unwrap();
    for kind in ["codex", "claude", "opencode"] {
        let mut native:Task=serde_json::from_value(json!({"id":"A","title":"native permissions","executor":{"kind":kind,"provider":"fixture","model":"fixture","permissions":"autonomous"}})).unwrap();
        let mut profile = snapshot("autonomous");
        autonomy::effective(&store.conn, "demo", &mut native, &mut profile).unwrap();
        assert_eq!(
            serde_json::to_value(native.executor).unwrap()["permissions"],
            "restricted"
        );
        assert_eq!(profile.values["autonomy"], "restricted");
        assert_eq!(profile.origins["autonomy"], "persisted_run_restricted");
    }
    let native: Task = serde_json::from_value(task("A", &[], "silent", "")).unwrap();
    let effective = store.prepare_attempt(&w, "demo", &native, "a1").unwrap();
    store.launched("a1", 42).unwrap();
    let profile: String = store
        .conn
        .query_row(
            "SELECT effective_profile FROM attempts WHERE id='a1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&profile).unwrap()["values"]["autonomy"],
        "restricted"
    );
    let ticket = jtasks::domain::tickets::write_attempt_ticket(&w, &effective, "a1").unwrap();
    assert!(
        fs::read_to_string(ticket)
            .unwrap()
            .contains("designated blocked result")
    );
    // A run without the explicit restriction still freezes an explicit task mode.
    store.conn.execute("INSERT INTO runs(id,checkout,plan,state) SELECT 'other',checkout,plan,'ready' FROM runs WHERE id='demo'",[]).unwrap();
    autonomy::launch(&store.conn, "other", false).unwrap();
    assert!(autonomy::launch(&store.conn, "other", true).is_err());
}

#[test]
fn structured_questions_require_restriction_and_never_synthesize_outcomes() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    let w = workspace(&f);
    let guard = ControllerGuard::acquire(&w.git_dir).unwrap();
    let mut store = Store::open(&w, &guard).unwrap();
    let task: Task = serde_json::from_value(task("A", &[], "silent", "")).unwrap();
    store.prepare_attempt(&w, "demo", &task, "a1").unwrap();
    store.launched("a1", 42).unwrap();
    assert!(questions::ask(&w, &store, "demo", "A", "a1", &question()).is_err());
    assert!(questions::decide(&w, &store, "demo", "wrong", "a1", &decision()).is_err());
    assert!(!questions::decide(&w, &store, "demo", "A", "a1", &decision()).unwrap());
    assert!(questions::decide(&w, &store, "demo", "A", "a1", &decision()).unwrap());
    let mut conflict = decision();
    conflict.decision = "conflict".into();
    assert!(questions::decide(&w, &store, "demo", "A", "a1", &conflict).is_err());
    assert!(
        questions::questions(&store.conn, "demo")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let outcome: Option<String> = store
        .conn
        .query_row("SELECT outcome FROM attempts WHERE id='a1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(outcome.is_none());
    store.retire("demo", "A", "a1", Some(0), false).unwrap();
    assert_eq!(f.snapshot()["tasks"][0]["state"], "needs_intervention");
}

#[test]
fn journal_recovery_answers_and_reads_preserve_authority() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    let w = workspace(&f);
    let guard = ControllerGuard::acquire(&w.git_dir).unwrap();
    let mut store = Store::open(&w, &guard).unwrap();
    autonomy::launch(&store.conn, "demo", true).unwrap();
    let task: Task = serde_json::from_value(task("A", &[], "silent", "")).unwrap();
    store.prepare_attempt(&w, "demo", &task, "a1").unwrap();
    store.launched("a1", 42).unwrap();
    // Force projection failure after the authoritative transaction commits.
    fs::write(w.data.join("runs"), "projection unavailable").unwrap();
    assert!(questions::ask(&w, &store, "demo", "A", "a1", &question()).is_err());
    assert_eq!(
        questions::read(&store.conn, "demo")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Keep the obstruction as a fixture artifact; point runtime projection to a new data root.
    let mut projection = w.clone();
    projection.data = w.data.join("recovered");
    questions::project(&projection, &store.conn, "demo").unwrap();
    let path = projection.data.join("runs/demo/decisions.md");
    let first = fs::read(&path).unwrap();
    questions::project(&projection, &store.conn, "demo").unwrap();
    assert_eq!(fs::read(&path).unwrap(), first);
    assert!(questions::ask(&projection, &store, "demo", "A", "a1", &question()).unwrap());
    let answer = Answer {
        request_id: "answer1".into(),
        question_id: "q1".into(),
        actor: "operator".into(),
        text: "Use staging".into(),
    };
    assert!(!questions::answer(&projection, &store, "demo", &answer).unwrap());
    assert!(questions::answer(&projection, &store, "demo", &answer).unwrap());
    let mut conflict = answer.clone();
    conflict.text = "production".into();
    assert!(questions::answer(&projection, &store, "demo", &conflict).is_err());
    assert_eq!(
        questions::questions(&store.conn, "demo").unwrap()[0]["pending"],
        false
    );
    let before = fs::read(w.database()).unwrap();
    let projected = fs::read(&path).unwrap();
    for args in [
        vec!["questions", "--run", "demo"],
        vec!["decisions", "list", "--run", "demo"],
        vec!["status", "--run", "demo"],
    ] {
        let result = f.call(&args);
        assert!(
            matches!(result.status.code(), Some(0 | 3)),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(fs::read(w.database()).unwrap(), before);
    assert_eq!(fs::read(path).unwrap(), projected);
    store
        .accept(
            "demo",
            "a1",
            &Receipt {
                receipt_id: "blocked1".into(),
                attempt_id: "a1".into(),
                task_id: "A".into(),
                outcome: Outcome::Blocked,
                summary: "Need management answer".into(),
            },
        )
        .unwrap();
    store.retire("demo", "A", "a1", Some(0), false).unwrap();
    assert_eq!(f.snapshot()["tasks"][0]["state"], "blocked");
    let ticket = jtasks::domain::tickets::write_attempt_ticket(&w, &task, "a1").unwrap();
    assert!(fs::read_to_string(ticket).unwrap().contains("Use staging"));
}

#[test]
fn public_restricted_launch_persists_without_changing_default_resume() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "done")]));
    let result = f.call(&["run", "--run", "demo", "--restricted"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let before = f.snapshot()["tasks"][0]["attempts"][0]["effective_profile"].clone();
    assert_eq!(before["values"]["autonomy"], "restricted");
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"][0]["effective_profile"],
        before
    );
}

#[test]
fn public_worker_control_question_answer_block_retire_and_retry_roundtrip() {
    let f = Fixture::new();
    let script = f.root.join("worker.py");
    fs::write(&script,r#"
import json, os, subprocess, sys
from pathlib import Path
sys.stdin.read()
binary=os.environ['JTASKS_BIN']
ticket=Path(os.environ['JTASKS_TICKET_PATH']).read_text()
def client(*args):
    result=subprocess.run([binary,*args],capture_output=True,text=True)
    assert result.returncode==0, result.stderr
    return json.loads(result.stdout)
if 'Use staging' in ticket:
    assert 'Effective autonomy: restricted' in ticket
    client('result','--receipt','completed-after-answer','--outcome','completed','--summary','Used staging from the durable management answer')
else:
    client('decisions','record','--request-id','routine1','--situation','Equivalent local options','--decision','Reuse existing code','--deviation','none')
    client('ask','--request-id','q1','--question','Which endpoint?','--blocked-requirement','Endpoint unspecified','--option','staging')
    Path('question-ready').write_text('ready')
    import time
    deadline=time.monotonic()+10
    while not Path('answer-recorded').exists():
        assert time.monotonic()<deadline, 'management answer deadline'
        time.sleep(.02)
    client('result','--receipt','blocked1','--outcome','blocked','--summary','Question requires bounded explicit retry after retirement')
print(json.dumps({'type':'thread.started','thread_id':'question-session'}),flush=True)
print(json.dumps({'type':'turn.completed','usage':{}}),flush=True)
"#).unwrap();
    f.import(json!([{"id":"A","title":"Scoped restricted question","unknown_capabilities":{"actor":"test","reason":"Deterministic Python executor substitute"},"executor":{"kind":"codex","executable":"python","launcher_args":[script],"model":"fixture","provider":"fixture"}}]));
    let mut controller = f
        .command()
        .args(["run", "--run", "demo", "--restricted"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !f.root.join("question-ready").is_file() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "worker question deadline; {:?}",
            controller.try_wait().unwrap()
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pending = f.call(&["questions", "--run", "demo"]);
    assert!(pending.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&pending.stdout).unwrap()["records"][0]["pending"],
        true
    );
    // The management endpoint is live. A worker token must fail this action.
    let management: jtasks::transport::ControlEndpoint =
        serde_json::from_slice(&fs::read(f.data.join("control.json")).unwrap()).unwrap();
    assert!(
        jtasks::transport::send(
            management.endpoint,
            &jtasks::transport::Request::Answer {
                token: "wrong-worker-token".into(),
                run: "demo".into(),
                answer: Answer {
                    request_id: "bad-answer".into(),
                    question_id: "q1".into(),
                    actor: "worker".into(),
                    text: "Not authorized".into()
                }
            }
        )
        .is_err()
    );
    let answered = f.call(&[
        "answer",
        "q1",
        "--run",
        "demo",
        "--request-id",
        "answer1",
        "--actor",
        "operator",
        "--text",
        "Use staging",
    ]);
    assert!(
        answered.status.success(),
        "{}",
        String::from_utf8_lossy(&answered.stderr)
    );
    fs::write(f.root.join("answer-recorded"), "ready").unwrap();
    assert_eq!(controller.wait().unwrap().code(), Some(3));
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "blocked");
    assert_eq!(
        status["tasks"][0]["attempts"][0]["process_state"],
        "retired"
    );
    let retried = f.call(&[
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        "Management question answered",
        "--request-id",
        "retry-answer1",
    ]);
    assert!(
        retried.status.success(),
        "{}",
        String::from_utf8_lossy(&retried.stderr)
    );
    let resumed = f.call(&["run", "--run", "demo"]);
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(status["complete"], true);
    let records = f.call(&["decisions", "list", "--run", "demo"]);
    let records: Value = serde_json::from_slice(&records.stdout).unwrap();
    assert_eq!(records["records"].as_array().unwrap().len(), 3);
}
