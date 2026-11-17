use crate::{
    controller::{
        activity::{self, Kind, Severity},
        clock,
    },
    storage::Workspace,
    transport::{self, Request},
};
use anyhow::{Result, ensure};
use clap::Args;
use rusqlite::{Connection, OpenFlags};
use std::net::SocketAddr;

#[derive(Args)]
pub struct Log {
    #[arg(long, env = "JTASKS_CONTROL_ENDPOINT")]
    pub endpoint: SocketAddr,
    #[arg(long, env = "JTASKS_RESULT_TOKEN", hide_env_values = true)]
    pub token: String,
    #[arg(long, env = "JTASKS_ATTEMPT_ID")]
    pub attempt: String,
    #[arg(long, env = "JTASKS_TASK_ID")]
    pub task: String,
    #[arg(long, value_enum, default_value = "message")]
    pub kind: Kind,
    #[arg(long, value_enum, default_value = "info")]
    pub severity: Severity,
    #[arg(long)]
    pub content: Option<String>,
}
#[derive(Args)]
pub struct Activity {
    #[arg(long)]
    pub run: String,
    #[arg(long)]
    pub task: String,
}
pub fn log(args: Log) -> Result<i32> {
    let reply = transport::send(
        args.endpoint,
        &Request::Log {
            token: args.token,
            attempt: args.attempt,
            task: args.task,
            kind: args.kind,
            severity: args.severity,
            content: args.content,
        },
    )?;
    println!("{}", serde_json::to_string(&reply)?);
    Ok(0)
}
pub fn read(workspace: &Workspace, args: Activity) -> Result<i32> {
    ensure!(workspace.database().is_file(), "no runtime database");
    let conn = Connection::open_with_flags(
        workspace.database(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("BEGIN")?;
    let mut q =
        conn.prepare("SELECT id FROM attempts WHERE run_id=?1 AND task_id=?2 ORDER BY rowid")?;
    let mut attempts = Vec::new();
    for attempt in q.query_map([&args.run, &args.task], |r| r.get::<_, String>(0))? {
        attempts.push(activity::projection(&conn, &attempt?)?);
    }
    // A task may be imported but never supervised: null is explicit.
    let has_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='task_clocks')",
        [],
        |r| r.get(0),
    )?;
    let has_clock = has_table
        && conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_clocks WHERE run_id=?1 AND task_id=?2)",
            [&args.run, &args.task],
            |r| r.get::<_, bool>(0),
        )?;
    let task_clock = if has_clock {
        clock::projection(&conn, &args.run, &args.task)?
    } else {
        serde_json::Value::Null
    };
    println!(
        "{}",
        serde_json::json!({"run":args.run,"task":args.task,"clock":task_clock,"attempts":attempts,"read_only":true})
    );
    Ok(0)
}
