mod support;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use support::Fixture;

fn command(f: &Fixture) -> Command {
    let mut c = f.command();
    c.env("JTASKS_CONFIG_HOME", f.root.join("isolated-user-config"));
    c
}
fn call(f: &Fixture, args: &[&str]) -> Output {
    command(f).args(args).output().unwrap()
}
fn success(f: &Fixture, args: &[&str]) -> Value {
    let out = call(f, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn write(f: &Fixture, name: &str, value: Value) -> std::path::PathBuf {
    let path = f.root.join(name);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    path
}
fn set(f: &Fixture, alias: &str, value: Value, scope: &str) {
    let file = write(f, "profile-input.json", value);
    success(
        f,
        &[
            "profiles",
            "set",
            alias,
            "--file",
            file.to_str().unwrap(),
            "--scope",
            scope,
        ],
    );
}

#[test]
fn public_alias_commands_replace_user_alias_and_report_every_effective_origin() {
    let f = Fixture::new();
    let initial = success(&f, &["profiles", "list"]);
    assert_eq!(initial["aliases"], json!([]));
    assert!(
        !f.data.exists(),
        "read-only listing must not create runtime/config"
    );
    set(
        &f,
        "work",
        json!({"executor":"codex","provider":"user-provider","model":"user-model","effort":"high","native_profile":"user-native","variant":"user-variant"}),
        "user",
    );
    set(
        &f,
        "work",
        json!({"executor":"codex","model":"project-model","native_profile":"native-profile"}),
        "project",
    );
    let missing = call(&f, &["profiles", "show", "work"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("provider"));
    let defaults = write(
        &f,
        "defaults.json",
        json!({"provider":"run-provider","effort":"medium"}),
    );
    let overrides = write(
        &f,
        "overrides.json",
        json!({"model":"task-model","autonomy":"restricted"}),
    );
    let view = success(
        &f,
        &[
            "profiles",
            "show",
            "work",
            "--defaults",
            defaults.to_str().unwrap(),
            "--overrides",
            overrides.to_str().unwrap(),
        ],
    );
    assert_eq!(view["alias"], "work");
    assert_eq!(view["values"]["model"], "task-model");
    assert_eq!(view["values"]["provider"], "run-provider");
    assert_eq!(view["values"]["effort"], "medium");
    assert_eq!(view["values"]["autonomy"], "restricted");
    assert!(
        view["values"].get("variant").is_none(),
        "replacement cannot inherit user variant"
    );
    assert_eq!(view["values"]["native_profile"], "native-profile");
    assert_eq!(view["origins"]["model"], "task");
    assert_eq!(view["origins"]["provider"], "run_defaults");
    assert!(
        view["origins"]["native_profile"]
            .as_str()
            .unwrap()
            .starts_with("project:")
    );
    assert_eq!(view["origins"]["executable"], "supported_default");
    assert!(view["executor_version"].is_null());
    let import = write(
        &f,
        "import.json",
        json!({"schema_version":1,"profiles":{"review":{"executor":"opencode","provider":"provider","model":"synthetic-model","variant":"synthetic-variant","native_agent":"native-reviewer"}}}),
    );
    assert_eq!(
        success(&f, &["profiles", "import", import.to_str().unwrap()])["imported"],
        1
    );
    let review = success(&f, &["profiles", "show", "review"]);
    assert_eq!(review["values"]["native_agent"], "native-reviewer");
    assert_eq!(review["values"]["variant"], "synthetic-variant");
    assert_eq!(
        success(&f, &["profiles", "list"])["aliases"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!f.data.join("state.sqlite3").exists());
}

#[test]
fn invalid_import_is_atomic_and_native_names_are_not_jtasks_aliases() {
    let f = Fixture::new();
    set(
        &f,
        "work",
        json!({"executor":"codex","provider":"p","model":"m"}),
        "project",
    );
    let before = fs::read(f.data.join("config/profiles.json")).unwrap();
    let file = write(
        &f,
        "invalid.json",
        json!({"schema_version":1,"profiles":{"work":{"executor":"codex","model":"changed"},"bad":{"executor":"made-up"}}}),
    );
    let out = call(&f, &["profiles", "import", file.to_str().unwrap()]);
    assert!(!out.status.success());
    assert_eq!(
        before,
        fs::read(f.data.join("config/profiles.json")).unwrap()
    );
    let out = call(&f, &["profiles", "show", "native-only"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("native_profile/native_agent"));
    let file = write(
        &f,
        "unknown.json",
        json!({"executor":"codex","model":"m","api_key":"must-not-be-accepted"}),
    );
    assert!(
        !call(
            &f,
            &["profiles", "set", "bad", "--file", file.to_str().unwrap()]
        )
        .status
        .success()
    );
}

const NATIVE_SUBSTITUTE: &str = r#"
import json, os, sys, time
from pathlib import Path
args=sys.argv[1:]
sys.stdin.read()
Path('seen.json').write_text(json.dumps(args))
schema=json.loads(Path(args[args.index('--output-schema')+1]).read_text())
result={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
result.update(outcome='completed',summary='isolated native substitute')
print(json.dumps({'type':'thread.started','thread_id':'isolated-exact-thread'}),flush=True)
print(json.dumps({'type':'turn.started'}),flush=True)
for _ in range(500):
    if Path('release-worker').exists(): break
    time.sleep(.01)
text=json.dumps(result)
Path(args[args.index('--output-last-message')+1]).write_text(text)
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':text}}),flush=True)
print(json.dumps({'type':'turn.completed'}),flush=True)
"#;

#[test]
fn running_attempt_keeps_exact_snapshot_after_profile_edit_and_next_attempt_revalidates() {
    let f = Fixture::new();
    let script = f.root.join("native-substitute.py");
    fs::write(&script, NATIVE_SUBSTITUTE).unwrap();
    set(
        &f,
        "work",
        json!({"executor":"codex","provider":"synthetic-provider","model":"model-before","effort":"high","native_profile":"native-not-alias","executable":"python","launcher_args":[script]}),
        "project",
    );
    let plan = write(
        &f,
        "profile-plan.json",
        json!({"schema_version":1,"completion":"judgment","operation_placement":"none","profile_defaults":{"effort":"low"},"tasks":[{"id":"T1","title":"Profile worker","executor":{"kind":"profile","alias":"work","overrides":{"effort":"medium","autonomy":"restricted"}}}]}),
    );
    success(&f, &["import", plan.to_str().unwrap(), "--run", "demo"]);
    let mut controller = command(&f)
        .args(["run", "--run", "demo"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let live = f.await_status(|s| {
        s["tasks"][0]["attempts"][0]["process_state"] == "running"
            && f.root.join("seen.json").exists()
    });
    let snapshot = live["tasks"][0]["attempts"][0]["effective_profile"].clone();
    assert_eq!(snapshot["alias"], "work");
    assert_eq!(snapshot["values"]["model"], "model-before");
    assert_eq!(snapshot["values"]["effort"], "medium");
    assert_eq!(snapshot["origins"]["effort"], "task");
    let args: Value = serde_json::from_slice(&fs::read(f.root.join("seen.json")).unwrap()).unwrap();
    for expected in [
        "model-before",
        "native-not-alias",
        "model_provider=\"synthetic-provider\"",
        "model_reasoning_effort=\"medium\"",
        "workspace-write",
    ] {
        assert!(
            args.as_array().unwrap().contains(&json!(expected)),
            "{expected}: {args}"
        );
    }
    set(
        &f,
        "work",
        json!({"executor":"codex","provider":"synthetic-provider","model":"model-after","executable":"python","launcher_args":[script]}),
        "project",
    );
    assert_eq!(
        snapshot,
        f.snapshot()["tasks"][0]["attempts"][0]["effective_profile"]
    );
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    assert!(
        conn.execute("UPDATE attempts SET effective_profile='{}'", [])
            .is_err()
    );
    fs::write(f.root.join("release-worker"), "").unwrap();
    assert!(controller.wait().unwrap().success());
    assert_eq!(
        snapshot,
        f.snapshot()["tasks"][0]["attempts"][0]["effective_profile"]
    );
    success(&f, &["import", plan.to_str().unwrap(), "--run", "next"]);
    success(&f, &["run", "--run", "next"]);
    let next = success(&f, &["status", "--run", "next"]);
    assert_eq!(
        next["tasks"][0]["attempts"][0]["effective_profile"]["values"]["model"],
        "model-after"
    );
}

#[test]
fn unresolved_alias_does_not_spawn_or_reserve_an_attempt() {
    let f = Fixture::new();
    set(&f, "incomplete", json!({"executor":"codex"}), "project");
    f.import(json!([{"id":"T1","title":"Incomplete","executor":{"kind":"profile","alias":"incomplete"}}]));
    let out = call(&f, &["run", "--run", "demo"]);
    assert_eq!(out.status.code(), Some(3));
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "needs_intervention");
    assert_eq!(status["tasks"][0]["attempts"], json!([]));
    assert!(!Path::new(&f.data.join("attempts")).exists());
}
