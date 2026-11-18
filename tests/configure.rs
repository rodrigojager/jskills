mod support;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::Fixture;

const WORKER: &str = r#"
import json,sys,time
from pathlib import Path
sys.stdin.read()
args=sys.argv[1:]
p=Path('calls.jsonl');p.open('a').write(json.dumps(args)+'\n')
n=len(p.read_text().splitlines())
if n==1:
    Path('first-started').write_text('')
    end=time.time()+20
    while not Path('release').exists() and time.time()<end:time.sleep(0.02)
session=args[args.index('--resume' if '--resume' in args else '--session-id')+1]
permission=args[args.index('--permission-mode')+1]
print(json.dumps({'type':'system','subtype':'init','session_id':session,'permissionMode':permission}))
schema=json.loads(args[args.index('--json-schema')+1]);r={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
r.update(outcome='completed',summary='done')
print(json.dumps({'type':'result','subtype':'success','session_id':session,'is_error':False,'structured_output':r}))
"#;

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
fn set_alias(f: &Fixture, alias: &str, value: Value) {
    let file = f.root.join("alias-input.json");
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    success(
        f,
        &[
            "profiles",
            "set",
            alias,
            "--file",
            file.to_str().unwrap(),
            "--scope",
            "project",
        ],
    );
}
fn worker(f: &Fixture) -> PathBuf {
    let path = f.root.join("worker.py");
    fs::write(&path, WORKER).unwrap();
    path
}
fn alias(script: &PathBuf, model: &str) -> Value {
    json!({"executor":"claude","provider":"anthropic","model":model,"effort":"medium","executable":"python","launcher_args":[script]})
}
fn tasks() -> Value {
    let exception = json!({"actor":"test","reason":"synthetic native protocol substitute, no account"});
    json!([
        {"id":"A","title":"First","unknown_capabilities":exception,"executor":{"kind":"profile","alias":"standard"},
         "reviews":{"max_calls":1,"deadline_seconds":30,"observer_unknown_capabilities":exception,"observer":{"kind":"profile","alias":"watcher"}}},
        {"id":"B","title":"Second","depends_on":["A"],"unknown_capabilities":exception,"executor":{"kind":"profile","alias":"standard"}},
        {"id":"C","title":"Third","depends_on":["B"],"unknown_capabilities":exception,"executor":{"kind":"profile","alias":"deep"}}
    ])
}
fn journal(f: &Fixture, event: &str) -> Vec<Value> {
    let c = Connection::open_with_flags(
        f.data.join("state.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    c.prepare("SELECT detail FROM journal WHERE event=?1 ORDER BY rowid")
        .unwrap()
        .query_map([event], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|d| serde_json::from_str(&d.unwrap()).unwrap())
        .collect()
}
fn prepared() -> Fixture {
    let f = Fixture::new();
    let script = worker(&f);
    set_alias(&f, "standard", alias(&script, "model-one"));
    set_alias(&f, "deep", alias(&script, "model-deep"));
    set_alias(&f, "watcher", alias(&script, "model-watch"));
    f.import(tasks());
    f
}

#[test]
fn list_shows_task_and_observer_aliases_in_plan_order() {
    let f = prepared();
    let list = success(&f, &["configure", "--run", "demo", "--list", "--json"]);
    assert_eq!(list["run"], "demo");
    let aliases = list["aliases"].as_array().unwrap();
    let names: Vec<_> = aliases
        .iter()
        .map(|a| (a["alias"].as_str().unwrap(), a["role"].as_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        [("standard", "tasks"), ("watcher", "observer"), ("deep", "tasks")]
    );
    assert_eq!(aliases[0]["tasks"], json!(["A", "B"]));
    assert_eq!(aliases[0]["configuration"]["model"], "model-one");
    assert_eq!(aliases[0]["source"], "project");
    assert_eq!(aliases[0]["states"]["pending"], 2);
    assert_eq!(aliases[1]["tasks"], json!(["A"]));
    // The folder's current run is used without --run.
    assert_eq!(success(&f, &["configure", "--list"])["run"], "demo");
}

#[test]
fn set_changes_only_given_fields_refuses_invalid_and_next_task_uses_it() {
    let f = prepared();
    let refused = call(
        &f,
        &["configure", "--run", "demo", "--set", "standard", "--effort", "bogus"],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("known-invalid"));
    let refused = call(
        &f,
        &["configure", "--run", "demo", "--set", "standard", "--executor", "nope"],
    );
    assert!(!refused.status.success());
    assert!(journal(&f, "alias_changed").is_empty());

    let mut child = command(&f)
        .args(["run", "--run", "demo"])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f.root.join("first-started").exists() {
        assert!(Instant::now() < deadline, "first task never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = success(
        &f,
        &[
            "configure", "--run", "demo", "--set", "standard", "--model", "model-two", "--actor",
            "tester",
        ],
    );
    assert_eq!(out["alias"], "standard");
    fs::write(f.root.join("release"), "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "foreground deadline");
        std::thread::sleep(Duration::from_millis(20));
    }
    let calls: Vec<Vec<String>> = fs::read_to_string(f.root.join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let model = |args: &Vec<String>| {
        args.windows(2)
            .find(|w| w[0] == "--model")
            .map(|w| w[1].clone())
    };
    let used: Vec<_> = calls.iter().map(model).collect();
    assert_eq!(
        used,
        [
            Some("model-one".to_string()),
            Some("model-two".to_string()),
            Some("model-deep".to_string())
        ]
    );
    let changes = journal(&f, "alias_changed");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["actor"], "tester");
    assert_eq!(changes[0]["alias"], "standard");
    assert_eq!(changes[0]["old"]["model"], "model-one");
    assert_eq!(changes[0]["new"]["model"], "model-two");
    assert_eq!(changes[0]["new"]["effort"], "medium");
    let list = success(&f, &["configure", "--list"]);
    assert_eq!(list["aliases"][0]["configuration"]["provider"], "anthropic");
}

fn menu(f: &Fixture, input: &str) -> Output {
    let mut child = command(f)
        .args(["configure", "--run", "demo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn menu_changes_an_alias_and_escape_writes_nothing() {
    let f = prepared();
    let out = menu(&f, "esc\n");
    let cancelled = menu(&f, "1\nclaude\nmodel-menu\n\x1b\n");
    assert!(cancelled.status.success());
    assert!(journal(&f, "alias_changed").is_empty());
    assert_eq!(
        success(&f, &["configure", "--list"])["aliases"][0]["configuration"]["model"],
        "model-one"
    );
    let done = menu(&f, "1\nclaude\nmodel-menu\nhigh\nanthropic\ny\n");
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let list = success(&f, &["configure", "--list"]);
    assert_eq!(list["aliases"][0]["configuration"]["model"], "model-menu");
    assert_eq!(list["aliases"][0]["configuration"]["effort"], "high");
    assert_eq!(journal(&f, "alias_changed").len(), 1);
    // An empty answer at the first question cancels as well.
    let empty = menu(&f, "\n");
    assert!(empty.status.success());
    assert_eq!(journal(&f, "alias_changed").len(), 1);
}

#[test]
fn choices_lists_efforts_and_models_in_use() {
    let f = prepared();
    let out = success(&f, &["configure", "--choices", "claude", "--json"]);
    assert_eq!(out["executor"], "claude");
    assert!(out["in_use"].as_array().unwrap().contains(&json!("model-one")));
    assert!(out["efforts"].as_array().unwrap().contains(&json!("high")));
    assert!(out["catalogue"].is_string());
    assert!(!call(&f, &["configure", "--choices", "nope"]).status.success());
}

#[test]
fn one_observer_alias_can_switch_between_native_agents() {
    let f = prepared();
    for (agent, provider, model, variant) in [
        ("codex", "custom-codex", "model-one", "high"),
        ("opencode", "company-ai", "vendor/model-two", "priority"),
        ("claude", "vertex", "model-three", "max"),
    ] {
        let change = success(&f, &[
            "configure", "--set", "watcher", "--agent", agent,
            "--provider", provider, "--model", model, "--variant", variant,
        ]);
        assert_eq!(change["changed"]["new"]["executor"], agent);
        assert_eq!(change["changed"]["new"]["provider"], provider);
        assert_eq!(change["changed"]["new"]["model"], model);
        assert_eq!(change["changed"]["new"]["effort"], variant);
    }
}
