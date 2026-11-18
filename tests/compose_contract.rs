mod support;

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use support::Fixture;

fn source() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn python() -> String {
    std::env::var("JTASKS_COMPOSE_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into())
}

const PLAN: &str = "# Greeting service

## Problem

Callers are not greeted.

## Solution

A small service greets them.

## Use cases

1. As a caller, I want a greeting, so that I am welcomed.
2. As an operator, I want a health check, so that
   I can monitor the service.

## Context for executors

Build the greeting service in two slices. Read
PLAN-delivery.md first.

## Parts

- [PLAN-delivery.md](PLAN-delivery.md): sub-plan: the slices and their proof.
";

const DELIVERY: &str = "# Delivery

Kind: sub-plan. The slices.

Part of [PLAN.md](PLAN.md).

## 1. Slices

| Slice | Delivers | Proof |
|---|---|---|
| S1 | The greeting endpoint | a request returns the greeting |

| S2 | The health check | `GET /health` answers 200 \\| 503 |

## 2. Acceptance cases

- **AC-1** (end to end): a caller is
  greeted by name.
- **AC-2**: not part of this example.

## 3. Files

| File | Slice |
|---|---|
| src/greet.rs | S1 |
| src/health.rs | S2 |
| src/routes.rs | S2 |
";

/// A to-plan plan in a container layout, with the shipped example spec beside its future intent.
struct Composition {
    fixture: Fixture,
    spec: PathBuf,
    output: PathBuf,
}

impl Composition {
    fn new() -> Self {
        let fixture = Fixture::new();
        let plan = fixture.root.join("_work/30-plan");
        fs::create_dir_all(plan.join("jtasks")).unwrap();
        fs::write(plan.join("PLAN.md"), PLAN).unwrap();
        fs::write(plan.join("PLAN-delivery.md"), DELIVERY).unwrap();
        let spec = plan.join("jtasks/example.spec.json");
        fs::copy(
            source().join("skills/source/to-jtasks/assets/compose.spec.json"),
            &spec,
        )
        .unwrap();
        Self {
            output: plan.join("jtasks/example.json"),
            fixture,
            spec,
        }
    }

    fn compose(&self, extra: &[&str]) -> Output {
        Command::new(python())
            .arg(source().join("skills/source/to-jtasks/scripts/compose.py"))
            .arg(&self.spec)
            .args(extra)
            .output()
            .expect("the compose helper needs Python 3.10+")
    }

    fn edit_spec(&self, change: impl FnOnce(&mut Value)) {
        let mut spec: Value = serde_json::from_slice(&fs::read(&self.spec).unwrap()).unwrap();
        change(&mut spec);
        fs::write(&self.spec, serde_json::to_vec(&spec).unwrap()).unwrap();
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn shipped_spec_composes_plan_text_and_passes_public_validation_without_state() {
    let c = Composition::new();
    let jtasks = env!("CARGO_BIN_EXE_jtasks");
    let output = c.compose(&["--jtasks", jtasks, "--root", c.fixture.root.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output));
    let summary = text(&output);
    assert!(summary.contains("jtasks validate: valid=True tasks=3 operations=2"), "{summary}");
    assert!(summary.contains("operation toolchain: prerequisite for 2 task(s)"), "{summary}");

    let intent: Value = serde_json::from_slice(&fs::read(&c.output).unwrap()).unwrap();
    let plan_dir = c.spec.parent().unwrap().parent().unwrap().canonicalize().unwrap();
    let plan_dir = plan_dir.to_str().unwrap().trim_start_matches(r"\\?\").replace('\\', "/");

    // Requirements come from the plan: numbered use cases, then the pointed list item, each on one line.
    let ids: Vec<&str> = intent["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["UC01", "UC02", "AC-1"]);
    assert_eq!(
        intent["requirements"][1]["text"],
        "As an operator, I want a health check, so that I can monitor the service."
    );
    assert_eq!(
        intent["requirements"][2]["text"],
        "AC-1 (end to end): a caller is greeted by name."
    );

    // The shared context is the plan's own paragraph and parts.
    assert_eq!(
        intent["context"]["brief"],
        "Greeting service. Build the greeting service in two slices. Read PLAN-delivery.md first."
    );
    assert_eq!(intent["context"]["references"][0], format!("{plan_dir}/PLAN.md"));
    assert_eq!(
        intent["context"]["references"][1],
        format!("{plan_dir}/PLAN-delivery.md (sub-plan: the slices and their proof.)")
    );
    assert_eq!(
        intent["context"]["limits"][0],
        format!("One line per finished slice in {plan_dir}/PROGRESS.md.")
    );

    // Group templates are filled from table rows, including a row a blank line cut off and an escaped pipe.
    let s2 = &intent["tasks"][1];
    assert_eq!(s2["title"], "S2: The health check");
    assert_eq!(s2["requirements"], serde_json::json!(["UC02"]));
    assert_eq!(s2["depends_on"], serde_json::json!(["S1"]));
    assert_eq!(s2["context"]["brief"], "Slice S2 of the plan (1. Slices).");
    assert_eq!(
        s2["context"]["requirements"],
        serde_json::json!([
            "Delivers: The health check",
            "Proof: `GET /health` answers 200 | 503",
            "Files it owns: src/health.rs, src/routes.rs"
        ])
    );
    assert_eq!(s2["context"]["dependency_artifacts"], serde_json::json!(["src/greet.rs from S1"]));
    assert_eq!(s2["executor"]["kind"], "fixture");
    assert_eq!(intent["tasks"][0]["admission"]["resources"], serde_json::json!(["path:src/greet.rs"]));

    // Group references and dependency shorthand are expanded.
    assert_eq!(intent["tasks"][2]["depends_on"], serde_json::json!(["S1", "S2"]));
    assert_eq!(
        intent["operations"][0]["stage"]["consumers"],
        serde_json::json!(["S1", "S2"])
    );
    assert_eq!(
        intent["operations"][1]["depends_on"],
        serde_json::json!([{"kind": "operation", "id": "toolchain"}])
    );
    assert!(
        !c.fixture.data.exists() && !c.fixture.root.join(".git/jtasks-v2").exists(),
        "composing, validating and previewing must not create runtime state"
    );
}

#[test]
fn check_mode_writes_nothing_and_detects_a_plan_that_moved_on() {
    let c = Composition::new();
    let missing = c.compose(&["--check"]);
    assert!(!missing.status.success());
    assert!(!c.output.exists(), "check mode must not write the intent");

    assert!(c.compose(&[]).status.success());
    let composed = fs::read(&c.output).unwrap();
    let same = c.compose(&["--check"]);
    assert!(same.status.success(), "{}", text(&same));

    let delivery = c.spec.parent().unwrap().parent().unwrap().join("PLAN-delivery.md");
    fs::write(&delivery, DELIVERY.replace("The health check", "The readiness check")).unwrap();
    let stale = c.compose(&["--check"]);
    assert!(!stale.status.success());
    assert!(text(&stale).contains("is not what the spec composes"), "{}", text(&stale));
    assert_eq!(fs::read(&c.output).unwrap(), composed, "check mode must not rewrite");
}

#[test]
fn mistakes_in_the_spec_are_named_and_nothing_is_written() {
    let cases: [(&str, fn(&mut Value), &str); 6] = [
        (
            "a column the table does not have",
            |s| s["groups"]["slice"]["title"] = "{id}: {slice.Delivery}".into(),
            "lookup 'slice' has no column 'Delivery'",
        ),
        (
            "a task the plan has no row for",
            |s| s["tasks"][0]["id"] = "S9".into(),
            "lookup 'slice' finds no row for 'S9'",
        ),
        (
            "a requirement no task covers",
            |s| s["tasks"][2]["requirements"] = serde_json::json!([]),
            "requirements no task covers: AC-1",
        ),
        (
            "a use case the plan does not number",
            |s| s["tasks"][0]["use_cases"] = serde_json::json!([7]),
            "unknown requirements UC07",
        ),
        (
            "a dependency on a task that does not exist",
            |s| s["tasks"][1]["depends_on"] = serde_json::json!(["S7"]),
            "depends on unknown tasks S7",
        ),
        (
            "a missing policy answer",
            |s| {
                s.as_object_mut().unwrap().remove("operation_placement");
            },
            "the spec needs 'operation_placement'",
        ),
    ];
    for (name, change, message) in cases {
        let c = Composition::new();
        c.edit_spec(change);
        let output = c.compose(&[]);
        assert!(!output.status.success(), "{name}");
        assert!(text(&output).contains(message), "{name}: {}", text(&output));
        assert!(!c.output.exists(), "{name}: a refused spec must not write the intent");
    }
}
