mod support;
use serde_json::{Value, json};
use std::{
    fs,
    process::Stdio,
    time::{Duration, Instant},
};
use support::{Fixture, task};

fn call(f: &Fixture, registry: &std::path::Path, args: &[&str]) -> std::process::Output {
    f.command()
        .args([
            "resources",
            "--registry",
            registry.to_str().unwrap(),
            "--json",
        ])
        .args(args)
        .output()
        .unwrap()
}
fn ok(f: &Fixture, registry: &std::path::Path, args: &[&str]) -> Value {
    let output = call(f, registry, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn reserve<'a>(request: &'a str, task: &'a str, rest: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "reserve",
        "--request",
        request,
        "--actor",
        "fixture-agent",
        "--reason",
        "declared work",
        "--run",
        "demo",
        "--task",
        task,
    ];
    args.extend(rest);
    args
}

#[test]
fn checkout_paths_use_normalized_segments_and_reads_never_create_state() {
    let f = Fixture::new();
    let other = Fixture::new();
    let registry = f.data.join("registry.sqlite3");
    assert_eq!(ok(&f, &registry, &["list"])["reservations"], json!([]));
    assert!(!f.data.exists());
    let original = ok(
        &f,
        &registry,
        &reserve("one", "A", &["--file", "src/new/../module"]),
    );
    assert_eq!(
        original["current"][0]["resource"]["identity"]["segments"],
        json!(["src", "module"])
    );
    assert!(
        !call(
            &f,
            &registry,
            &reserve("conflict", "B", &["--file", "src/module/file.rs"])
        )
        .status
        .success()
    );
    ok(
        &f,
        &registry,
        &reserve("prefix", "B", &["--file", "src/modules"]),
    );
    ok(
        &other,
        &registry,
        &reserve("checkout", "B", &["--file", "src/module"]),
    );
    assert!(
        !call(
            &f,
            &registry,
            &reserve("escape", "B", &["--file", "../outside"])
        )
        .status
        .success()
    );
    assert!(
        !call(
            &f,
            &registry,
            &reserve("glob", "B", &["--file", "src/*.rs"])
        )
        .status
        .success()
    );
    #[cfg(windows)]
    {
        assert!(
            !call(
                &f,
                &registry,
                &reserve("case", "B", &["--file", "SRC\\MODULE\\file.rs"])
            )
            .status
            .success()
        );
    }
    let before = fs::read(&registry).unwrap();
    assert_eq!(
        ok(&f, &registry, &["list"])["reservations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(fs::read(&registry).unwrap(), before);
}

#[test]
fn shared_names_modes_and_atomic_sets_coordinate_independent_checkouts() {
    let f = Fixture::new();
    let other = Fixture::new();
    let registry = f.data.join("registry.sqlite3");
    let shared = [
        "--resource",
        "ports/8080",
        "--shared-scope",
        "host-a/dev",
        "--mode",
        "shared",
    ];
    ok(&f, &registry, &reserve("a", "A", &shared));
    ok(&other, &registry, &reserve("b", "B", &shared));
    assert!(
        !call(
            &other,
            &registry,
            &reserve(
                "all-or-none",
                "C",
                &[
                    "--resource",
                    "ports/8080",
                    "--resource",
                    "database/free",
                    "--shared-scope",
                    "host-a/dev"
                ]
            )
        )
        .status
        .success()
    );
    assert_eq!(
        ok(&f, &registry, &["list"])["reservations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    ok(
        &other,
        &registry,
        &reserve(
            "different-host",
            "C",
            &["--resource", "ports/8080", "--shared-scope", "host-b/dev"],
        ),
    );
    ok(
        &other,
        &registry,
        &reserve("local", "C", &["--resource", "ports/8080"]),
    );
    ok(
        &f,
        &registry,
        &reserve("local-other", "C", &["--resource", "ports/8080"]),
    );
    // Independent CLI processes race for a full set; SQLite serializes admission.
    let one = reserve(
        "race-one",
        "D",
        &[
            "--resource",
            "build/one",
            "--resource",
            "build/two",
            "--shared-scope",
            "host-a/dev",
        ],
    );
    let two = reserve(
        "race-two",
        "E",
        &[
            "--resource",
            "build/two",
            "--resource",
            "build/one",
            "--shared-scope",
            "host-a/dev",
        ],
    );
    let mut a = f
        .command()
        .args(["resources", "--registry", registry.to_str().unwrap()])
        .args(one)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut b = other
        .command()
        .args(["resources", "--registry", registry.to_str().unwrap()])
        .args(two)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert_ne!(a.wait().unwrap().success(), b.wait().unwrap().success());
    assert_eq!(
        ok(&f, &registry, &["list"])["reservations"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
}

#[test]
fn active_override_and_unknown_release_are_durable_idempotent_audited_and_do_not_dispatch() {
    let f = Fixture::new();
    let registry = f.data.join("registry.sqlite3");
    let mut spec = task("A", &[], "completed", "owner completed normally");
    spec["executor"]["delay_ms"] = json!(1500);
    f.import(json!([spec]));
    let mut controller = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    f.await_status(|s| !s["tasks"][0]["attempts"].as_array().unwrap().is_empty());
    let held = ok(
        &f,
        &registry,
        &reserve(
            "held",
            "A",
            &["--file", "source.rs", "--owner-state", "active"],
        ),
    );
    let id = held["current"][0]["id"].as_str().unwrap();
    let args = [
        "override",
        id,
        "--to-task",
        "B",
        "--to-run",
        "demo",
        "--request",
        "move",
        "--actor",
        "another-agent",
        "--reason",
        "correct mistaken assignment",
    ];
    let receipt = ok(&f, &registry, &args);
    assert_eq!(receipt["prior"][0]["owner_observation"], "active");
    assert_eq!(receipt["current"][0]["owner"]["task"], "B");
    assert_eq!(receipt["current"][0]["owner_observation"], "unknown");
    assert_eq!(receipt["deliberate_override"], true);
    assert_eq!(receipt["processes_stopped"], false);
    assert_eq!(receipt["attempts_dispatched"], 0);
    assert_eq!(receipt, ok(&f, &registry, &args));
    assert!(
        controller.try_wait().unwrap().is_none(),
        "override must leave the controller/owner alive"
    );
    let release = [
        "release",
        id,
        "--request",
        "release",
        "--actor",
        "third-agent",
        "--reason",
        "release advisory assignment",
    ];
    let released = ok(&f, &registry, &release);
    assert_eq!(released["prior"][0]["owner_observation"], "unknown");
    assert_eq!(released, ok(&f, &registry, &release));
    assert_eq!(
        receipt,
        ok(&f, &registry, &args),
        "old retry returns original receipt without resurrecting reservation"
    );
    let conflict = [
        "release",
        id,
        "--request",
        "move",
        "--actor",
        "another-agent",
        "--reason",
        "different request",
    ];
    assert!(!call(&f, &registry, &conflict).status.success());
    assert_eq!(ok(&f, &registry, &["list"])["reservations"], json!([]));
    let started = Instant::now();
    while controller.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(status["tasks"][0]["state"], "completed");
}

#[test]
fn versioned_protocol_is_reusable_and_rejects_conflicting_or_invalid_requests() {
    let f = Fixture::new();
    let registry = f.data.join("registry.sqlite3");
    let owner = jtasks::resources::registry::Owner {
        checkout: jtasks::resources::identity::checkout_key(&f.root).unwrap(),
        run: "demo".into(),
        task: "A".into(),
        attempt: Some("immutable-attempt".into()),
    };
    let request = jtasks::resources::registry::Request {
        protocol_version: 1,
        request_id: "consumer-one".into(),
        actor: "jlock-consumer".into(),
        reason: "explicit resource set".into(),
        operation: jtasks::resources::registry::Action::Reserve {
            owner,
            observation: jtasks::resources::registry::Observation::Unknown,
            resources: vec![jtasks::resources::registry::Resource {
                identity: jtasks::resources::identity::Identity::named(
                    &f.root,
                    Some("host/dev"),
                    "database",
                    "integration",
                )
                .unwrap(),
                mode: jtasks::resources::registry::Mode::Exclusive,
            }],
        },
    };
    let file = f.root.join("request.json");
    fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
    let result = ok(&f, &registry, &["apply", file.to_str().unwrap()]);
    assert_eq!(result["protocol_version"], 1);
    assert_eq!(result["actor"], "jlock-consumer");
    assert_eq!(
        result,
        ok(&f, &registry, &["apply", file.to_str().unwrap()])
    );
    let mut value = serde_json::to_value(&request).unwrap();
    value["protocol_version"] = json!(2);
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !call(&f, &registry, &["apply", file.to_str().unwrap()])
            .status
            .success()
    );
    value["protocol_version"] = json!(1);
    value["actor"] = json!(" ");
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !call(&f, &registry, &["apply", file.to_str().unwrap()])
            .status
            .success()
    );
}
