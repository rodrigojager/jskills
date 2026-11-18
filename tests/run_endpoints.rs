mod support;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Child, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

/// A run controller started from one checkout; stopped and reaped on drop.
struct Controller<'a> {
    fixture: &'a Fixture,
    run: &'static str,
    child: Child,
}
impl Drop for Controller<'_> {
    fn drop(&mut self) {
        let _ = self.fixture.call(&["stop", "--run", self.run]);
        let _ = self.child.wait();
    }
}

fn mapped(id: &str, outcome: &str) -> Value {
    let mut t = task(id, &[], outcome, "Attributable fixture outcome");
    t["requirements"] = json!(["R"]);
    t["context"] = json!({"requirements":["Original task criterion"]});
    t
}

/// A long task keeps the controller live; F fails at once with no automatic retry; B waits for A.
fn plan() -> Value {
    let mut a = mapped("A", "completed");
    a["executor"]["delay_ms"] = json!(60000);
    let mut b = mapped("B", "completed");
    b["depends_on"] = json!(["A"]);
    let mut f = mapped("F", "failed");
    f["recovery"] = json!({"work_retries":0});
    json!({"schema_version":1,"completion":"judgment","operation_placement":"none","scheduling":{"task_capacity":2,"heavy_capacity":1},"requirements":[{"id":"R","text":"Original hard requirement"}],"tasks":[a,b,f]})
}

fn import(f: &Fixture, run: &str) {
    let path = f.root.join("plan.json");
    fs::write(&path, plan().to_string()).unwrap();
    let output = f.call(&["import", path.to_str().unwrap(), "--run", run]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn start<'a>(f: &'a Fixture, run: &'static str) -> Controller<'a> {
    let child = f
        .command()
        .args(["run", "--run", run])
        .env("JTASKS_SHUTDOWN_GRACE_MS", "200")
        .env("JTASKS_SHUTDOWN_FORCE_MS", "2000")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Controller {
        fixture: f,
        run,
        child,
    }
}

fn state(status: &Value, id: &str) -> Value {
    status["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .map(|t| t["state"].clone())
        .unwrap_or(Value::Null)
}

/// A status read; it answers 3 while the run is incomplete.
fn status(f: &Fixture, run: &str) -> Value {
    let output = f.call(&["status", "--run", run]);
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)))
}

fn await_status(f: &Fixture, run: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        let value = status(f, run);
        if predicate(&value) {
            return value;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "status deadline for {run}: {value}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ok(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "exit {:?}: {} {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn propose(f: &Fixture, run: &str) -> Value {
    let mut changed = plan();
    let mut b1 = mapped("B1", "completed");
    b1["depends_on"] = json!(["A"]);
    changed["tasks"] = json!([changed["tasks"][0].clone(), b1, changed["tasks"][2].clone()]);
    let request = json!({"request_id":format!("split-{run}"),"base_revision":0,"reason":"Decompose unresolved work","plan":changed,"ancestry":{"B1":"B"}});
    let path = f.root.join("proposal.json");
    fs::write(&path, request.to_string()).unwrap();
    ok(f.call(&[
        "graph",
        "propose",
        "--run",
        run,
        "--file",
        path.to_str().unwrap(),
    ]))
}

fn retry(f: &Fixture, run: &str) -> Value {
    ok(f.call(&[
        "retry",
        "F",
        "--run",
        run,
        "--reason",
        "configuration fixed",
        "--request-id",
        &format!("retry-{run}"),
    ]))
}

fn exited(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn two_runs_from_two_checkouts_in_one_data_folder_each_reach_their_own_controller() {
    let first = Fixture::new();
    let mut second = Fixture::new();
    second.data = first.data.clone();
    import(&first, "alpha");
    import(&second, "beta");

    // Alpha's controller starts first; beta's, started later, used to overwrite alpha's endpoint.
    let mut alpha = start(&first, "alpha");
    await_status(&first, "alpha", |s| {
        state(s, "A") == "running" && state(s, "F") == "retry_exhausted"
    });
    let mut beta = start(&second, "beta");
    await_status(&second, "beta", |s| {
        state(s, "A") == "running" && state(s, "F") == "retry_exhausted"
    });
    let endpoint = |run| -> jtasks::transport::ControlEndpoint {
        serde_json::from_slice(
            &fs::read(jtasks::transport::control_file(&first.data, run)).unwrap(),
        )
        .unwrap()
    };
    let (a, b) = (endpoint("alpha"), endpoint("beta"));
    assert_eq!((a.run.as_str(), b.run.as_str()), ("alpha", "beta"));
    assert_ne!(a.endpoint, b.endpoint);
    assert!(
        jtasks::transport::commands_file(&first.data, Some("alpha")).is_file()
            && jtasks::transport::commands_file(&first.data, Some("beta")).is_file()
    );

    // graph propose: each checkout's writer is busy, so each proposal goes to its own run's controller.
    assert_eq!(propose(&first, "alpha")["current_total"], 3);
    let alpha_status = await_status(&first, "alpha", |s| state(s, "B") == "superseded");
    assert_eq!(state(&alpha_status, "B1"), "pending");
    assert_eq!(
        state(&status(&second, "beta"), "B"),
        "pending",
        "alpha's proposal reached beta"
    );
    assert_eq!(propose(&second, "beta")["current_total"], 3);
    await_status(&second, "beta", |s| state(s, "B") == "superseded");

    // retry: each grant reaches its own controller, which launches F again.
    retry(&first, "alpha");
    await_status(&first, "alpha", |s| {
        s["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "F")
            .is_some_and(|t| t["attempts"].as_array().unwrap().len() == 2)
    });
    retry(&second, "beta");
    await_status(&second, "beta", |s| {
        s["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "F")
            .is_some_and(|t| t["attempts"].as_array().unwrap().len() == 2)
    });

    // stop: stopping alpha leaves beta running.
    ok(first.call(&["stop", "--run", "alpha"]));
    assert!(exited(&mut alpha.child), "alpha's controller did not stop");
    assert!(
        beta.child.try_wait().unwrap().is_none(),
        "stopping alpha stopped beta"
    );
    assert_eq!(state(&status(&second, "beta"), "A"), "running");
    // Alpha's endpoint file is now stale and ignored.
    assert!(jtasks::transport::control_file(&first.data, "alpha").is_file());
    let again = first.call(&["stop", "--run", "alpha"]);
    assert_eq!(again.status.code(), Some(3));
    ok(second.call(&["stop", "--run", "beta"]));
    assert!(exited(&mut beta.child), "beta's controller did not stop");
}
