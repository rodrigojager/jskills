use crate::{
    domain::{
        graph::{Proposal, Requirement},
        types::{Plan, Task},
    },
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request},
};
use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Subcommand};
use rusqlite::OptionalExtension;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Args)]
pub struct Graph {
    #[command(subcommand)]
    pub action: Action,
}
#[derive(Subcommand)]
pub enum Action {
    /// Apply precise pending intent under a validated base revision.
    Propose {
        #[arg(long)]
        run: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Build a proposal from the run's stored plan: add tasks, rewire pending dependencies.
    Revise(Revise),
}
pub fn execute(workspace: &Workspace, args: Graph) -> Result<i32> {
    let (run, proposal) = match args.action {
        Action::Propose { run, file } => {
            ensure!(
                workspace.database().is_file(),
                "no runtime database; import first"
            );
            let proposal: Proposal = serde_json::from_slice(&fs::read(file)?)?;
            (run, proposal)
        }
        Action::Revise(args) => match revise(workspace, args)? {
            Some(revision) => revision,
            None => return Ok(0),
        },
    };
    let receipt = match ControllerGuard::acquire(&workspace.git_dir) {
        Ok(guard) => {
            let store = Store::open(workspace, &guard)?;
            crate::controller::graph::propose(workspace, &store, &run, &proposal)?
        }
        Err(_) => {
            let endpoint = transport::control_endpoint(&workspace.data, &run)
                .context("checkout writer busy; this run's task controller is unavailable")?;
            send(workspace, endpoint, run, proposal)?
                .graph
                .context("controller returned no graph receipt")?
        }
    };
    println!("{receipt}");
    Ok(0)
}

/// Send inline when the request fits one control message; otherwise pass the
/// proposal by reference through the data folder and remove the file after.
fn send(
    workspace: &Workspace,
    endpoint: ControlEndpoint,
    run: String,
    proposal: Proposal,
) -> Result<transport::Response> {
    let inline = Request::Graph {
        token: endpoint.token,
        run,
        proposal: Box::new(proposal),
    };
    if transport::fits(&inline)? {
        return transport::send(endpoint.endpoint, &inline);
    }
    let Request::Graph {
        token,
        run,
        proposal,
    } = inline
    else {
        unreachable!()
    };
    let (path, sha256) = crate::controller::graph::write_referenced(workspace, &proposal)?;
    let response = transport::send(
        endpoint.endpoint,
        &Request::GraphFile {
            token,
            run,
            path: path.clone(),
            sha256,
        },
    );
    let _ = fs::remove_file(&path);
    response
}

#[derive(Args)]
pub struct Revise {
    #[arg(long)]
    pub run: String,
    /// JSON with the new tasks: an array of tasks, or an object with `tasks` and the new
    /// `requirements` they map to (a composed intent works: tasks the run already has keep
    /// their stored version).
    #[arg(long, value_name = "FILE")]
    pub add_tasks: Option<PathBuf>,
    /// Replace a task's dependencies: `<task>=<id>,<id>`; nothing after `=` clears them.
    #[arg(long = "depends", value_name = "TASK=IDS")]
    pub depends: Vec<String>,
    /// Why the graph changes; also the reason of each new requirement's scope addition.
    #[arg(long)]
    pub reason: Option<String>,
    /// Idempotency key; by default derived from the base revision and the changes.
    #[arg(long)]
    pub request_id: Option<String>,
    /// Write the proposal for review instead of submitting it.
    #[arg(long)]
    pub dry_run: bool,
    /// File the dry run writes (default: standard output).
    #[arg(long, requires = "dry_run", value_name = "FILE")]
    pub output: Option<PathBuf>,
}

/// The run's stored plan, its current graph revision and every task ID it ever had.
fn stored(workspace: &Workspace, run: &str) -> Result<(Plan, u32, HashSet<String>)> {
    let conn = crate::read::projections::open(workspace)?;
    let plan: String = conn
        .query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))
        .optional()?
        .with_context(|| format!("unknown run {run}"))?;
    let baselines: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_baselines')",
        [],
        |r| r.get(0),
    )?;
    let revision = if baselines {
        conn.query_row(
            "SELECT revision FROM graph_baselines WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0)
    } else {
        0
    };
    let mut statement = conn.prepare("SELECT id FROM tasks WHERE run_id=?1")?;
    let history = statement
        .query_map([run], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok((serde_json::from_str(&plan)?, revision, history))
}

/// Apply only the requested changes to the stored plan. Returns the proposal to
/// submit, or `None` after a dry run wrote it for review.
fn revise(workspace: &Workspace, args: Revise) -> Result<Option<(String, Proposal)>> {
    ensure!(
        args.add_tasks.is_some() || !args.depends.is_empty(),
        "nothing to revise: give --add-tasks and/or --depends"
    );
    let (stored, base_revision, history) = stored(workspace, &args.run)?;
    let mut plan = stored.clone();
    let mut added = Vec::new();
    let mut kept = Vec::new();
    let mut new_requirements: Vec<String> = Vec::new();
    if let Some(file) = &args.add_tasks {
        let value: Value = serde_json::from_slice(
            &fs::read(file).with_context(|| format!("cannot read {}", file.display()))?,
        )
        .context("--add-tasks file is not JSON")?;
        let (tasks, offered) = match value {
            Value::Array(_) => (value, Value::Null),
            Value::Object(mut fields) => (
                fields
                    .remove("tasks")
                    .context("--add-tasks object needs a tasks array")?,
                fields.remove("requirements").unwrap_or(Value::Null),
            ),
            _ => bail!("--add-tasks file must hold a task array or an object with tasks"),
        };
        let tasks: Vec<Task> =
            serde_json::from_value(tasks).context("--add-tasks: invalid task")?;
        let offered: Vec<Requirement> = if offered.is_null() {
            Vec::new()
        } else {
            serde_json::from_value(offered).context("--add-tasks: invalid requirements")?
        };
        let in_plan: HashSet<String> = stored.tasks.iter().map(|t| t.id.clone()).collect();
        for task in tasks {
            if in_plan.contains(&task.id) {
                kept.push(task.id);
                continue;
            }
            ensure!(
                !history.contains(&task.id) && !added.contains(&task.id),
                "task ID {} is already used in this run; choose a new one",
                task.id
            );
            for id in &task.requirements {
                if plan.requirements.iter().any(|r| &r.id == id) {
                    continue;
                }
                let requirement = offered.iter().find(|r| &r.id == id).with_context(|| {
                    format!(
                        "new requirement {id} of task {} needs its text in the file's requirements",
                        task.id
                    )
                })?;
                plan.requirements.push(requirement.clone());
                new_requirements.push(id.clone());
            }
            added.push(task.id.clone());
            plan.tasks.push(task);
        }
    }
    let mut rewired = Vec::new();
    for change in &args.depends {
        let (task, ids) = change
            .split_once('=')
            .with_context(|| format!("--depends takes <task>=<ids>, got {change}"))?;
        let task = task.trim();
        ensure!(
            !rewired.iter().any(|t| t == task),
            "--depends names task {task} twice"
        );
        let target = plan
            .tasks
            .iter_mut()
            .find(|t| t.id == task)
            .with_context(|| format!("--depends: the stored plan has no task {task}"))?;
        target.depends_on = ids
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(String::from)
            .collect();
        rewired.push(task.to_string());
    }
    ensure!(
        serde_json::to_value(&plan)? != serde_json::to_value(&stored)?,
        "the revision changes nothing in the stored plan"
    );
    crate::domain::validation::validate(&plan)?;
    let reason = args.reason.unwrap_or_else(|| {
        let mut parts = Vec::new();
        if !added.is_empty() {
            parts.push(format!("add {}", added.join(", ")));
        }
        if !rewired.is_empty() {
            parts.push(format!("change the dependencies of {}", rewired.join(", ")));
        }
        format!("Revision from the stored plan: {}", parts.join("; "))
    });
    let scope_additions: BTreeMap<String, String> = new_requirements
        .into_iter()
        .map(|id| (id, reason.clone()))
        .collect();
    let mut proposal = Proposal {
        request_id: String::new(),
        base_revision,
        reason,
        plan,
        ancestry: BTreeMap::new(),
        scope_additions,
    };
    proposal.request_id = match args.request_id {
        Some(id) => id,
        None => {
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&proposal)?));
            format!("revise-{base_revision}-{}", &digest[..16])
        }
    };
    crate::domain::validation::identifier(&proposal.request_id)?;
    if !kept.is_empty() {
        eprintln!(
            "kept the stored version of tasks already in the run: {}",
            kept.join(", ")
        );
    }
    if args.dry_run {
        let text = serde_json::to_string_pretty(&proposal)?;
        match &args.output {
            Some(path) => {
                fs::write(path, text + "\n")?;
                eprintln!(
                    "proposal written to {}; submit it with `jtasks graph propose --run {} --file <it>`",
                    path.display(),
                    args.run
                );
            }
            None => println!("{text}"),
        }
        return Ok(None);
    }
    Ok(Some((args.run, proposal)))
}
