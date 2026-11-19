mod support;
use jtasks::controller::cooldowns::{self, Hint};
use serde_json::{Value, json};
use std::{fs, process::Stdio};
use support::{Fixture, task};

fn writer(
    f: &Fixture,
) -> (
    jtasks::storage::Workspace,
    jtasks::resources::ControllerGuard,
    jtasks::storage::Store,
) {
    let w = jtasks::storage::Workspace {
        checkout: f.root.clone(),
        git_dir: f.root.join(".git"),
        data: f.data.clone(),
    };
    let guard = jtasks::resources::ControllerGuard::acquire(&w.git_dir).unwrap();
    let store = jtasks::storage::Store::open(&w, &guard).unwrap();
    (w, guard, store)
}

#[test]
fn computed_backoff_reset_precedence_and_persisted_scopes() {
    for (index, seconds) in [10, 20, 40, 80, 120, 120].into_iter().enumerate() {
        assert_eq!(
            cooldowns::due(100, index as u32, &Hint::default()),
            (100 + seconds * 1000, "connection_backoff")
        );
    }
    assert_eq!(
        cooldowns::due(
            100,
            0,
            &Hint {
                reset_at_ms: Some(9_000_000),
                retry_after_ms: Some(1)
            }
        ),
        (9_000_000, "provider_reset")
    );
    assert_eq!(
        cooldowns::due(
            100,
            0,
            &Hint {
                reset_at_ms: None,
                retry_after_ms: Some(9_000_000)
            }
        ),
        (9_000_100, "retry_after")
    );
    let f = Fixture::new();
    let mut a = task("A", &[], "silent", "");
    a["recovery"] =
        json!({"cooldown_scope":{"provider":"synthetic","account_alias":"account-a","model":null}});
    let mut b = a.clone();
    b["id"] = json!("B");
    b["recovery"]["cooldown_scope"]["model"] = json!("model-b");
    f.import(json!([a, b, task("C", &[], "silent", "")]));
    let due = jtasks::controller::admission::now_ms() + 100_000;
    {
        let (_, _, mut s) = writer(&f);
        s.intent("demo", "A", "prior").unwrap();
        s.retire("demo", "A", "prior", Some(1), false).unwrap();
        cooldowns::schedule(
            &s.conn,
            "demo",
            "A",
            "prior",
            &Hint {
                reset_at_ms: Some(due),
                retry_after_ms: None,
            },
            100,
        )
        .unwrap();
        assert!(!cooldowns::eligible(&s.conn, "demo", "A", due - 1).unwrap());
        assert!(!cooldowns::eligible(&s.conn, "demo", "B", due - 1).unwrap());
        assert!(cooldowns::eligible(&s.conn, "demo", "C", due - 1).unwrap());
        assert!(cooldowns::eligible(&s.conn, "demo", "B", due).unwrap());
    }
    let (_, _, s) = writer(&f);
    assert_eq!(
        cooldowns::deadline(&s.conn, "demo", "B").unwrap(),
        Some(due)
    );
    assert_eq!(
        f.snapshot()["tasks"][0]["cooldown"]["latest"]["scope_certainty"],
        "known_explicit_alias"
    );
}

const NATIVE: &str = r#"
import json,sys,time
from pathlib import Path
sys.stdin.read()
p=Path('calls');n=int(p.read_text())+1 if p.exists() else 1;p.write_text(str(n))
args=sys.argv[1:];session=args[args.index('--resume' if '--resume' in args else '--session-id')+1]
permission=args[args.index('--permission-mode')+1]
print(json.dumps({'type':'system','subtype':'init','session_id':session,'permissionMode':permission}))
if n==1:
 print(json.dumps({'type':'rate_limit_event','session_id':session,'rate_limit_info':{'status':'rejected','resetsAt':int(time.time())+3600,'rateLimitType':'five_hour'}}))
 print(json.dumps({'type':'result','subtype':'error_during_execution','session_id':session,'is_error':True,'errors':['native execution failed']}))
