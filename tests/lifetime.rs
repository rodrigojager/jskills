mod support;
use jtasks::{
    execution::{commands::CommandSpec, custody::OwnedProcess},
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
    }
}
fn until(mut check: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(Instant::now() < end, "isolated fixture deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn wait(process: &mut Process) -> std::process::ExitStatus {
    let mut status = None;
    until(|| {
        status = process.0.try_wait().unwrap();
        status.is_some()
    });
    status.unwrap()
}
fn fixture_command(directory: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "custody_fixture_process", "--nocapture"])
        .env("JTASKS_LIFETIME_MODE", mode)
        .env("JTASKS_LIFETIME_DIRECTORY", directory)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
// A bounded subprocess fixture used by real worker/command launches. The test
// parent never discovers or terminates anything by executable name or PID scan.
#[test]
fn custody_fixture_process() {
    let Ok(mode) = std::env::var("JTASKS_LIFETIME_MODE") else {
        return;
    };
    let directory = PathBuf::from(std::env::var_os("JTASKS_LIFETIME_DIRECTORY").unwrap());
    fs::create_dir_all(&directory).unwrap();
    if mode == "leaf" {
        fs::write(directory.join("leaf.pid"), std::process::id().to_string()).unwrap();
        let end = Instant::now() + Duration::from_secs(12);
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(directory.join("heartbeat"))
            .unwrap();
        while Instant::now() < end {
            file.write_all(b".").unwrap();
            file.flush().unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
    } else {
        let mut child = fixture_command(&directory, "leaf").spawn().unwrap();
        until(|| directory.join("heartbeat").is_file());
        fs::write(directory.join("leader.ready"), b"ready").unwrap();
        if mode == "linger" {
            child.wait().unwrap();
        }
        // leader_exit intentionally leaves its descendant for custody to stop.
    }
}

#[cfg(windows)]
struct Leaf(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Leaf {
    fn observe(directory: &Path) -> Self {
        use windows_sys::Win32::System::Threading::*;
        until(|| directory.join("leader.ready").is_file());
        let pid = fs::read_to_string(directory.join("leaf.pid"))
            .unwrap()
            .parse()
            .unwrap();
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        assert!(
            !handle.is_null(),
            "leaf must still exist when observation begins"
        );
        Self(handle)
    }
    fn stopped(&self) -> bool {
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0)
                == windows_sys::Win32::Foundation::WAIT_OBJECT_0
        }
    }
}
#[cfg(windows)]
impl Drop for Leaf {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(target_os = "linux")]
struct Leaf(PathBuf);
#[cfg(target_os = "linux")]
impl Leaf {
    fn observe(directory: &Path) -> Self {
        until(|| directory.join("leader.ready").is_file());
        Self(PathBuf::from(format!(
            "/proc/{}/stat",
            fs::read_to_string(directory.join("leaf.pid")).unwrap()
        )))
    }
    fn stopped(&self) -> bool {
        fs::read_to_string(&self.0)
            .map(|s| {
                s.rsplit_once(") ")
                    .is_some_and(|(_, s)| s.starts_with('Z') || s.starts_with('X'))
            })
            .unwrap_or(true)
    }
}

#[test]
fn leader_exit_is_not_retirement_until_actual_descendant_stops() {
    let f = Fixture::new();
    let directory = f.root.join("tree");
    let mut command = fixture_command(&directory, "leader_exit");
    let mut process = OwnedProcess::spawn(&mut command, &f.root.join("custody")).unwrap();
    // Parent helper remains alive until the leaf starts; poll can safely observe
    // leader exit even when the OS has already ended that direct child.
    until(|| directory.join("leader.ready").is_file());
    let heartbeat = directory.join("heartbeat");
    until(|| process.poll().unwrap().is_some());
    let before = fs::read(&heartbeat).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::read(&heartbeat).unwrap(), before);
    let record: Value =
        serde_json::from_slice(&fs::read(f.root.join("custody/custody.json")).unwrap()).unwrap();
    assert_eq!(record["bound"], true);
    assert_eq!(record["empty"], true);
    assert!(record["start"].is_string());
}

