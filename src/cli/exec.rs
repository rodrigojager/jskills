use crate::{
    controller::commands::{self, Action, Endpoint},
    execution::commands::CommandSpec,
    storage::{Store, Workspace},
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Args)]
pub struct Exec {
    /// All command receipts are JSON; accepted explicitly for scripted clients.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub action: ExecAction,
}
#[derive(Subcommand)]
pub enum ExecAction {
    /// Own the foreground command controller until explicit shutdown.
    Serve,
    Start {
        #[arg(long)]
        request: PathBuf,
    },
    Status {
        id: String,
    },
    Output {
        id: String,
        #[arg(long, default_value = "stdout")]
        stream: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 65536)]
        limit: usize,
    },
    /// Bound only this client's wait; expiry leaves the command running.
    Wait {
        id: String,
        #[arg(long, default_value_t = 20)]
        wait_seconds: u64,
    },
    Stop {
        id: String,
        #[arg(long)]
        reason: String,
    },
    Shutdown,
}

pub fn execute(workspace: &Workspace, exec: Exec) -> Result<i32> {
    if matches!(exec.action, ExecAction::Serve) {
        return commands::serve(workspace);
    }
    let endpoint = || -> Result<Endpoint> {
        serde_json::from_slice(
            &fs::read(workspace.data.join("commands-control.json"))
                .context("no active command controller; use exec serve or run")?,
        )
        .map_err(Into::into)
    };
    let read = |id: &str, output_args: Option<(&str, u64, usize)>| -> Result<Value> {
        let action = || match output_args {
            Some((stream, offset, limit)) => Action::Output {
                id: id.into(),
                stream: stream.into(),
                offset,
                limit,
            },
            None => Action::Status { id: id.into() },
        };
        if let Ok(endpoint) = endpoint() {
            match commands::send(&endpoint, action()) {
                Ok(value) => return Ok(value),
                Err(error) if error.downcast_ref::<std::io::Error>().is_none() => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        // Offline projections open strictly read-only; they neither reconcile nor repair.
        let store = Store {
            conn: rusqlite::Connection::open_with_flags(
                workspace.database(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
        };
        let mut value = match output_args {
            Some((stream, offset, limit)) => {
                commands::output(&store, &workspace.data, id, stream, offset, limit)?
            }
            None => commands::status(&store, &workspace.data, id)?,
        };
        value["controller_available"] = Value::Bool(false);
        if matches!(
            value["state"].as_str(),
            Some("intent" | "running" | "stopping")
        ) {
            value["recorded_state"] = value["state"].clone();
            value["state"] = Value::String("unknown".into());
        }
        Ok(value)
    };
    let value = match exec.action {
        ExecAction::Start { request } => {
            let spec: CommandSpec = serde_json::from_slice(&fs::read(request)?)?;
            commands::send(&endpoint()?, Action::Start { spec })?
        }
        ExecAction::Status { id } => read(&id, None)?,
        ExecAction::Output {
            id,
            stream,
            offset,
            limit,
        } => read(&id, Some((&stream, offset, limit)))?,
        ExecAction::Wait { id, wait_seconds } => {
            let started = Instant::now();
            loop {
                let mut receipt = read(&id, None)?;
                let running = matches!(
                    receipt["state"].as_str(),
                    Some("intent" | "running" | "stopping")
                );
                if !running || started.elapsed() >= Duration::from_secs(wait_seconds) {
                    receipt["wait_expired"] = Value::Bool(running);
                    break receipt;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        ExecAction::Stop { id, reason } => {
            commands::send(&endpoint()?, Action::Stop { id, reason })?
        }
        ExecAction::Shutdown => commands::send(&endpoint()?, Action::Shutdown)?,
        ExecAction::Serve => unreachable!(),
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(0)
}
