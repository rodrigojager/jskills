mod support;
use jtasks::controller::{
    activity::{self, Actor, Event, Kind as EventKind, Severity},
    clock::{self, Clock},
    due::{self, Checker, Kind, Work},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

fn memory() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(jtasks::storage::schema::SQL).unwrap();
    c.execute_batch(jtasks::controller::recovery::SCHEMA)
        .unwrap();
    c.execute_batch(jtasks::controller::cooldowns::SCHEMA)
        .unwrap();
    activity::initialize(&c).unwrap();
    clock::initialize(&c).unwrap();
    due::initialize(&c).unwrap();
    c.execute("INSERT INTO runs VALUES('r','','{}','running')", [])
        .unwrap();
    c.execute(
        "INSERT INTO tasks VALUES('r','A',0,?1,'running')",
        [task("A", &[], "silent", "").to_string()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO attempts(id,run_id,task_id,state) VALUES('a','r','A','running')",
        [],
    )
    .unwrap();
    activity::begin(&c, "a", 10).unwrap();
    c
}
fn event(actor: Actor, kind: EventKind, severity: Severity, content: &str) -> Event {
    Event {
        actor,
        kind,
        severity,
        content: Some(content.into()),
        source: "test_normalized".into(),
    }
}
#[test]
fn atomic_sequence_and_null_distinct_tool_current_worker_semantics() {
    let c = memory();
    let empty = activity::projection(&c, "a").unwrap();
    assert_eq!(empty["start_ms"], 10);
    assert!(empty["last_activity_ms"].is_null());
    assert!(empty["last_log_ms"].is_null());
    let message = event(Actor::Worker, EventKind::Message, Severity::Info, "A");
    activity::commit(&c, "r", "A", "a", &message, 100).unwrap();
    activity::commit(&c, "r", "A", "a", &message, 101).unwrap();
    activity::commit(
        &c,
        "r",
        "A",
        "a",
        &event(Actor::Worker, EventKind::Message, Severity::Info, "B"),
        102,
    )
    .unwrap();
    activity::commit(&c, "r", "A", "a", &message, 103).unwrap();
    assert_eq!(
        activity::projection(&c, "a").unwrap()["last_distinct_content_ms"],
        102
    );
    activity::commit(
        &c,
        "r",
        "A",
        "a",
        &event(Actor::Worker, EventKind::Tool, Severity::Info, "tool"),
        104,
    )
    .unwrap();
    for actor in [
        Actor::Reviewer,
        Actor::Controller,
        Actor::Reader,
        Actor::Unattributed,
    ] {
        activity::commit(
            &c,
            "r",
            "A",
            "a",
            &event(actor, EventKind::Message, Severity::Info, "not worker"),
            105,
        )
        .unwrap();
    }
    activity::commit(
        &c,
        "r",
        "A",
        "a",
        &event(Actor::Worker, EventKind::Message, Severity::Error, "error"),
        106,
    )
    .unwrap();
    c.execute("UPDATE attempts SET state='retired' WHERE id='a'", [])
        .unwrap();
    activity::commit(&c, "r", "A", "a", &message, 107).unwrap();
    let value = activity::projection(&c, "a").unwrap();
    assert_eq!(value["last_log_ms"], 107);
    assert_eq!(value["last_activity_ms"], 104);
    assert_eq!(value["last_tool_activity_ms"], 104);
    assert_eq!(
        c.query_row("SELECT count(*) FROM activity_events", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        11
    );
    let before = c
        .query_row("SELECT count(*) FROM journal", [], |r| r.get::<_, u32>(0))
        .unwrap();
    c.execute_batch("CREATE TRIGGER test_projection_failure BEFORE UPDATE ON attempt_activity BEGIN SELECT RAISE(ABORT,'synthetic projection failure'); END;").unwrap();
    assert!(activity::commit(&c, "r", "A", "a", &message, 999).is_err());
    assert_eq!(
        c.query_row("SELECT count(*) FROM journal", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        before
    );
    assert_eq!(activity::projection(&c, "a").unwrap(), value);
}

#[test]
fn monotonic_union_waits_reviewer_time_and_controller_gap() {
    let c = memory();
    c.execute_batch("CREATE TABLE managed_commands(id TEXT,run_id TEXT,task_id TEXT,state TEXT);INSERT INTO managed_commands VALUES('cmd','r','A','running');").unwrap();
    let mut clock = Clock::start(&c, "r", 1000).unwrap();
    clock.set_reviewing("A", true);
    clock.sample_at(&c, "r", Duration::ZERO, 1000).unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(3), 500)
        .unwrap(); // UTC rolled back
    clock.set_reviewing("A", false);
    clock
        .sample_at(&c, "r", Duration::from_secs(3), 500)
        .unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(5), 9_000_000)
        .unwrap();
    c.execute_batch(
        "UPDATE attempts SET state='retired';UPDATE tasks SET state='needs_intervention';",
    )
    .unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(5), 9_000_000)
        .unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(10), 9_005_000)
        .unwrap();
    c.execute("UPDATE managed_commands SET state='exited'", [])
        .unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(10), 9_005_000)
        .unwrap();
    let value = clock::projection(&c, "r", "A").unwrap();
    assert_eq!(value["active_ms"], 10_000);
    assert_eq!(value["reviewer_ms"], 3000);
    assert_eq!(
        c.query_row(
            "SELECT active_ms FROM attempt_clocks WHERE attempt_id='a'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        5000
    );
    c.execute(
        "UPDATE tasks SET state='pending',spec=?1",
        [task("A", &[], "silent", "").to_string()],
    )
    .unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(10), 9_005_000)
        .unwrap();
    clock
        .sample_at(&c, "r", Duration::from_secs(12), 9_007_000)
        .unwrap();
    assert_eq!(
        clock::projection(&c, "r", "A").unwrap()["resource_wait_ms"],
        2000
    );
    let mut resumed = Clock::start(&c, "r", 99_000_000).unwrap();
    resumed
        .sample_at(&c, "r", Duration::ZERO, 99_000_000)
        .unwrap();
    let after = clock::projection(&c, "r", "A").unwrap();
    assert_eq!(after["active_ms"], 10_000);
    assert_eq!(after["resource_wait_ms"], 2000);
    assert_eq!(after["timing_uncertain"], true);
}

