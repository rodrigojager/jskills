use crate::{
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, Request},
};
use anyhow::{Context, Result, ensure};
use clap::Args;

#[derive(Args)]
pub struct Retry {
    pub task: String,
    #[arg(long)]
    pub run: String,
    #[arg(long)]
    pub reason: String,
    /// Replay this exact request safely if its receipt was lost.
    #[arg(long)]
    pub request_id: Option<String>,
}
pub fn execute(workspace: &Workspace, args: Retry) -> Result<i32> {
    ensure!(
        workspace.database().is_file(),
        "no runtime database; import first"
    );
    let request = args
        .request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let receipt = match ControllerGuard::acquire(&workspace.git_dir) {
        Ok(guard) => {
            let store = Store::open(workspace, &guard)?;
            crate::controller::cooldowns::retry(
                workspace,
                &store,
                &args.run,
                &args.task,
                &args.reason,
                &request,
                false,
            )?
        }
        Err(_) => {
            let endpoint = transport::control_endpoint(&workspace.data, &args.run).context(
                "checkout writer is busy; this run's foreground task controller is unavailable",
            )?;
            transport::send(
                endpoint.endpoint,
                &Request::Retry {
                    token: endpoint.token,
                    run: args.run,
                    task: args.task,
                    reason: args.reason,
                    request_id: request,
                },
            )?
            .retry
            .context("controller returned no retry receipt")?
        }
    };
    println!("{}", receipt);
    Ok(0)
}
