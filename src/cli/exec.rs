use crate::{
    controller::commands::{self, Action, Endpoint},
    execution::commands::CommandSpec,
    storage::Workspace,
};
use anyhow::{Context, Result, ensure};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Args)]
pub struct Exec {
    /// All command receipts are JSON; accepted explicitly for scripted clients.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub action: ExecAction,
}
#[derive(Subcommand)]
pub enum ExecAction {
    /// Own the foreground command controller until explicit shutdown.
    Serve,
    Start {
        #[arg(long)]
        request: PathBuf,
    },
    /// Start a managed command for this worker's attempt and wait for it.
    ///
    /// The controller owns the command beyond the worker's session. The same
    /// request ID returns the same command, also to a later attempt of the
    /// task, so a retry waits for it instead of starting it again.
    #[command(
        after_help = "Example: jtasks exec run --request-id build-1 -- cargo build --release\nExample: jtasks exec run --request-id tests-1 --shell \"cargo test 2>&1\"\nWhile the receipt says wait_expired: true, repeat: jtasks exec wait <command_id> --wait-seconds 300"
    )]
    Run(Run),
    Status {
        id: String,
    },
    Output {
        id: String,
        #[arg(long, default_value = "stdout")]
        stream: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 65536)]
        limit: usize,
        /// Print the page as text only (no byte array); continue at next_offset.
        #[arg(long)]
        text: bool,
    },
    /// Bound only this client's wait; expiry leaves the command running.
    Wait {
        id: String,
        #[arg(long, default_value_t = 20)]
        wait_seconds: u64,
        /// Bytes of each stream's end included once the command has ended.
        #[arg(long, default_value_t = 4000)]
        tail_bytes: usize,
    },
    Stop {
        id: String,
        #[arg(long)]
        reason: String,
    },
    Shutdown,
}

#[derive(Args)]
pub struct Run {
    /// Stable ID of this command; repeating it never starts a second command.
    #[arg(long)]
    request_id: String,
    /// Defaults to the run of the worker's attempt.
    #[arg(long)]
    run: Option<String>,
    #[arg(long, env = "JTASKS_TASK_ID")]
    task: Option<String>,
    #[arg(long, env = "JTASKS_ATTEMPT_ID")]
    attempt: Option<String>,
    /// Bind the command to an operation instead of a task attempt (the
    /// worker's task and attempt variables are then ignored).
    #[arg(long)]
    operation: Option<String>,
    /// Defaults to the current directory.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Named resources held while the command runs (besides the task's own).
    #[arg(long = "resource")]
    resources: Vec<String>,
    /// No deadline unless one is given.
    #[arg(long)]
    deadline_ms: Option<u64>,
    /// How long this call waits; the command keeps running afterwards.
    #[arg(long, default_value_t = 300)]
    wait_seconds: u64,
    #[arg(long, default_value_t = 4000)]
    tail_bytes: usize,
    /// A script for the platform shell (pwsh -Command on Windows, sh -c elsewhere).
    #[arg(long, conflicts_with = "program")]
    shell: Option<String>,
    /// The program and its arguments, after `--`.
    #[arg(last = true)]
    program: Vec<String>,
}

