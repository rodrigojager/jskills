pub mod activity;
pub mod catalog;
pub mod checkpoint;
pub mod clean;
pub mod decisions;
pub mod doctor;
pub mod exec;
pub mod graph;
pub mod help;
pub mod operations;
pub mod profiles;
pub mod questions;
pub mod read;
pub mod resources;
pub mod retry;
pub mod watch;
use crate::{
    controller,
    domain::{
        types::{FixtureOutcome, Outcome, Plan, Receipt},
        validation,
    },
    executors,
    read::projections::{Request as ReadRequest, View},
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request},
};
use anyhow::{Context, Result, ensure};
use clap::{FromArgMatches, Parser, Subcommand};
use std::{fs, net::SocketAddr, path::PathBuf};

/// Independent foreground task execution. Exit 3 means unfinished/intervention.
#[derive(Parser)]
#[command(
    version,
    about,
    after_help = "Example: jtasks validate plan.json\nExample: jtasks import plan.json --run demo\nExample: jtasks run --run demo\nExample: jtasks status --run demo"
)]
pub struct Cli {
    /// Human language only; --lang > JTASKS_LANG > JLOCK_LANG > system > English.
    #[arg(long, global = true)]
    pub lang: Option<String>,
    /// Emit the structured command projection (commands already default to JSON).
    #[arg(long, global = true)]
    pub json: bool,
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
    /// Review or explicitly apply a declared scoped Git checkpoint.
    Checkpoint(checkpoint::Checkpoint),
    /// Inventory historical run data; execute only an unchanged approved inventory.
    Clean(clean::Clean),
    /// Watch persisted progress without scheduling or control capabilities.
    Watch(watch::Watch),
    /// Consume the versioned read contract used by CLI, terminal and HTTP.
    Read(read::Read),
    Runs(read::Page),
    Tasks(read::Page),
    Task(read::Entity),
    Attempt(read::Entity),
    Commands(read::Page),
    Logs(read::Logs),
    /// Read or record explicit durable routine decisions.
    Decisions(decisions::Decisions),
    /// Read structured pending/answered questions without mutations.
    Questions(questions::Questions),
    /// Submit a question in explicitly restricted execution; block separately.
    Ask(questions::Ask),
    /// Record a management answer; explicit bounded retry resumes stopped work.
    Answer(questions::AnswerArgs),
    /// Revise pending work while preserving baseline, history and retry ancestry.
    Graph(graph::Graph),
    /// Commit an attributable log through the current worker scope.
    Log(activity::Log),
    /// Read activity and monotonic duration checkpoints without mutations.
    Activity(activity::Activity),
    /// Read cached scoped executor metadata without launching discovery helpers.
    Executors,
    /// Advance task eligibility without clearing recovery history or starting a controller.
    Retry(retry::Retry),
    /// Read or explicitly refresh scoped discovery metadata; never infer access.
    Catalog(catalog::Catalog),
    /// Read diagnostics without repairs, refresh, credentials or provider calls.
    Doctor,
    /// Manage jtasks profile aliases separately from native agent settings.
    Profiles(profiles::Profiles),
    /// Coordinate advisory files/named resources independently of process custody.
    Resources(resources::Resources),
    /// Preview declared operation placement or read its durable outcomes.
    Operations(operations::Operations),
    /// Start, inspect, page, await and stop managed commands.
    Exec(exec::Exec),
    /// Validate explicit version-1 JSON intent without creating runtime state.
    Validate {
        plan: PathBuf,
    },
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
        /// Persist restricted permissions for every worker invocation in this run.
        #[arg(long)]
        restricted: bool,
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
    let matches = help::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match cli.command {
        Action::Ask(args) => questions::ask(args),
        Action::Log(args) => activity::log(args),
        Action::Validate { plan } => {
            let plan = load_plan(&plan)?;
            let workspace = Workspace::resolve(&cli.root, cli.data_dir.as_deref())?;
            let mut capabilities = Vec::new();
            for task in &plan.tasks {
                let (_, snapshot) =
                    crate::domain::profiles::resolve(&workspace, task, &plan.profile_defaults)?;
                capabilities.push(
                    serde_json::json!({"task":task.id,"validation":snapshot.catalog_validation}),
                );
            }
            println!(
                "{}",
                serde_json::json!({"valid":true,"tasks":plan.tasks.len(),"operations":plan.operations.len(),"operation_placement":plan.operation_placement,"capabilities":capabilities})
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
                Action::Checkpoint(args) => checkpoint::execute(&workspace, args),
                Action::Clean(args) => clean::execute(&workspace, args),
                Action::Read(args) => read::execute(&workspace, args),
                Action::Watch(args) => watch::execute(&workspace, args, cli.json),
                Action::Runs(args) => read::print(&workspace, &args.request(View::Runs)),
                Action::Tasks(args) => read::print(&workspace, &args.request(View::Tasks)),
                Action::Task(args) => read::entity(&workspace, args, View::Task),
                Action::Attempt(args) => read::entity(&workspace, args, View::Attempt),
                Action::Commands(args) => read::print(&workspace, &args.request(View::Commands)),
                Action::Logs(args) => read::logs(&workspace, args),
                Action::Decisions(args) => decisions::execute(&workspace, args),
                Action::Questions(args) => questions::read(&workspace, &args.run, true),
                Action::Answer(args) => questions::answer(&workspace, args),
                Action::Graph(args) => graph::execute(&workspace, args),
                Action::Activity(args) => activity::read(&workspace, args),
                Action::Retry(args) => retry::execute(&workspace, args),
                Action::Executors => {
                    println!("{}", catalog::executors(&workspace)?);
                    Ok(0)
                }
                Action::Catalog(args) => catalog::execute(&workspace, args),
                Action::Doctor => doctor::execute(&workspace),
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
                Action::Run { run, restricted } => {
                    controller::dispatch::run_with_panel(&workspace, &run, restricted, !cli.json)
                }
                Action::Status { run } => {
                    read::print(&workspace, &ReadRequest::new(View::Status, Some(&run)))
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