fn review(id: &str, due_ms: u64, generation: u32) -> Work {
    Work {
        id: id.into(),
        run: "r".into(),
        task: Some("A".into()),
        attempt: Some("a".into()),
        kind: Kind::Review,
        generation,
        due_ms,
    }
}
#[test]
fn one_checker_rechecks_generation_completion_and_monotonic_due_without_prose_renewal() {
    let c = memory();
    let work = review("review-a", 1500, 0);
    due::schedule(&c, &work).unwrap();
    let mut checker = Checker::new();
    assert!(checker.ready_at(Duration::ZERO));
    assert!(
        checker
            .scan_at(&c, "r", Duration::ZERO, 1000)
            .unwrap()
            .is_empty()
    );
    activity::commit(
        &c,
        "r",
        "A",
        "a",
        &event(
            Actor::Worker,
            EventKind::Message,
            Severity::Info,
            "different prose",
        ),
        1300,
    )
    .unwrap();
    assert!(
        checker
            .scan_at(&c, "r", Duration::from_millis(499), 9_000_000)
            .unwrap()
            .is_empty()
    );
    let queued = checker
        .scan_at(&c, "r", Duration::from_millis(500), 100)
        .unwrap();
    assert_eq!(queued, vec![work.clone()]);
    let later = review("review-a", 3000, 1);
    due::schedule(&c, &later).unwrap();
    assert!(
        checker
            .admit_at(&c, &work, Duration::from_millis(500), 100)
            .unwrap()
            .is_none()
    );
    assert!(
        checker
            .scan_at(&c, "r", Duration::from_millis(500), 1000)
            .unwrap()
            .is_empty()
    );
    let queued = checker
        .scan_at(&c, "r", Duration::from_millis(3000), 2000)
        .unwrap();
    assert_eq!(queued, vec![later.clone()]);
    assert_eq!(
        checker
            .admit_at(&c, &later, Duration::from_millis(3500), 2000)
            .unwrap(),
        Some(1000)
    );
    assert!(
        checker
            .admit_at(&c, &later, Duration::from_millis(3500), 2000)
            .unwrap()
            .is_none()
    );
    due::complete(&c, &later.id, later.generation).unwrap();
    let completed = review("review-a", 1, 2);
    due::schedule(&c, &completed).unwrap();
    checker
        .scan_at(&c, "r", Duration::from_millis(3500), 2000)
        .unwrap();
    c.execute("UPDATE attempts SET outcome='completed'", [])
        .unwrap();
    assert!(
        checker
            .admit_at(&c, &completed, Duration::from_millis(3500), 2000)
            .unwrap()
            .is_none()
    );
    assert!(
        checker
            .scan_at(&c, "r", Duration::from_millis(3500), 2000)
            .unwrap()
            .is_empty()
    );
    for millis in 1..1000 {
        assert!(!checker.ready_at(Duration::from_millis(millis)));
    }
    assert!(checker.ready_at(Duration::from_millis(1000)));
    assert_eq!(checker.scans, 2);
}

