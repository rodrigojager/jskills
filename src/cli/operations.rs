use crate::{read::status, storage::Workspace};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct Operations {
    #[command(subcommand)]
    pub action: Action,
}
#[derive(Subcommand)]
pub enum Action {
    /// Validate and show exactly the authored initial invocations before execution.
    Preview { plan: PathBuf },
    /// Read existing operation outcomes without launching or repairing anything.
    Status {
        #[arg(long)]
        run: String,
    },
}
pub fn execute(workspace: &Workspace, command: Operations) -> Result<i32> {
    match command.action {
        Action::Preview { plan } => {
            let plan = super::load_plan(&plan)?;
            println!(
                "{}",
                serde_json::json!({"completion":plan.completion,"operation_placement":plan.operation_placement,"operations":plan.operations})
            );
            Ok(0)
        }
        Action::Status { run } => {
            let snapshot = status::read(&workspace.database(), &run)?;
            println!(
                "{}",
                serde_json::json!({"run":run,"verification_complete":snapshot.verification_complete,"operations":snapshot.operations})
            );
            Ok(if snapshot.verification_complete { 0 } else { 3 })
        }
    }
}
