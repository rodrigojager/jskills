use super::support::Fixture;
use serde_json::{Value, json};
use std::fs;

// An independent executable emits the documented root CLI stream. No account,
// global configuration, hooks or native sessions are touched by these tests.
const SUBSTITUTE: &str = r#"
import json, os, subprocess, sys
from pathlib import Path
mode=sys.argv[1]
args=sys.argv[2:]
prompt=sys.stdin.read()
Path('seen.json').write_text(json.dumps({'args':args,'prompt':prompt,'cwd':os.getcwd(),'ticket':os.environ.get('JTASKS_TICKET_PATH')}))
schema=json.loads(Path(args[args.index('--output-schema')+1]).read_text())
final=Path(args[args.index('--output-last-message')+1])
result={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
result.update(outcome='completed',summary='substitute completed')
def emit(v):
    data=(json.dumps(v,ensure_ascii=False)+'\n').encode()
    os.write(1,data[:7]); os.write(1,data[7:])
emit({'type':'thread.started','thread_id':'exact-thread-123'})
emit({'type':'turn.started'})
emit({'type':'future.event','raw':{'unknown':'preserved'}})
emit({'type':'item.completed','item':{'type':'command_execution','aggregated_output':json.dumps(result)}})
if mode in ('same','conflict','client-prose'):
    summary=result['summary'] if mode=='same' else 'conflicting client result'
    subprocess.run([os.environ['JTASKS_BIN'],'result','--receipt','scoped-receipt','--outcome','completed','--summary',summary],check=True,stdout=subprocess.DEVNULL)
if mode=='client':
    subprocess.run([os.environ['JTASKS_BIN'],'result','--receipt','scoped-receipt','--outcome','completed','--summary','client result'],check=True,stdout=subprocess.DEVNULL)
elif mode!='helper':
    text='ordinary prose' if mode in ('prose','client-prose') else json.dumps(result)
    emit({'type':'item.completed','item':{'type':'agent_message','text':text}})
    final.write_text(text)
if mode!='noterminal': emit({'type':'turn.completed','usage':{}})
if mode=='partial': os.write(1,b'{"unfinished":')
os.write(2,b'available diagnostic bytes\n')
"#;

fn setup(mode: &str) -> Fixture {
    let f = Fixture::new();
    let script = f.root.join("substitute.py");
    fs::write(&script, SUBSTITUTE).unwrap();
    f.import_native_substitute(json!([{"id":"T01","title":"Direct Codex", "executor":{
        "kind":"codex","executable":"python","launcher_args":[script,mode],
        "prompt":"literal ; $(Write-Output INJECTED) `echo wrong` \"quotes\"\nUnicode: 日本語",
        "model":"selected-model","provider":"selected-provider","reasoning_effort":"high",
        "native_profile":"native-config-profile"
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
fn dir(f: &Fixture, snapshot: &Value) -> std::path::PathBuf {
    f.data
        .join("attempts")
        .join(snapshot["tasks"][0]["attempts"][0]["id"].as_str().unwrap())
}
#[test]
fn public_cli_launches_codex_with_safe_prompt_exact_identity_and_raw_capture() {
    let f = setup("valid");
    let status = run(&f);
    assert_eq!(status["complete"], true);
    let dir = dir(&f, &status);
    let observation: Value =
        serde_json::from_slice(&fs::read(dir.join("codex-observation.json")).unwrap()).unwrap();
    assert_eq!(observation["thread_id"], "exact-thread-123");
    assert_eq!(observation["unknown_records"], 1);
    assert_eq!(observation["invalid_records"], 0);
    let stdout = fs::read_to_string(dir.join("stdout.log")).unwrap();
    assert!(stdout.contains("future.event"));
    assert_eq!(
        fs::read_to_string(dir.join("stderr.log")).unwrap(),
        "available diagnostic bytes\n"
    );
    let seen: Value = serde_json::from_slice(&fs::read(f.root.join("seen.json")).unwrap()).unwrap();
    assert!(
        seen["prompt"]
            .as_str()
            .unwrap()
            .contains("$(Write-Output INJECTED)")
    );
    assert!(seen["prompt"].as_str().unwrap().contains("日本語"));
    let args = seen["args"].as_array().unwrap();
    for option in [
        "--json",
        "--model",
        "selected-model",
        "--profile",
        "native-config-profile",
        "model_provider=\"selected-provider\"",
        "model_reasoning_effort=\"high\"",
        "--dangerously-bypass-approvals-and-sandbox",
    ] {
        assert!(args.contains(&json!(option)), "missing {option}: {args:?}");
    }
    assert_eq!(args.last().unwrap(), "-");
    let launch: Value =
        serde_json::from_slice(&fs::read(dir.join("codex-launch.json")).unwrap()).unwrap();
    assert_eq!(launch["capabilities"]["direct_execution"], true);
    assert!(
        launch["capabilities"]["effective_configuration"]
            .as_str()
            .unwrap()
            .contains("unknown")
    );
}
#[test]
fn incomplete_tail_retains_raw_bytes_and_rejects_native_acceptance() {
    let f = setup("partial");
    let status = run(&f);
    assert_eq!(status["complete"], false);
    assert_eq!(status["tasks"][0]["state"], "retry_exhausted");
    let path = dir(&f, &status);
    let observation: Value =
        serde_json::from_slice(&fs::read(path.join("codex-observation.json")).unwrap()).unwrap();
    assert_eq!(observation["invalid_records"], 1);
    assert!(observation["protocol_error"].is_string());
    assert!(
        fs::read_to_string(path.join("stdout.log"))
            .unwrap()
            .ends_with("{\"unfinished\":")
    );
}

#[test]
fn only_designated_native_or_scoped_payloads_complete_and_conflicts_fail() {
    for (mode, completed, state) in [
        ("helper", false, "retry_exhausted"),
        ("prose", false, "retry_exhausted"),
        ("client", true, "completed"),
        ("client-prose", true, "completed"),
        ("same", true, "completed"),
        ("conflict", false, "retry_exhausted"),
        ("noterminal", false, "retry_exhausted"),
    ] {
        let f = setup(mode);
        let status = run(&f);
        assert_eq!(status["complete"], completed, "{mode}: {status}");
        assert_eq!(status["tasks"][0]["state"], state, "{mode}: {status}");
        if mode == "conflict" {
            let observation: Value = serde_json::from_slice(
                &fs::read(dir(&f, &status).join("codex-observation.json")).unwrap(),
            )
            .unwrap();
            assert!(
                observation["protocol_error"]
                    .as_str()
                    .unwrap()
                    .contains("conflicting")
            );
        }
    }
}

#[test]
#[ignore = "requires installed Codex CLI; invokes --version only, without an account turn"]
fn installed_codex_launches_through_public_cli_without_a_model_turn() {
    let f = Fixture::new();
    f.import_native_substitute(json!([{"id":"T01","title":"Native version probe","executor":{"kind":"codex","launcher_args":["--version"]}}]));
    let status = run(&f);
    assert_eq!(status["complete"], false);
    let stdout = fs::read_to_string(dir(&f, &status).join("stdout.log")).unwrap();
    assert!(stdout.starts_with("codex-cli "), "{stdout}");
}
#[test]
fn exact_same_task_resume_and_restricted_flags_are_explicit() {
    let f = Fixture::new();
    let script = f.root.join("substitute.py");
    fs::write(&script, SUBSTITUTE).unwrap();
    let previous = f.data.join("attempts/prior");
    fs::create_dir_all(&previous).unwrap();
    let observation = json!({"task_id":"T01","attempt_id":"prior","thread_id":"exact-thread-123","completed_turn":false,"protocol_error":null,"unknown_records":0,"invalid_records":0,"raw_stdout":"stdout.log","raw_stderr":"stderr.log","failure_class":"transient_service_or_connection"});
    fs::write(
        previous.join("codex-observation.json"),
        serde_json::to_vec(&observation).unwrap(),
    )
    .unwrap();
    f.import_native_substitute(json!([{"id":"T01","title":"Resume exact task","executor":{"kind":"codex","executable":"python","launcher_args":[script,"valid"],"resume_attempt":"prior","permissions":"restricted"}}]));
    rusqlite::Connection::open(f.data.join("state.sqlite3"))
        .unwrap()
        .execute(
            "INSERT INTO attempts(id,run_id,task_id,state) VALUES('prior','demo','T01','retired')",
            [],
        )
        .unwrap();
    f.prepare_native_resume();
    assert_eq!(run(&f)["complete"], true);
    let seen: Value = serde_json::from_slice(&fs::read(f.root.join("seen.json")).unwrap()).unwrap();
    let args = seen["args"].as_array().unwrap();
    assert!(
        args.windows(2)
            .any(|w| w == [json!("resume"), json!("exact-thread-123")])
    );
    assert!(!args.contains(&json!("--last")));
    assert!(args.contains(&json!("approval_policy=\"never\"")));
    assert!(args.contains(&json!("workspace-write")));
    assert!(!args.contains(&json!("--dangerously-bypass-approvals-and-sandbox")));
}
