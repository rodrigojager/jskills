mod support;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::{Fixture, task};

// A small Codex protocol substitute. It never calls an account. The first call
// waits for a release file so the test can edit the alias between two reviews.
const OBSERVER: &str = r#"import sys,json,time,pathlib
args=sys.argv[1:]
prompt=sys.stdin.read()
schema=json.loads(prompt.split('RESPONSE SCHEMA:\n',1)[1])
p=schema['properties']
result={k:p[k]['const'] for k in ['run','task','attempt','review','generation']}
root=pathlib.Path(__file__).parent
with open(root/'observer-calls.jsonl','a') as calls:calls.write(json.dumps(args)+'\n')
count=len((root/'observer-calls.jsonl').read_text().splitlines())
if count==1:
 (root/'first-review-started').write_text('')
 deadline=time.time()+20
 while not (root/'release-first-review').exists() and time.time()<deadline:time.sleep(0.02)
print(json.dumps({'type':'thread.started','thread_id':'review-'+result['review']}),flush=True)
print(json.dumps({'type':'turn.started'}),flush=True)
result.update(decision='continue',rationale='Current command evidence was compared with the prior review',evidence=['commands','diff'],delay_cause='The declared operation needs a finite additional interval',next_review_after_seconds=1)
text=json.dumps(result)
pathlib.Path(args[args.index('--output-last-message')+1]).write_text(text)
print(json.dumps({'type':'item.completed','item':{'id':'final','type':'agent_message','text':text}}),flush=True)
print(json.dumps({'type':'turn.completed','usage':{}}),flush=True)
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
fn script(f: &Fixture) -> PathBuf {
    let path = f.root.join("observer.py");
    fs::write(&path, OBSERVER).unwrap();
    path
}
fn codex_alias(script: &PathBuf, model: &str) -> Value {
    json!({"executor":"codex","provider":"synthetic-provider","model":model,"executable":"python","launcher_args":[script]})
}
fn plan(f: &Fixture, alias: &str, max_calls: u32) -> PathBuf {
    let mut a = task("A", &[], "silent", "");
    a["executor"]["delay_ms"] = json!(30000);
    a["estimate_seconds"] = json!(1);
    a["reviews"] = json!({"max_calls":max_calls,"deadline_seconds":30,"observer_unknown_capabilities":{"actor":"test","reason":"synthetic native protocol substitute, no account"},"observer":{"kind":"profile","alias":alias}});
    f.plan(json!([a]))
}

