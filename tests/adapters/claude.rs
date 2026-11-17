use super::support::Fixture;
use serde_json::{Value, json};
use std::fs;

// Native substitute exercises documented CLI NDJSON without account turns,
// native hooks, provider credentials or a reporting-only agent.
const SUBSTITUTE: &str = r#"
import json, os, subprocess, sys
from pathlib import Path
mode=sys.argv[1]
args=sys.argv[2:]
prompt=sys.stdin.read()
Path('seen.json').write_text(json.dumps({'args':args,'prompt':prompt,'cwd':os.getcwd(),'providers':{k:os.environ.get(k) for k in ['CLAUDE_CODE_USE_BEDROCK','CLAUDE_CODE_USE_VERTEX','CLAUDE_CODE_USE_FOUNDRY']}}))
schema=json.loads(args[args.index('--json-schema')+1])
session=args[args.index('--resume' if '--resume' in args else '--session-id')+1]
permission=args[args.index('--permission-mode')+1]
result={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
result.update(outcome='completed',summary='substitute completed')
def emit(v):
    data=(json.dumps(v,ensure_ascii=False)+'\n').encode()
    for offset in range(0,len(data),7): os.write(1,data[offset:offset+7])
if mode=='session': session='99999999-9999-4999-8999-999999999999'
emit({'type':'system','subtype':'init','session_id':session,'model':'native-resolved-model','permissionMode':permission})
emit({'type':'future.event','raw':{'unknown':'preserved'}})
emit({'type':'stream_event','event':{'delta':{'text':'日本語'}},'session_id':session,'parent_tool_use_id':None})
emit({'type':'assistant','message':{'content':[{'type':'text','text':json.dumps(result)}]},'parent_tool_use_id':None})
emit({'type':'result','subtype':'success','is_error':False,'session_id':'helper-session','parent_tool_use_id':'tool-123','structured_output':result})
if mode in ('same','conflict','client','client-prose'):
    summary='conflicting client' if mode=='conflict' else result['summary']
    subprocess.run([os.environ['JTASKS_BIN'],'result','--receipt','scoped-receipt','--outcome','completed','--summary',summary],check=True,stdout=subprocess.DEVNULL)
native={'type':'result','subtype':'success','session_id':session,'is_error':False,'result':json.dumps(result)}
if mode not in ('helper','missing','prose','client','client-prose','noterminal'): native['structured_output']=result
if mode=='invalid': native['structured_output']='ordinary prose'
if mode=='identity': result['task_id']='OTHER'
if mode=='empty': result['summary']=' '
if mode=='error': native.update(subtype='error_during_execution',is_error=True,errors=['authentication rejected'])
if mode=='mismatch': native['session_id']='88888888-8888-4888-8888-888888888888'
if mode not in ('helper','missing','noterminal'): emit(native)
if mode=='double': emit(native)
if mode=='partial': os.write(1,b'{"unfinished":')
os.write(2,b'available diagnostic bytes\n')
"#;

fn setup(mode: &str) -> Fixture {
    let f = Fixture::new();
    let script = f.root.join("substitute.py");
    fs::write(&script, SUBSTITUTE).unwrap();
    f.import_native_substitute(json!([{"id":"T01","title":"Direct Claude", "executor":{
        "kind":"claude","executable":"python","launcher_args":[script,mode],
        "prompt":"literal ; $(Write-Output INJECTED) `echo wrong` \"quotes\"\nUnicode: 日本語",
        "model":"selected-model","provider":"anthropic","effort":"high","native_agent":"native-reviewer"
    }}]));
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
fn dir(f: &Fixture, status: &Value) -> std::path::PathBuf {
    f.data
        .join("attempts")
        .join(status["tasks"][0]["attempts"][0]["id"].as_str().unwrap())
}
fn read(path: impl AsRef<std::path::Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn public_cli_launches_native_flags_with_literal_stdin_and_preserves_raw_chunks() {
    let f = setup("valid");
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let dir = dir(&f, &status);
    let observation = read(dir.join("claude-observation.json"));
    assert_eq!(observation["unknown_records"], 1);
    assert_eq!(observation["invalid_records"], 0);
    assert_eq!(observation["activity_records"], 3);
    assert_eq!(observation["native_model"], "native-resolved-model");
    assert_eq!(observation["result_source"], "native_structured_output");
    assert!(
        fs::read_to_string(dir.join("stdout.log"))
            .unwrap()
            .contains("future.event")
    );
    assert_eq!(
        fs::read_to_string(dir.join("stderr.log")).unwrap(),
        "available diagnostic bytes\n"
    );
    let launch = read(dir.join("claude-launch.json"));
    assert_eq!(observation["session_id"], launch["session_id"]);
    let seen = read(f.root.join("seen.json"));
    assert!(
        seen["prompt"]
            .as_str()
            .unwrap()
            .contains("$(Write-Output INJECTED)")
    );
    assert!(seen["prompt"].as_str().unwrap().contains("日本語"));
    let args = seen["args"].as_array().unwrap();
    for option in [
        "-p",
        "--verbose",
        "--include-partial-messages",
        "--json-schema",
        "stream-json",
        "--model",
        "selected-model",
        "--effort",
        "high",
        "--agent",
        "native-reviewer",
        "--permission-prompts",
        "none",
        "--dangerously-skip-permissions",
        "bypassPermissions",
    ] {
        assert!(args.contains(&json!(option)), "missing {option}: {args:?}");
    }
    assert_eq!(seen["providers"]["CLAUDE_CODE_USE_BEDROCK"], "0");
    assert_eq!(seen["providers"]["CLAUDE_CODE_USE_VERTEX"], "0");
    assert_eq!(seen["providers"]["CLAUDE_CODE_USE_FOUNDRY"], "0");
    let custody = read(dir.join("custody.json"));
    assert_eq!(custody["bound"], true);
    assert_eq!(custody["empty"], true);
}

#[test]
fn incomplete_tail_retains_raw_bytes_and_rejects_native_acceptance() {
    let f = setup("partial");
    let status = run(&f);
    assert_eq!(status["complete"], false);
    assert_eq!(status["tasks"][0]["state"], "retry_exhausted");
    let path = dir(&f, &status);
    let observation: Value =
        serde_json::from_slice(&fs::read(path.join("claude-observation.json")).unwrap()).unwrap();
    assert_eq!(observation["invalid_records"], 1);
    assert!(observation["protocol_error"].is_string());
    assert!(
        fs::read_to_string(path.join("stdout.log"))
            .unwrap()
            .ends_with("{\"unfinished\":")
    );
}

#[test]
fn only_root_structured_result_or_scoped_client_can_complete() {
    for (mode, completed, state) in [
        ("helper", false, "retry_exhausted"),
        ("missing", false, "retry_exhausted"),
        ("prose", false, "retry_exhausted"),
        ("noterminal", false, "retry_exhausted"),
        ("client", true, "completed"),
        ("client-prose", true, "completed"),
        ("same", true, "completed"),
        ("conflict", false, "retry_exhausted"),
        ("invalid", false, "retry_exhausted"),
        ("identity", false, "retry_exhausted"),
        ("empty", false, "retry_exhausted"),
        ("error", false, "needs_intervention"),
        ("session", false, "retry_exhausted"),
        ("mismatch", false, "retry_exhausted"),
        ("double", false, "retry_exhausted"),
    ] {
        let f = setup(mode);
        let status = run(&f);
        assert_eq!(status["complete"], completed, "{mode}: {status}");
        assert_eq!(status["tasks"][0]["state"], state, "{mode}: {status}");
        if mode == "same" {
            assert_eq!(
                read(dir(&f, &status).join("claude-observation.json"))["result_source"],
                "deduplicated_native_and_scoped_client"
            );
            let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
            assert_eq!(
                conn.query_row("SELECT count(*) FROM receipts", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
}

#[test]
fn resume_requires_exact_retired_same_task_session_and_restricted_mode() {
    let f = Fixture::new();
    let script = f.root.join("substitute.py");
    fs::write(&script, SUBSTITUTE).unwrap();
    f.import_native_substitute(json!([{"id":"T01","title":"Resume Claude","executor":{"kind":"claude","executable":"python","launcher_args":[script,"valid"],"resume_attempt":"prior","permissions":"restricted"}}]));
    let prior = f.data.join("attempts/prior");
    fs::create_dir_all(&prior).unwrap();
    let session = "12345678-1234-4234-8234-123456789abc";
    fs::write(prior.join("claude-observation.json"),serde_json::to_vec(&json!({"task_id":"T01","attempt_id":"prior","session_id":session,"completed_turn":false,"protocol_error":null,"unknown_records":0,"invalid_records":0,"activity_records":0,"raw_stdout":"stdout.log","raw_stderr":"stderr.log","failure_class":"transient_service_or_connection","native_model":null,"native_permissions":null,"result_source":null})).unwrap()).unwrap();
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    conn.execute(
        "INSERT INTO attempts(id,run_id,task_id,state) VALUES('prior','demo','T01','retired')",
        [],
    )
    .unwrap();
    drop(conn);
    f.prepare_native_resume();
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let args = read(f.root.join("seen.json"))["args"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        args.windows(2)
            .any(|w| w == [json!("--resume"), json!(session)])
    );
    assert!(!args.contains(&json!("--continue")) && !args.contains(&json!("--fork-session")));
    assert!(
        args.contains(&json!("dontAsk"))
            && !args.contains(&json!("--dangerously-skip-permissions"))
    );
}

#[test]
fn foreign_resume_is_rejected_before_spawning_and_unknown_effort_never_falls_back() {
    for (field, value) in [
        ("resume_attempt", json!("foreign")),
        ("effort", json!("invented")),
        ("provider", json!("invented")),
    ] {
        let f = Fixture::new();
        let script = f.root.join("substitute.py");
        fs::write(&script, SUBSTITUTE).unwrap();
        let mut executor =
            json!({"kind":"claude","executable":"python","launcher_args":[script,"valid"]});
        executor[field] = value;
        f.import_native_substitute(
            json!([{"id":"T01","title":"Invalid native config","executor":executor}]),
        );
        let output = f.call(&["run", "--run", "demo"]);
        assert!(!output.status.success(), "{field}");
        assert!(!f.root.join("seen.json").exists(), "{field} spawned");
    }
}

#[test]
fn jtasks_alias_resolves_claude_native_agent_and_immutable_configuration() {
    let f = Fixture::new();
    let script = f.root.join("substitute.py");
    fs::write(&script, SUBSTITUTE).unwrap();
    fs::create_dir_all(f.data.join("config")).unwrap();
    fs::write(f.data.join("config/profiles.json"),serde_json::to_vec(&json!({"schema_version":1,"profiles":{
        "review":{"executor":"claude","provider":"bedrock","model":"profile-model","effort":"medium","native_agent":"native-reviewer","autonomy":"restricted","executable":"python","launcher_args":[script,"valid"]}
    }})).unwrap()).unwrap();
    f.import_native_substitute(json!([{"id":"T01","title":"Alias Claude","executor":{"kind":"profile","alias":"review","overrides":{"model":"task-model","effort":"high"}}}]));
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let seen = read(f.root.join("seen.json"));
    let args = seen["args"].as_array().unwrap();
    assert!(
        args.contains(&json!("task-model"))
            && args.contains(&json!("native-reviewer"))
            && args.contains(&json!("high"))
    );
    assert_eq!(seen["providers"]["CLAUDE_CODE_USE_BEDROCK"], "1");
    assert!(args.contains(&json!("dontAsk")));
    let config = read(dir(&f, &status).join("claude-launch.json"));
    assert_eq!(config["configuration"]["provider"], "bedrock");
    let snapshot = &status["tasks"][0]["attempts"][0]["effective_profile"];
    assert_eq!(snapshot["values"]["native_agent"], "native-reviewer");
    assert_eq!(snapshot["origins"]["model"], "task");
    let conn = rusqlite::Connection::open(f.data.join("state.sqlite3")).unwrap();
    let persisted: String = conn
        .query_row(
            "SELECT effective_profile FROM attempts WHERE task_id='T01'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        &serde_json::from_str::<Value>(&persisted).unwrap(),
        snapshot
    );
}

#[test]
#[ignore = "installed Claude --version only; no account turn"]
fn installed_claude_launches_through_common_custody_without_a_model_turn() {
    let f = Fixture::new();
    f.import_native_substitute(json!([{"id":"T01","title":"Native version probe","executor":{"kind":"claude","launcher_args":["--version"]}}]));
    let status = run(&f);
    assert_eq!(status["complete"], false);
    let dir = dir(&f, &status);
    assert!(
        fs::read_to_string(dir.join("stdout.log"))
            .unwrap()
            .contains("Claude Code")
    );
    assert_eq!(read(dir.join("custody.json"))["empty"], true);
}
