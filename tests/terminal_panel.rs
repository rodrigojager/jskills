mod support;
use jtasks::{
    domain::types::{Outcome, Receipt, Task},
    read::terminal::{self, Panel},
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use support::{Fixture, task};

fn workspace(f: &Fixture) -> Workspace {
    Workspace::resolve(&f.root, Some(&f.data)).unwrap()
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn offline_watch_is_pure_read_and_non_tty_only_reports_transition_and_final() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    fs::write(f.data.join("control.json"), "MUST_NOT_CONTACT_CONTROL").unwrap();
    let before = files(&f.data);
    let output = f.call(&[
        "watch",
        "--run",
        "demo",
        "--duration-ms",
        "180",
        "--interval-ms",
        "50",
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(events.len(), 2, "unchanged polls must remain quiet");
    assert_eq!(events[0]["event"], "transition");
    assert_eq!(events[1]["event"], "final_summary");
    let value = &events[1]["view"]["status"];
    assert_eq!(value["availability"]["controller"], "unobserved");
    assert!(value["as_of"].is_string());
    assert_eq!(files(&f.data), before);
    let human = f.call(&["watch", "--run", "demo", "--once"]);
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("Persisted as-of="));
    assert!(text.contains("controller=unobserved"));
    assert!(text.contains("Dashboard: unavailable"));
    assert!(!text.contains("MUST_NOT_CONTACT_CONTROL"));
    assert!(!text.contains('\x1b'));
    assert_eq!(files(&f.data), before);
}

#[test]
fn development_complete_does_not_hide_required_final_check_or_question() {
    let f = Fixture::new();
    let path = f.root.join("plan.json");
    fs::write(&path,json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[task("A",&[],"completed","accepted")],"operations":[{"id":"final","stage":{"kind":"final","tasks":["A"]},"required":true,"revision":"r1","inputs":["src"],"repair_task":"A","initial":{"program":"uninvoked-fixture","cwd":"."}}]}).to_string()).unwrap();
    assert!(
        f.call(&["import", path.to_str().unwrap(), "--run", "demo"])
            .status
            .success()
    );
    let w = workspace(&f);
    {
        let guard = ControllerGuard::acquire(&w.git_dir).unwrap();
        let mut store = Store::open(&w, &guard).unwrap();
        let task: Task = serde_json::from_value(task("A", &[], "completed", "accepted")).unwrap();
        store.prepare_attempt(&w, "demo", &task, "a1").unwrap();
        store.launched("a1", 42).unwrap();
        store
            .accept(
                "demo",
                "a1",
                &Receipt {
                    receipt_id: "r1".into(),
                    attempt_id: "a1".into(),
                    task_id: "A".into(),
                    outcome: Outcome::Completed,
                    summary: "Designated judgment".into(),
                },
            )
            .unwrap();
        store.retire("demo", "A", "a1", Some(0), false).unwrap();
        store.conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES('demo','A','a1','restricted_question',?1)",[json!({"request_id":"q1","question":"Select deployment target"}).to_string()]).unwrap();
    }
    let snapshot = terminal::snapshot(&w, "demo").unwrap();
    let text = terminal::render(&snapshot, None, false);
    assert!(
        text.contains("development=complete | run=pending"),
        "{text}"
    );
    assert!(text.contains("Required verification=pending | pending required operations=1"));
    assert!(text.contains("operation final: pending"));
    assert!(text.contains("question q1 task=A pending=true: Select deployment target"));
    assert!(text.contains("current liveness unobserved"));
    assert!(!terminal::complete(&snapshot));
}

