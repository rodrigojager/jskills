mod support;

use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use support::Fixture;

fn source() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn python() -> String {
    std::env::var("JTASKS_ESTIMATOR_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into())
}

struct Estimation {
    fixture: Fixture,
    plan: Value,
}

impl Estimation {
    fn new() -> Self {
        let fixture = Fixture::new();
        let mut plan: Value = serde_json::from_slice(
            &fs::read(source().join("skills/source/to-jtasks/assets/mixed.json")).unwrap(),
        )
        .unwrap();
        plan["tasks"][0]["estimate_seconds"] = json!(90);
        plan["tasks"][1]["estimate_seconds"] = Value::Null;
        // This invocation would leave a marker if any operation were dispatched.
        plan["operations"][0]["initial"]["program"] = json!(python());
        plan["operations"][0]["initial"]["args"] = json!([
            "-c",
            "from pathlib import Path; Path('heavy-operation-started').write_text('bad')"
        ]);
        fs::write(
            fixture.root.join("plan.json"),
            serde_json::to_vec(&plan).unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.root.join("policy.json"),
            serde_json::to_vec(&json!({
                "agent":"codex",
                "effective_configuration":{"model":"fixture-only-not-a-real-model","effort":"fixture-effort"},
                "evidence":["Deterministic protocol fixture, not a model timing measurement."],
                "limitations":["No real harness, provider or account was exercised."]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.root.join("fake_harness.py"),
            r#"import json, os, sys, time
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text())
request = json.loads(sys.stdin.read())
Path('observed.json').write_text(json.dumps({'request': request, 'environment': {k:v for k,v in os.environ.items() if k.startswith('JTASKS_') or k in ('CODEX_THREAD_ID','CLAUDE_CODE_SESSION_ID','OPENCODE_SESSION_ID')}}))
if payload.get('mutate'):
    Path('plan.json').write_bytes(Path('plan.json').read_bytes() + b'\n')
time.sleep(payload.get('sleep', 0))
print(payload['reply'])
sys.exit(payload.get('exit', 0))
"#,
        )
        .unwrap();
        Self { fixture, plan }
    }

    fn command(&self, payload: Value) -> Command {
        fs::write(
            self.fixture.root.join("response.json"),
            serde_json::to_vec(&payload).unwrap(),
        )
        .unwrap();
        let mut command = Command::new(python());
        command
            .arg(source().join("skills/source/estimate-jtasks/scripts/estimate.py"))
            .arg(self.fixture.root.join("plan.json"))
            .arg("--policy")
            .arg(self.fixture.root.join("policy.json"))
            .args(["--agent", "codex", "--protocol", "json", "--program"])
            .arg(python())
            .arg("--agent-arg")
            .arg(self.fixture.root.join("fake_harness.py"))
            .arg("--agent-arg")
            .arg(self.fixture.root.join("response.json"))
            .arg("--jtasks")
            .arg(env!("CARGO_BIN_EXE_jtasks"))
            .arg("--root")
            .arg(&self.fixture.root)
            .arg("--candidate")
            .arg(self.fixture.root.join("estimator-candidate.json"))
            .args(["--timeout-seconds", "10"])
            .env("JTASKS_RESULT_TOKEN", "fixture-context-must-not-leak")
            .env("CODEX_THREAD_ID", "planner-session-must-not-be-reused")
            .env(
                "CLAUDE_CODE_SESSION_ID",
                "planner-session-must-not-be-reused",
            )
            .env("OPENCODE_SESSION_ID", "planner-session-must-not-be-reused");
        command
    }

    fn read(&self) -> Value {
        serde_json::from_slice(&fs::read(self.fixture.root.join("plan.json")).unwrap()).unwrap()
    }

    fn assert_no_execution(&self) {
        assert!(!self.fixture.data.exists());
        assert!(!self.fixture.root.join(".git/jtasks-v2").exists());
        assert!(!self.fixture.root.join("heavy-operation-started").exists());
    }
}

fn reply(seconds: Value) -> Value {
    json!({"schema_version":1,"estimates":[{"task_id":"T2","active_seconds":seconds,
        "rationale":"Synthetic active-work estimate for protocol testing; not measured."}],
        "limitations":["Insufficient real-model timing evidence; fixture only."]})
}

#[test]
fn fresh_external_session_merges_only_missing_values_and_public_validation_passes() {
    let e = Estimation::new();
    let output = e
        .command(json!({"reply":reply(json!(480)).to_string()}))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["changed_tasks"], json!(["T2"]));
    assert_eq!(report["session_invoked"], true);
    let mut expected = e.plan.clone();
    expected["tasks"][1]["estimate_seconds"] = json!(480);
    assert_eq!(
        e.read(),
        expected,
        "all graph/criteria/resources/policy fields must survive unchanged"
    );
    let observed: Value =
        serde_json::from_slice(&fs::read(e.fixture.root.join("observed.json")).unwrap()).unwrap();
    assert_eq!(observed["environment"], json!({}));
    assert_eq!(observed["request"]["missing_task_ids"], json!(["T2"]));
    assert_eq!(
        observed["request"]["selected_policy"]["effective_configuration"]["model"],
        "fixture-only-not-a-real-model"
    );
    assert_eq!(observed["request"]["plan"], e.plan);
    let validation = e.fixture.call(&[
        "validate",
        e.fixture.root.join("plan.json").to_str().unwrap(),
    ]);
    assert!(
        validation.status.success(),
        "{}",
        String::from_utf8_lossy(&validation.stderr)
    );
    e.assert_no_execution();
}

#[test]
fn insufficient_evidence_remains_missing_and_all_explicit_estimates_skip_the_session() {
    let e = Estimation::new();
    let before = fs::read(e.fixture.root.join("plan.json")).unwrap();
    let output = e
        .command(json!({"reply":reply(Value::Null).to_string()}))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(e.fixture.root.join("plan.json")).unwrap(), before);
    assert_eq!(e.read(), e.plan);
    e.assert_no_execution();

    let e = Estimation::new();
    let mut explicit = e.plan.clone();
    explicit["tasks"][1]["estimate_seconds"] = json!(300);
    fs::write(
        e.fixture.root.join("plan.json"),
        serde_json::to_vec(&explicit).unwrap(),
    )
    .unwrap();
    let output = e
        .command(json!({"reply":"would be malformed if invoked"}))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!e.fixture.root.join("observed.json").exists());
    assert_eq!(e.read(), explicit);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["session_invoked"], false);
    e.assert_no_execution();
}

