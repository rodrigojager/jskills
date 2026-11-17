//! Separate read-only watcher. Dropping it has no controller capability.
use crate::{read::terminal, storage::Workspace};
use anyhow::{Result, ensure};
use clap::Args;
use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

#[derive(Args)]
pub struct Watch {
    #[arg(long)]
    pub run: String,
    /// Show one persisted snapshot and return (exit 3 when unfinished).
    #[arg(long)]
    pub once: bool,
    /// Bounded client lifetime; expiration stops only this watcher.
    #[arg(long)]
    pub duration_ms: Option<u64>,
    #[arg(long, default_value_t = 500)]
    pub interval_ms: u64,
}

pub fn execute(workspace: &Workspace, args: Watch, json: bool) -> Result<i32> {
    ensure!(
        (50..=60000).contains(&args.interval_ms),
        "watch interval must be between 50 and 60000 ms"
    );
    let start = Instant::now();
    let mut panel = terminal::Panel::new(io::stdout(), io::stdout().is_terminal(), json);
    loop {
        let value = terminal::snapshot(workspace, &args.run)?;
        let complete = terminal::complete(&value);
        let expired = args
            .duration_ms
            .is_some_and(|ms| start.elapsed() >= Duration::from_millis(ms));
        let final_summary = args.once || complete || expired;
        if let Err(error) = panel.update(&value, None, final_summary) {
            if error.kind() == io::ErrorKind::BrokenPipe {
                return Ok(0);
            }
            return Err(error.into());
        }
        if final_summary {
            return Ok(if complete { 0 } else { 3 });
        }
        let delay = args
            .duration_ms
            .map(|ms| Duration::from_millis(ms).saturating_sub(start.elapsed()))
            .unwrap_or(Duration::from_millis(args.interval_ms))
            .min(Duration::from_millis(args.interval_ms));
        std::thread::sleep(delay);
    }
}
