mod support;
use jtasks::{
    read::projections::{self, Request, View},
    storage::Workspace,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use support::{Fixture, task};

fn read(w: &Workspace, view: View) -> Value {
    projections::read(w, &Request::new(view, Some("demo"))).unwrap()
}

/// The answer without volatile fields, to compare before and after history grows.
fn stable(mut value: Value) -> Value {
    for key in ["elapsed", "as_of", "event_cursor", "next_automatic_action"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value
}

#[test]
fn status_and_operations_do_not_grow_with_past_attempts_and_events() {
    let f = Fixture::new();
    f.import(json!([task("A", &[], "silent", "")]));
    let w = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let operations_before = stable(read(&w, View::Operations));
    let status_before = stable(read(&w, View::Status));

    let conn = Connection::open(w.database()).unwrap();
    conn.execute_batch(
        "BEGIN;
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<10000)
         INSERT INTO attempts(id,run_id,task_id,state,outcome,summary,effective_profile)
           SELECT 'old-'||i,'demo','A','retired','completed',printf('%0200d',i),'null' FROM n;
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<10000)
         INSERT INTO journal(run_id,task_id,attempt_id,event,detail)
           SELECT 'demo','A','old-'||i,'activity','{\"severity\":\"info\",\"content\":\"x\"}' FROM n;
         COMMIT;",
    )
    .unwrap();
    drop(conn);

    let started = Instant::now();
    let operations = stable(read(&w, View::Operations));
    let status = stable(read(&w, View::Status));
    let elapsed = started.elapsed();

    // The read answers the same as before the history existed.
    assert_eq!(operations, operations_before);
    for key in ["complete", "development_complete", "verification_complete"] {
        assert_eq!(status[key], status_before[key], "{key}");
    }
    assert_eq!(status["tasks"][0]["id"], "A");
    assert_eq!(status["task_counts"], status_before["task_counts"]);

    // Past attempts are summarized by the latest few plus a total.
    let attempts = status["tasks"][0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 20);
    assert_eq!(attempts[19]["id"], "old-10000");
    assert_eq!(status["tasks"][0]["attempts_total"], 10000);

    // Fixed bound: the answer's size and the read's time do not follow history.
    let size = serde_json::to_vec(&status).unwrap().len();
    assert!(size < 64 * 1024, "status answer is {size} bytes");
    assert!(
        elapsed < Duration::from_secs(5),
        "reads took {elapsed:?} with 10000 past attempts and events"
    );
}