#[test]
fn dozens_of_due_items_and_independent_instances_use_two_scans_not_task_timers() {
    for _ in 0..3 {
        let c = memory();
        for i in 0..48 {
            due::schedule(
                &c,
                &Work {
                    id: format!("synthetic-{i}"),
                    run: "r".into(),
                    task: None,
                    attempt: None,
                    kind: Kind::Retry,
                    generation: 0,
                    due_ms: 2000,
                },
            )
            .unwrap();
        }
        let mut checker = Checker::new();
        assert!(checker.ready_at(Duration::ZERO));
        assert!(
            checker
                .scan_at(&c, "r", Duration::ZERO, 1000)
                .unwrap()
                .is_empty()
        );
        for millis in 1..1000 {
            assert!(!checker.ready_at(Duration::from_millis(millis)));
        }
        assert!(checker.ready_at(Duration::from_secs(1)));
        assert_eq!(
            checker
                .scan_at(&c, "r", Duration::from_secs(1), 2000)
                .unwrap()
                .len(),
            48
        );
        assert_eq!(checker.scans, 2);
    }
}

#[test]
fn read_only_uninitialized_runtime_reports_explicit_nulls() {
    let c = Connection::open_in_memory().unwrap();
    assert_eq!(
        activity::projection(&c, "old").unwrap()["silence_baseline"],
        "not_initialized"
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "done")]));
    let before = fs::read(f.data.join("state.sqlite3")).unwrap();
    let output = f.call(&["activity", "--run", "demo", "--task", "A"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["clock"].is_null());
    assert_eq!(value["attempts"], json!([]));
    assert_eq!(before, fs::read(f.data.join("state.sqlite3")).unwrap());
}

struct Running<'a> {
    child: Child,
    f: &'a Fixture,
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            let _ = self.f.call(&["stop", "--run", "demo"]);
            let _ = self.child.wait();
        }
    }
}

