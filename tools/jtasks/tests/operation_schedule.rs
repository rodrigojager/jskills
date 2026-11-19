mod support;
use serde_json::{Value, json};
use std::{fs, process::Stdio};
use support::{Fixture, task};

fn operation(id: &str, stage: Value, required: bool, delay: u64, exit: i32) -> Value {
    json!({"id":id,"stage":stage,"required":required,"revision":"declared-input-r1","inputs":["src/"],"initial":{"program":env!("CARGO_BIN_EXE_jtasks"),"args":["fixture","--outcome","silent","--delay-ms",delay.to_string(),"--exit-code",exit.to_string()],"cwd":".","deadline_ms":5000},"repair_task":"A"})
}
fn write(f: &Fixture, placement: &str, tasks: Value, operations: Value) -> std::path::PathBuf {
    let path = f.root.join("operations.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","operation_placement":placement,"tasks":tasks,"operations":operations})).unwrap()).unwrap();
    path
}
fn import(f: &Fixture, placement: &str, tasks: Value, operations: Value) {
    let path = write(f, placement, tasks, operations);
    let result = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn run(f: &Fixture) -> std::process::Child {
    f.command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn validator_requires_explicit_policies_and_accepts_every_placement() {
    let f = Fixture::new();
    for (placement, stage) in [
        ("none", None),
        ("task", Some(json!({"kind":"task","task":"A"}))),
        ("group", Some(json!({"kind":"group","tasks":["A"]}))),
        ("final", Some(json!({"kind":"final","tasks":["A"]}))),
        (
            "mixed",
            Some(json!({"kind":"prerequisite","consumers":["A"]})),
        ),
    ] {
        let ops = stage
            .map(|stage| json!([operation("check", stage, true, 0, 0)]))
            .unwrap_or(json!([]));
        let path = write(
            &f,
            placement,
            json!([task("A", &[], "completed", "judgment")]),
            ops,
        );
        let result = f.call(&["validate", path.to_str().unwrap()]);
        assert!(
            result.status.success(),
            "{placement}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let preview = f.call(&["operations", "preview", path.to_str().unwrap()]);
        assert!(preview.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&preview.stdout).unwrap()["operation_placement"],
            placement
        );
        let mut plan: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for key in ["completion", "operation_placement"] {
            let saved = plan[key].take();
            plan.as_object_mut().unwrap().remove(key);
            fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
            assert!(
                !f.call(&["validate", path.to_str().unwrap()])
                    .status
                    .success()
            );
            plan[key] = saved;
        }
    }
    assert!(
        !f.data.exists(),
        "validation/preview must not write runtime state"
    );
}

#[test]
fn combined_validation_rejects_completion_check_and_prerequisite_cycles() {
    let f = Fixture::new();
    let mut check = operation("check", json!({"kind":"task","task":"A"}), true, 0, 0);
    check["depends_on"] = json!([{"kind":"task_completed","id":"A"}]);
    let mut setup = operation(
        "setup",
        json!({"kind":"prerequisite","consumers":["A"]}),
        true,
        0,
        0,
    );
    setup["depends_on"] = json!([{"kind":"task_candidate","id":"A"}]);
    let mut group = operation("group", json!({"kind":"group","tasks":["B"]}), true, 0, 0);
    group["depends_on"] = json!([{"kind":"operation","id":"setup"}]);
    let mut chained = operation(
        "setup",
        json!({"kind":"prerequisite","consumers":["A"]}),
        true,
        0,
        0,
    );
    chained["depends_on"] = json!([{"kind":"operation","id":"group"}]);
    for operations in [json!([check]), json!([setup]), json!([chained, group])] {
        let path = write(
            &f,
            "mixed",
            json!([
                task("A", &[], "completed", "A"),
                task("B", &["A"], "completed", "B")
            ]),
            operations,
        );
        let result = f.call(&["validate", path.to_str().unwrap()]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("cycle"));
    }
    let path = write(
        &f,
        "none",
        json!([task("A", &[], "completed", "A")]),
        json!([operation(
            "check",
            json!({"kind":"task","task":"A"}),
            true,
            0,
            0
        )]),
    );
    assert!(
        !f.call(&["validate", path.to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn prerequisites_gate_consumers_while_independent_development_advances() {
    let f = Fixture::new();
    import(
        &f,
        "mixed",
        json!([
            task("A", &[], "completed", "consumer"),
            task("B", &[], "completed", "independent")
        ]),
        json!([
            operation(
                "setup",
                json!({"kind":"prerequisite","consumers":["A"]}),
                true,
                800,
                0
            ),
            operation(
                "group",
                json!({"kind":"group","tasks":["A","B"]}),
                true,
                0,
                0
            ),
            operation(
                "final",
                json!({"kind":"final","tasks":["A","B"]}),
                true,
                0,
                0
            )
        ]),
    );
    let mut controller = run(&f);
    let snapshot = f.await_status(|s| {
        s["operations"][0]["state"] == "running" && s["tasks"][1]["state"] == "completed"
    });
    assert_eq!(snapshot["tasks"][0]["attempts"], json!([]));
    assert_eq!(snapshot["operations"][1]["state"], "pending");
    assert_eq!(snapshot["operations"][2]["state"], "pending");
    assert!(!snapshot["verification_complete"].as_bool().unwrap());
    assert!(controller.wait().unwrap().success());
    let snapshot = f.snapshot();
    assert_eq!(snapshot["complete"], true);
    assert!(
        snapshot["operations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|op| op["state"] == "passed")
    );
}

#[test]
fn task_check_runs_after_candidate_before_completion_without_blocking_other_work() {
    let f = Fixture::new();
    import(
        &f,
        "task",
        json!([
            task("A", &[], "completed", "candidate"),
            task("B", &["A"], "completed", "dependent"),
            task("C", &[], "completed", "independent")
        ]),
        json!([operation(
            "local",
            json!({"kind":"task","task":"A"}),
            true,
            800,
            0
        )]),
    );
    let mut controller = run(&f);
    let snapshot = f.await_status(|s| {
        s["operations"][0]["state"] == "running" && s["tasks"][2]["state"] == "completed"
    });
    assert_eq!(snapshot["tasks"][0]["state"], "verification_pending");
    assert_eq!(snapshot["tasks"][0]["attempts"][0]["outcome"], "completed");
    assert_eq!(
        snapshot["tasks"][0]["attempts"][0]["process_state"],
        "retired"
    );
    assert_eq!(snapshot["tasks"][1]["attempts"], json!([]));
    assert!(controller.wait().unwrap().success());
    assert_eq!(f.snapshot()["tasks"][1]["state"], "completed");
}

#[test]
fn required_final_failure_blocks_run_success_and_initial_invocation_never_repeats() {
    let f = Fixture::new();
    import(
        &f,
        "final",
        json!([task("A", &[], "completed", "no broad task checks")]),
        json!([operation(
            "final",
            json!({"kind":"final","tasks":["A"]}),
            true,
            0,
            7
        )]),
    );
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let snapshot = f.snapshot();
    assert_eq!(snapshot["development_complete"], true);
    assert_eq!(snapshot["verification_complete"], false);
    assert_eq!(snapshot["operations"][0]["state"], "failed");
    assert_eq!(snapshot["operations"][0]["exit_code"], 7);
    let command = snapshot["operations"][0]["command_id"].clone();
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    assert_eq!(f.snapshot()["operations"][0]["command_id"], command);
    let db = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM managed_commands", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let before = fs::read(f.data.join("state.sqlite3")).unwrap();
    assert_eq!(
        f.call(&["operations", "status", "--run", "demo"])
            .status
            .code(),
        Some(3)
    );
    assert_eq!(fs::read(f.data.join("state.sqlite3")).unwrap(), before);
}

#[test]
fn optional_failure_is_visible_and_none_does_not_synthesize_commands() {
    for optional in [false, true] {
        let f = Fixture::new();
        import(
            &f,
            if optional { "task" } else { "none" },
            json!([task("A", &[], "completed", "judgment without evidence")]),
            if optional {
                json!([operation(
                    "optional",
                    json!({"kind":"task","task":"A"}),
                    false,
                    0,
                    9
                )])
            } else {
                json!([])
            },
        );
        let result = f.call(&["run", "--run", "demo"]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let snapshot = f.snapshot();
        assert_eq!(snapshot["complete"], true);
        if optional {
            assert_eq!(snapshot["operations"][0]["state"], "failed");
        } else {
            assert_eq!(snapshot["operations"], json!([]));
        }
        let db = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM managed_commands", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            i64::from(optional)
        );
    }
}

#[test]
fn failed_required_checks_block_dependents_and_failed_prerequisites_never_launch_consumers() {
    for prerequisite in [false, true] {
        let f = Fixture::new();
        let stage = if prerequisite {
            json!({"kind":"prerequisite","consumers":["A"]})
        } else {
            json!({"kind":"task","task":"A"})
        };
        let mut dependent = operation(
            "dependent",
            json!({"kind":"final","tasks":["B"]}),
            true,
            0,
            0,
        );
        dependent["depends_on"] = json!([{"kind":"operation","id":"check"}]);
        // Put the dependent first to exercise fixed-point blocked propagation.
        import(
            &f,
            "mixed",
            json!([
                task("A", &[], "completed", "candidate"),
                task("B", &["A"], "completed", "must wait")
            ]),
            json!([dependent, operation("check", stage, true, 0, 4)]),
        );
        assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
        let snapshot = f.snapshot();
        assert_eq!(snapshot["operations"][0]["state"], "blocked");
        assert_eq!(snapshot["operations"][1]["state"], "failed");
        assert_eq!(snapshot["tasks"][1]["attempts"], json!([]));
        if prerequisite {
            assert_eq!(snapshot["tasks"][0]["attempts"], json!([]));
        } else {
            assert_eq!(snapshot["tasks"][0]["state"], "verification_pending");
        }
    }
}

#[test]
fn explicit_stop_reconciles_operation_outcome_and_does_not_repeat_initial_call() {
    let f = Fixture::new();
    import(
        &f,
        "final",
        json!([task("A", &[], "completed", "development done")]),
        json!([operation(
            "final",
            json!({"kind":"final","tasks":["A"]}),
            true,
            3000,
            0
        )]),
    );
    let mut controller = run(&f);
    let running = f.await_status(|s| s["operations"][0]["state"] == "running");
    assert!(f.call(&["stop", "--run", "demo"]).status.success());
    assert_eq!(controller.wait().unwrap().code(), Some(3));
    let stopped = f.snapshot();
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(stopped["operations"][0]["state"], "failed");
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    assert_eq!(
        f.snapshot()["operations"][0]["command_id"],
        running["operations"][0]["command_id"]
    );
}
