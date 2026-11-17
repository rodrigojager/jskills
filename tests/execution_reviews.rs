mod support;
use jtasks::{
    controller::{
        activity, clock,
        due::{self, Kind, Work},
        reviews,
    },
    domain::reviews::{Assessment, Decision, Policy},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

fn memory(estimate: Option<u64>) -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(jtasks::storage::schema::SQL).unwrap();
    activity::initialize(&c).unwrap();
    clock::initialize(&c).unwrap();
    due::initialize(&c).unwrap();
    reviews::initialize(&c).unwrap();
    c.execute("INSERT INTO runs VALUES('r','','{}','running')", [])
        .unwrap();
    let mut spec = task("A", &[], "silent", "");
    spec["estimate_seconds"] = json!(estimate);
    c.execute(
        "INSERT INTO tasks VALUES('r','A',0,?1,'running')",
        [spec.to_string()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO attempts(id,run_id,task_id,state) VALUES('a','r','A','running')",
        [],
    )
    .unwrap();
    reviews::begin(&c, "a", 100).unwrap();
    c
}
fn work(generation: u32) -> Work {
    Work {
        id: "review-a".into(),
        run: "r".into(),
        task: Some("A".into()),
        attempt: Some("a".into()),
        kind: Kind::Review,
        generation,
        due_ms: 3100,
    }
}
fn call(c: &Connection, id: &str, generation: u32) {
    c.execute("INSERT INTO reviewer_calls(id,run_id,task_id,attempt_id,generation,state,invoked,evidence,deadline_ms) VALUES(?1,'r','A','a',?2,'running',1,'{}',10000)",params![id,generation]).unwrap();
}
fn assessment(id: &str, generation: u32, decision: Decision) -> Assessment {
    Assessment {
        run: "r".into(),
        task: "A".into(),
        attempt: "a".into(),
        review: id.into(),
        generation,
        decision,
        rationale: "Current bounded command evidence supports this decision".into(),
        evidence: vec!["commands".into()],
        delay_cause: "The selected operation is still producing new bounded evidence".into(),
        next_review_after_seconds: Some(1),
    }
}
#[test]
fn optional_estimate_first_review_and_finite_validation() {
    let c = memory(Some(2));
    let due: i64 = c
        .query_row("SELECT due_ms FROM due_work", [], |r| r.get(0))
        .unwrap();
    assert_eq!(due, 3100);
    reviews::begin(&c, "a", 999999).unwrap();
    assert_eq!(
        c.query_row("SELECT due_ms FROM due_work", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3100
    );
    let c = memory(None);
    assert_eq!(
        reviews::projection(&c, "a").unwrap()["estimate_trigger"],
        "unconfigured"
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM due_work", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
    let mut a = assessment("x", 1, Decision::Continue);
    for interval in [None, Some(0), Some(3601)] {
        a.next_review_after_seconds = interval;
        assert!(a.validate(&Policy::default()).is_err());
    }
    a.next_review_after_seconds = Some(1);
    a.evidence = vec!["process".into()];
    assert!(a.validate(&Policy::default()).is_err());
}
#[test]
fn actual_failures_share_cap_one_infrastructure_retry_and_original_deadline() {
    let c = memory(Some(2));
    c.execute("UPDATE due_work SET state='admitted'", [])
        .unwrap();
    call(&c, "one", 1);
    reviews::finish_call(
        &c,
        &work(1),
        "one",
        None,
        true,
        10000,
        4000,
        "transient connection",
    )
    .unwrap();
    assert_eq!(reviews::projection(&c, "a").unwrap()["actual_calls"], 1);
    assert_eq!(
        reviews::projection(&c, "a").unwrap()["infrastructure_retries"],
        1
    );
    assert_eq!(
        c.query_row("SELECT state FROM due_work", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "queued"
    );
    call(&c, "two", 1);
    reviews::finish_call(
        &c,
        &work(1),
        "two",
        Some(&assessment("two", 1, Decision::Continue)),
        false,
        10000,
        5000,
        "valid",
    )
    .unwrap();
    assert_eq!(
        c.query_row("SELECT due_ms FROM due_work", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        6000
    );
    call(&c, "three", 2);
    reviews::finish_call(
        &c,
        &work(2),
        "three",
        Some(&assessment("three", 2, Decision::Continue)),
        false,
        20000,
        7000,
        "final extension",
    )
    .unwrap();
    assert_eq!(reviews::projection(&c, "a").unwrap()["action"], "pause");
    assert_eq!(reviews::projection(&c, "a").unwrap()["actual_calls"], 3);
    let c = memory(Some(2));
    call(&c, "one", 1);
    reviews::finish_call(&c, &work(1), "one", None, true, 10000, 4000, "transient").unwrap();
    call(&c, "two", 1);
    reviews::finish_call(
        &c,
        &work(1),
        "two",
        None,
        true,
        10000,
        5000,
        "transient again",
    )
    .unwrap();
    assert_eq!(reviews::projection(&c, "a").unwrap()["action"], "pause");
    let c = memory(Some(2));
    call(&c, "one", 1);
    reviews::finish_call(&c, &work(1), "one", None, true, 4000, 4000, "deadline").unwrap();
    assert_eq!(
        reviews::projection(&c, "a").unwrap()["infrastructure_retries"],
        0
    );
}
#[test]
fn stale_result_generation_and_wrong_identity_cannot_act() {
    let c = memory(Some(2));
    call(&c, "one", 1);
    c.execute("UPDATE attempts SET outcome='completed' WHERE id='a'", [])
        .unwrap();
    reviews::finish_call(
        &c,
        &work(1),
        "one",
        Some(&assessment("one", 1, Decision::Retry)),
        false,
        10000,
        4000,
        "late",
    )
    .unwrap();
    assert!(reviews::projection(&c, "a").unwrap()["action"].is_null());
    assert_eq!(
        c.query_row("SELECT state FROM reviewer_calls", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "stale"
    );
    let c = memory(Some(2));
    call(&c, "one", 1);
    c.execute("UPDATE execution_reviews SET generation=2", [])
        .unwrap();
    reviews::finish_call(
        &c,
        &work(1),
        "one",
        Some(&assessment("one", 1, Decision::Retry)),
        false,
        10000,
        4000,
        "old generation",
    )
    .unwrap();
    assert!(reviews::projection(&c, "a").unwrap()["action"].is_null());
    let c = memory(Some(2));
    call(&c, "one", 1);
    let mut bad = assessment("one", 1, Decision::Retry);
    bad.attempt = "other".into();
    reviews::finish_call(
        &c,
        &work(1),
        "one",
        Some(&bad),
        false,
        10000,
        4000,
        "wrong binding",
    )
    .unwrap();
    assert_eq!(reviews::projection(&c, "a").unwrap()["action"], "pause");
    assert!(
        c.execute("UPDATE reviewer_calls SET evidence='changed'", [])
            .is_err()
    );
}

#[test]
fn observer_selection_requires_own_unknown_exception_and_rejects_known_invalid() {
    let f = Fixture::new();
    let workspace = jtasks::storage::Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let mut p: Policy =
        serde_json::from_value(json!({"observer":observer(&f,"continue")})).unwrap();
    assert!(
        reviews::validate_observer(&workspace, &p)
            .unwrap_err()
            .to_string()
            .contains("unknown capabilities")
    );
    p.observer_unknown_capabilities = Some(jtasks::executors::catalog::UnknownException {
        actor: "test".into(),
        reason: "synthetic protocol, no account".into(),
    });
    assert_eq!(
        reviews::validate_observer(&workspace, &p).unwrap().state,
        "unknown_allowed"
    );
    let mut value = serde_json::to_value(&p).unwrap();
    value["observer"]["reasoning_effort"] = json!("impossible");
    let p: Policy = serde_json::from_value(value).unwrap();
    assert!(
        reviews::validate_observer(&workspace, &p)
            .unwrap_err()
            .to_string()
            .contains("known-invalid")
    );
}

#[test]
fn startup_reconciles_snapshot_scope_and_keeps_unproven_custody_unknown() {
    let f = Fixture::new();
    let workspace = jtasks::storage::Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let c = memory(Some(2));
    jtasks::controller::recovery::initialize(&c).unwrap();
    call(&c, "snapshot", 1);
    c.execute("UPDATE reviewer_calls SET state='intent',invoked=0", [])
        .unwrap();
    let dir = f.data.join("reviews/snapshot/snapshot");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("custody.json"), "torn record").unwrap();
    let store = jtasks::storage::Store { conn: c };
    reviews::reconcile(&workspace, &store, "r").unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT state FROM reviewer_calls", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "unknown"
    );
    assert_eq!(
        store
            .conn
            .query_row("SELECT next_kind FROM recovery_lineages", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "blocked"
    );
    assert_eq!(
        reviews::projection(&store.conn, "a").unwrap()["actual_calls"],
        0
    );
    // No observer custody record exists; the real snapshot scope still must be stopped.
    let mut command = std::process::Command::new("python");
    command.args(["-c", "import time; time.sleep(30)"]);
    let mut child = jtasks::execution::custody::OwnedProcess::spawn(&mut command, &dir).unwrap();
    reviews::reconcile(&workspace, &store, "r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.poll().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let custody: Value =
        serde_json::from_slice(&fs::read(dir.join("custody.json")).unwrap()).unwrap();
    assert_eq!(custody["empty"], true);
    assert_eq!(
        store
            .conn
            .query_row("SELECT state FROM reviewer_calls", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "failed"
    );
}

// A faithful small native protocol substitute. It never calls an account.
fn observer(f: &Fixture, scenario: &str) -> Value {
    let path = f.root.join("observer.py");
    fs::write(&path,r#"import sys,json,time,pathlib,os
args=sys.argv[1:]
prompt=sys.stdin.read()
schema=json.loads(prompt.split('RESPONSE SCHEMA:\n',1)[1])
p=schema['properties']
result={k:p[k]['const'] for k in ['run','task','attempt','review','generation']}
scenario=args[0]
root=pathlib.Path(__file__).parent
count_path=root/'observer-count'
count=int(count_path.read_text())+1 if count_path.exists() else 1
count_path.write_text(str(count))
(root/'observer-args.json').write_text(json.dumps(args))
(root/'observer-env.json').write_text(json.dumps([k for k in os.environ if k.startswith('JTASKS_')]))
if scenario=='hang':time.sleep(30)
if scenario=='race':time.sleep(2)
print(json.dumps({'type':'thread.started','thread_id':'review-'+result['review']}),flush=True)
print(json.dumps({'type':'turn.started'}),flush=True)
if scenario=='infra' and count<=2:
 print(json.dumps({'type':'turn.failed','error':{'message':'connection overloaded'}}),flush=True)
 sys.exit(1)
result.update(decision='retry' if scenario=='retry' else 'continue',rationale='Current command evidence was compared with the prior review',evidence=['commands','diff'],delay_cause='The declared operation needs a finite additional interval',next_review_after_seconds=1)
if scenario=='malformed':result['attempt']='other'
text=json.dumps(result)
pathlib.Path(args[args.index('--output-last-message')+1]).write_text(text)
print(json.dumps({'type':'item.completed','item':{'id':'final','type':'agent_message','text':text}}),flush=True)
print(json.dumps({'type':'turn.completed','usage':{}}),flush=True)
"#).unwrap();
    json!({"kind":"codex","executable":"python","launcher_args":[path.to_str().unwrap(),scenario]})
}
fn configured(f: &Fixture, scenario: &str, max_calls: u32) -> Value {
    let mut a = task("A", &[], "silent", "");
    a["executor"]["delay_ms"] = json!(30000);
    a["estimate_seconds"] = json!(1);
    a["reviews"] = json!({"max_calls":max_calls,"deadline_seconds":5,"observer_unknown_capabilities":{"actor":"test","reason":"synthetic native protocol substitute, no account"},"observer":observer(f,scenario)});
    a
}
struct Foreground(Child);
impl Drop for Foreground {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn start(f: &Fixture) -> Foreground {
    Foreground(
        f.command()
            .args(["run", "--run", "demo"])
            .env("JTASKS_SHUTDOWN_GRACE_MS", "0")
            .env("JTASKS_SHUTDOWN_FORCE_MS", "2000")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}
fn finish(child: &mut Foreground) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(exit) = child.0.try_wait().unwrap() {
            return exit;
        }
        assert!(Instant::now() < deadline, "foreground deadline");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn db(f: &Fixture) -> Connection {
    Connection::open_with_flags(
        f.data.join("state.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

#[test]
fn actual_foreground_cap_preserves_edits_and_only_pauses_affected_task() {
    let f = Fixture::new();
    let a = configured(&f, "continue", 3);
    f.import(json!([a, task("B", &[], "completed", "independent work")]));
    fs::write(f.root.join("preserved-edit.txt"), "existing edit").unwrap();
    let mut child = start(&f);
    assert_eq!(finish(&mut child).code(), Some(3));
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "needs_intervention");
    assert_eq!(status["tasks"][1]["state"], "completed");
    assert_eq!(
        status["tasks"][0]["attempts"][0]["review"]["actual_calls"],
        3
    );
    assert_eq!(
        fs::read_to_string(f.root.join("preserved-edit.txt")).unwrap(),
        "existing edit"
    );
    let args: Vec<String> =
        serde_json::from_slice(&fs::read(f.root.join("observer-args.json")).unwrap()).unwrap();
    assert!(args.windows(2).any(|w| w == ["--sandbox", "read-only"]));
    assert!(!args.iter().any(|s| s.contains("bypass") || s == "resume"));
    assert_eq!(
        fs::read_to_string(f.root.join("observer-env.json")).unwrap(),
        "[]"
    );
    let c = db(&f);
    assert_eq!(c.query_row("SELECT count(*) FROM reviewer_calls WHERE state IN ('running','intent','stopping','unknown')",[],|r|r.get::<_,u32>(0)).unwrap(),0);
    let path: String = c
        .query_row(
            "SELECT id FROM reviewer_calls ORDER BY rowid LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let prompt = fs::read_to_string(f.data.join("reviews").join(path).join("prompt.txt")).unwrap();
    for evidence in [
        "previous_review",
        "stdout",
        "stderr",
        "process",
        "elapsed",
        "operations",
        "diff",
    ] {
        assert!(prompt.contains(evidence));
    }
    let before = fs::read(f.data.join("state.sqlite3")).unwrap();
    let _ = f.snapshot();
    assert_eq!(before, fs::read(f.data.join("state.sqlite3")).unwrap());
}
#[test]
fn native_infrastructure_failures_are_counted_and_retry_only_once() {
    let f = Fixture::new();
    f.import(json!([configured(&f, "infra", 3)]));
    let mut child = start(&f);
    assert_eq!(finish(&mut child).code(), Some(3));
    let review = &f.snapshot()["tasks"][0]["attempts"][0]["review"];
    assert_eq!(review["actual_calls"], 2);
    assert_eq!(review["infrastructure_retries"], 1);
    let c = db(&f);
    let deadlines: Vec<i64> = c
        .prepare("SELECT deadline_ms FROM reviewer_calls ORDER BY rowid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(deadlines.len(), 2);
    assert_eq!(deadlines[0], deadlines[1]);
}

#[test]
fn claude_observer_protocol_is_fresh_bounded_and_retires_without_worker_authority() {
    let f = Fixture::new();
    let path = f.root.join("claude-observer.py");
    fs::write(&path, r#"import sys,json,pathlib,os
args=sys.argv[1:];sys.stdin.read()
pathlib.Path('claude-observer-args.json').write_text(json.dumps(args))
pathlib.Path('claude-observer-env.json').write_text(json.dumps([k for k in os.environ if k.startswith('JTASKS_')]))
schema=json.loads(args[args.index('--json-schema')+1]);p=schema['properties'];result={k:p[k]['const'] for k in ['run','task','attempt','review','generation']}
result.update(decision='continue',rationale='Current capture and diff inspected',evidence=['commands','diff'],delay_cause='Finite extension would permit the check to complete',next_review_after_seconds=1)
session=args[args.index('--session-id')+1]
print(json.dumps({'type':'system','subtype':'init','session_id':session,'permissionMode':'dontAsk'}),flush=True)
print(json.dumps({'type':'result','subtype':'success','session_id':session,'is_error':False,'structured_output':result}),flush=True)
"#).unwrap();
    let mut a = configured(&f, "continue", 1);
    a["reviews"]["observer"] =
        json!({"kind":"claude","executable":"python","launcher_args":[path]});
    f.import(json!([
        a,
        task("B", &[], "completed", "Unrelated work finishes")
    ]));
    let mut child = start(&f);
    assert_eq!(finish(&mut child).code(), Some(3));
    let status = f.snapshot();
    assert_eq!(
        status["tasks"][0]["attempts"][0]["review"]["actual_calls"],
        1
    );
    assert_eq!(status["tasks"][0]["state"], "needs_intervention");
    assert_eq!(status["tasks"][1]["state"], "completed");
    let args: Vec<String> =
        serde_json::from_slice(&fs::read(f.root.join("claude-observer-args.json")).unwrap())
            .unwrap();
    assert!(args.windows(2).any(|w| w == ["--tools", ""]));
    assert!(
        args.windows(2)
            .any(|w| w == ["--permission-mode", "dontAsk"])
    );
    assert!(args.iter().any(|v| v == "--strict-mcp-config"));
    assert!(!args.iter().any(|v| v == "--resume" || v.contains("bypass")));
    assert_eq!(
        fs::read_to_string(f.root.join("claude-observer-env.json")).unwrap(),
        "[]"
    );
    let c = db(&f);
    let id: String = c
        .query_row("SELECT id FROM reviewer_calls", [], |r| r.get(0))
        .unwrap();
    let dir = f.data.join("reviews").join(id);
    let custody: Value =
        serde_json::from_slice(&fs::read(dir.join("custody.json")).unwrap()).unwrap();
    assert_eq!(custody["empty"], true);
    assert!(
        fs::read_to_string(dir.join("stdout.log"))
            .unwrap()
            .contains("structured_output")
    );
}
#[test]
fn finite_reviewer_deadline_stops_its_scope_then_worker() {
    let f = Fixture::new();
    let mut a = configured(&f, "hang", 3);
    a["reviews"]["deadline_seconds"] = json!(1);
    f.import(json!([a]));
    let mut child = start(&f);
    assert_eq!(finish(&mut child).code(), Some(3));
    let c = db(&f);
    let id: String = c
        .query_row("SELECT id FROM reviewer_calls", [], |r| r.get(0))
        .unwrap();
    let custody: Value = serde_json::from_slice(
        &fs::read(f.data.join("reviews").join(id).join("custody.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(custody["empty"], true);
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"][0]["review"]["actual_calls"],
        1
    );
}
#[test]
fn worker_completion_invalidates_running_observer_and_does_not_replace() {
    let f = Fixture::new();
    let mut a = configured(&f, "race", 3);
    a["reviews"]["deadline_seconds"] = json!(5);
    a["executor"]["outcome"] = json!("completed");
    a["executor"]["summary"] = json!("finished while being observed");
    a["executor"]["delay_ms"] = json!(2400);
    f.import(json!([a]));
    let mut child = start(&f);
    assert!(finish(&mut child).success());
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "completed");
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 1);
    let c = db(&f);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM reviewer_calls WHERE state='stale'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn reviewer_retry_uses_normal_work_allowance_after_confirmed_stop() {
    let f = Fixture::new();
    let mut a = configured(&f, "retry", 3);
    a["recovery"] = json!({"work_retries":1});
    f.import(json!([a]));
    fs::write(f.root.join("edit.txt"), "keep").unwrap();
    let mut child = start(&f);
    assert_eq!(finish(&mut child).code(), Some(3));
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(status["tasks"][0]["state"], "retry_exhausted");
    for attempt in status["tasks"][0]["attempts"].as_array().unwrap() {
        let record: Value = serde_json::from_slice(
            &fs::read(
                f.data
                    .join("attempts")
                    .join(attempt["id"].as_str().unwrap())
                    .join("custody.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(record["empty"], true);
    }
    assert_eq!(fs::read_to_string(f.root.join("edit.txt")).unwrap(), "keep");
    let c = db(&f);
    let events:Vec<String>=c.prepare("SELECT event FROM journal WHERE task_id='A' AND event IN ('launch_intent','process_retired') ORDER BY sequence").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(
        events,
        vec![
            "launch_intent",
            "process_retired",
            "launch_intent",
            "process_retired"
        ]
    );
}