#[test]
fn invalid_or_rewriting_responses_never_modify_intent() {
    let mut explicit = reply(json!(1));
    explicit["estimates"][0]["task_id"] = json!("T1");
    let mut duplicate = reply(json!(1));
    let entry = duplicate["estimates"][0].clone();
    duplicate["estimates"].as_array_mut().unwrap().push(entry);
    let mut rewritten = reply(json!(1));
    rewritten["tasks"] = json!([]);
    let mut unknown = reply(json!(1));
    unknown["estimates"][0]["task_id"] = json!("unknown");
    let mut cases: Vec<String> = [
        json!(0),
        json!(-1),
        json!(true),
        json!(1.25),
        json!(u64::MAX),
    ]
    .into_iter()
    .map(|v| reply(v).to_string())
    .collect();
    cases.extend(
        [
            explicit,
            duplicate,
            rewritten,
            unknown,
            json!({"schema_version":1,"estimates":[],"limitations":[]}),
        ]
        .into_iter()
        .map(|v| v.to_string()),
    );
    cases.extend(["NaN", "Infinity", "-Infinity"].map(|n| {
        reply(json!(1))
            .to_string()
            .replace("\"active_seconds\":1", &format!("\"active_seconds\":{n}"))
    }));
    cases.push("arbitrary prose".into());
    for raw in cases {
        let e = Estimation::new();
        let before = fs::read(e.fixture.root.join("plan.json")).unwrap();
        let output = e.command(json!({"reply":raw})).output().unwrap();
        assert!(!output.status.success(), "accepted invalid response");
        assert_eq!(fs::read(e.fixture.root.join("plan.json")).unwrap(), before);
        assert!(!e.fixture.root.join("estimator-candidate.json").exists());
        e.assert_no_execution();
    }
}