fn server(f: &Fixture, directory: &Path) -> Process {
    let mut command = f.command();
    command
        .args(["exec", "serve"])
        .env("JTASKS_SHUTDOWN_GRACE_MS", "20")
        .env("JTASKS_SHUTDOWN_FORCE_MS", "1500")
        .env("JTASKS_LIFETIME_MODE", "linger")
        .env("JTASKS_LIFETIME_DIRECTORY", directory)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Process(command.spawn().unwrap())
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
fn start(f: &Fixture, specification: &CommandSpec) -> Value {
    let path = f.root.join("command.json");
    fs::write(&path, serde_json::to_vec(specification).unwrap()).unwrap();
    let mut value = None;
    until(|| {
        let output = f.call(&["exec", "start", "--request", path.to_str().unwrap()]);
        if output.status.success() {
            value = Some(serde_json::from_slice(&output.stdout).unwrap());
            true
        } else {
            false
        }
    });
    value.unwrap()
}
#[test]
fn managed_commands_stop_descendants_and_reconcile_controller_loss_without_relaunch() {
    for mode in ["stop", "deadline", "crash"] {
        let f = Fixture::new();
        f.import(json!([task("A", &[], "silent", "")]));
        let directory = f.root.join("command-tree");
        let mut service = server(&f, &directory);
        let specification = CommandSpec {
            request_id: "owned-tree".into(),
            run: "demo".into(),
            task: None,
            attempt: None,
            operation: Some("check".into()),
            program: std::env::current_exe().unwrap().to_string_lossy().into(),
            args: vec![
                "--exact".into(),
                "custody_fixture_process".into(),
                "--nocapture".into(),
            ],
            script: None,
            cwd: f.root.clone(),
            environment_refs: BTreeMap::new(),
            resources: vec![],
            deadline_ms: (mode == "deadline").then_some(1000),
        };
        let receipt = start(&f, &specification);
        let id = receipt["command_id"].as_str().unwrap();
        let leaf = Leaf::observe(&directory);
        if mode == "crash" {
            service.0.kill().unwrap();
            wait(&mut service);
            #[cfg(windows)]
            until(|| leaf.stopped());
            // Linux retains unknown while a historical group remains; this test
            // exercises the Windows kill-on-controller-close reconciliation.
            #[cfg(target_os = "linux")]
            {
                continue;
            }
            #[cfg(windows)]
            {
                service = server(&f, &directory);
                let replay = start(&f, &specification);
                assert_eq!(replay["command_id"], id);
                assert_eq!(replay["duplicate"], true);
                assert_eq!(replay["state"], "stopped");
            }
        } else {
            if mode == "stop" {
                call(&f, &["exec", "stop", id, "--reason", "isolated stop"]);
            }
            let terminal = call(&f, &["exec", "wait", id, "--wait-seconds", "5"]);
            assert_eq!(terminal["state"], "stopped");
            until(|| leaf.stopped());
        }
        let db = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM managed_commands", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        call(&f, &["exec", "shutdown"]);
        assert!(wait(&mut service).success());
    }
}

#[test]
fn receipt_gap_remains_unknown_and_does_not_touch_unrelated_live_process_or_edits() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "completed", "must not launch")]));
    let unrelated_directory = f.root.join("unrelated");
    fs::create_dir_all(&unrelated_directory).unwrap();
    let mut unrelated = Process(
        fixture_command(&unrelated_directory, "leaf")
            .spawn()
            .unwrap(),
    );
    until(|| unrelated_directory.join("heartbeat").exists());
    fs::write(f.root.join("user-edit.txt"), b"preserve me").unwrap();
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    {
        let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
        Store::open(&workspace, &guard)
            .unwrap()
            .intent("demo", "A", "receipt-gap")
            .unwrap();
    }
    // No binding receipt exists. Even repeated explicit resume may not invent one.
    for _ in 0..2 {
        let output = f.call(&["run", "--run", "demo"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("custody is unknown"));
        assert_eq!(
            f.snapshot()["tasks"][0]["attempts"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            f.snapshot()["tasks"][0]["attempts"][0]["process_state"],
            "uncertain"
        );
        assert!(unrelated.0.try_wait().unwrap().is_none());
        assert_eq!(
            fs::read(f.root.join("user-edit.txt")).unwrap(),
            b"preserve me"
        );
    }
}

#[test]
fn canonical_aliases_share_existing_mutex_while_independent_checkouts_do_not() {
    let f = Fixture::new();
    let second = Fixture::new();
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let alias = Workspace::resolve(&f.root.join("."), Some(&f.root.join("other-data"))).unwrap();
    assert_eq!(workspace.git_dir, alias.git_dir);
    let first = ControllerGuard::acquire(&workspace.git_dir).unwrap();
    assert!(ControllerGuard::acquire(&alias.git_dir).is_err());
    let other = Workspace::resolve(&second.root, Some(&second.data)).unwrap();
    let _other = ControllerGuard::acquire(&other.git_dir).unwrap();
    drop(first);
    assert!(ControllerGuard::acquire(&alias.git_dir).is_ok());
}

#[cfg(windows)]
#[test]
fn worker_controller_loss_kills_descendants_and_explicit_resume_preserves_edits() {
    let f = Fixture::new();
    let directory = f.root.join("worker-tree");
    let script = f.root.join("worker.ps1");
    fs::write(&script, "$ErrorActionPreference='Stop'\nAdd-Content -LiteralPath (Join-Path $env:JTASKS_LIFETIME_DIRECTORY '../edits.txt') -Value 'worker edit'\n& $env:JTASKS_LIFETIME_TEST_EXE --exact custody_fixture_process --nocapture\n& $env:JTASKS_BIN result --receipt ('result-'+$env:JTASKS_ATTEMPT_ID) --outcome completed --summary 'resumed same task after proven cleanup'\n").unwrap();
    fs::create_dir_all(&directory).unwrap();
    fs::write(f.root.join("edits.txt"), "user edit\n").unwrap();
    let worker = json!({"id":"A","title":"Owned worker","executor":{"kind":"codex","executable":"pwsh","launcher_args":["-NoLogo","-NoProfile","-File",script]}});
    f.import(json!([worker]));
    let controller = |mode: &str| {
        let mut command = f.command();
        command
            .args(["run", "--run", "demo"])
            .env("JTASKS_LIFETIME_MODE", mode)
            .env("JTASKS_LIFETIME_DIRECTORY", &directory)
            .env("JTASKS_LIFETIME_TEST_EXE", std::env::current_exe().unwrap())
            .env("JTASKS_SHUTDOWN_GRACE_MS", "20")
            .env("JTASKS_SHUTDOWN_FORCE_MS", "1500")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Process(command.spawn().unwrap())
    };
    let mut first = controller("linger");
    let leaf = Leaf::observe(&directory);
    let before = f.snapshot();
    let duplicate = f.call(&["run", "--run", "demo"]);
    let discovery = String::from_utf8_lossy(&duplicate.stderr);
    assert!(!duplicate.status.success());
    assert!(
        discovery.contains("discovery=")
            && discovery.contains("endpoint")
            && discovery.contains("start")
    );
    let old = before["tasks"][0]["attempts"][0]["id"].as_str().unwrap();
    first.0.kill().unwrap();
    wait(&mut first);
    until(|| leaf.stopped());
    let mut resumed = controller("leader_exit");
    assert!(wait(&mut resumed).success());
    let snapshot = f.snapshot();
    let attempts = snapshot["tasks"][0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0]["process_state"], "interrupted");
    assert_ne!(attempts[0]["id"], attempts[1]["id"]);
    assert_eq!(snapshot["state"], "completed");
    let edits = fs::read_to_string(f.root.join("edits.txt")).unwrap();
    assert!(edits.starts_with("user edit"));
    assert_eq!(edits.matches("worker edit").count(), 2);
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let guard = ControllerGuard::acquire(&workspace.git_dir).unwrap();
    let mut store = Store::open(&workspace, &guard).unwrap();
    let stale = jtasks::domain::types::Receipt {
        receipt_id: "stale-old-event".into(),
        attempt_id: old.into(),
        task_id: "A".into(),
        outcome: jtasks::domain::types::Outcome::Completed,
        summary: "old event rejected".into(),
    };
    assert!(store.accept("demo", old, &stale).is_err());
}

