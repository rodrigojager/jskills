use crate::{
    domain::autonomy::Decision,
    storage::Workspace,
    transport::{self, Request},
};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::net::SocketAddr;

#[derive(Args)]
pub struct Worker {
    #[arg(long, env = "JTASKS_CONTROL_ENDPOINT")]
    pub endpoint: SocketAddr,
    #[arg(long, env = "JTASKS_RESULT_TOKEN", hide_env_values = true)]
    pub token: String,
    #[arg(long, env = "JTASKS_ATTEMPT_ID")]
    pub attempt: String,
    #[arg(long, env = "JTASKS_TASK_ID")]
    pub task: String,
}
#[derive(Args)]
pub struct Decisions {
    #[command(subcommand)]
    pub action: Action,
}
#[derive(Subcommand)]
pub enum Action {
    /// Read authoritative journal records, without creating file projections.
    List {
        #[arg(long)]
        run: String,
    },
    /// Record a concise routine decision and deviation; continue within scope.
    Record {
        #[command(flatten)]
        worker: Worker,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        situation: String,
        #[arg(long)]
        decision: String,
        #[arg(long)]
        deviation: String,
    },
}
pub fn execute(workspace: &Workspace, args: Decisions) -> Result<i32> {
    match args.action {
        Action::List { run } => super::questions::read(workspace, &run, false),
        Action::Record {
            worker,
            request_id,
            situation,
            decision,
            deviation,
        } => {
            println!(
                "{}",
                serde_json::to_string(&transport::send(
                    worker.endpoint,
                    &Request::Decision {
                        token: worker.token,
                        task: worker.task,
                        attempt: worker.attempt,
                        decision: Decision {
                            request_id,
                            situation,
                            decision,
                            deviation
                        }
                    }
                )?)?
            );
            Ok(0)
        }
    }
}
