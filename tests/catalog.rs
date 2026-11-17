mod support;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Output,
};
use support::Fixture;

fn call(f: &Fixture, args: &[&str]) -> Output {
    f.command()
        .env("JTASKS_CONFIG_HOME", f.root.join("isolated-user"))
        .args(args)
        .output()
        .unwrap()
}
fn success(f: &Fixture, args: &[&str]) -> Value {
    let output = call(f, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn write(f: &Fixture, name: &str, value: &Value) -> PathBuf {
    let path = f.root.join(name);
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}
fn tree(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(tree(&path));
            } else {
                files.insert(path.clone(), fs::read(path).unwrap());
            }
        }
    }
    files
}
fn cached(f: &Fixture, executor: &str, executable: &str) -> Value {
    success(f, &["catalog", executor, "--executable", executable])["snapshot"].clone()
}
fn snapshot(f: &Fixture, executor: &str) -> Value {
    let mut value = cached(f, executor, executor);
    value["observed_at"] = json!(1);
    value["recommended_refresh_at"] = json!(4102444800u64);
    value["executor_version"] = json!(if executor == "codex" {
        "0.169.0"
    } else {
        "2.3.0"
    });
    value["availability"] = json!("available_partial");
    value["choices"] = json!([
        {"field":"provider","value":"p","provider":null,"model":null},
        {"field":"model","value":"m","provider":"p","model":null}
    ]);
    value["complete"] = json!(["provider", "model"]);
    value
}
fn save(f: &Fixture, executor: &str, value: &Value) {
    let path = write(f, "snapshot-input.json", value);
    success(
        f,
        &["catalog", executor, "--snapshot", path.to_str().unwrap()],
    );
}
fn plan(f: &Fixture, executor: Value, exception: Option<Value>) -> PathBuf {
    let mut task = json!({"id":"T1","title":"Catalog admission","executor":executor});
    if let Some(exception) = exception {
        task["unknown_capabilities"] = exception;
    }
    f.plan(json!([task]))
}
fn exception() -> Value {
    json!({"actor":"test-operator","reason":"isolated substitute; unknown evidence accepted"})
}

#[test]
fn reads_do_not_probe_create_or_change_any_cached_bytes() {
    let f = Fixture::new();
    for args in [
        &["executors", "--json"][..],
        &["doctor"][..],
        &["catalog", "codex"][..],
    ] {
        success(&f, args);
        assert!(!f.data.exists());
    }
    save(&f, "codex", &snapshot(&f, "codex"));
    let before = tree(&f.data);
    for args in [
        &["executors"][..],
        &["doctor"][..],
        &["catalog", "codex", "--provider", "p", "--model", "m"][..],
    ] {
        let value = success(&f, args);
        assert!(!value.to_string().contains("api_key"));
        assert_eq!(before, tree(&f.data));
    }
    assert!(!f.data.join("state.sqlite3").exists());
}

#[test]
fn complete_cached_catalog_validates_without_inventing_account_access() {
    let f = Fixture::new();
    save(&f, "codex", &snapshot(&f, "codex"));
    let path = plan(&f, json!({"kind":"codex","provider":"p","model":"m"}), None);
    let validated = success(&f, &["validate", path.to_str().unwrap(), "--json"]);
    assert_eq!(validated["capabilities"][0]["validation"]["state"], "known");
    assert_eq!(
        validated["capabilities"][0]["validation"]["account_access"],
        "not_verified"
    );
    assert_eq!(
        validated["capabilities"][0]["validation"]["catalog"]["source"],
        "explicit_snapshot"
    );
    let bytes = tree(&f.data);
    success(&f, &["validate", path.to_str().unwrap()]);
    assert_eq!(bytes, tree(&f.data));
}

#[test]
fn known_invalid_complete_absence_and_adapter_invalidity_survive_unknown_exception() {
    let f = Fixture::new();
    save(&f, "codex", &snapshot(&f, "codex"));
    for executor in [
        json!({"kind":"codex","provider":"p","model":"absent"}),
        json!({"kind":"codex","provider":"p","model":"m","reasoning_effort":"invented"}),
        json!({"kind":"claude","provider":"bad-provider","model":"m"}),
    ] {
        let path = plan(&f, executor, Some(exception()));
        let output = call(&f, &["validate", path.to_str().unwrap()]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("known-invalid"));
    }
    let path = plan(
        &f,
        json!({"kind":"codex","provider":"p","model":"absent"}),
        Some(exception()),
    );
    success(&f, &["import", path.to_str().unwrap(), "--run", "demo"]);
    assert_eq!(call(&f, &["run", "--run", "demo"]).status.code(), Some(3));
    let state = f.snapshot();
    assert_eq!(state["tasks"][0]["attempts"], json!([]));
    assert_eq!(state["tasks"][0]["state"], "needs_intervention");
    assert!(!f.data.join("attempts").exists());
}

