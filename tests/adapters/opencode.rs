use super::support::Fixture;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

// Faithful 2.3.0 run JSONL and session-export substitute. No account turn,
// global configuration, shared service, or extra result-repair model call.
const SUBSTITUTE: &str = r#"
import json, os, subprocess, sys
from pathlib import Path
mode=sys.argv[1]
args=sys.argv[2:]
if args[0]=='session':
    Path('resume-check.json').write_text(json.dumps(args))
    if mode=='unavailable': sys.exit(1)
    print(json.dumps({'sessionID':args[-1]}))
    sys.exit(0)
prompt=sys.stdin.read()
Path('seen.json').write_text(json.dumps({'args':args,'prompt':prompt,'cwd':os.getcwd(),'pwd':os.environ.get('PWD')}))
session=args[args.index('--session')+1] if '--session' in args else 'ses_exact123'
def emit(value):
    raw=(json.dumps(value,ensure_ascii=False)+'\n').encode()
    for n in range(0,len(raw),7): os.write(1,raw[n:n+7])
emit({'type':'future.event','sessionID':'ses_ignore','payload':'preserved'})
emit({'type':'step_start','sessionID':session,'part':{'sessionID':session,'type':'step-start'}})
emit({'type':'text','sessionID':'ses_helper','parentID':session,'part':{'text':'helper completed'}})
emit({'type':'text','sessionID':session,'part':{'sessionID':session,'text':'日本語 {"outcome":"completed","summary":"not designated"}'}})
emit({'type':'tool_use','sessionID':session,'part':{'sessionID':session,'tool':'subagent','state':{'status':'completed','metadata':{'sessionID':'ses_helper'},'output':'{"outcome":"completed"}'}}})
if mode in ['client','partial','exit','client-error','resume','unavailable']:
    subprocess.run([os.environ['JTASKS_BIN'],'result','--receipt','scoped-opencode','--outcome','completed','--summary','substitute completed'],check=True,stdout=subprocess.DEVNULL)