else:
 schema=json.loads(args[args.index('--json-schema')+1]);r={k:v['const'] for k,v in schema['properties'].items() if 'const' in v};r.update(outcome='completed',summary='retry completed')
 print(json.dumps({'type':'result','subtype':'success','session_id':session,'is_error':False,'structured_output':r}))
"#;

#[test]
fn actual_controller_wait_releases_capacity_and_resource_then_manual_retry_resumes() {
    let f = Fixture::new();
    let script = f.root.join("native.py");
    fs::write(&script, NATIVE).unwrap();
    let a = json!({"id":"A","title":"Synthetic quota","unknown_capabilities":{"actor":"cooldown-test","reason":"Synthetic emitter without provider calls"},"admission":{"resources":["test/shared"]},"executor":{"kind":"claude","executable":"python","launcher_args":[script]}});
    let mut b = task("B", &[], "completed", "independent finished during wait");
    b["admission"] = json!({"resources":["test/shared"]});
    f.import(json!([a, b]));
    let mut child = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let status = f.await_status(|v| {
        v["tasks"][0]["cooldown"]["latest"]["state"] == "waiting"
            && v["tasks"][1]["state"] == "completed"
    });
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        status["tasks"][0]["cooldown"]["latest"]["scope_certainty"],
        "unknown_task_local"
    );
    assert_eq!(
        status["tasks"][0]["cooldown"]["latest"]["source"],
        "provider_reset"
    );
    let due = status["tasks"][0]["cooldown"]["latest"]["due_ms"].clone();
    assert!(f.call(&["stop", "--run", "demo"]).status.success());
    assert_eq!(child.wait().unwrap().code(), Some(3));
    assert_eq!(
        f.snapshot()["tasks"][0]["cooldown"]["latest"]["due_ms"],
        due
    );
    let retry = f.call(&[
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        "external availability changed",
        "--request-id",
        "manual-a",
    ]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let receipt: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(receipt["status"], "requires_foreground_resume");
    assert_eq!(f.snapshot()["state"], "stopped");
    assert_eq!(fs::read_to_string(f.root.join("calls")).unwrap(), "1");
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    assert_eq!(fs::read_to_string(f.root.join("calls")).unwrap(), "2");
}

const BLOCKED_THEN_DONE: &str = r#"
import json,sys
from pathlib import Path
sys.stdin.read()
p=Path('calls');n=int(p.read_text())+1 if p.exists() else 1;p.write_text(str(n))
args=sys.argv[1:];session=args[args.index('--resume' if '--resume' in args else '--session-id')+1]
permission=args[args.index('--permission-mode')+1]
print(json.dumps({'type':'system','subtype':'init','session_id':session,'permissionMode':permission}))
schema=json.loads(args[args.index('--json-schema')+1]);r={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
r.update(outcome='blocked' if n==1 else 'completed',summary='blocked once' if n==1 else 'done after retry')
print(json.dumps({'type':'result','subtype':'success','session_id':session,'is_error':False,'structured_output':r}))
"#;

#[test]
fn blocked_task_is_retried_with_history_and_completes() {
    let f = Fixture::new();
    let script = f.root.join("native.py");
    fs::write(&script, BLOCKED_THEN_DONE).unwrap();
    let a = json!({"id":"A","title":"Blocked once","unknown_capabilities":{"actor":"blocked-test","reason":"Synthetic emitter without provider calls"},"executor":{"kind":"claude","executable":"python","launcher_args":[script]}});
    f.import(json!([a]));
    let first = f.call(&["run", "--run", "demo"]);
    let _ = first;
    assert_eq!(fs::read_to_string(f.root.join("calls")).unwrap(), "1");
    let retry = f.call(&[
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        "unblocked externally",
        "--request-id",
        "blocked-a",
    ]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    f.call(&["run", "--run", "demo"]);
    assert_eq!(fs::read_to_string(f.root.join("calls")).unwrap(), "2");
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "completed");
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 2);
}