#[test]
fn quiet_actual_worker_has_launch_baseline_before_any_log() {
    let f = Fixture::new();
    let mut a = task("A", &[], "completed", "done");
    a["executor"]["delay_ms"] = json!(600);
    f.import(json!([a]));
    let mut run = Running {
        child: f
            .command()
            .args(["run", "--run", "demo"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
        f: &f,
    };
    f.await_status(|v| v["tasks"][0]["attempts"][0]["process_state"] == "running");
    let output = f.call(&["activity", "--run", "demo", "--task", "A"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let a = &value["attempts"][0];
    assert!(a["start_ms"].is_number());
    assert_eq!(a["silence_baseline"], "attempt_start");
    assert!(a["last_log_ms"].is_null());
    assert!(a["last_activity_ms"].is_null());
    assert!(run.child.wait().unwrap().success());
}

const NATIVE: &str = r#"
import json,sys,time,os,subprocess
from pathlib import Path
sys.stdin.read()
args=sys.argv[1:];session=args[args.index('--session-id')+1]
def emit(v):print(json.dumps(v),flush=True)
emit({'type':'system','subtype':'init','session_id':session,'permissionMode':args[args.index('--permission-mode')+1]})
emit({'type':'assistant','session_id':session,'message':{'content':[{'type':'text','text':'logical message'}]}})
emit({'type':'tool_progress','session_id':session})
print('benign stderr transport diagnostic',file=sys.stderr,flush=True)
subprocess.run([os.environ['JTASKS_BIN'],'log','--kind','tool','--content','explicit managed activity'],check=True,stdout=subprocess.DEVNULL)
Path('ready').write_text('ready')
end=time.monotonic()+5
while not Path('release').exists() and time.monotonic()<end:time.sleep(.01)
schema=json.loads(args[args.index('--json-schema')+1]);r={k:v['const'] for k,v in schema['properties'].items() if 'const' in v};r.update(outcome='completed',summary='synthetic native completed')
emit({'type':'result','subtype':'success','session_id':session,'is_error':False,'structured_output':r})
"#;
// A worker that streams thousands of content-less deltas (what `--include-partial-messages` produces) while
// another process holds a read transaction on the database for seconds: the controller must keep writing,
// coalesce the deltas instead of journaling each one, and finish the task. On 2026-11-18 a busy database
// ended a real controller with "database is locked".
const FLOOD: &str = r#"
import json,sys,time
from pathlib import Path
sys.stdin.read()
args=sys.argv[1:];session=args[args.index('--session-id')+1]
def emit(v):print(json.dumps(v),flush=True)
emit({'type':'system','subtype':'init','session_id':session,'permissionMode':args[args.index('--permission-mode')+1]})
for i in range(3000):
    emit({'type':'stream_event','session_id':session,'event':{'type':'content_block_delta','delta':{'type':'input_json_delta','partial_json':'x'}}})
    if i % 100 == 0: time.sleep(0.1)
Path('flooded').write_text('ok')
while not Path('release').exists(): time.sleep(.01)
schema=json.loads(args[args.index('--json-schema')+1]);r={k:v['const'] for k,v in schema['properties'].items() if 'const' in v};r.update(outcome='completed',summary='flood completed')
emit({'type':'result','subtype':'success','session_id':session,'is_error':False,'structured_output':r})
"#;
#[test]
fn a_long_reader_and_a_flooding_worker_do_not_stop_the_controller() {
    let f = Fixture::new();
    let path = f.root.join("flood.py");
    fs::write(&path, FLOOD).unwrap();
    f.import(json!([{"id":"A","title":"Flooding native stream","unknown_capabilities":{"actor":"activity-test","reason":"Synthetic protocol emitter, no provider account"},"executor":{"kind":"claude","executable":"python","launcher_args":[path]}}]));
    let mut run = Running {
        child: f
            .command()
            .args(["run", "--run", "demo"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
        f: &f,
    };
    // Once the controller is up (its endpoint is written, the database is in WAL), a reader holds a read
    // transaction for 4 seconds while the worker floods, longer than the old 2 s wait.
    let up = Instant::now() + Duration::from_secs(30);
    while !jtasks::transport::control_file(&f.data, "demo").exists() {
        assert!(Instant::now() < up, "the controller never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let reader = jtasks::storage::open_reader(&f.data.join("state.sqlite3")).unwrap();
    reader.execute_batch("BEGIN").unwrap();
    let _: i64 = reader
        .query_row("SELECT count(*) FROM journal", [], |r| r.get(0))
        .unwrap();
    std::thread::sleep(Duration::from_secs(4));
    reader.execute_batch("COMMIT").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !f.root.join("flooded").exists() {
        assert!(Instant::now() < deadline, "the worker never finished flooding");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(reader);
    assert!(
        run.child.try_wait().unwrap().is_none(),
        "the controller must still be running after a long reader"
    );
    fs::write(f.root.join("release"), "release").unwrap();
    assert!(run.child.wait().unwrap().success());
    assert_eq!(f.snapshot()["tasks"][0]["state"], "completed");
    let c = jtasks::storage::open_reader(&f.data.join("state.sqlite3")).unwrap();
    let rows: i64 = c
        .query_row(
            "SELECT count(*) FROM journal WHERE event='activity'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        rows < 300,
        "3000 content-less deltas must be coalesced, found {rows} activity rows"
    );
}

#[test]
fn native_and_explicit_logs_are_committed_live_before_retirement() {
    let f = Fixture::new();
    let path = f.root.join("native.py");
    fs::write(&path, NATIVE).unwrap();
    f.import(json!([{"id":"A","title":"Live synthetic native stream","unknown_capabilities":{"actor":"activity-test","reason":"Synthetic protocol emitter, no provider account"},"executor":{"kind":"claude","executable":"python","launcher_args":[path]}}]));
    let mut run = Running {
        child: f
            .command()
            .args(["run", "--run", "demo"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
        f: &f,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f.root.join("ready").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    // Native output is read on a 250 ms cadence, so the clocks may trail the ready file.
    let polled = Instant::now() + Duration::from_secs(10);
    let value: Value = loop {
        let output = f.call(&["activity", "--run", "demo", "--task", "A"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let a = &value["attempts"][0];
        if (a["last_log_ms"].is_number()
            && a["last_activity_ms"].is_number()
            && a["last_distinct_content_ms"].is_number()
            && a["last_tool_activity_ms"].is_number())
            || Instant::now() > polled
        {
            break value;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let a = &value["attempts"][0];
    assert!(a["last_log_ms"].is_number());
    assert!(a["last_activity_ms"].is_number());
    assert!(a["last_distinct_content_ms"].is_number());
    assert!(a["last_tool_activity_ms"].is_number());
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"][0]["process_state"],
        "running"
    );
    let c = Connection::open_with_flags(
        f.data.join("state.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let events = c
        .prepare("SELECT event FROM activity_events")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let events: Vec<Event> = events
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| e.source == "explicit_worker" && e.kind == EventKind::Tool)
    );
    assert!(events.iter().any(|e| e.source == "native:claude:stderr"
        && e.severity == Severity::Info
        && e.kind == EventKind::Other));
    fs::write(f.root.join("release"), "release").unwrap();
    assert!(run.child.wait().unwrap().success());
}
