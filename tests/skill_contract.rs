#[path = "../scripts/skills.rs"]
mod packages;
mod support;

use serde_json::{Value, json};
use std::{fs, path::Path};
use support::Fixture;

fn source() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn packages_are_exactly_derived_and_drift_is_detected() {
    packages::check(source()).unwrap();
    // Exercise an isolated build: only source inputs, never installed skills.
    let f = Fixture::new();
    for skill in packages::SKILLS {
        fn copy(src: &Path, dest: &Path) {
            fs::create_dir_all(dest).unwrap();
            for entry in fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &dest.join(entry.file_name()));
                } else {
                    fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
                }
            }
        }
        copy(
            &source().join("skills/source").join(skill),
            &f.root.join("skills/source").join(skill),
        );
    }
    packages::build(&f.root).unwrap();
    let entry = f.root.join("skills/opencode/jtasks/SKILL.md");
    fs::write(&entry, "stale instruction").unwrap();
    assert!(packages::check(&f.root).is_err());
    packages::build(&f.root).unwrap();
    let unknown = f.root.join("skills/codex/jtasks/executing-core.js");
    fs::write(&unknown, "unexpected runtime copy").unwrap();
    assert!(
        packages::check(&f.root).is_err(),
        "unexpected package files must not be ignored or deleted"
    );
    assert!(
        unknown.is_file(),
        "parity checker must preserve unexpected user files"
    );
}

#[test]
fn shipped_examples_validate_and_preview_without_execution_or_state_writes() {
    let f = Fixture::new();
    for example in ["judgment.json", "mixed.json"] {
        let path = source()
            .join("skills/source/to-jtasks/assets")
            .join(example);
        let intent: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let result = f.call(&["validate", path.to_str().unwrap()]);
        assert!(
            result.status.success(),
            "{example}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let preview = f.call(&["operations", "preview", path.to_str().unwrap()]);
        assert!(
            preview.status.success(),
            "{}",
            String::from_utf8_lossy(&preview.stderr)
        );
        let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
        assert_eq!(preview["completion"], intent["completion"]);
        assert_eq!(
            preview["operation_placement"],
            intent["operation_placement"]
        );
        assert_eq!(
            preview["operations"].as_array().unwrap().len(),
            intent
                .get("operations")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        );
        for (actual, expected) in preview["operations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(intent["operations"].as_array().into_iter().flatten())
        {
            assert_eq!(actual["id"], expected["id"]);
            assert_eq!(actual["stage"], expected["stage"]);
            assert_eq!(actual["required"], expected["required"]);
            assert_eq!(actual["initial"]["program"], expected["initial"]["program"]);
            assert_eq!(actual["initial"]["args"], expected["initial"]["args"]);
        }
    }
    assert!(
        !f.data.exists(),
        "authoring and preview must not create runtime state"
    );
}

#[test]
fn judgment_example_reaches_designated_completion_with_no_checks() {
    let f = Fixture::new();
    let path = source().join("skills/source/to-jtasks/assets/judgment.json");
    let output = f.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = f.call(&["--json", "run", "--run", "demo"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = f.snapshot();
    assert_eq!(snapshot["tasks"][0]["state"], "completed");
    let output = f.call(&["operations", "status", "--run", "demo"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["verification_complete"], true);
}

#[test]
fn selected_operation_policy_and_prerequisite_consumers_survive_cli_validation() {
    let f = Fixture::new();
    let source = source().join("skills/source/to-jtasks/assets/mixed.json");
    let mut plan: Value = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    let path = f.root.join("policy.json");
    // A real prerequisite cannot be silently retained under a no-operations policy.
    plan["operation_placement"] = json!("none");
    fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(
        !f.call(&["validate", path.to_str().unwrap()])
            .status
            .success()
    );
    plan["operation_placement"] = json!("mixed");
    plan["operations"][0]["stage"]["consumers"] = json!(["missing-task"]);
    fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(
        !f.call(&["validate", path.to_str().unwrap()])
            .status
            .success()
    );
    assert!(!f.data.exists());
}