#[test]
fn invalid_explicit_values_and_session_reuse_are_rejected_before_invocation() {
    let e = Estimation::new();
    let mut invalid = e.plan.clone();
    invalid["tasks"][0]["estimate_seconds"] = json!(0);
    fs::write(
        e.fixture.root.join("plan.json"),
        serde_json::to_vec(&invalid).unwrap(),
    )
    .unwrap();
    let output = e
        .command(json!({"reply":reply(json!(4)).to_string()}))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        e.read(),
        invalid,
        "an explicit invalid value must be rejected, never fixed"
    );
    assert!(!e.fixture.root.join("observed.json").exists());
    e.assert_no_execution();
    for forbidden in ["resume", "fork", "--resume=last", "--session", "--json"] {
        let e = Estimation::new();
        let output = e
            .command(json!({"reply":reply(json!(4)).to_string()}))
            .arg(format!("--agent-arg={forbidden}"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(e.read(), e.plan);
        assert!(!e.fixture.root.join("observed.json").exists());
    }
}

#[test]
fn harness_failure_deadline_and_concurrent_edits_are_not_silently_overwritten() {
    for payload in [
        json!({"reply":reply(json!(4)).to_string(),"exit":7}),
        json!({"reply":reply(json!(4)).to_string(),"sleep":5}),
        json!({"reply":reply(json!(4)).to_string(),"mutate":true}),
    ] {
        let e = Estimation::new();
        let before = fs::read(e.fixture.root.join("plan.json")).unwrap();
        let mut command = e.command(payload.clone());
        if payload.get("sleep").is_some() {
            // The argparse last occurrence is intentional; no model operation is involved.
            command.args(["--timeout-seconds", "1"]);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        if payload.get("mutate").is_some() {
            let mut modified = before.clone();
            modified.push(b'\n');
            assert_eq!(
                fs::read(e.fixture.root.join("plan.json")).unwrap(),
                modified
            );
        } else {
            assert_eq!(fs::read(e.fixture.root.join("plan.json")).unwrap(), before);
        }
        e.assert_no_execution();
    }
}

#[test]
fn supported_wrapped_protocols_are_explicit_and_mismatches_are_rejected() {
    for (agent, protocol, raw) in [
        (
            "claude",
            "claude-json",
            json!({"is_error":false,"structured_output":reply(json!(35))}).to_string(),
        ),
        (
            "opencode",
            "opencode-json",
            format!(
                "{}\n{}\n{}",
                json!({"type":"step_start"}),
                json!({"type":"text","part":{"text":reply(json!(35)).to_string()}}),
                json!({"type":"step_finish"})
            ),
        ),
    ] {
        let e = Estimation::new();
        let policy_path = e.fixture.root.join("policy.json");
        let mut policy: Value = serde_json::from_slice(&fs::read(&policy_path).unwrap()).unwrap();
        policy["agent"] = json!(agent);
        fs::write(policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
        let output = e
            .command(json!({"reply":raw}))
            .args(["--agent", agent, "--protocol", protocol])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut expected = e.plan.clone();
        expected["tasks"][1]["estimate_seconds"] = json!(35);
        assert_eq!(e.read(), expected);
        e.assert_no_execution();
    }
    let e = Estimation::new();
    let output = e
        .command(json!({"reply":reply(json!(1)).to_string()}))
        .args(["--protocol", "claude-json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!e.fixture.root.join("observed.json").exists());
    assert_eq!(e.read(), e.plan);
}
