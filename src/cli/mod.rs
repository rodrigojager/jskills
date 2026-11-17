pub mod exec;
pub mod operations;
pub mod profiles;
pub mod resources;
use crate::{
    controller,
    domain::{
        types::{FixtureOutcome, Outcome, Plan, Receipt},
        validation,
    },
    executors,
    read::status,
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request},
};
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{fs, net::SocketAddr, path::PathBuf};

/// Independent foreground task execution. Exit 3 means unfinished/intervention.
#[derive(Parser)]
#[command(
    version,
    about,
    after_help = "Example: jtasks validate plan.json\nExample: jtasks import plan.json --run demo\nExample: jtasks run --run demo\nExample: jtasks status --run demo"
)]
pub struct Cli {
    #[arg(long, global = true, default_value = ".")]
    pub root: PathBuf,
    /// Runtime data directory; relative paths are relative to the container/checkout.
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Action,
}

#[derive(Subcommand)]
pub enum Action {
    /// Manage jtasks profile aliases separately from native agent settings.
    Profiles(profiles::Profiles),
    /// Coordinate advisory files/named resources independently of process custody.
    Resources(resources::Resources),
    /// Preview declared operation placement or read its durable outcomes.
    Operations(operations::Operations),
    /// Start, inspect, page, await and stop managed commands.
    Exec(exec::Exec),
    /// Validate explicit version-1 JSON intent without creating runtime state.
    Validate { plan: PathBuf },
    /// Import an immutable plan into SQLite, under the checkout writer lock.
    Import {
        plan: PathBuf,
        #[arg(long)]
        run: String,
    },
    /// Own the controller in this foreground process until completion/intervention.
    Run {
        #[arg(long)]
        run: String,
    },
    /// Read an existing database without creating, migrating or repairing it.
    Status {
        #[arg(long)]
        run: String,
    },
    /// Ask the live controller to retire its process and stop scheduling.
    Stop {
        #[arg(long)]
        run: String,
    },
    /// Submit a designated outcome through an attempt-scoped control client.
    Result {
        #[arg(long, env = "JTASKS_CONTROL_ENDPOINT")]
        endpoint: SocketAddr,
        #[arg(long, env = "JTASKS_RESULT_TOKEN", hide_env_values = true)]
        token: String,
        #[arg(long, env = "JTASKS_ATTEMPT_ID")]
        attempt: String,
        #[arg(long, env = "JTASKS_TASK_ID")]
        task: String,
        #[arg(long)]
        receipt: String,
        #[arg(long, value_enum)]
        outcome: Outcome,
        #[arg(long)]
        summary: String,
    },
    /// Deterministic subprocess substitute for focused lifecycle checks.
    Fixture {
        #[arg(long, value_enum)]
        outcome: FixtureOutcome,
        #[arg(long, default_value = "")]
        summary: String,
        #[arg(long, default_value_t = 0)]
        delay_ms: u64,
        #[arg(long, default_value_t = 0)]
        linger_ms: u64,
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        exit_code: i32,
    },
}

pub fn execute() -> Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Action::Validate { plan } => {
            let plan = load_plan(&plan)?;
            println!(
                "{}",
                serde_json::json!({"valid":true,"tasks":plan.tasks.len(),"operations":plan.operations.len(),"operation_placement":plan.operation_placement})
            );
            Ok(0)
        }
        Action::Fixture {
            outcome,
            summary,
            delay_ms,
            linger_ms,
            exit_code,
        } => executors::fixture::execute(outcome, summary, delay_ms, linger_ms, exit_code),
        Action::Result {
            endpoint,
            token,
            attempt,
            task,
            receipt,
            outcome,
            summary,
        } => {
            let request = Request::Result {
                token,
                receipt: Receipt {
                    receipt_id: receipt,
                    attempt_id: attempt,
                    task_id: task,
                    outcome,
                    summary,
                },
            };
            println!(
                "{}",
                serde_json::to_string(&transport::send(endpoint, &request)?)?
            );
            Ok(0)
        }
        action => {
            let workspace = Workspace::resolve(&cli.root, cli.data_dir.as_deref())?;
            match action {
                Action::Profiles(profiles) => profiles::execute(&workspace, profiles),
                Action::Resources(resources) => resources::execute(&workspace, resources),
                Action::Exec(exec) => exec::execute(&workspace, exec),
                Action::Operations(operations) => operations::execute(&workspace, operations),
                Action::Import { plan, run } => {
                    let plan = load_plan(&plan)?;
                    validation::identifier(&run)?;
                    let guard = ControllerGuard::acquire(&workspace.git_dir)?;
                    Store::open(&workspace, &guard)?.import(&workspace, &run, &plan)?;
                    println!(
                        "{}",
                        serde_json::json!({"run":run,"imported":plan.tasks.len()})
                    );
                    Ok(0)
                }
                Action::Run { run } => controller::dispatch::run(&workspace, &run),
                Action::Status { run } => {
                    let snapshot = status::read(&workspace.database(), &run)?;
                    println!("{}", serde_json::to_string(&snapshot)?);
                    Ok(if snapshot.complete { 0 } else { 3 })
                }
                Action::Stop { run } => {
                    let endpoint: ControlEndpoint = serde_json::from_slice(
                        &fs::read(workspace.data.join("control.json"))
                            .context("no controller endpoint; run must be active")?,
                    )?;
                    ensure!(endpoint.run == run, "controller serves another run");
                    let response = transport::send(
                        endpoint.endpoint,
                        &Request::Stop {
                            token: endpoint.token,
                            run,
                        },
                    )?;
                    println!("{}", serde_json::to_string(&response)?);
                    Ok(0)
                }
                _ => unreachable!(),
            }
        }
    }
}

fn load_plan(path: &std::path::Path) -> Result<Plan> {
    let plan: Plan = serde_json::from_slice(&fs::read(path).context("cannot read plan JSON")?)
        .context("invalid explicit plan JSON")?;
    validation::validate(&plan)?;
    Ok(plan)
}
