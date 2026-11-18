use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

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
}

#[derive(Debug, Serialize)]
pub struct TaskStatus {
    pub lineage_root: String,
    pub cooldown: serde_json::Value,
    pub id: String,
    pub state: String,
    pub attempts: Vec<AttemptStatus>,
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

pub fn from_connection(conn: &Connection, run: &str) -> Result<Status> {
    let state = conn.query_row("SELECT state FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    let mut statement =
        conn.prepare("SELECT id,state FROM tasks WHERE run_id=?1 ORDER BY ordinal")?;
    let rows = statement.query_map([run], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut tasks = Vec::new();
    for row in rows {
        let (id, state) = row?;
        let mut attempts = conn.prepare("SELECT a.id,a.state,a.pid,a.outcome,a.summary,a.exit_code,r.id,a.effective_profile FROM attempts a LEFT JOIN receipts r ON r.attempt_id=a.id WHERE a.run_id=?1 AND a.task_id=?2 ORDER BY a.rowid")?;
        let attempts = attempts
            .query_map([run, &id], |r| {
                Ok(AttemptStatus {
                    review: crate::controller::reviews::projection(&conn, &r.get::<_, String>(0)?)
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
        tasks.push(TaskStatus {
            lineage_root: crate::controller::graph::root(&conn, run, &id)?,
            cooldown: crate::controller::cooldowns::read(
                conn,
                run,
                &id,
                crate::controller::admission::now_ms(),
            )?,
            id,
            state,
            attempts,
        });
    }
    let operations = crate::controller::operations::read(&conn, run)?;
    let development_complete = !tasks.is_empty()
        && tasks.iter().all(|t| {
            matches!(
                t.state.as_str(),
                "completed" | "verification_pending" | "superseded"
            )
        });
    let verification_complete = operations
        .iter()
        .filter(|op| op.required)
        .all(|op| op.state == "passed");
    let graph = crate::controller::graph::projection(&conn, run)?;
    let coverage_complete = graph["coverage"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["satisfied"] == true);
    let complete = !tasks.is_empty()
        && tasks
            .iter()
            .all(|t| matches!(t.state.as_str(), "completed" | "superseded"))
        && verification_complete
        && coverage_complete;
    Ok(Status {
        graph,
        run: run.to_owned(),
        state,
        complete,
        development_complete,
        verification_complete,
        operations,
        tasks,
    })
}
