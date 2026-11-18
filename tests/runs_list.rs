mod support;
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{Fixture, task};

fn call(f: &Fixture, args: &[&str]) -> Value {
    let out = f.call(args);
    assert!(
        matches!(out.status.code(), Some(0 | 3)),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

// AC-3: a run whose checkout was deleted is left out unless --all; the other is current.
#[test]
fn deleted_checkout_is_hidden_unless_all_and_the_existing_run_is_current() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    let db = jtasks::storage::Workspace::resolve(&f.root, Some(&f.data))
        .unwrap()
        .database();
    let conn = Connection::open(db).unwrap();
    let gone = f.root.join("deleted-checkout");
    conn.execute(
        "INSERT INTO runs(id,checkout,plan,state) VALUES('gone',?1,'{}','pending')",
        [gone.to_string_lossy().as_ref()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO journal(run_id,event,detail) VALUES('gone','launch_intent','{}')",
        [],
    )
    .unwrap();
    drop(conn);

    let shown = call(&f, &["read", "runs"]);
    let runs = shown["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1, "{shown}");
    assert_eq!(runs[0]["run"], "demo");
    assert_eq!(runs[0]["current"], true);
    assert_eq!(runs[0]["checkout_exists"], true);
    assert_eq!(runs[0]["controller"], "stopped");
    assert_eq!(runs[0]["counts"]["pending"], 1);
    assert!(runs[0]["last_event_at"].is_string());

    let all = call(&f, &["read", "runs", "--all"]);
    let runs = all["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2, "{all}");
    let gone = runs.iter().find(|r| r["run"] == "gone").unwrap();
    assert_eq!(gone["checkout_exists"], false);
    assert_eq!(gone["current"], false);
}
