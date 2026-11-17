use crate::{
    controller::activity::{Kind, Severity},
    storage::Workspace,
    transport::{self, Request},
};
use anyhow::Result;
use clap::Args;
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
    let mut request = crate::read::projections::Request::new(
        crate::read::projections::View::Activity,
        Some(&args.run),
    );
    request.task = Some(args.task);
    super::read::print(workspace, &request)
}
