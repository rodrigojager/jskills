use crate::{controller::cleanup, storage::Workspace};
use anyhow::{Result, ensure};
use clap::Args;
use std::{fs, path::PathBuf};

#[derive(Args)]
#[command(
    after_help = "Preview: jtasks clean --run old --json > approval.json\nExecute the reviewed exact inventory: jtasks clean --approve approval.json --yes\nFor jproject: jtasks clean --previous --finished-only --json\nPrevious selects completed runs only. Inactive incomplete history requires explicit --run selection and --discard-resume at execution. Preview never reconciles custody or releases reservations. The shared SQLite database/sidecars and advisory registry are retained; only selected run rows and owned output are removed. Cleanup refuses source-contained data overrides outside the Git directory; use the default Git data root or a dedicated external runtime root."
)]
pub struct Clean {
    #[arg(long, conflicts_with_all = ["previous", "approve"])]
    run: Vec<String>,
    #[arg(long, conflicts_with = "approve")]
    previous: bool,
    /// Strict consumer contract: every task and required operation must be completed.
    #[arg(long, conflicts_with = "approve")]
    finished_only: bool,
    /// File containing the exact versioned preview that was reviewed.
    #[arg(long, conflicts_with_all = ["run", "previous", "finished_only"])]
    approve: Option<PathBuf>,
    #[arg(long, requires = "approve")]
    yes: bool,
    #[arg(long, requires = "approve")]
    discard_resume: bool,
}

pub fn execute(workspace: &Workspace, args: Clean) -> Result<i32> {
    // Clap allows a conflicting selector to suppress a conditional requirement.
    // Enforce execution intent here too, before falling through to pure preview.
    ensure!(
        args.approve.is_some() || (!args.yes && !args.discard_resume),
        "execution flags require a reviewed --approve inventory file"
    );
    if let Some(path) = args.approve {
        ensure!(
            args.yes,
            "execution requires --yes after reviewing the exact preview"
        );
        let approval: cleanup::Preview = serde_json::from_slice(&fs::read(path)?)?;
        let receipt = cleanup::execute(workspace, &approval, args.discard_resume)?;
        println!("{}", serde_json::to_string(&receipt)?);
    } else {
        ensure!(
            args.previous || !args.run.is_empty(),
            "select explicit --run IDs or --previous"
        );
        println!(
            "{}",
            serde_json::to_string(&cleanup::preview(
                workspace,
                &args.run,
                args.previous,
                args.finished_only
            )?)?
        );
    }
    Ok(0)
}