#[test]
fn panel_updates_one_tty_display_and_ignores_clock_only_churn() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    let mut value = terminal::snapshot(&workspace(&f), "demo").unwrap();
    let mut bytes = Vec::new();
    {
        let mut panel = Panel::new(&mut bytes, true, false);
        assert!(
            panel
                .update(&value, Some("http://127.0.0.1:43123/"), false)
                .unwrap()
        );
        value["status"]["event_cursor"] = json!(999);
        value["status"]["as_of"] = json!("changed timestamp");
        value["status"]["tasks"][0]["cooldown"]["remaining_ms"] = json!(200);
        assert!(
            !panel
                .update(&value, Some("http://127.0.0.1:43123/"), false)
                .unwrap()
        );
        value["status"]["state"] = json!("stopped");
        assert!(
            panel
                .update(&value, Some("http://127.0.0.1:43123/"), false)
                .unwrap()
        );
        panel.update(&value, None, true).unwrap();
    }
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text.matches("\x1b[?1049h").count(), 1);
    assert_eq!(text.matches("\x1b[H\x1b[2J").count(), 2);
    assert_eq!(text.matches("\x1b[?1049l").count(), 1);
    assert!(text.rfind("\x1b[?1049l").unwrap() < text.rfind("Final summary").unwrap());
}

#[test]
fn effective_assignment_waits_and_next_action_are_preserved_without_terminal_injection() {
    let value = json!({"status":{"identity":{"run":"demo"},"state":"running","availability":{"controller":"unobserved"},"tasks":[{"id":"T1","state":"running","cooldown":{"effective_due_ms":999,"latest":{"state":"waiting","reason":"provider reset"}},"attempts":[{"id":"a1","process_state":"running","effective_profile":{"alias":"execute","values":{"executor":"codex","native_agent":"worker","native_profile":"local","model":"declared-model","effort":"high"}},"review":{"state":"queued","reason":"capacity wait","actual_calls":0,"max_calls":2}}]}],"next_automatic_action":{"id":"review-a1","kind":"review","due_ms":999,"state":"queued"},"objective":"Read\u{1b}[2Jcarefully"}});
    let mut value = value;
    value["assignments"] = json!({"tasks":[{"id":"T1","current_attempt":"a1"}]});
    value["status"]["objective"] = json!(format!("Read{}[2Jcarefully", char::from(27)));
    let text = terminal::render(&value, Some("http://127.0.0.1:12345/"), false);
    for expected in [
        "effective executor=codex profile=execute native_agent=worker",
        "model=declared-model effort=high",
        "cooldown: due_ms=999 state=waiting reason=provider reset",
        "review: queued reason=capacity wait",
        "Next persisted action: review-a1 kind=review",
        "Dashboard: http://127.0.0.1:12345/",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!text.contains('\x1b'));
    value["assignments"]["tasks"][0]["current_attempt"] = json!("a101");
    let clipped = terminal::render(&value, None, false);
    assert!(clipped.contains("current attempt=a101 assignment omitted"));
    assert!(!clipped.contains("effective executor=codex"));
    assert!(!clipped.contains("review: queued"));
}

#[test]
fn closing_a_watcher_keeps_controller_and_worker_alive_then_foreground_reports_final() {
    let f = Fixture::new();
    let mut first = task("A", &[], "completed", "unaffected by watcher closure");
    first["executor"]["delay_ms"] = json!(700);
    f.import(json!([first]));
    let controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    f.await_status(|s| s["tasks"][0]["state"] == "running");
    let mut watcher = f
        .command()
        .args(["watch", "--run", "demo", "--interval-ms", "50"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // This is deliberately only the independent watcher process.
    watcher.kill().unwrap();
    watcher.wait().unwrap();
    let output = controller.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = f.snapshot();
    assert_eq!(snapshot["state"], "completed");
    assert_eq!(
        snapshot["tasks"][0]["attempts"][0]["summary"],
        "unaffected by watcher closure"
    );
    let panel = String::from_utf8(output.stderr).unwrap();
    assert!(panel.contains("Final summary | jtasks demo"));
    assert!(panel.contains("Dashboard: http://127.0.0.1:"));
    assert!(panel.contains("run=complete"));
    // Existing final JSON stdout stays consumable and contains no human panel.
    let final_json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(final_json["complete"], true);
}

#[test]
fn missing_database_watch_does_not_create_it_and_invalid_interval_is_rejected() {
    let f = Fixture::new();
    assert!(
        !f.call(&["watch", "--run", "demo", "--once"])
            .status
            .success()
    );
    assert!(!f.data.exists());
    assert!(
        !f.call(&["watch", "--run", "demo", "--interval-ms", "0"])
            .status
            .success()
    );
    assert!(!f.data.exists());
}