struct Foreground(Child);
impl Drop for Foreground {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn start(f: &Fixture) -> Foreground {
    Foreground(
        command(f)
            .args(["run", "--run", "demo"])
            .env("JTASKS_SHUTDOWN_GRACE_MS", "0")
            .env("JTASKS_SHUTDOWN_FORCE_MS", "2000")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}
fn finish(child: &mut Foreground) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(exit) = child.0.try_wait().unwrap() {
            return exit;
        }
        assert!(Instant::now() < deadline, "foreground deadline");
        std::thread::sleep(Duration::from_millis(20));
    }
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

#[test]
fn observer_alias_resolves_when_each_review_is_due_and_is_journaled() {
    let f = Fixture::new();
    let script = script(&f);
    set_alias(&f, "observer", codex_alias(&script, "observer-before"));
    let plan = plan(&f, "observer", 2);
    success(&f, &["import", plan.to_str().unwrap(), "--run", "demo"]);
    let mut child = start(&f);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f.root.join("first-review-started").exists() {
        assert!(Instant::now() < deadline, "first review never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The first review already resolved its alias; the next one must see the edit.
    set_alias(&f, "observer", codex_alias(&script, "observer-after"));
    fs::write(f.root.join("release-first-review"), "").unwrap();
    // The second (last allowed) review asks for more time, which pauses the task.
    assert_eq!(finish(&mut child).code(), Some(3));
    let calls: Vec<Vec<String>> = fs::read_to_string(f.root.join("observer-calls.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(calls.len(), 2);
    for (args, model) in calls.iter().zip(["observer-before", "observer-after"]) {
        assert!(
            args.windows(2).any(|w| w == ["--model", model]),
            "{model}: {args:?}"
        );
        assert!(
            args.iter()
                .any(|a| a == "model_provider=\"synthetic-provider\"")
        );
        assert!(args.windows(2).any(|w| w == ["--sandbox", "read-only"]));
    }
    let reviews = journal(&f, "execution_review");
    assert_eq!(reviews.len(), 2);
    for (review, model) in reviews.iter().zip(["observer-before", "observer-after"]) {
        let observer = &review["observer"];
        assert_eq!(observer["form"], "alias");
        assert_eq!(observer["alias"], "observer");
        assert_eq!(observer["observer"]["kind"], "codex");
        assert_eq!(observer["observer"]["model"], model);
        assert_eq!(observer["values"]["model"], model);
        assert!(
            observer["origins"]["model"]
                .as_str()
                .unwrap()
                .starts_with("project:")
        );
    }
    let status = f.snapshot();
    assert_eq!(status["tasks"][0]["state"], "needs_intervention");
    assert_eq!(
        status["tasks"][0]["attempts"][0]["review"]["actual_calls"],
        2
    );
}

#[test]
fn opencode_observer_honors_provider_model_and_variant_and_assesses_root_response() {
    let f = Fixture::new();
    let launcher = f.root.join("opencode.py");
    fs::write(&launcher, r#"import json, os, sys
args = sys.argv[1:]
assert args[args.index('--model') + 1] == 'synthetic-provider/vendor/synthetic-model#priority'
assert '--standalone' in args and '--session' not in args and '--auto' not in args
settings = json.loads(os.environ['OPENCODE_CONFIG_CONTENT'])
assert {'action':'tool.use','resource':'*','effect':'deny'} in settings['policies']
assert not any(k.startswith('JTASKS_') for k in os.environ)
schema = json.loads(sys.stdin.read().split('RESPONSE SCHEMA:\n', 1)[1])
p = schema['properties']
result = {k:p[k]['const'] for k in ['run','task','attempt','review','generation']}
result.update(decision='retry', rationale='No execution progress', evidence=['commands'], delay_cause='Stalled command')
print(json.dumps({'type':'text','sessionID':'ses_synthetic','part':{'sessionID':'ses_synthetic','text':json.dumps(result)}}))
"#).unwrap();
    let mut a = task("A", &[], "silent", "");
    a["executor"]["delay_ms"] = json!(30000);
    a["estimate_seconds"] = json!(1);
    a["reviews"] = json!({
        "max_calls":1,"deadline_seconds":30,
        "observer_unknown_capabilities":{"actor":"test","reason":"synthetic provider"},
        "observer":{"agent":"opencode","provider":"synthetic-provider","model":"vendor/synthetic-model",
            "variant":"priority","executable":"python","launcher_args":[launcher]}
    });
    let path = f.plan(json!([a]));
    success(&f, &["import", path.to_str().unwrap(), "--run", "demo"]);
    let mut child = start(&f);
    assert_eq!(finish(&mut child).code(), Some(3));
    let status = f.snapshot();
    let attempt = &status["tasks"][0]["attempts"][0];
    assert_eq!(attempt["review"]["actual_calls"], 1);
    let conn = Connection::open(f.data.join("state.sqlite3")).unwrap();
    let response: String = conn
        .query_row(
            "SELECT response FROM reviewer_calls WHERE state='finished' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap()["rationale"],
        "No execution progress"
    );
    let recorded: String = conn
        .query_row(
            "SELECT selection FROM reviewer_selections LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let recorded: Value = serde_json::from_str(&recorded).unwrap();
    assert_eq!(recorded["values"]["agent"], "opencode");
    assert_eq!(recorded["values"]["provider"], "synthetic-provider");
    assert_eq!(recorded["values"]["model"], "vendor/synthetic-model");
    assert_eq!(recorded["values"]["effort"], "priority");
    assert_eq!(recorded["observer"]["variant"], "priority");
}
