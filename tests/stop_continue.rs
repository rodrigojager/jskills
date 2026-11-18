//! C15: a stop the controller performed itself is not a custody gap. The next
//! `jtasks run` starts the stopped task again without a manual retry, outside
//! the retry budget; a controller that died without a stop still pauses.
mod support;
use serde_json::{Value, json};
use std::{
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

struct Foreground(Child);
impl Drop for Foreground {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
impl Foreground {
    fn exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(exit) = self.0.try_wait().unwrap() {
                return exit;
            }
            assert!(Instant::now() < deadline, "controller exit deadline");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A slow task with a scheduled (not yet due) execution review, as in the
/// 2026-11-18 runs; `work_retries` 0 shows the stop spends no retry.
fn slow_reviewed_task(work_retries: Option<u32>) -> Value {
    let mut a = task("A", &[], "completed", "finished after the restart");
    a["executor"]["delay_ms"] = json!(3000);
    a["estimate_seconds"] = json!(600);
    if let Some(retries) = work_retries {
        a["recovery"] = json!({"work_retries": retries});
    }
    a
}

fn start(f: &Fixture) -> Foreground {
    let controller = Foreground(
        f.command()
            .args(["run", "--run", "demo"])
            .env("JTASKS_SHUTDOWN_GRACE_MS", "0")
            .env("JTASKS_SHUTDOWN_FORCE_MS", "2000")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    f.await_status(|s| {
        s["tasks"][0]["attempts"]
            .as_array()
            .is_some_and(|a| !a.is_empty() && a[0]["process_state"] == "running")
    });
    controller
}

fn database(f: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap()
}

#[test]
fn stopped_task_runs_again_on_the_next_run_without_a_retry() {
    let f = Fixture::new();
    f.import(json!([slow_reviewed_task(Some(0))]));
    let mut controller = start(&f);
    let stop = f.call(&["stop", "--run", "demo"]);
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert_eq!(controller.exit().code(), Some(3));
    let stopped = f.snapshot();
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(stopped["tasks"][0]["state"], "pending");
    assert_eq!(
        stopped["tasks"][0]["attempts"][0]["process_state"],
        "interrupted"
    );

    // No `jtasks retry`: the next run alone starts the task again.
    let again = f.call(&["run", "--run", "demo"]);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let snapshot = f.snapshot();
    assert_eq!(snapshot["state"], "completed");
    assert_eq!(snapshot["tasks"][0]["state"], "completed");
    let attempts = snapshot["tasks"][0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0]["process_state"], "interrupted");
    assert_ne!(attempts[0]["id"], attempts[1]["id"]);

    let conn = database(&f);
    let first = attempts[0]["id"].as_str().unwrap();
    let second = attempts[1]["id"].as_str().unwrap();
    let marked: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM journal WHERE attempt_id=?1 AND event='stop_retired')",
            [first],
            |r| r.get(0),
        )
        .unwrap();
    assert!(marked, "the controller records that it retired the worker");
    let (review, reason): (String, String) = conn
        .query_row(
            "SELECT state,reason FROM execution_reviews WHERE attempt_id=?1",
            [first],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(review, "stopped");
    assert!(!reason.contains("custody gap"));
    // The restart is not a work retry: with zero work retries it still ran.
    let kind: String = conn
        .query_row(
            "SELECT kind FROM recovery_invocations WHERE attempt_id=?1",
            [second],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "stop");
    let retried: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM journal WHERE event='manual_retry')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!retried, "no manual retry was recorded");
}

#[test]
fn killed_controller_without_a_stop_still_needs_intervention() {
    let f = Fixture::new();
    f.import(json!([slow_reviewed_task(None)]));
    let mut controller = start(&f);
    // A controller that dies does not retire its workers itself.
    controller.0.kill().unwrap();
    controller.exit();
    let first = f.snapshot()["tasks"][0]["attempts"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let again = f.call(&["run", "--run", "demo"]);
    assert!(!again.status.success());
    let snapshot = f.snapshot();
    assert_eq!(snapshot["tasks"][0]["state"], "needs_intervention");
    let attempts = snapshot["tasks"][0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "no replacement after a custody gap");

    let conn = database(&f);
    let marked: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM journal WHERE event='stop_retired')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!marked);
    let reason: String = conn
        .query_row(
            "SELECT reason FROM execution_reviews WHERE attempt_id=?1",
            [&first],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reason, "worker timing/custody gap requires intervention");
}