// This helper only runs inside a fresh hidden console made below. CTRL_C_EVENT
// with group 0 reaches that private console, never the user's terminal/session.
#[cfg(windows)]
#[test]
fn isolated_ctrlc_helper() {
    if std::env::var("JTASKS_PRIVATE_TEST_CONSOLE").as_deref() != Ok("1") {
        return;
    }
    unsafe extern "system" fn ignore(_: u32) -> i32 {
        1
    }
    use windows_sys::Win32::System::Console::*;
    assert_ne!(unsafe { SetConsoleCtrlHandler(Some(ignore), 1) }, 0);
    for mode in ["worker", "idle_commands", "live_commands"] {
        let f = Fixture::new();
        let mut worker = task("A", &[], "completed", "interrupted before outcome");
        worker["executor"]["delay_ms"] = json!(5000);
        f.import(json!([worker]));
        let directory = f.root.join("signal-tree");
        let mut controller = if mode == "worker" {
            let mut command = f.command();
            command
                .args(["run", "--run", "demo"])
                .env("JTASKS_SHUTDOWN_GRACE_MS", "20")
                .env("JTASKS_SHUTDOWN_FORCE_MS", "1500")
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            Process(command.spawn().unwrap())
        } else {
            server(&f, &directory)
        };
        let mut leaf = None;
        if mode == "worker" {
            f.await_status(|s| {
                s["tasks"][0]["attempts"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty() && a[0]["process_state"] == "running")
            });
        } else {
            until(|| f.data.join("commands-control.json").exists());
            if mode == "live_commands" {
                let specification = CommandSpec {
                    request_id: "signal-command".into(),
                    run: "demo".into(),
                    task: None,
                    attempt: None,
                    operation: Some("check".into()),
                    program: std::env::current_exe().unwrap().to_string_lossy().into(),
                    args: vec![
                        "--exact".into(),
                        "custody_fixture_process".into(),
                        "--nocapture".into(),
                    ],
                    script: None,
                    cwd: f.root.clone(),
                    environment_refs: BTreeMap::new(),
                    resources: vec![],
                    deadline_ms: None,
                };
                start(&f, &specification);
                leaf = Some(Leaf::observe(&directory));
            }
        }
        assert_ne!(unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) }, 0);
        let status = wait(&mut controller);
        if mode == "worker" {
            assert_eq!(status.code(), Some(3));
            assert_eq!(f.snapshot()["state"], "stopped");
        } else {
            assert!(status.success());
        }
        if let Some(leaf) = leaf {
            until(|| leaf.stopped());
        }
    }
}

