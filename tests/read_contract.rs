mod support;
use jtasks::{
    domain::types::Task,
    read::{
        output,
        projections::{self, Request, View},
    },
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, Cursor},
    path::{Path, PathBuf},
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
fn call(f: &Fixture, args: &[&str]) -> Value {
    let result = f.call(args);
    assert!(
        matches!(result.status.code(), Some(0 | 3)),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn prepared(f: &Fixture) -> (Workspace, String) {
    f.import(json!([task("A", &[], "silent", "")]));
    let w = workspace(f);
    let guard = ControllerGuard::acquire(&w.git_dir).unwrap();
    let mut store = Store::open(&w, &guard).unwrap();
    let task: Task = serde_json::from_value(task("A", &[], "silent", "")).unwrap();
    store.prepare_attempt(&w, "demo", &task, "a1").unwrap();
    store.launched("a1", 42).unwrap();
    (w, "a1".into())
}

#[test]
fn project_events_are_recent_cross_task_bounded_and_read_only() {
    let f = Fixture::new();
    let (w, _) = prepared(&f);
    {
        let conn = Connection::open(w.database()).unwrap();
        conn.execute_batch(
            "INSERT INTO journal(run_id,task_id,event,detail) VALUES
            ('demo','A','activity','{\"severity\":\"info\",\"content\":\"working\"}'),
            ('demo','B','activity','{\"severity\":\"error\",\"content\":\"cross-task failure\"}'),
            ('demo','A','process_retired','failed'),
            ('demo',NULL,'recovery_decided','plain non-JSON detail');",
        )
        .unwrap();
        conn.execute("INSERT INTO runs(id,checkout,plan,state) SELECT 'other',checkout,plan,state FROM runs WHERE id='demo'", []).unwrap();
        conn.execute(
            "INSERT INTO journal(run_id,event,detail) VALUES('other','process_retired','failed')",
            [],
        )
        .unwrap();
    }
    let before = files(&w.data);
    let mut request = Request::new(View::Progress, Some("demo"));
    request.limit = 2;
    let first = projections::read(&w, &request).unwrap();
    assert_eq!(first["events"].as_array().unwrap().len(), 2);
    assert_eq!(first["events"][0]["detail"], "plain non-JSON detail");
    assert_eq!(first["events"][1]["event"], "process_retired");
    assert_eq!(first["page"]["has_more"], true);
    request.after = first["page"]["next_after"].as_u64().unwrap();
    let second = projections::read(&w, &request).unwrap();
    assert_eq!(second["events"][0]["task"], "B");
    assert_eq!(second["events"][1]["detail"]["content"], "working");
    assert!(second["events"][0]["sequence"].as_u64().unwrap() < request.after);
    request.view = View::Errors;
    request.after = 0;
    let errors = projections::read(&w, &request).unwrap();
    assert_eq!(errors["events"].as_array().unwrap().len(), 2);
    assert_eq!(errors["events"][0]["detail"], "failed");
    assert_eq!(errors["events"][1]["task"], "B");
    assert_eq!(errors["page"]["has_more"], false);
    request.after = errors["page"]["next_after"].as_u64().unwrap();
    assert!(
        projections::read(&w, &request).unwrap()["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    request.limit = 101;
    assert!(projections::read(&w, &request).is_err());
    assert_eq!(files(&w.data), before);
}

#[test]
fn elapsed_uses_execution_start_and_marks_legacy_completion_uncertainty() {
    let f = Fixture::new();
    f.import(json!([
        task("A", &[], "silent", ""),
        task("B", &[], "silent", "")
    ]));
    let w = workspace(&f);
    let request = Request::new(View::Status, Some("demo"));
    let initial = projections::read(&w, &request).unwrap();
    assert!(initial["elapsed"]["started_at"].is_null());
    assert!(initial["elapsed"]["elapsed_ms"].is_null());
    {
        let conn = Connection::open(w.database()).unwrap();
        conn.execute_batch(
            "UPDATE journal SET recorded_at='2020-01-01T00:00:00.000Z';
            INSERT INTO journal(run_id,task_id,event,detail,recorded_at) VALUES
            ('demo','A','launch_intent','persisted before spawn','2020-01-02T00:00:00.000Z'),
            ('demo','B','launch_intent','parallel task','2020-01-02T00:00:00.000Z'),
            ('demo','B','process_retired','completed','2020-01-02T01:00:00.000Z'),
            ('demo','A','process_retired','completed','2020-01-02T01:00:00.000Z');",
        )
        .unwrap();
    }
    let running = projections::read(&w, &request).unwrap();
    assert_eq!(running["elapsed"]["started_at"], "2020-01-02T00:00:00.000Z");
    assert_eq!(running["elapsed"]["measurement"], "wall_elapsed");
    assert!(running["elapsed"]["elapsed_ms"].as_i64().unwrap() > 3_600_000);
    Connection::open(w.database())
        .unwrap()
        .execute("UPDATE runs SET state='completed' WHERE id='demo'", [])
        .unwrap();
    let before = files(&w.data);
    let completed = projections::read(&w, &request).unwrap();
    // Two concurrent hour-long intervals still span one wall-clock hour.
    assert_eq!(completed["elapsed"]["elapsed_ms"], 3_600_000);
    assert_eq!(completed["elapsed"]["measurement"], "recorded_span");
    assert!(
        completed["elapsed"]["end_source"]
            .as_str()
            .unwrap()
            .contains("exact completion time was not recorded")
    );
    assert_eq!(files(&w.data), before);
}

#[test]
fn all_read_surfaces_share_identity_snapshot_and_never_write_or_contact_control() {
    let f = Fixture::new();
    let (w, _) = prepared(&f);
    {
        let conn = Connection::open(w.database()).unwrap();
        conn.execute_batch("CREATE TABLE managed_commands(id TEXT PRIMARY KEY,request_id TEXT,run_id TEXT,task_id TEXT,attempt_id TEXT,state TEXT,pid INTEGER,exit_code INTEGER,reason TEXT);INSERT INTO managed_commands VALUES('c1','request1','demo','A','a1','running',42,NULL,NULL);").unwrap();
        jtasks::controller::questions::initialize(&conn).unwrap();
        conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES('demo','A','a1','autonomous_decision',?1)",[json!({"request_id":"d1","situation":"Equivalent choices","decision":"Reuse existing implementation","deviation":"none"}).to_string()]).unwrap();
    }
    fs::create_dir_all(w.data.join("commands/c1")).unwrap();
    fs::write(w.data.join("commands/c1/stdout.log"), "retained\n").unwrap();
    // Invalid endpoints and credential-shaped content must never be consulted by reads.
    fs::write(w.data.join("control.json"), "DO_NOT_LEAK").unwrap();
    fs::write(w.data.join("commands-control.json"), "DO_NOT_LEAK").unwrap();
    fs::create_dir_all(jtasks::transport::run_folder(&w.data, "demo")).unwrap();
    fs::write(
        jtasks::transport::control_file(&w.data, "demo"),
        "DO_NOT_LEAK",
    )
    .unwrap();
    fs::write(
        jtasks::transport::commands_file(&w.data, Some("demo")),
        "DO_NOT_LEAK",
    )
    .unwrap();
    let before = files(&w.data);
    let mut common = None;
    for args in [
        vec!["status", "--run", "demo"],
        vec!["tasks", "--run", "demo"],
        vec!["task", "A", "--run", "demo"],
        vec!["attempt", "a1", "--run", "demo"],
        vec!["activity", "--run", "demo", "--task", "A"],
        vec!["operations", "status", "--run", "demo"],
        vec!["decisions", "list", "--run", "demo"],
        vec!["questions", "--run", "demo"],
        vec!["commands", "--run", "demo"],
        vec!["exec", "status", "c1"],
        vec!["exec", "output", "c1", "--limit", "3"],
        vec!["read", "attempts", "--run", "demo", "--task", "A"],
    ] {
        for _ in 0..2 {
            let value = call(&f, &args);
            assert_eq!(value["contract_version"], 1);
            assert_eq!(value["identity"]["run"], "demo");
            assert_eq!(value["read_only"], true);
            assert_eq!(value["native_helpers"]["state"], "unobserved");
            assert!(value["native_helpers"]["count"].is_null());
            assert!(!value.to_string().contains("DO_NOT_LEAK"));
            let metadata = json!([
                value["identity"],
                value["graph_revision"],
                value["event_cursor"],
                value["as_of"],
                value["availability"]
            ]);
            if let Some(expected) = &common {
                assert_eq!(&metadata, expected);
            } else {
                common = Some(metadata);
            }
        }
    }
    let attempt = call(&f, &["attempt", "a1", "--run", "demo"]);
    assert!(attempt["attempt"]["capture"]["raw_retained"].is_null());
    assert!(attempt["attempt"]["effective_profile"]["origins"].is_object());
    assert_eq!(
        attempt["attempt"]["retry_authorizations"]["state"],
        "available"
    );
    assert_eq!(
        attempt["attempt"]["reservations"]["state"],
        "not_initialized"
    );
    assert_eq!(files(&w.data), before);
    let conn = projections::open(&w).unwrap();
    assert!(
        conn.execute(
            "INSERT INTO journal(run_id,event,detail) VALUES('demo','forbidden','read')",
            []
        )
        .is_err()
    );
}

#[test]
fn missing_auxiliary_tables_are_unknown_and_reads_do_not_initialize_them() {
    let f = Fixture::new();
    let w = workspace(&f);
    fs::create_dir_all(&w.data).unwrap();
    let plan = json!({"schema_version":1,"completion":"judgment","operation_placement":"none","tasks":[task("A",&[],"silent","")]});
    {
        let conn = Connection::open(w.database()).unwrap();
        conn.execute_batch(jtasks::storage::schema::SQL).unwrap();
        conn.execute(
            "INSERT INTO runs VALUES('demo',?1,?2,'stopped')",
            params![w.checkout.to_string_lossy(), plan.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks VALUES('demo','A',0,?1,'pending')",
            [plan["tasks"][0].to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attempts(id,run_id,task_id,state) VALUES('a1','demo','A','uncertain')",
            [],
        )
        .unwrap();
    }
    let before = files(&w.data);
    let status = call(&f, &["status", "--run", "demo"]);
    assert!(status["as_of"].is_null());
    assert!(status["graph_revision"].is_null());
    assert_eq!(
        status["availability"]["auxiliary"]["execution_reviews"],
        "not_initialized"
    );
    let activity = call(&f, &["activity", "--run", "demo", "--task", "A"]);
    assert!(activity["clock"].is_null());
    assert_eq!(
        activity["attempts"][0]["silence_baseline"],
        "not_initialized"
    );
    let attempt = call(&f, &["attempt", "a1", "--run", "demo"]);
    assert_eq!(attempt["attempt"]["review"]["state"], "not_initialized");
    assert_eq!(attempt["attempt"]["commands"]["state"], "not_initialized");
    assert_eq!(
        call(&f, &["questions", "--run", "demo"])["records"],
        json!([])
    );
    let logs = call(&f, &["logs", "--run", "demo", "--attempt", "a1"]);
    assert_eq!(
        logs["availability"],
        json!({"runtime":"available","controller":"unobserved","persisted_run_state":"stopped","auxiliary":status["availability"]["auxiliary"]})
    );
    assert_eq!(logs["capture_state"], "unknown");
    assert_eq!(logs["gap"]["reason"], "raw_capture_missing");
    assert_eq!(files(&w.data), before);
}

#[test]
fn byte_paging_is_lossless_bounded_and_reports_missing_invalid_and_io_errors() {
    let f = Fixture::new();
    let (w, _) = prepared(&f);
    let dir = w.data.join("attempts/a1");
    fs::create_dir_all(&dir).unwrap();
    let original = "é🦀 raw\n".repeat(20000).into_bytes();
    let raw = dir.join("stdout.log");
    fs::write(&raw, &original).unwrap();
    let mut request = Request::new(View::Logs, Some("demo"));
    request.attempt = Some("a1".into());
    request.limit = 65536;
    let mut bytes = Vec::new();
    loop {
        let value = projections::read(&w, &request).unwrap();
        let chunk: Vec<u8> = serde_json::from_value(value["bytes"].clone()).unwrap();
        assert!(chunk.len() <= 65536);
        bytes.extend(chunk);
        request.after = value["next_offset"].as_u64().unwrap();
        if value["has_more"] == false {
            break;
        }
    }
    assert_eq!(bytes, original);
    assert_eq!(fs::read(&raw).unwrap(), original);
    // Deliberately split a UTF-8 scalar: preview can be lossy, byte cursor cannot.
    request.after = 1;
    request.limit = 1;
    assert_eq!(
        projections::read(&w, &request).unwrap()["bytes"],
        json!([0xa9])
    );
    request.after = original.len() as u64 + 1;
    assert!(projections::read(&w, &request).is_err());
    request.after = 0;
    request.limit = 65537;
    assert!(projections::read(&w, &request).is_err());
    request.limit = 10;
    request.stream = "../control".into();
    assert!(projections::read(&w, &request).is_err());
    request.stream = "stderr".into();
    let missing = projections::read(&w, &request).unwrap();
    assert_eq!(missing["capture_state"], "unknown");
    assert!(missing["captured_bytes"].is_null());
    fs::create_dir(dir.join("stderr.log")).unwrap();
    assert!(projections::read(&w, &request).is_err());
    let result = f.call(&[
        "--json",
        "logs",
        "--run",
        "demo",
        "--attempt",
        "a1",
        "--after",
        "999999999",
    ]);
    assert_eq!(result.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(error["error"]["code"], "read_or_command_failed");
    assert!(result.stderr.is_empty());
}

#[test]
fn entity_and_journal_cursors_are_bounded_revision_scoped_and_unambiguous() {
    let f = Fixture::new();
    f.import(json!([
        task("A", &[], "silent", ""),
        task("B", &[], "silent", ""),
        task("C", &[], "silent", "")
    ]));
    let w = workspace(&f);
    let first = call(&f, &["tasks", "--run", "demo", "--limit", "2"]);
    assert_eq!(first["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(first["page"]["has_more"], true);
    let second = call(
        &f,
        &[
            "tasks",
            "--run",
            "demo",
            "--limit",
            "2",
            "--after",
            "2",
            "--revision",
            "0",
        ],
    );
    assert_eq!(second["tasks"][0]["id"], "C");
    assert_eq!(second["page"]["has_more"], false);
    assert!(
        projections::read(
            &w,
            &Request {
                revision: Some(999),
                ..Request::new(View::Tasks, Some("demo"))
            }
        )
        .is_err()
    );
    {
        let conn = Connection::open(w.database()).unwrap();
        for id in 0..4 {
            conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES('demo','A','fixture','autonomous_decision',?1)",[json!({"request_id":format!("d{id}"),"decision":"explicit"}).to_string()]).unwrap();
        }
    }
    let first = call(&f, &["read", "decisions", "--run", "demo", "--limit", "2"]);
    let after = first["page"]["next_after"].as_u64().unwrap();
    assert_eq!(first["page"]["has_more"], true);
    let second = call(
        &f,
        &[
            "read",
            "decisions",
            "--run",
            "demo",
            "--limit",
            "2",
            "--after",
            &after.to_string(),
        ],
    );
    assert_eq!(second["records"].as_array().unwrap().len(), 2);
    assert_eq!(second["page"]["has_more"], false);
    assert!(second["records"][0]["sequence"].as_u64().unwrap() > after);
    let imported = f.call(&[
        "import",
        f.root.join("plan.json").to_str().unwrap(),
        "--run",
        "other",
    ]);
    assert!(imported.status.success());
    let runs = call(&f, &["runs"]);
    assert_eq!(runs["runs"].as_array().unwrap().len(), 2);
    assert!(projections::read(&w, &Request::new(View::Tasks, None)).is_err());
    assert!(
        projections::read(
            &w,
            &Request {
                limit: 101,
                ..Request::new(View::Tasks, Some("demo"))
            }
        )
        .is_err()
    );
}

#[test]
fn all_native_decoders_bound_records_retain_raw_and_report_exact_gap_offsets() {
    let f = Fixture::new();
    let dir = f.root.join("native");
    fs::create_dir(&dir).unwrap();
    let cases = [
        (
            "codex",
            json!({"type":"thread.started","thread_id":"thread-1"}),
            json!({"type":"turn.started"}),
        ),
        (
            "claude",
            json!({"type":"system","subtype":"init","session_id":"550e8400-e29b-41d4-a716-446655440000"}),
            json!({"type":"assistant","message":{"content":[]}}),
        ),
        (
            "opencode",
            json!({"type":"step_start","sessionID":"ses_fixture"}),
            json!({"type":"step_finish","sessionID":"ses_fixture"}),
        ),
    ];
    for (kind, first, last) in cases {
        let mut raw = format!("{first}\n").into_bytes();
        let start = raw.len() as u64;
        raw.extend(vec![b'x'; 4 * 1024 * 1024]);
        raw.push(b'\n');
        let end = raw.len() as u64;
        raw.extend(format!("{last}\nnot-json\npartial").as_bytes());
        fs::write(dir.join("stdout.log"), &raw).unwrap();
        fs::write(dir.join("stderr.log"), "").unwrap();
        let value = match kind {
            "codex" => serde_json::to_value(
                jtasks::executors::codex::observe(&dir, "A", "a1")
                    .unwrap()
                    .0,
            )
            .unwrap(),
            "claude" => serde_json::to_value(
                jtasks::executors::claude::observe(&dir, "A", "a1")
                    .unwrap()
                    .0,
            )
            .unwrap(),
            _ => {
                serde_json::to_value(jtasks::executors::opencode::observe(&dir, "A", "a1").unwrap())
                    .unwrap()
            }
        };
        assert_eq!(value["decoding"]["scanned_bytes"], raw.len() as u64);
        assert_eq!(
            value["decoding"]["gaps"][0],
            json!({"start":start,"end":end,"reason":"oversized_record"})
        );
        assert_eq!(value["decoding"]["gaps"][1]["reason"], "invalid_json");
        assert_eq!(value["decoding"]["gaps"][2]["reason"], "incomplete_record");
        assert!(value["protocol_error"].is_string());
        assert_eq!(value["invalid_records"], 3);
        assert_eq!(fs::read(dir.join("stdout.log")).unwrap(), raw);
    }
    let oversized = vec![b'x'; output::MAX_RECORD_BYTES + 1];
    let input = [oversized.as_slice(), b"\n"].concat().repeat(40);
    let mut reader = output::Records::new(BufReader::with_capacity(17, Cursor::new(input)));
    while let Some(record) = reader.next_record().unwrap() {
        assert!(record.bytes.is_none());
    }
    assert_eq!(reader.provenance().gaps.len(), 32);
    assert_eq!(reader.provenance().omitted_gaps, 8);
}

#[test]
fn a_missing_database_is_an_error_without_creating_runtime_state() {
    let f = Fixture::new();
    let w = workspace(&f);
    assert!(!w.data.exists());
    assert!(projections::read(&w, &Request::new(View::Runs, None)).is_err());
    assert!(!w.data.exists());
}

#[test]
fn completed_development_and_required_final_verification_remain_separate() {
    let f = Fixture::new();
    let path = f.root.join("plan.json");
    fs::write(&path,json!({"schema_version":1,"completion":"judgment","operation_placement":"final","tasks":[task("A",&[],"completed","accepted")],"operations":[{"id":"final","stage":{"kind":"final","tasks":["A"]},"required":true,"revision":"r1","inputs":["src"],"repair_task":"A","initial":{"program":"uninvoked-fixture","cwd":"."}}]}).to_string()).unwrap();
    let imported = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
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
                &jtasks::domain::types::Receipt {
                    receipt_id: "r1".into(),
                    attempt_id: "a1".into(),
                    task_id: "A".into(),
                    outcome: jtasks::domain::types::Outcome::Completed,
                    summary: "Development accepted by designated judgment".into(),
                },
            )
            .unwrap();
        store.retire("demo", "A", "a1", Some(0), false).unwrap();
    }
    let status = call(&f, &["status", "--run", "demo"]);
    assert_eq!(status["development_complete"], true);
    assert_eq!(status["complete"], false);
    assert_eq!(status["verification_complete"], false);
    assert_eq!(status["pending_required_operations"], 1);
    assert_eq!(status["pending_verification"], true);
    assert_eq!(status["task_counts"]["completed"], 1);
    let operation = call(&f, &["operations", "status", "--run", "demo"]);
    assert_eq!(operation["pending_verification"], true);
    assert_eq!(operation["operations"][0]["state"], "pending");
    let attempt = call(&f, &["attempt", "a1", "--run", "demo"]);
    assert_eq!(attempt["attempt"]["result_provenance"]["receipt_id"], "r1");
}
