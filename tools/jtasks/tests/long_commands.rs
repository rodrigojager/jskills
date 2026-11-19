//! Backlog B-13: a worker whose command outlives its tool's wait reports the
//! command's real result, and a later attempt waits for the same command
//! instead of starting it again.
mod support;
use serde_json::{Value, json};
use std::fs;
use support::Fixture;

// Claude stream-json substitute (no account turn) that runs its build through
// `jtasks exec run` with a 1 s wait while the build takes 3 s.
// - wait: repeats bounded `jtasks exec wait` calls and reports the real result.
// - give-up: the first attempt reports failure while the build still runs
//   (the JGates B-13 case); the controller decides the retry once the build
//   ends, and the retried attempt asks for the same request ID, gets the same
//   command and reports its real result.
const WORKER: &str = r#"
import json, os, subprocess, sys
from pathlib import Path
mode=sys.argv[1]
args=sys.argv[2:]
sys.stdin.read()
schema=json.loads(args[args.index('--json-schema')+1])
session=args[args.index('--resume' if '--resume' in args else '--session-id')+1]
result={k:v['const'] for k,v in schema['properties'].items() if 'const' in v}
jtasks=os.environ['JTASKS_BIN']
marker=Path('attempts.txt')
n=int(marker.read_text()) if marker.exists() else 0
marker.write_text(str(n+1))
def call(*a):
    out=subprocess.run([jtasks,*a],check=True,capture_output=True)
    return json.loads(out.stdout)
receipt=call('exec','run','--request-id','long-build','--wait-seconds','1','--',jtasks,'fixture','--outcome','prose','--summary','built','--delay-ms','3000')
Path(f'receipt-{n}.json').write_text(json.dumps(receipt))
def emit(v):
    sys.stdout.write(json.dumps(v)+'\n'); sys.stdout.flush()
emit({'type':'system','subtype':'init','session_id':session,'model':'substitute','permissionMode':'bypassPermissions'})
if mode=='give-up' and n==0:
    result.update(outcome='failed',summary='gave up: the build is still running')
else:
    waits=0
    while receipt['wait_expired']:
        receipt=call('exec','wait',receipt['command_id'],'--wait-seconds','1')
        waits+=1
    Path('final-receipt.json').write_text(json.dumps(dict(receipt,waits=waits)))
    result.update(outcome='completed',summary='exit %s: %s' % (receipt['exit_code'],receipt['stdout_tail'].strip()))
emit({'type':'result','subtype':'success','is_error':False,'session_id':session,'structured_output':result})
"#;

fn read(path: impl AsRef<std::path::Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn setup(mode: &str) -> Fixture {
    let f = Fixture::new();
    let script = f.root.join("worker.py");
    fs::write(&script, WORKER).unwrap();
    f.import_native_substitute(json!([{"id":"T01","title":"Long build","executor":{
        "kind":"claude","executable":"python","launcher_args":[script,mode]
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
fn commands(f: &Fixture) -> i64 {
    rusqlite::Connection::open(f.data.join("state.sqlite3"))
        .unwrap()
        .query_row("SELECT count(*) FROM managed_commands", [], |r| r.get(0))
        .unwrap()
}
fn real_result(f: &Fixture, status: &Value) -> Value {
    let last = read(f.root.join("final-receipt.json"));
    assert_eq!(last["state"], "exited");
    assert_eq!(last["exit_code"], 0);
    assert_eq!(last["wait_expired"], false);
    assert!(last["next"].as_str().unwrap().contains("exec output"));
    let attempts = status["tasks"][0]["attempts"].as_array().unwrap();
    let latest = attempts
        .iter()
        .find(|a| a["outcome"] == "completed")
        .unwrap();
    assert_eq!(latest["summary"], "exit 0: Completed: built", "{status}");
    last
}

#[test]
fn a_command_longer_than_the_tool_wait_reports_its_real_result() {
    let f = setup("wait");
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    // The first bounded wait expired while the build ran on...
    let first = read(f.root.join("receipt-0.json"));
    assert_eq!(first["wait_expired"], true);
    assert_eq!(first["duplicate"], false);
    assert!(
        first["next"]
            .as_str()
            .unwrap()
            .starts_with("jtasks exec wait ")
    );
    // ...and repeated waits in the same turn reached the build's real end.
    let last = real_result(&f, &status);
    assert!(last["waits"].as_u64().unwrap() >= 1);
    assert_eq!(commands(&f), 1);
}

#[test]
fn a_retried_attempt_waits_for_the_same_command_instead_of_starting_it_again() {
    let f = setup("give-up");
    // One controller: no manual retry, no second build.
    let status = run(&f);
    assert_eq!(status["complete"], true, "{status}");
    let attempts = status["tasks"][0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{status}");
    assert!(attempts.iter().any(|a| a["outcome"] == "failed"));
    let first = read(f.root.join("receipt-0.json"));
    assert_eq!(first["wait_expired"], true);
    let second = read(f.root.join("receipt-1.json"));
    assert_eq!(second["command_id"], first["command_id"]);
    assert_eq!(second["duplicate"], true);
    assert_eq!(second["started_by_attempt"], first["attempt"]);
    real_result(&f, &status);
    assert_eq!(commands(&f), 1);
}

#[test]
fn a_conflicting_request_is_refused_and_output_pages_as_text() {
    let f = Fixture::new();
    f.import(json!([support::task("A", &[], "silent", "")]));
    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            if self.0.try_wait().unwrap().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let mut server = Server(
        f.command()
            .args(["exec", "serve"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = std::time::Instant::now();
    while !f.data.join("commands-control.json").exists() {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let run = |program: &str| {
        f.call(&[
            "exec",
            "run",
            "--run",
            "demo",
            "--operation",
            "check",
            "--request-id",
            "same-id",
            "--wait-seconds",
            "5",
            "--",
            program,
            "fixture",
            "--outcome",
            "prose",
            "--summary",
            "paged",
        ])
    };
    let output = run(env!("CARGO_BIN_EXE_jtasks"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["state"], "exited");
    assert_eq!(
        receipt["stdout_tail"].as_str().unwrap().trim(),
        "Completed: paged"
    );
    let page = f.call(&[
        "exec",
        "output",
        receipt["command_id"].as_str().unwrap(),
        "--limit",
        "9",
        "--text",
    ]);
    let page: Value = serde_json::from_slice(&page.stdout).unwrap();
    assert_eq!(page["text"], "Completed");
    assert_eq!(page["next_offset"], 9);
    assert!(page.get("bytes").is_none());
    // Same request ID, different command: refused, nothing started.
    let refused = run(&f.root.join("other-program").to_string_lossy());
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("conflicting command request id"));
    assert!(f.call(&["exec", "shutdown"]).status.success());
    assert!(server.0.wait().unwrap().success());
}
