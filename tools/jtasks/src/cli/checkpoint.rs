use crate::{controller::checkpoints, storage::Workspace};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::{fs, path::PathBuf};

#[derive(Args)]
pub struct Checkpoint {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Read the exact attributable file hashes after the declared operation passes.
    Review {
        #[arg(long)]
        run: String,
        #[arg(long)]
        operation: String,
        #[arg(long)]
        reviewer: String,
        #[arg(long)]
        attribution: String,
        #[arg(long)]
        message: String,
    },
    /// Explicitly commit an unchanged reviewed scope; preserve unrelated index/work.
    Apply {
        #[arg(long)]
        review: PathBuf,
        /// Wait for the checkout controller to retire (at most 300 seconds).
        #[arg(long, default_value_t = 0)]
        wait_seconds: u64,
    },
}

pub fn execute(workspace: &Workspace, command: Checkpoint) -> Result<i32> {
    let value = match command.action {
        Action::Review {
            run,
            operation,
            reviewer,
            attribution,
            message,
        } => serde_json::to_value(checkpoints::review(
            workspace,
            &run,
            &operation,
            &reviewer,
            &attribution,
            &message,
        )?)?,
        Action::Apply {
            review,
            wait_seconds,
        } => checkpoints::apply(
            workspace,
            &serde_json::from_slice(&fs::read(review)?)?,
            wait_seconds,
        )?,
    };
    println!("{}", value);
    Ok(0)
}
