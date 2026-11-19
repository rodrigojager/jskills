mod support;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{fs, process::Stdio};
use support::{Fixture, task};

struct Running<'a> {
    fixture: &'a Fixture,
    child: std::process::Child,
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        let _ = self.fixture.call(&["stop", "--run", "demo"]);
        let _ = self.child.wait();
    }
}

fn plan(tasks: Value) -> Value {
    json!({"schema_version":1,"completion":"judgment","operation_placement":"none","requirements":[{"id":"R","text":"Original hard requirement"}],"tasks":tasks})
}
fn mapped(id: &str, outcome: &str) -> Value {
    let mut t = task(id, &[], outcome, "Attributable fixture outcome");
    t["requirements"] = json!(["R"]);
    t["context"] = json!({"requirements":["Original task criterion"]});
    t
}
fn import(f: &Fixture, p: &Value) {
    let path = f.root.join("plan.json");
    fs::write(&path, p.to_string()).unwrap();
    let output = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn proposal(id: &str, base: u32, p: Value, ancestry: Value) -> Value {
    json!({"request_id":id,"base_revision":base,"reason":"Decompose unresolved work","plan":p,"ancestry":ancestry})
}
fn submit(f: &Fixture, p: &Value) -> std::process::Output {
    let path = f.root.join("proposal.json");
    fs::write(&path, p.to_string()).unwrap();
    f.call(&[
        "graph",
        "propose",
        "--run",
        "demo",
        "--file",
        path.to_str().unwrap(),
    ])
}
fn accepted(f: &Fixture, p: &Value) -> Value {
    let o = submit(f, p);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}
/// Persisted state only: a running run's `elapsed` is measured at read time.
fn persisted(f: &Fixture) -> Value {
    let mut snapshot = f.snapshot();
    snapshot.as_object_mut().unwrap().remove("elapsed");
    snapshot
}
fn rejected(f: &Fixture, p: &Value) {
    let before = persisted(f);
    let o = submit(f, p);
    assert!(
        !o.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&o.stdout)
    );
    assert_eq!(persisted(f), before, "rejected proposal mutated runtime");
}
fn db(f: &Fixture) -> Connection {
    Connection::open_with_flags(
        f.data.join("state.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

#[test]
fn public_split_replays_atomically_preserves_checks_and_reports_actual_coverage() {
    let f = Fixture::new();
    let mut p = plan(json!([mapped("A", "completed")]));
    p["operation_placement"] = json!("group");
    p["operations"] = json!([{"id":"O","stage":{"kind":"group","tasks":["A"]},"required":true,"revision":"v1","inputs":["src/feature.rs"],"initial":{"program":"python","args":["-c","pass"],"cwd":"."},"repair_task":"A"}]);
    import(&f, &p);
    p["tasks"] = json!([mapped("B", "completed"), mapped("C", "completed")]);
    p["operations"][0]["stage"]["tasks"] = json!(["B", "C"]);
    p["operations"][0]["repair_task"] = json!("B");
    let request = proposal("split", 0, p.clone(), json!({"B":"A","C":"A"}));
    let receipt = accepted(&f, &request);
    assert_eq!(receipt["original_total"], 1);
    assert_eq!(receipt["current_total"], 2);
    assert_eq!(receipt["delta"], 1);
    assert_eq!(receipt["invalidated_operations"], json!(["O"]));
    assert_eq!(accepted(&f, &request), receipt);
    assert_eq!(f.snapshot()["graph"]["coverage"][0]["satisfied"], false);
    let mut conflict = request.clone();
    conflict["reason"] = json!("changed payload");
    rejected(&f, &conflict);
    let mut stale = request.clone();
    stale["request_id"] = json!("stale");
    rejected(&f, &stale);
    let output = f.call(&["run", "--run", "demo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = f.snapshot();
    assert_eq!(status["graph"]["completed"], 2);
    assert_eq!(status["graph"]["coverage"][0]["satisfied"], true);
    assert_eq!(status["tasks"][0]["state"], "superseded");
    assert_eq!(status["tasks"][1]["lineage_root"], "A");
    assert_eq!(
        status["operations"][0]["repair"]["shared_ledger"]["initial"],
        1
    );
    assert_eq!(
        status["operations"][0]["repair"]["shared_ledger"]["work"],
        1
    );
    let c = db(&f);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM journal WHERE event='graph_revised'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
    let mut completed_change = proposal("completed-change", 1, p, json!({}));
    completed_change["plan"]["tasks"][0]["title"] = json!("Rewrite completed history");
    rejected(&f, &completed_change);
}

#[test]
fn graph_validation_rejects_cycles_requirement_edits_check_waivers_and_fake_new_work() {
    let f = Fixture::new();
    let mut p = plan(json!([mapped("A", "completed")]));
    p["operation_placement"] = json!("task");
    p["operations"] = json!([{"id":"O","stage":{"kind":"task","task":"A"},"required":true,"revision":"v1","inputs":["src/feature.rs"],"initial":{"program":"python","args":["-c","pass"],"cwd":"."}}]);
    import(&f, &p);
    let base = proposal("invalid", 0, p, json!({}));
    let mut q = base.clone();
    q["plan"]["requirements"][0]["text"] = json!("Rewrite hard requirement");
    rejected(&f, &q);
    let mut q = base.clone();
    q["plan"]["tasks"][0]["depends_on"] = json!(["A"]);
    rejected(&f, &q);
    let mut q = base.clone();
    q["plan"]["operations"][0]["required"] = json!(false);
    rejected(&f, &q);
    let mut q = base.clone();
    q["plan"]["operations"][0]["initial"]["args"] = json!(["-c", "print('waived')"]);
    rejected(&f, &q);
    let mut q = base.clone();
    q["plan"]["tasks"]
        .as_array_mut()
        .unwrap()
        .push(mapped("B", "completed"));
    rejected(&f, &q);
    let mut q = base.clone();
    q["plan"]["tasks"][0]["context"]["requirements"] = json!(["Delete original criterion"]);
    rejected(&f, &q);
    let mut q = base.clone();
    q["plan"]["tasks"][0]["admission"] = json!({"resources":["shared:missing-namespace"]});
    rejected(&f, &q);
    let mut q = base;
    q["request_id"] = json!("scope");
    q["plan"]["requirements"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"NEW","text":"Explicit additional scope"}));
    let mut b = mapped("B", "completed");
    b["requirements"] = json!(["NEW"]);
    q["plan"]["tasks"].as_array_mut().unwrap().push(b);
    q["scope_additions"] = json!({"NEW":"User-visible newly requested requirement"});
    assert_eq!(
        accepted(&f, &q)["scope_additions"]["NEW"],
        "User-visible newly requested requirement"
    );
    assert_eq!(f.snapshot()["graph"]["coverage"][1]["satisfied"], false);
}

#[test]
fn repeated_splits_and_restart_share_exhaustion_and_manual_grants() {
    let f = Fixture::new();
    import(&f, &plan(json!([mapped("A", "failed")])));
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    assert_eq!(
        f.snapshot()["tasks"][0]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let grant = f.call(&[
        "retry",
        "A",
        "--run",
        "demo",
        "--reason",
        "One authorized extra attempt",
        "--request-id",
        "grant",
    ]);
    assert!(
        grant.status.success(),
        "{}",
        String::from_utf8_lossy(&grant.stderr)
    );
    let q = proposal(
        "split",
        0,
        plan(json!([mapped("B", "failed"), mapped("C", "failed")])),
        json!({"B":"A","C":"A"}),
    );
    accepted(&f, &q);
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let snapshot = f.snapshot();
    assert_eq!(
        snapshot["tasks"][1]["attempts"].as_array().unwrap().len(),
        1
    );
    assert!(
        snapshot["tasks"][2]["attempts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let q = proposal(
        "split-again",
        1,
        plan(json!([
            mapped("B", "failed"),
            mapped("D", "failed"),
            mapped("E", "failed")
        ])),
        json!({"D":"C","E":"C"}),
    );
    accepted(&f, &q);
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let c = db(&f);
    assert_eq!(
        c.query_row("SELECT count(*) FROM recovery_lineages", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM recovery_invocations", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        c.query_row(
            "SELECT root_id FROM graph_ancestry WHERE task_id='E'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "A"
    );
    assert_eq!(
        c.query_row(
            "SELECT parent_id FROM graph_ancestry WHERE task_id='E'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "C"
    );
    assert_eq!(
        c.query_row("SELECT work_grants FROM recovery_lineages", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert!(
        c.query_row(
            "SELECT consumed_by IS NOT NULL FROM manual_retries WHERE request_id='grant'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert_eq!(
        jtasks::controller::recovery::ledger(&c, "demo", "E")
            .unwrap()
            .work,
        3
    );
    assert!(!jtasks::controller::recovery::eligible(&c, "demo", "E").unwrap());
}

#[test]
fn live_controller_serializes_pending_changes_without_rewriting_active_attempt_or_review() {
    let f = Fixture::new();
    let mut a = mapped("A", "completed");
    a["executor"]["delay_ms"] = json!(3000);
    a["estimate_seconds"] = json!(3600);
    let mut b = mapped("B", "completed");
    b["depends_on"] = json!(["A"]);
    let p = plan(json!([a, b]));
    import(&f, &p);
    let child = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running { fixture: &f, child };
    f.await_status(|s| s["tasks"][0]["state"] == "running");
    let attempt = f.snapshot()["tasks"][0]["attempts"][0].clone();
    let due = db(&f)
        .query_row("SELECT payload FROM due_work WHERE task_id='A'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    let mut changed = p.clone();
    changed["tasks"][0]["title"] = json!("Active rewrite");
    rejected(&f, &proposal("active", 0, changed, json!({})));
    let mut changed = p;
    changed["tasks"][1]["title"] = json!("Adjusted pending deliverable");
    accepted(&f, &proposal("pending", 0, changed, json!({})));
    let after = f.snapshot();
    assert_eq!(after["tasks"][0]["attempts"][0], attempt);
    assert_eq!(
        db(&f)
            .query_row("SELECT payload FROM due_work WHERE task_id='A'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        due
    );
    let output = f.call(&["stop", "--run", "demo"]);
    assert!(output.status.success());
    let _ = running.child.wait().unwrap();
    let c = db(&f);
    assert_eq!(
        c.query_row("SELECT count(*) FROM attempts WHERE task_id='A'", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM due_work WHERE task_id='B'", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn live_controller_applies_a_proposal_larger_than_one_control_message() {
    let f = Fixture::new();
    let mut a = mapped("A", "completed");
    a["executor"]["delay_ms"] = json!(1500);
    let mut b = mapped("B", "completed");
    b["depends_on"] = json!(["A"]);
    let p = plan(json!([a, b]));
    import(&f, &p);
    let child = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running { fixture: &f, child };
    f.await_status(|s| s["tasks"][0]["state"] == "running");
    let padding = "Large revision detail. ".repeat(1200);
    let mut changed = p;
    let ids: Vec<String> = (1..=3).map(|i| format!("B{i}")).collect();
    let mut tasks = vec![changed["tasks"][0].clone()];
    let mut ancestry = serde_json::Map::new();
    for id in &ids {
        let mut t = mapped(id, "completed");
        t["depends_on"] = json!(["A"]);
        t["context"]["brief"] = json!(padding);
        tasks.push(t);
        ancestry.insert(id.clone(), json!("B"));
    }
    changed["tasks"] = json!(tasks);
    let request = proposal("large", 0, changed, Value::Object(ancestry));
    assert!(request.to_string().len() > 65536);
    let receipt = accepted(&f, &request);
    assert_eq!(receipt["current_total"], 4);
    assert!(
        fs::read_dir(f.data.join("proposals"))
            .unwrap()
            .next()
            .is_none(),
        "referenced proposal file left behind"
    );
    let status = f.await_status(|s| {
        s["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| ids.iter().any(|id| t["id"] == id.as_str()))
            .filter(|t| t["state"] == "completed")
            .count()
            == ids.len()
    });
    let b = status["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "B")
        .unwrap();
    assert_eq!(b["state"], "superseded");
    let _ = running.child.wait().unwrap();
}

#[test]
fn split_keeps_each_original_task_check_at_task_completion() {
    let f = Fixture::new();
    let mut p = plan(json!([mapped("A", "completed")]));
    p["operation_placement"] = json!("task");
    p["operations"] = json!([{"id":"O","stage":{"kind":"task","task":"A"},"required":true,"revision":"v1","inputs":["src/feature.rs"],"initial":{"program":"python","args":["-c","pass"],"cwd":"."}}]);
    import(&f, &p);
    p["tasks"] = json!([mapped("B", "completed"), mapped("C", "completed")]);
    p["operations"][0]["stage"]["task"] = json!("B");
    let mut q = proposal("split-check", 0, p, json!({"B":"A","C":"A"}));
    rejected(&f, &q);
    let mut clone = q["plan"]["operations"][0].clone();
    clone["id"] = json!("O-C");
    clone["stage"]["task"] = json!("C");
    q["plan"]["operations"].as_array_mut().unwrap().push(clone);
    let mut moved = q.clone();
    moved["plan"]["operation_placement"] = json!("mixed");
    moved["plan"]["operations"][0]["stage"] = json!({"kind":"final","tasks":["B","C"]});
    moved["plan"]["operations"][0]["repair_task"] = json!("B");
    rejected(&f, &moved);
    accepted(&f, &q);
    let o = f.call(&["run", "--run", "demo"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        f.snapshot()["operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["state"] == "passed")
            .count(),
        2
    );
}

#[test]
fn relevant_pending_changes_invalidate_passed_prerequisite_without_erasing_receipts() {
    let f = Fixture::new();
    let mut a = mapped("A", "completed");
    a["admission"] = json!({"enabled":false});
    let mut p = plan(json!([a]));
    p["operation_placement"] = json!("mixed");
    p["operations"] = json!([{"id":"O","stage":{"kind":"prerequisite","consumers":["A"]},"required":true,"revision":"v1","inputs":["src/feature.rs"],"initial":{"program":"python","args":["-c","pass"],"cwd":"."}}]);
    import(&f, &p);
    assert_eq!(f.call(&["run", "--run", "demo"]).status.code(), Some(3));
    let old = f.snapshot();
    assert_eq!(old["operations"][0]["state"], "passed");
    let command = old["operations"][0]["command_id"].as_str().unwrap();
    p["tasks"][0]["admission"]["enabled"] = json!(true);
    accepted(&f, &proposal("new-input", 0, p, json!({})));
    let invalidated = f.snapshot();
    assert_eq!(invalidated["operations"][0]["state"], "pending");
    assert!(invalidated["operations"][0]["command_id"].is_null());
    assert_eq!(
        db(&f)
            .query_row(
                "SELECT state FROM managed_commands WHERE id=?1",
                [command],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "exited"
    );
    assert!(f.call(&["run", "--run", "demo"]).status.success());
    assert_eq!(
        db(&f)
            .query_row("SELECT count(*) FROM managed_commands", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(f.snapshot()["graph"]["coverage"][0]["satisfied"], true);
}

fn revise(f: &Fixture, extra: &[&str]) -> std::process::Output {
    let mut args = vec!["graph", "revise", "--run", "demo"];
    args.extend_from_slice(extra);
    f.call(&args)
}
fn stored_plan(f: &Fixture) -> Value {
    serde_json::from_str(
        &db(f)
            .query_row("SELECT plan FROM runs WHERE id='demo'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
    )
    .unwrap()
}
fn task_ids(plan: &Value) -> Vec<String> {
    plan["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn revise_adds_tasks_and_rewires_pending_dependencies_after_the_plan_files_changed() {
    let f = Fixture::new();
    let mut b = mapped("B", "completed");
    b["depends_on"] = json!(["A"]);
    let mut p = plan(json!([mapped("A", "completed"), b]));
    p["context"] = json!({"brief":"Shared context as imported"});
    import(&f, &p);
    // The plan files changed after import and the intent was composed again: shared
    // context, requirement text and task text differ, and a new task N maps to a new
    // requirement. Submitting it as a whole proposal is refused.
    let mut recomposed = p.clone();
    recomposed["context"]["brief"] = json!("Shared context rewritten in the plan files");
    recomposed["requirements"] = json!([
        {"id":"R","text":"Original hard requirement, edited later"},
        {"id":"R2","text":"New use case"}
    ]);
    recomposed["tasks"][0]["title"] = json!("Retitled in the plan files");
    let mut n = task("N", &["A"], "completed", "Attributable fixture outcome");
    n["requirements"] = json!(["R2"]);
    recomposed["tasks"].as_array_mut().unwrap().push(n);
    rejected(&f, &proposal("whole", 0, recomposed.clone(), json!({})));
    let intent = f.root.join("recomposed.json");
    fs::write(&intent, recomposed.to_string()).unwrap();
    let intent = intent.to_str().unwrap();

    // A dry run writes the proposal for review and changes nothing.
    let before = persisted(&f);
    let review = f.root.join("revision.json");
    let output = revise(
        &f,
        &[
            "--add-tasks",
            intent,
            "--depends",
            "B=A,N",
            "--dry-run",
            "--output",
            review.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("A, B"));
    assert_eq!(persisted(&f), before, "dry run mutated runtime");
    let written: Value = serde_json::from_slice(&fs::read(&review).unwrap()).unwrap();
    assert_eq!(written["base_revision"], 0);
    assert_eq!(written["plan"]["context"], stored_plan(&f)["context"]);
    assert_eq!(written["plan"]["tasks"][0]["title"], "Fixture A");
    assert_eq!(written["plan"]["tasks"][1]["depends_on"], json!(["A", "N"]));
    assert_eq!(task_ids(&written["plan"]), ["A", "B", "N"]);
    assert_eq!(
        written["plan"]["requirements"],
        json!([
            {"id":"R","text":"Original hard requirement"},
            {"id":"R2","text":"New use case"}
        ])
    );
    assert_eq!(written["scope_additions"].as_object().unwrap().len(), 1);
    assert!(
        written["scope_additions"]["R2"]
            .as_str()
            .unwrap()
            .contains("add N")
    );

    // Applied without hand editing; the derived request ID matches the reviewed one.
    let args = ["--add-tasks", intent, "--depends", "B=A,N"];
    let output = revise(&f, &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["revision"], 1);
    assert_eq!(receipt["request_id"], written["request_id"]);
    assert_eq!(receipt["current_total"], 3);
    assert_eq!(
        receipt["scope_additions"]["R2"],
        written["scope_additions"]["R2"]
    );
    let stored = stored_plan(&f);
    assert_eq!(stored["tasks"][1]["depends_on"], json!(["A", "N"]));
    assert_eq!(stored["context"]["brief"], "Shared context as imported");
    // Nothing left to change: refused without a new revision.
    let before = persisted(&f);
    assert!(!revise(&f, &args).status.success());
    assert_eq!(persisted(&f), before);

    // The revised graph runs to completion with every requirement covered.
    let output = f.call(&["run", "--run", "demo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = f.snapshot();
    assert!(
        status["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["state"] == "completed"),
        "{status}"
    );
    assert_eq!(status["graph"]["revision"], 1);
    assert!(
        status["graph"]["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["satisfied"] == true)
    );
}

#[test]
fn revise_reaches_a_live_controller_and_refuses_bad_changes_without_mutation() {
    let f = Fixture::new();
    let mut a = mapped("A", "completed");
    a["executor"]["delay_ms"] = json!(1500);
    let mut b = mapped("B", "completed");
    b["depends_on"] = json!(["A"]);
    import(&f, &plan(json!([a, b])));
    let additions = f.root.join("additions.json");
    let mut n = task("N", &["A"], "completed", "Attributable fixture outcome");
    n["requirements"] = json!(["R2"]);
    fs::write(
        &additions,
        json!({"tasks":[n],"requirements":[{"id":"R2","text":"New use case"}]}).to_string(),
    )
    .unwrap();
    let additions = additions.to_str().unwrap();
    // A new requirement without text, an unknown task, a cycle and no change are refused.
    let unmapped = f.root.join("unmapped.json");
    let mut m = task("M", &[], "completed", "Attributable fixture outcome");
    m["requirements"] = json!(["R3"]);
    fs::write(&unmapped, json!([m]).to_string()).unwrap();
    let before = persisted(&f);
    for bad in [
        vec!["--add-tasks", unmapped.to_str().unwrap()],
        vec!["--depends", "X=A"],
        vec!["--depends", "A=B"],
        vec![],
    ] {
        let output = revise(&f, &bad);
        assert!(!output.status.success(), "{bad:?} was accepted");
    }
    assert_eq!(persisted(&f), before);

    let child = f
        .command()
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running { fixture: &f, child };
    f.await_status(|s| s["tasks"][0]["state"] == "running");
    let output = revise(
        &f,
        &[
            "--add-tasks",
            additions,
            "--depends",
            "B=N",
            "--reason",
            "Add N before B",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["reason"], "Add N before B");
    assert_eq!(receipt["scope_additions"]["R2"], "Add N before B");
    let status = f.await_status(|s| {
        let tasks = s["tasks"].as_array().unwrap();
        tasks.len() == 3 && tasks.iter().all(|t| t["state"] == "completed")
    });
    assert_eq!(status["graph"]["revision"], 1);
    let _ = running.child.wait().unwrap();
    assert_eq!(stored_plan(&f)["tasks"][1]["depends_on"], json!(["N"]));
}