fn ticket(f: &Fixture, attempt: &Value) -> String {
    fs::read_to_string(
        f.data
            .join("attempts")
            .join(attempt["id"].as_str().unwrap())
            .join("ticket.md"),
    )
    .unwrap()
}

#[test]
fn retry_reason_reaches_the_next_attempt_ticket() {
    let f = Fixture::new();
    let script = f.root.join("native.py");
    fs::write(&script, BLOCKED_THEN_DONE).unwrap();
    let a = json!({"id":"A","title":"Blocked once","unknown_capabilities":{"actor":"blocked-test","reason":"Synthetic emitter without provider calls"},"executor":{"kind":"claude","executable":"python","launcher_args":[script]}});
    f.import(json!([a]));
    f.call(&["run", "--run", "demo"]);
    let reason = "Operator answer: yes, use the staging database zq-7731";
    let retry = f.call(&[
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        reason,
        "--request-id",
        "answer-a",
    ]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    f.call(&["run", "--run", "demo"]);
    let status = f.snapshot();
    let attempts = status["tasks"][0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    let first = attempts.iter().find(|a| a["outcome"] == "blocked").unwrap();
    let second = attempts
        .iter()
        .find(|a| a["outcome"] == "completed")
        .unwrap();
    let before = ticket(&f, first);
    assert!(before.contains("## Notes from the operator since the last attempt\nThe operator"));
    assert!(!before.contains(reason), "{before}");
    let after = ticket(&f, second);
    let section = after
        .split("## Notes from the operator since the last attempt")
        .nth(1)
        .unwrap()
        .split("## Effective autonomy")
        .next()
        .unwrap();
    assert!(section.contains(&format!(": {reason}")), "{after}");
}

#[test]
fn final_operation_blocked_by_a_blocked_task_runs_after_retry() {
    let f = Fixture::new();
    let script = f.root.join("native.py");
    fs::write(&script, BLOCKED_THEN_DONE).unwrap();
    let a = json!({"id":"A","title":"Blocked once","unknown_capabilities":{"actor":"blocked-test","reason":"Synthetic emitter without provider calls"},"executor":{"kind":"claude","executable":"python","launcher_args":[script]}});
    let check = json!({"id":"final","stage":{"kind":"final","tasks":["A"]},"required":true,"revision":"r1","inputs":["src/"],"initial":{"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent","--exit-code","0"],"cwd":".","deadline_ms":5000},"repair_task":"A"});
    let path = f.root.join("operations.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[a],"operations":[check]})).unwrap()).unwrap();
    let import = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    f.call(&["run", "--run", "demo"]);
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "blocked");
    assert_eq!(status["operations"][0]["state"], "blocked");
    let retry = f.call(&[
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        "unblocked externally",
        "--request-id",
        "blocked-final",
    ]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let run = f.call(&["run", "--run", "demo"]);
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "completed");
    assert_eq!(status["operations"][0]["state"], "passed", "{status}");
    assert_eq!(status["operations"][0]["exit_code"], 0);
    assert!(run.status.success(), "{status}");
}

#[test]
fn manual_exhausted_grant_is_one_durable_idempotent_attempt() {
    let f = Fixture::new();
    let mut a = task("A", &[], "failed", "still unfinished");
    a["recovery"] = json!({"work_retries":0});
    f.import(json!([a]));
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let args = [
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        "configuration fixed",
        "--request-id",
        "one-grant",
    ];
    for _ in 0..2 {
        assert!(f.call(&args).status.success());
    }
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let (_, _, s) = writer(&f);
    let ledger = jtasks::controller::recovery::ledger(&s.conn, "demo", "A").unwrap();
    assert_eq!((ledger.work, ledger.work_limit), (1, 1));
    let consumed: String = s
        .conn
        .query_row(
            "SELECT consumed_by FROM manual_retries WHERE request_id='one-grant'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!consumed.is_empty());
    assert_eq!(
        s.conn
            .query_row("SELECT count(*) FROM manual_retries", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn retry_cannot_duplicate_live_work_or_bypass_dependencies() {
    let f = Fixture::new();
    let mut a = task("A", &[], "completed", "done");
    a["executor"]["delay_ms"] = json!(2500);
    f.import(json!([a, task("B", &["A"], "completed", "dependent")]));
    let mut child = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    f.await_status(|v| v["tasks"][0]["state"] == "running");
    assert!(
        !f.call(&[
            "retry",
            "A",
            "--run",
            "demo",
            "--reason",
            "cannot duplicate"
        ])
        .status
        .success()
    );
    let retry = f.call(&["retry", "B", "--run", "demo", "--reason", "still blocked"]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let receipt: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(receipt["status"], "dependency_blocked");
    assert_eq!(
        f.snapshot()["tasks"][1]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(child.wait().unwrap().success());
}

#[test]
fn native_metadata_needs_rejected_exact_session_and_current_quota_context() {
    let event = json!({"type":"rate_limit_event","session_id":"11111111-1111-4111-8111-111111111111","rate_limit_info":{"status":"allowed","resetsAt":2000000000,"rateLimitType":"five_hour","overageStatus":"rejected"}});
    assert!(!Hint::claude_rate_limit(&event).explicit());
    let mut rejected = event.clone();
    rejected["rate_limit_info"]["status"] = json!("rejected");
    assert_eq!(
        Hint::claude_rate_limit(&rejected).reset_at_ms,
        Some(2_000_000_000_000)
    );
    let f = Fixture::new();
    let dir = f.root.join("observe");
    fs::create_dir_all(&dir).unwrap();
    let session = "11111111-1111-4111-8111-111111111111";
    let init = json!({"type":"system","subtype":"init","session_id":session});
    let error = json!({"type":"result","subtype":"error_during_execution","is_error":true,"session_id":session,"errors":["native execution failed"]});
    for (telemetry, expected) in [(event, false), (rejected.clone(), true)] {
        fs::write(
            dir.join("stdout.log"),
            format!("{init}\n{telemetry}\n{error}\n"),
        )
        .unwrap();
        let (o, _) = jtasks::executors::claude::observe(&dir, "A", "attempt").unwrap();
        assert_eq!(o.cooldown_hint.explicit(), expected);
        assert_eq!(
            o.failure_class.as_deref(),
            Some(if expected {
                "quota_reset_unknown"
            } else {
                "unknown"
            })
        );
    }
    let mut allowed = rejected.clone();
    allowed["rate_limit_info"]["status"] = json!("allowed_warning");
    fs::write(
        dir.join("stdout.log"),
        format!("{init}\n{rejected}\n{allowed}\n{error}\n"),
    )
    .unwrap();
    let (o, _) = jtasks::executors::claude::observe(&dir, "A", "attempt").unwrap();
    assert!(!o.cooldown_hint.explicit());
    assert_eq!(o.failure_class.as_deref(), Some("unknown"));
    rejected["session_id"] = json!("foreign-session");
    fs::write(
        dir.join("stdout.log"),
        format!("{init}\n{rejected}\n{error}\n"),
    )
    .unwrap();
    let (o, _) = jtasks::executors::claude::observe(&dir, "A", "attempt").unwrap();
    assert!(!o.cooldown_hint.explicit());
}

#[test]
fn codex_and_opencode_native_quota_errors_persist_timed_recovery() {
    let f = Fixture::new();
    // Match the real native runner envelopes, using bounded, deterministic
    // local emitters instead of spending provider quotas during the test.
    let codex_reset = chrono::Local::now() + chrono::TimeDelta::hours(2);
    let message = format!(
        "You’ve hit your usage limit. Try again at {}th, {}.",
        codex_reset.format("%b %-d"),
        codex_reset.format("%Y %-I:%M %p"),
    );
    let open_timestamp = jtasks::controller::admission::now_ms();
    let open_delay = 7_100_000;
    let session = "ses_e1084f09cffeIZChByMMw5K1uu";
    let cases = [
        (
            "codex",
            vec![
                json!({"type":"thread.started","thread_id":"exact-codex-thread"}),
                json!({"type":"error","message":message}),
                json!({"type":"turn.failed","error":{"message":"turn failed"}}),
            ],
        ),
        (
            "opencode",
            vec![
                json!({"type":"step_start","sessionID":session}),
                json!({"type":"error","sessionID":session,"timestamp":open_timestamp,
                   "error":{"type":"provider.quota","message":"Usage limit","status":429,
                            "retryAfterMs":open_delay}}),
            ],
        ),
    ];
    let tasks = cases
        .into_iter()
        .map(|(kind, records)| {
            let path = f.root.join(format!("{kind}.jsonl"));
            let wire = records.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
            fs::write(&path, format!("{wire}\n")).unwrap();
            let mut executor = json!({"kind":kind,"permissions":"restricted","executable":"python",
                "launcher_args":["-c","import sys; from pathlib import Path; sys.stdout.buffer.write(Path(sys.argv[1]).read_bytes()); sys.exit(1)",path]});
            if kind == "opencode" {
                executor["provider"] = json!("fixture");
                executor["model"] = json!("fixture-model");
            }
            json!({"id":kind,"title":kind,
                "unknown_capabilities":{"actor":"cooldown-test","reason":"Native protocol emitter"},
                "executor":executor})
        })
        .collect::<Vec<_>>();
    f.import(json!(tasks));
    let mut child = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let status = f.await_status(|value| {
        value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["cooldown"]["latest"]["state"] == "waiting")
    });
    assert!(f.call(&["stop", "--run", "demo"]).status.success());
    assert_eq!(child.wait().unwrap().code(), Some(3));

    for task in status["tasks"].as_array().unwrap() {
        let latest = &task["cooldown"]["latest"];
        let expected = if task["id"] == "codex" {
            (codex_reset.timestamp_millis() as u64 / 60_000 + 1) * 60_000
        } else {
            open_timestamp + open_delay
        };
        assert_eq!(latest["source"], "provider_reset");
        assert_eq!(latest["due_ms"], expected, "{}", task["id"]);
        assert_eq!(task["attempts"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn opencode_requires_terminal_quota_timing_from_the_root_session() {
    let f = Fixture::new();
    let dir = f.root.join("observe");
    fs::create_dir_all(&dir).unwrap();
    let session = "ses_root";
    let start = json!({"type":"step_start","sessionID":session});
    let native_error = json!({
        "type":"error","timestamp":2000000000000_u64,"sessionID":session,
        "error":{"type":"provider.rate-limit","status":429,"retryAfterMs":90000}
    });
    let fails = [
        ("missing timing", "error", session, Value::Null, false),
        (
            "different session",
            "error",
            "ses_other",
            json!(90000),
            false,
        ),
        ("internal retry", "retry", session, json!(90000), false),
        ("terminal rate limit", "error", session, json!(90000), true),
    ];
    for (label, kind, id, delay, expected) in fails {
        let mut record = native_error.clone();
        record["type"] = json!(kind);
        record["sessionID"] = json!(id);
        record["error"]["retryAfterMs"] = delay;
        fs::write(dir.join("stdout.log"), format!("{start}\n{record}\n")).unwrap();
        let observation = jtasks::executors::opencode::observe(&dir, "A", "attempt").unwrap();
        assert_eq!(
            observation.cooldown_hint.reset_at_ms,
            expected.then_some(2_000_000_090_000),
            "{label}",
        );
    }
}