pub fn execute(workspace: &Workspace, exec: Exec) -> Result<i32> {
    if matches!(exec.action, ExecAction::Serve) {
        return commands::serve(workspace);
    }
    // The run's own controller when it is live, otherwise the standalone `exec serve` one.
    let endpoint =
        |run: Option<&str>| -> Result<Endpoint> { commands::endpoint_for(&workspace.data, run) };
    let read = |id: &str, output_args: Option<(&str, u64, usize)>| -> Result<Value> {
        use crate::read::projections::{self, Request, View};
        let mut request = Request::new(
            if output_args.is_some() {
                View::Logs
            } else {
                View::Command
            },
            None,
        );
        request.command = Some(id.into());
        if let Some((stream, offset, limit)) = output_args {
            request.stream = stream.into();
            request.after = offset;
            request.limit = limit;
        }
        // Never contact a writer from status/output/wait, even while it is live.
        projections::read(workspace, &request)
    };
    let value = match exec.action {
        ExecAction::Start { request } => {
            let spec: CommandSpec = serde_json::from_slice(&fs::read(request)?)?;
            let run = spec.run.clone();
            commands::send(&endpoint(Some(&run))?, Action::Start { spec })?
        }
        ExecAction::Run(args) => {
            let (wait_seconds, tail_bytes) = (args.wait_seconds, args.tail_bytes);
            let spec = run_spec(workspace, args)?;
            let run = spec.run.clone();
            let started = commands::send(&endpoint(Some(&run))?, Action::Start { spec })?;
            let id = started["command_id"]
                .as_str()
                .context("command controller omitted the command id")?
                .to_owned();
            let mut receipt = wait(&read, &id, wait_seconds, tail_bytes)?;
            receipt["duplicate"] = started["duplicate"].clone();
            if !started["started_by_attempt"].is_null() {
                receipt["started_by_attempt"] = started["started_by_attempt"].clone();
            }
            receipt
        }
        ExecAction::Status { id } => read(&id, None)?,
        ExecAction::Output {
            id,
            stream,
            offset,
            limit,
            text,
        } => {
            let mut page = read(&id, Some((&stream, offset, limit)))?;
            if text && let Some(object) = page.as_object_mut() {
                object.remove("bytes");
            }
            page
        }
        ExecAction::Wait {
            id,
            wait_seconds,
            tail_bytes,
        } => wait(&read, &id, wait_seconds, tail_bytes)?,
        ExecAction::Stop { id, reason } => {
            // The command's run, when its receipt is readable, names the controller that owns it.
            let receipt = read(&id, None).ok();
            let run = receipt.as_ref().and_then(|r| r["run"].as_str());
            commands::send(&endpoint(run)?, Action::Stop { id, reason })?
        }
        ExecAction::Shutdown => commands::send(&endpoint(None)?, Action::Shutdown)?,
        ExecAction::Serve => unreachable!(),
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(0)
}

type Reader<'a> = dyn Fn(&str, Option<(&str, u64, usize)>) -> Result<Value> + 'a;

/// Poll persisted receipts for at most `seconds`, without asserting live
/// custody. An ended command carries the end of both streams and how to page
/// the rest; a running one names the call that continues waiting.
fn wait(read: &Reader, id: &str, seconds: u64, tail_bytes: usize) -> Result<Value> {
    let started = Instant::now();
    let mut receipt = loop {
        let receipt = read(id, None)?;
        let running = matches!(
            receipt["recorded_state"].as_str(),
            Some("queued" | "intent" | "running" | "stopping")
        );
        if !running || started.elapsed() >= Duration::from_secs(seconds) {
            break receipt;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let running = matches!(
        receipt["recorded_state"].as_str(),
        Some("queued" | "intent" | "running" | "stopping")
    );
    receipt["wait_expired"] = Value::Bool(running);
    if running {
        receipt["next"] = json!(format!(
            "jtasks exec wait {id} --wait-seconds {}",
            seconds.max(1)
        ));
        return Ok(receipt);
    }
    if tail_bytes > 0 {
        let limit = tail_bytes.min(crate::read::output::MAX_PAGE_BYTES);
        for stream in ["stdout", "stderr"] {
            let end = receipt[format!("{stream}_cursor")].as_u64().unwrap_or(0);
            if end == 0 {
                receipt[format!("{stream}_tail")] = json!("");
                continue;
            }
            let from = end.saturating_sub(limit as u64);
            let page = read(id, Some((stream, from, limit)))?;
            receipt[format!("{stream}_tail")] = page["text"].clone();
            receipt[format!("{stream}_tail_offset")] = json!(from);
        }
    }
    receipt["next"] = json!(format!(
        "jtasks exec output {id} --stream stdout --offset 0 --text"
    ));
    Ok(receipt)
}

/// The command request of `exec run`, bound to the worker's attempt.
fn run_spec(workspace: &Workspace, args: Run) -> Result<CommandSpec> {
    let (program, program_args, script) = match (&args.shell, args.program.split_first()) {
        (Some(script), _) => {
            let (shell, flags): (&str, &[&str]) = if cfg!(windows) {
                (
                    "pwsh",
                    &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"],
                )
            } else {
                ("sh", &["-c"])
            };
            (
                shell.to_owned(),
                flags.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                Some(script.clone()),
            )
        }
        (None, Some((program, rest))) => (program.clone(), rest.to_vec(), None),
        (None, None) => anyhow::bail!("give the program after `--`, or --shell <script>"),
    };
    let task = args.operation.is_none().then_some(args.task).flatten();
    let attempt = args.operation.is_none().then_some(args.attempt).flatten();
    ensure!(
        args.operation.is_some() || (task.is_some() && attempt.is_some()),
        "exec run needs the worker's JTASKS_TASK_ID and JTASKS_ATTEMPT_ID (or --task and --attempt), or --operation"
    );
    let run = match args.run {
        Some(run) => run,
        None => {
            let attempt = attempt
                .as_deref()
                .context("--run is required with --operation")?;
            crate::storage::open_reader(&workspace.database())?
                .query_row("SELECT run_id FROM attempts WHERE id=?1", [attempt], |r| {
                    r.get(0)
                })
                .context("the attempt is unknown in this checkout's runs; give --run")?
        }
    };
    let cwd = match args.cwd {
        Some(cwd) => cwd,
        None => std::env::current_dir()?,
    };
    let cwd = if cwd.is_absolute() {
        cwd
    } else {
        std::env::current_dir()?.join(cwd)
    };
    Ok(CommandSpec {
        request_id: args.request_id,
        run,
        task,
        attempt,
        operation: args.operation,
        program,
        args: program_args,
        script,
        cwd,
        environment_refs: BTreeMap::new(),
        resources: args.resources,
        deadline_ms: args.deadline_ms,
    })
}
