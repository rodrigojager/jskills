pub const VERSION: i64 = 1;
pub const SQL: &str = r#"
BEGIN IMMEDIATE;
CREATE TABLE runs(id TEXT PRIMARY KEY, checkout TEXT NOT NULL, plan TEXT NOT NULL, state TEXT NOT NULL);
CREATE TABLE tasks(run_id TEXT NOT NULL REFERENCES runs(id), id TEXT NOT NULL, ordinal INTEGER NOT NULL, spec TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(run_id,id));
CREATE TABLE attempts(id TEXT PRIMARY KEY, run_id TEXT NOT NULL, task_id TEXT NOT NULL, state TEXT NOT NULL, pid INTEGER, outcome TEXT, summary TEXT, exit_code INTEGER, FOREIGN KEY(run_id,task_id) REFERENCES tasks(run_id,id));
CREATE UNIQUE INDEX one_live_attempt ON attempts(run_id,task_id) WHERE state IN ('launch_intent','running','uncertain');
CREATE TRIGGER immutable_attempt BEFORE UPDATE OF id,run_id,task_id ON attempts BEGIN SELECT RAISE(ABORT,'attempt identity is immutable'); END;
CREATE TABLE receipts(id TEXT PRIMARY KEY, attempt_id TEXT NOT NULL UNIQUE REFERENCES attempts(id), payload TEXT NOT NULL);
CREATE TABLE journal(sequence INTEGER PRIMARY KEY, run_id TEXT NOT NULL, task_id TEXT, attempt_id TEXT, event TEXT NOT NULL, detail TEXT NOT NULL, recorded_at TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now')));
PRAGMA user_version=1;
COMMIT;
"#;
