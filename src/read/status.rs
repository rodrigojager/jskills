use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

/// Most recent attempts listed per task; the total is reported beside them.
pub const ATTEMPTS_PER_TASK: i64 = 20;

#[derive(Debug, Serialize)]
pub struct Status {
    pub graph: serde_json::Value,
    pub run: String,
    pub state: String,
    pub complete: bool,
    pub development_complete: bool,
    pub verification_complete: bool,
    pub operations: Vec<crate::controller::operations::OperationStatus>,
    pub tasks: Vec<TaskStatus>,
    /// Every task and operation of the run, whatever the page holds.
    #[serde(skip)]
    pub tasks_total: u64,
    #[serde(skip)]
    pub operations_total: u64,
}

#[derive(Debug, Serialize)]
pub struct TaskStatus {
    pub lineage_root: String,
    pub cooldown: serde_json::Value,
    pub id: String,
    pub state: String,
    /// The latest attempts, oldest first, at most `ATTEMPTS_PER_TASK`.
    pub attempts: Vec<AttemptStatus>,
    pub attempts_total: u64,
}

#[derive(Debug, Serialize)]
pub struct AttemptStatus {
    pub review: serde_json::Value,
    pub effective_profile: serde_json::Value,
    pub id: String,
    pub process_state: String,
    pub pid: Option<u32>,
    pub outcome: Option<String>,
    pub summary: Option<String>,
    pub exit_code: Option<i32>,
    pub receipt_id: Option<String>,
}

pub fn read(path: &Path, run: &str) -> Result<Status> {
    ensure!(path.is_file(), "no runtime database; import a plan first");
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(crate::storage::READER_BUSY_TIMEOUT)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        version == crate::storage::schema::VERSION,
        "unsupported runtime schema (read does not migrate)"
    );
    // One coherent read transaction. It performs no reconciliation or writes.
    conn.execute_batch("BEGIN")?;
    from_connection(&conn, run)
}

/// Every task and operation of the run (small runs and completion checks).
pub fn from_connection(conn: &Connection, run: &str) -> Result<Status> {
    page(conn, run, 0, i64::MAX, 0, i64::MAX)
}

pub struct OperationsPage {
    pub operations: Vec<crate::controller::operations::OperationStatus>,
    pub total: u64,
    pub verification_complete: bool,
}

/// One page of operations plus the run-wide verification flag.
pub fn operations_page(
    conn: &Connection,
    run: &str,
    after: i64,
    limit: i64,
) -> Result<OperationsPage> {
    conn.query_row("SELECT 1 FROM runs WHERE id=?1", [run], |r| r.get::<_, i64>(0))?;
    let operations = crate::controller::operations::read_page(conn, run, after, limit)?;
    let (total, unfinished): (i64, i64) = conn.query_row(
        "SELECT count(*),coalesce(sum(required=1 AND state<>'passed'),0) FROM operations WHERE run_id=?1",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(OperationsPage {
        operations,
        total: total as u64,
        verification_complete: unfinished == 0,
    })
}

/// Loads only the requested tasks and operations from storage. Completion
/// flags come from counts over the whole run, never from the loaded page.
pub fn page(
    conn: &Connection,
    run: &str,
    task_after: i64,
    task_limit: i64,
    operation_after: i64,
    operation_limit: i64,
) -> Result<Status> {
    let state = conn.query_row("SELECT state FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    let mut statement = conn.prepare(
        "SELECT id,state FROM tasks WHERE run_id=?1 ORDER BY ordinal LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement
        .query_map(rusqlite::params![run, task_limit, task_after], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut tasks = Vec::new();
    for (id, state) in rows {
        let mut attempts = conn.prepare("SELECT id,state,pid,outcome,summary,exit_code,receipt,effective_profile FROM (SELECT a.rowid AS rid,a.id AS id,a.state AS state,a.pid AS pid,a.outcome AS outcome,a.summary AS summary,a.exit_code AS exit_code,r.id AS receipt,a.effective_profile AS effective_profile FROM attempts a LEFT JOIN receipts r ON r.attempt_id=a.id WHERE a.run_id=?1 AND a.task_id=?2 ORDER BY a.rowid DESC LIMIT ?3) ORDER BY rid")?;
        let attempts = attempts
            .query_map(rusqlite::params![run, &id, ATTEMPTS_PER_TASK], |r| {
                Ok(AttemptStatus {
                    review: crate::controller::reviews::projection(conn, &r.get::<_, String>(0)?)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                    effective_profile: serde_json::from_str(&r.get::<_, String>(7)?).map_err(
                        |error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                7,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        },
                    )?,
                    id: r.get(0)?,
                    process_state: r.get(1)?,
                    pid: r.get(2)?,
                    outcome: r.get(3)?,
                    summary: r.get(4)?,
                    exit_code: r.get(5)?,
                    receipt_id: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let attempts_total: i64 = conn.query_row(
            "SELECT count(*) FROM attempts WHERE run_id=?1 AND task_id=?2",
            [run, &id],
            |r| r.get(0),
        )?;
        tasks.push(TaskStatus {
            lineage_root: crate::controller::graph::root(conn, run, &id)?,
            cooldown: crate::controller::cooldowns::read(
                conn,
                run,
                &id,
                crate::controller::admission::now_ms(),
            )?,
            id,
            state,
            attempts,
            attempts_total: attempts_total.max(0) as u64,
        });
    }
    let operations =
        crate::controller::operations::read_page(conn, run, operation_after, operation_limit)?;
    // (all tasks, development-done tasks, finished tasks) over the whole run.
    let (tasks_total, development, finished): (i64, i64, i64) = conn.query_row(
        "SELECT count(*),
         coalesce(sum(state IN ('completed','verification_pending','superseded')),0),
         coalesce(sum(state IN ('completed','superseded')),0) FROM tasks WHERE run_id=?1",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let (operations_total, unfinished_required): (i64, i64) = conn.query_row(
        "SELECT count(*),coalesce(sum(required=1 AND state<>'passed'),0) FROM operations WHERE run_id=?1",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let development_complete = tasks_total > 0 && development == tasks_total;
    let verification_complete = unfinished_required == 0;
    let graph = crate::controller::graph::projection(conn, run)?;
    let coverage_complete = graph["coverage"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["satisfied"] == true);
    let complete =
        tasks_total > 0 && finished == tasks_total && verification_complete && coverage_complete;
    Ok(Status {
        graph,
        run: run.to_owned(),
        state,
        complete,
        development_complete,
        verification_complete,
        operations,
        tasks,
        tasks_total: tasks_total as u64,
        operations_total: operations_total as u64,
    })
}