if mode=='error' or mode=='client-error': emit({'type':'error','sessionID':session,'error':{'type':'authentication','message':'authentication rejected'}})
if mode=='mismatch': emit({'type':'text','sessionID':'ses_other','part':{'sessionID':'ses_other','text':'wrong session'}})
if mode=='part': emit({'type':'text','sessionID':session,'part':{'sessionID':'ses_other','text':'wrong part'}})
if mode!='missing': emit({'type':'step_finish','sessionID':session,'part':{'sessionID':session,'type':'step-finish','reason':'stop','tokens':{},'cost':0}})
if mode=='partial': os.write(1,b'{"unfinished":')
os.write(2,b'available diagnostic bytes\n')
if mode=='exit': sys.exit(7)
"#;

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn executor(f: &Fixture, mode: &str) -> Value {
    let script = f.root.join("substitute.py");
    fs::write(&script, SUBSTITUTE).unwrap();
    json!({"kind":"opencode","executable":"python","launcher_args":[script,mode],
        "provider":"selected-provider","model":"selected/model","variant":"selected-variant","native_agent":"native-reviewer",
        "prompt":"literal ; $(Write-Output INJECTED) `echo wrong` \"quotes\"\nUnicode: 日本語"})
}
fn setup(mode: &str) -> Fixture {
    let f = Fixture::new();
    let executor = executor(&f, mode);
    f.import(json!([{"id":"T01","title":"Direct OpenCode","executor":executor}]));
    f
}
fn run(f: &Fixture) -> Value {
    let output = f.call(&["run", "--run", "demo"]);
    assert!(
        matches!(output.status.code(), Some(0 | 3)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    f.snapshot()
}
fn dir(f: &Fixture, status: &Value) -> PathBuf {
    let attempt = status["tasks"][0]["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] != "prior")
        .unwrap();
    f.data
        .join("attempts")
        .join(attempt["id"].as_str().unwrap())
}

#[test]
fn public_cli_uses_private_custody_explicit_selection_and_literal_stdin() {
    let f = setup("partial");
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let dir = dir(&f, &status);
    let observation = read(dir.join("opencode-observation.json"));
    assert_eq!(observation["session_id"], "ses_exact123");
    assert_eq!(observation["step_finishes"], 1);
    assert_eq!(observation["unknown_records"], 1);
    assert_eq!(observation["invalid_records"], 1);
    assert_eq!(observation["activity_records"], 4);
    assert_eq!(observation["result_source"], "scoped_client");
    assert!(
        fs::read_to_string(dir.join("stdout.log"))
            .unwrap()
            .ends_with("{\"unfinished\":")
    );
    assert_eq!(
        fs::read(dir.join("stderr.log")).unwrap(),
        b"available diagnostic bytes\n"
    );
    let seen = read(f.root.join("seen.json"));
    let args = seen["args"].as_array().unwrap();
    for option in [
        "run",
        "--standalone",
        "--format",
        "json",
        "--model",
        "selected-provider/selected/model#selected-variant",
        "--agent",
        "native-reviewer",
        "--auto",
    ] {
        assert!(args.contains(&json!(option)), "missing {option}: {args:?}");
    }
    for absent in ["--continue", "--server", "--fork", "--worktree"] {
        assert!(!args.contains(&json!(absent)));
    }
    assert!(
        seen["prompt"]
            .as_str()
            .unwrap()
            .contains("$(Write-Output INJECTED)")
    );
    assert!(seen["prompt"].as_str().unwrap().contains("日本語"));
    assert_eq!(
        fs::canonicalize(seen["cwd"].as_str().unwrap()).unwrap(),
        fs::canonicalize(seen["pwd"].as_str().unwrap()).unwrap()
    );
    assert_eq!(read(dir.join("custody.json"))["empty"], true);
    let launch = read(dir.join("opencode-launch.json"));
    assert_eq!(launch["capabilities"]["structured_native_final"], false);
}

#[test]
fn prose_helpers_steps_and_exit_never_designate_a_result() {
    for (mode, complete, state) in [
        ("prose", false, "needs_intervention"),
        ("missing", false, "needs_intervention"),
        ("error", false, "failed"),
        ("mismatch", false, "failed"),
        ("part", false, "failed"),
        ("client", true, "completed"),
        ("client-error", true, "completed"),
        ("exit", true, "completed"),
    ] {
        let f = setup(mode);
        let status = run(&f);
        assert_eq!(status["complete"], complete, "{mode}: {status}");
        assert_eq!(status["tasks"][0]["state"], state, "{mode}: {status}");
        if mode == "error" {
            assert_eq!(
                read(dir(&f, &status).join("opencode-observation.json"))["failure_class"],
                "authentication_or_access"
            );
        }
    }
}

fn prior(f: &Fixture, task: &str, session: &str) {
    let path = f.data.join("attempts/prior");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("opencode-observation.json"),serde_json::to_vec(&json!({"task_id":task,"attempt_id":"prior","session_id":session,
        "step_finishes":0,"activity_records":0,"unknown_records":0,"invalid_records":0,"protocol_error":null,"failure_class":null,"result_source":null,"raw_stdout":"stdout.log","raw_stderr":"stderr.log"})).unwrap()).unwrap();
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    conn.execute(
        "INSERT INTO attempts(id,run_id,task_id,state) VALUES('prior','demo','T01','retired')",
        [],
    )
    .unwrap();
}

#[test]
fn exact_same_task_resume_is_checked_before_launch_and_restricted_rejects_asks() {
    let f = Fixture::new();
    let mut config = executor(&f, "resume");
    config["resume_attempt"] = json!("prior");
    config["permissions"] = json!("restricted");
    f.import(json!([{"id":"T01","title":"Resume","executor":config}]));
    prior(&f, "T01", "ses_retired123");
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let args = read(f.root.join("seen.json"))["args"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        args.windows(2)
            .any(|w| w == [json!("--session"), json!("ses_retired123")])
    );
    assert!(!args.contains(&json!("--continue")) && !args.contains(&json!("--auto")));
    let check = read(f.root.join("resume-check.json"));
    assert_eq!(
        check,
        json!([
            "session",
            "export",
            "--standalone",
            "--sanitize",
            "ses_retired123"
        ])
    );
    assert_eq!(
        read(dir(&f, &status).join("opencode-launch.json"))["capabilities"]["permission_asks"],
        "auto-reject"
    );
}

#[test]
fn foreign_invalid_or_unavailable_resume_never_starts_a_fresh_worker() {
    for mode in ["foreign", "invalid", "unavailable"] {
        let f = Fixture::new();
        let mut config = executor(&f, mode);
        config["resume_attempt"] = json!("prior");
        f.import(json!([{"id":"T01","title":"Resume","executor":config}]));
        prior(
            &f,
            if mode == "foreign" { "OTHER" } else { "T01" },
            if mode == "invalid" {
                "last"
            } else {
                "ses_prior123"
            },
        );
        let output = f.call(&["run", "--run", "demo"]);
        assert!(!output.status.success(), "{mode}");
        assert!(!f.root.join("seen.json").exists(), "{mode} spawned");
    }
}

#[test]
fn alias_launch_uses_immutable_task_selection_and_records_native_defaults() {
    let f = Fixture::new();
    let native = executor(&f, "client");
    fs::create_dir_all(f.data.join("config")).unwrap();
    fs::write(f.data.join("config/profiles.json"),serde_json::to_vec(&json!({"schema_version":1,"profiles":{"review":{
        "executor":"opencode","provider":"profile-provider","model":"profile-model","variant":"profile-variant",
        "autonomy":"restricted","executable":"python","launcher_args":native["launcher_args"]}}})).unwrap()).unwrap();
    f.import(json!([{"id":"T01","title":"Alias","executor":{"kind":"profile","alias":"review","overrides":{"model":"task-model","variant":"task-variant"}}}]));
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let args = read(f.root.join("seen.json"))["args"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        args.contains(&json!("profile-provider/task-model#task-variant"))
            && args.contains(&json!("build"))
    );
    assert!(!args.contains(&json!("--auto")));
    let snapshot = &status["tasks"][0]["attempts"][0]["effective_profile"];
    assert_eq!(snapshot["origins"]["model"], "task");
    assert_eq!(snapshot["origins"]["native_agent"], "supported_default");
    assert_eq!(snapshot["values"]["native_agent"], "build");
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    let saved: String = conn
        .query_row(
            "SELECT effective_profile FROM attempts WHERE task_id='T01'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(&serde_json::from_str::<Value>(&saved).unwrap(), snapshot);
}

#[test]
fn malformed_native_selectors_fail_without_provider_fallback() {
    for (field, value) in [
        ("provider", "provider/model"),
        ("model", "model#hidden-variant"),
        ("variant", ""),
        ("native_agent", "bad agent"),
    ] {
        let f = Fixture::new();
        let mut config = executor(&f, "client");
        config[field] = json!(value);
        f.import(json!([{"id":"T01","title":"Invalid","executor":config}]));
        let output = f.call(&["run", "--run", "demo"]);
        assert!(!output.status.success(), "{field}");
        assert!(!f.root.join("seen.json").exists(), "{field} spawned");
    }
}

#[test]
#[ignore = "installed OpenCode --version only; no model turn, metadata service or account mutation"]
fn installed_opencode_native_npm_shim_uses_common_custody_without_a_model_turn() {
    let f = Fixture::new();
    f.import(json!([{"id":"T01","title":"Version probe","executor":{"kind":"opencode","launcher_args":["--version"],"provider":"unprobed","model":"unprobed"}}]));
    let status = run(&f);
    assert_eq!(status["complete"], false);
    let dir = dir(&f, &status);
    assert!(
        fs::read_to_string(dir.join("stdout.log"))
            .unwrap()
            .contains("opencode v2.")
    );
    assert_eq!(read(dir.join("custody.json"))["empty"], true);
}