#[test]
fn incomplete_stale_and_differently_scoped_catalogs_need_recorded_exception() {
    let f = Fixture::new();
    let executor = json!({"kind":"codex","provider":"p","model":"m"});
    let path = plan(&f, executor.clone(), None);
    assert!(
        !call(&f, &["validate", path.to_str().unwrap()])
            .status
            .success()
    );
    let path = plan(&f, executor.clone(), Some(exception()));
    let value = success(&f, &["validate", path.to_str().unwrap()]);
    assert_eq!(
        value["capabilities"][0]["validation"]["state"],
        "unknown_allowed"
    );
    assert_eq!(
        value["capabilities"][0]["validation"]["exception"]["actor"],
        "test-operator"
    );
    let mut stale = snapshot(&f, "codex");
    stale["recommended_refresh_at"] = json!(2);
    save(&f, "codex", &stale);
    let path = plan(&f, executor.clone(), None);
    assert!(
        !call(&f, &["validate", path.to_str().unwrap()])
            .status
            .success()
    );
    save(&f, "codex", &snapshot(&f, "codex"));
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["tasks"][0]["catalog_scope"] = json!("different-native-config");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !call(&f, &["validate", path.to_str().unwrap()])
            .status
            .success()
    );
    value["tasks"][0]["unknown_capabilities"] = json!({"actor":"","reason":""});
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !call(&f, &["validate", path.to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn scoped_snapshot_cannot_claim_account_access_or_leak_arbitrary_native_metadata() {
    let f = Fixture::new();
    let mut value = snapshot(&f, "codex");
    value["account_access"] = json!("authorized");
    let input = write(&f, "bad.json", &value);
    assert!(
        !call(
            &f,
            &["catalog", "codex", "--snapshot", input.to_str().unwrap()]
        )
        .status
        .success()
    );
    value["account_access"] = json!("not_verified");
    value["scope"]["checkout"] = json!(f.root.join("another-checkout"));
    fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !call(
            &f,
            &["catalog", "codex", "--snapshot", input.to_str().unwrap()]
        )
        .status
        .success()
    );
    assert!(!f.data.exists());
}

const METADATA: &str = r#"
import sys
from pathlib import Path
args=sys.argv[1:]
with Path('metadata-calls').open('a') as f: f.write(' '.join(args)+'\n')
if args==['--version']: print('codex-cli 0.169.0')
elif args==['exec','--help']: print('--model --json --output-schema --output-last-message')
elif args==['app-server','--stdio']:
    import json
    for line in sys.stdin:
        request=json.loads(line)
        method=request['method']
        assert method in ['initialize','initialized','model/list']
        if method=='initialize': result={'userAgent':'substitute'}
        elif method=='initialized': continue
        else:
            assert request['params']['includeHidden'] is True
            assert request['params']['modelProvider']=='p'
            more=request['params']['cursor'] is None
            entry={'model':'m' if more else 'n'}
            if more: entry['supportedReasoningEfforts']=[{'reasoningEffort':'high'}]
            result={'data':[entry], 'nextCursor':'second' if more else None}
        print(json.dumps({'id':request['id'],'result':result}),flush=True)
else: raise RuntimeError('a provider/model turn must never be started')
print('synthetic-secret-DO-NOT-PERSIST',file=sys.stderr)
"#;
#[test]
fn refresh_enumerates_paginated_models_and_efforts_without_any_turn_and_replaces_cache() {
    let f = Fixture::new();
    let script = f.root.join("metadata.py");
    fs::write(&script, METADATA).unwrap();
    let args = [
        "catalog",
        "codex",
        "--refresh",
        "--provider",
        "p",
        "--executable",
        "python",
        "--launcher-arg",
        script.to_str().unwrap(),
    ];
    let value = success(&f, &args);
    assert_eq!(value["snapshot"]["executor_version"], "0.169.0");
    assert_eq!(value["snapshot"]["availability"], "available_partial");
    assert_eq!(value["snapshot"]["metadata_status"], "observed");
    assert_eq!(value["snapshot"]["source"], "native_metadata");
    assert_eq!(value["snapshot"]["complete"], json!(["model"]));
    assert!(
        value["snapshot"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["field"] == "effort" && c["model"] == "m" && c["value"] == "high")
    );
    assert!(
        value["snapshot"]["native_flags"]
            .as_array()
            .unwrap()
            .contains(&json!("--model"))
    );
    assert_eq!(
        fs::read_to_string(f.root.join("metadata-calls"))
            .unwrap()
            .replace("\r\n", "\n"),
        "--version\nexec --help\napp-server --stdio\n"
    );
    let before = tree(&f.data);
    success(
        &f,
        &[
            "catalog",
            "codex",
            "--executable",
            "python",
            "--launcher-arg",
            script.to_str().unwrap(),
        ],
    );
    assert_eq!(before, tree(&f.data));
    assert!(
        !String::from_utf8_lossy(&fs::read(f.data.join("config/catalog.json")).unwrap())
            .contains("synthetic-secret")
    );
    // Replacing an existing target exercises Windows rename replacement, not
    // merely first creation. The cache remains a single valid scoped snapshot.
    fs::write(&script, METADATA.replace("0.169.0", "0.170.0")).unwrap();
    let updated = success(&f, &args);
    assert_eq!(updated["snapshot"]["executor_version"], "0.170.0");
    let cache: Value =
        serde_json::from_slice(&fs::read(f.data.join("config/catalog.json")).unwrap()).unwrap();
    assert_eq!(cache["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(cache["snapshots"][0]["executor_version"], "0.170.0");
    // A complete per-model list establishes incompatible effort, while missing
    // effort metadata for another model remains an explicit unknown.
    let executor = |model: &str, effort: &str| json!({"kind":"codex","provider":"p","model":model,"reasoning_effort":effort,"executable":"python","launcher_args":[script]});
    let path = plan(&f, executor("m", "high"), None);
    assert_eq!(
        success(&f, &["validate", path.to_str().unwrap()])["capabilities"][0]["validation"]["state"],
        "known"
    );
    let path = plan(&f, executor("m", "low"), Some(exception()));
    let output = call(&f, &["validate", path.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("known-invalid effort"));
    let path = plan(&f, executor("n", "low"), Some(exception()));
    assert_eq!(
        success(&f, &["validate", path.to_str().unwrap()])["capabilities"][0]["validation"]["state"],
        "unknown_allowed"
    );
}

#[test]
fn stale_explicit_negative_evidence_stays_invalid_but_expired_absence_is_unknown() {
    let f = Fixture::new();
    let mut value = snapshot(&f, "codex");
    value["recommended_refresh_at"] = json!(2);
    value["known_invalid"] =
        json!([{"field":"model","value":"retired","provider":"p","model":null}]);
    save(&f, "codex", &value);
    let path = plan(
        &f,
        json!({"kind":"codex","provider":"p","model":"retired"}),
        Some(exception()),
    );
    let output = call(&f, &["validate", path.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("known-invalid cached model"));
    let path = plan(
        &f,
        json!({"kind":"codex","provider":"p","model":"newly-added"}),
        Some(exception()),
    );
    assert_eq!(
        success(&f, &["validate", path.to_str().unwrap()])["capabilities"][0]["validation"]["state"],
        "unknown_allowed"
    );
}

#[test]
fn opencode_enumerates_project_models_in_a_private_service_and_claude_reports_limit() {
    let f = Fixture::new();
    let script = f.root.join("metadata.py");
    fs::write(
        &script,
        r#"
import sys
args=sys.argv[1:]
if args==['--version']: print('2.3.0')
elif args==['run','--help']: print('--model --agent --variant')
elif args==['models','--standalone']: print('p/m\np/n\nq/another/model')
elif args==['--help']: print('--model --agent --effort')
else: raise RuntimeError('unexpected service or turn')
"#,
    )
    .unwrap();
    let value = success(
        &f,
        &[
            "catalog",
            "opencode",
            "--refresh",
            "--executable",
            "python",
            "--launcher-arg",
            script.to_str().unwrap(),
        ],
    );
    assert_eq!(value["snapshot"]["metadata_status"], "observed");
    assert_eq!(value["snapshot"]["complete"], json!(["provider", "model"]));
    assert!(
        value["snapshot"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["field"] == "model"
                && c["value"] == "another/model"
                && c["provider"] == "q")
    );
    let value = success(
        &f,
        &[
            "catalog",
            "claude",
            "--refresh",
            "--executable",
            "python",
            "--launcher-arg",
            script.to_str().unwrap(),
        ],
    );
    assert_eq!(
        value["snapshot"]["metadata_status"],
        "native_interface_unavailable"
    );
    assert_eq!(value["snapshot"]["choices"], json!([]));
}

#[test]
fn metadata_timeout_retires_descendants_without_uncontrolled_helpers() {
    let f = Fixture::new();
    let script = f.root.join("hanging-metadata.py");
    fs::write(&script,r#"
import subprocess,sys,time
from pathlib import Path
if '--version' in sys.argv:
    subprocess.Popen([sys.executable,'-c',"from pathlib import Path; import time; time.sleep(1.5); Path('escaped-helper').write_text('bad')"])
    time.sleep(60)
"#).unwrap();
    let start = std::time::Instant::now();
    let value = success(
        &f,
        &[
            "catalog",
            "codex",
            "--refresh",
            "--timeout-ms",
            "100",
            "--executable",
            "python",
            "--launcher-arg",
            script.to_str().unwrap(),
        ],
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(value["snapshot"]["availability"], "probe_timeout");
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert!(!f.root.join("escaped-helper").exists());
}

#[test]
fn alias_native_snapshot_is_enriched_and_rejected_before_admission() {
    let f = Fixture::new();
    save(&f, "codex", &snapshot(&f, "codex"));
    let profile = write(
        &f,
        "alias.json",
        &json!({"executor":"codex","provider":"p","model":"m"}),
    );
    success(
        &f,
        &[
            "profiles",
            "set",
            "work",
            "--file",
            profile.to_str().unwrap(),
        ],
    );
    let shown = success(&f, &["profiles", "show", "work", "--resolved", "--json"]);
    assert_eq!(shown["executor_version"], "0.169.0");
    let path = plan(
        &f,
        json!({"kind":"profile","alias":"work","overrides":{"model":"invalid"}}),
        Some(exception()),
    );
    assert!(
        !call(&f, &["validate", path.to_str().unwrap()])
            .status
            .success()
    );
}
