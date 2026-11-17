use crate::controller::commands::{self, Action as ControlAction, Endpoint};
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
    /// Enter one charged corrective work stage for the declared responsible task.
    RepairBegin {
        #[arg(long)]
        run: String,
        #[arg(long)]
        operation: String,
        #[arg(long)]
        worker: String,
        #[arg(long)]
        request_id: String,
    },
    /// Designate a retired managed command as the matching original check.
    RepairFinish {
        #[arg(long)]
        report: PathBuf,
    },
    /// Record changed declared inputs; invalidate only matching checks and dependents.
    InputsChanged {
        #[arg(long)]
        run: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        revision: String,
        #[arg(long, required = true)]
        input: Vec<String>,
        #[arg(long)]
        reason: String,
    },
}
pub fn execute(workspace: &Workspace, command: Operations) -> Result<i32> {
    let control = |action| -> Result<i32> {
        let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(
            workspace.data.join("commands-control.json"),
        )?)?;
        println!("{}", commands::send(&endpoint, action)?);
        Ok(0)
    };
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
        Action::RepairBegin {
            run,
            operation,
            worker,
            request_id,
        } => control(ControlAction::RepairBegin {
            run,
            operation,
            worker,
            request_id,
        }),
        Action::RepairFinish { report } => control(ControlAction::RepairFinish {
            report: serde_json::from_slice(&std::fs::read(report)?)?,
        }),
        Action::InputsChanged {
            run,
            request_id,
            revision,
            input,
            reason,
        } => control(ControlAction::InputsChanged {
            run,
            request_id,
            revision,
            inputs: input,
            reason,
        }),
    }
}