#[cfg(windows)]
#[test]
fn ctrlc_stops_worker_and_idle_or_live_command_servers_within_bound() {
    use std::{
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
        ptr::null,
    };
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    let executable = std::env::current_exe().unwrap();
    let mut command: Vec<u16> = format!(
        "\"{}\" --exact isolated_ctrlc_helper --nocapture",
        executable.display()
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    let mut variables: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| key != "JTASKS_PRIVATE_TEST_CONSOLE")
        .collect();
    variables.push(("JTASKS_PRIVATE_TEST_CONSOLE".into(), "1".into()));
    variables.sort_by_key(|(key, _)| key.to_string_lossy().to_lowercase());
    let mut environment: Vec<u16> = Vec::new();
    for (key, value) in variables {
        environment.extend(key.encode_wide());
        environment.push('=' as u16);
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    unsafe {
        let mut startup: STARTUPINFOW = zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESHOWWINDOW;
        startup.wShowWindow = 0; // SW_HIDE
        let mut process: PROCESS_INFORMATION = zeroed();
        assert_ne!(
            CreateProcessW(
                null(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                null(),
                &startup,
                &mut process
            ),
            0,
            "{}",
            std::io::Error::last_os_error()
        );
        CloseHandle(process.hThread);
        let ended = WaitForSingleObject(process.hProcess, 20000);
        if ended != WAIT_OBJECT_0 {
            TerminateProcess(process.hProcess, 1);
        }
        let mut code = 0;
        GetExitCodeProcess(process.hProcess, &mut code);
        CloseHandle(process.hProcess);
        assert_eq!(
            ended, WAIT_OBJECT_0,
            "isolated private-console signal fixture timed out"
        );
        assert_eq!(code, 0, "isolated private-console signal fixture failed");
    }
}
