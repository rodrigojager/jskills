mod support;
use jtasks::{
    controller::commands::{Action, Endpoint, Request},
    execution::commands::CommandSpec,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    net::TcpStream,
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

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
    let server = Server(
        f.command()
            .args(["exec", "serve"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while !f.data.join("commands-control.json").exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    server
}
fn setup() -> Fixture {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    f
}
fn spec(f: &Fixture, request: &str, delay: u64) -> CommandSpec {
    CommandSpec {
        request_id: request.into(),
        run: "demo".into(),
        task: None,
        attempt: None,
        operation: Some("verification".into()),
        program: env!("CARGO_BIN_EXE_jtasks").into(),
        args: vec![
            "fixture".into(),
            "--outcome".into(),
            "prose".into(),
            "--summary".into(),
            "é literal $(text)".into(),
            "--delay-ms".into(),
            delay.to_string(),
        ],
        script: None,
        cwd: f.root.clone(),
        environment_refs: BTreeMap::new(),
        resources: vec![],
        deadline_ms: None,
    }
}
fn request_file(f: &Fixture, spec: &CommandSpec) -> String {
    let path = f.root.join("command.json");
    fs::write(&path, serde_json::to_vec(spec).unwrap()).unwrap();
    path.to_str().unwrap().into()
}
fn call(f: &Fixture, args: &[&str]) -> Value {
    let output = f.call(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn client_exit_wait_expiry_replay_conflict_and_byte_cursors() {
    let f = setup();
    let mut server = serve(&f);
    let specification = spec(&f, "lost-receipt", 500);
    let path = request_file(&f, &specification);
    let endpoint: Endpoint =
        serde_json::from_slice(&fs::read(f.data.join("commands-control.json")).unwrap()).unwrap();
    // Simulate a client that exits after sending intent but before reading its receipt.
    let mut stream = TcpStream::connect(endpoint.endpoint).unwrap();
    jtasks::transport::write_message(
        &mut stream,
        &Request {
            token: endpoint.token.clone(),
            action: Action::Start {
                spec: specification.clone(),
            },
        },
    )
    .unwrap();
    drop(stream);
    let replay = call(&f, &["exec", "start", "--request", &path, "--json"]);
    assert_eq!(replay["duplicate"], true);
    let id = replay["command_id"].as_str().unwrap();
    let expired = call(&f, &["exec", "wait", id, "--wait-seconds", "0"]);
    assert_eq!(expired["wait_expired"], true);
    // Pure reads cannot prove current live custody, even while the controller
    // is serving. Keep its persisted pending state separate and continue the
    // bounded client wait until a terminal receipt is committed.
    assert_eq!(expired["state"], "unknown");
    assert_eq!(expired["recorded_state"], "running");
    assert_eq!(expired["availability"]["controller"], "unobserved");
    let terminal = call(&f, &["exec", "wait", id, "--wait-seconds", "5"]);
    assert_eq!(terminal["state"], "exited");
    assert_eq!(terminal["exit_code"], 0);
    assert_eq!(terminal["wait_expired"], false);
    let mut offset = 0;
    let mut collected = Vec::new();
    loop {
        let page = call(
            &f,
            &[
                "exec",
                "output",
                id,
                "--offset",
                &offset.to_string(),
                "--limit",
                "1",
            ],
        );
        let bytes: Vec<u8> = serde_json::from_value(page["bytes"].clone()).unwrap();
        if bytes.is_empty() {
            break;
        }
        collected.extend(bytes);
        offset = page["next_offset"].as_u64().unwrap();
    }
    assert_eq!(
        String::from_utf8(collected).unwrap().trim(),
        "Completed: é literal $(text)"
    );
    assert!(
        !f.call(&["exec", "output", id, "--limit", "65537"])
            .status
            .success()
    );
    let mut conflict = specification;
    conflict.args.push("conflicting".into());
    request_file(&f, &conflict);
    assert!(
        !f.call(&["exec", "start", "--request", &path])
            .status
            .success()
    );
    let db = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM managed_commands", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    call(&f, &["exec", "shutdown"]);
    assert!(server.0.wait().unwrap().success());
    let before = fs::read(f.data.join("state.sqlite3")).unwrap();
    assert_eq!(call(&f, &["exec", "status", id])["state"], "exited");
    assert_eq!(fs::read(f.data.join("state.sqlite3")).unwrap(), before);
}

#[test]
fn explicit_stop_and_command_deadline_are_separate_from_wait_timeout() {
    let f = setup();
    let mut server = serve(&f);
    for (request, deadline) in [("stop", None), ("deadline", Some(50))] {
        let mut specification = spec(&f, request, 5000);
        specification.deadline_ms = deadline;
        let path = request_file(&f, &specification);
        let receipt = call(&f, &["exec", "start", "--request", &path]);
        let id = receipt["command_id"].as_str().unwrap();
        if deadline.is_none() {
            call(&f, &["exec", "stop", id, "--reason", "test explicit stop"]);
        }
        let terminal = call(&f, &["exec", "wait", id, "--wait-seconds", "5"]);
        assert_eq!(terminal["state"], "stopped");
        assert_eq!(
            terminal["reason"],
            if deadline.is_some() {
                "command_deadline"
            } else {
                "test explicit stop"
            }
        );
    }
    call(&f, &["exec", "shutdown"]);
    assert!(server.0.wait().unwrap().success());
}

#[test]
fn uncertain_intent_is_not_repeated_and_offline_reads_do_not_reconcile() {
    let f = setup();
    let mut server = serve(&f);
    call(&f, &["exec", "shutdown"]);
    assert!(server.0.wait().unwrap().success());
    let specification = spec(&f, "uncertain", 0);
    let db = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    db.execute("INSERT INTO managed_commands(id,request_id,run_id,spec,state) VALUES('uncertain-handle','uncertain','demo',?1,'intent')",[serde_json::to_string(&specification).unwrap()]).unwrap();
    assert_eq!(
        call(&f, &["exec", "status", "uncertain-handle"])["state"],
        "unknown"
    );
    assert_eq!(
        db.query_row("SELECT state FROM managed_commands", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "intent"
    );
    // Starting another foreground controller reconciles visibility, never relaunches.
    let mut server = Server(
        f.command()
            .args(["exec", "serve"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let path = request_file(&f, &specification);
    let started = Instant::now();
    let replay = loop {
        let result = f.call(&["exec", "start", "--request", &path]);
        if result.status.success() {
            break serde_json::from_slice::<Value>(&result.stdout).unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(replay["command_id"], "uncertain-handle");
    assert_eq!(replay["state"], "unknown");
    assert_eq!(replay["duplicate"], true);
    assert!(!f.data.join("commands/uncertain-handle").exists());
    assert!(
        !f.call(&[
            "exec",
            "stop",
            "uncertain-handle",
            "--reason",
            "cannot prove stopped"
        ])
        .status
        .success()
    );
    call(&f, &["exec", "shutdown"]);
    assert!(server.0.wait().unwrap().success());
}

#[test]
fn foreground_run_owns_followup_commands_after_initial_worker_finishes() {
    let f = Fixture::new();
    let mut worker = task("A", &[], "completed", "structured completion");
    worker["executor"]["delay_ms"] = json!(1000);
    f.import(json!([worker]));
    let mut server = Server(
        f.command()
            .args(["run", "--run", "demo"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while !jtasks::transport::commands_file(&f.data, Some("demo")).exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    let specification = spec(&f, "followup-in-controller", 1500);
    let path = request_file(&f, &specification);
    let receipt = call(&f, &["exec", "start", "--request", &path]);
    let id = receipt["command_id"].as_str().unwrap();
    let terminal = call(&f, &["exec", "wait", id, "--wait-seconds", "5"]);
    assert_eq!(terminal["state"], "exited");
    assert!(server.0.wait().unwrap().success());
    assert_eq!(f.snapshot()["state"], "completed");
}

#[test]
fn explicit_shell_and_launch_failure_have_durable_receipts() {
    let f = setup();
    let mut server = serve(&f);
    let mut specification = spec(&f, "shell", 0);
    #[cfg(windows)]
    {
        specification.program = "cmd.exe".into();
        specification.args = vec!["/D".into(), "/C".into()];
    }
    #[cfg(unix)]
    {
        specification.program = "/bin/sh".into();
        specification.args = vec!["-c".into()];
    }
    specification.script = Some("echo shell-value".into());
    let path = request_file(&f, &specification);
    let receipt = call(&f, &["exec", "start", "--request", &path]);
    let id = receipt["command_id"].as_str().unwrap();
    assert_eq!(
        call(&f, &["exec", "wait", id, "--wait-seconds", "5"])["state"],
        "exited"
    );
    let page = call(&f, &["exec", "output", id]);
    let bytes: Vec<u8> = serde_json::from_value(page["bytes"].clone()).unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap().trim(), "shell-value");
    specification.request_id = "missing-program".into();
    specification.program = f.root.join("missing-program").to_str().unwrap().into();
    let path = request_file(&f, &specification);
    let failure = call(&f, &["exec", "start", "--request", &path]);
    assert_eq!(failure["state"], "launch_failed");
    assert_eq!(
        call(&f, &["exec", "start", "--request", &path])["command_id"],
        failure["command_id"]
    );
    call(&f, &["exec", "shutdown"]);
    assert!(server.0.wait().unwrap().success());
}
