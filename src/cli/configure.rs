//! `jtasks configure`: list, edit and choose the agent aliases a run uses.
use crate::{
    domain::profiles::{self, ProfileConfig},
    executors::catalog,
    read::projections,
    storage::Workspace,
};
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
};

#[derive(Args)]
pub struct Configure {
    /// Defaults to the current run of this folder.
    #[arg(long)]
    pub run: Option<String>,
    /// Show the run's aliases and their use.
    #[arg(long, conflicts_with_all = ["set", "choices"])]
    pub list: bool,
    /// Change the given fields of one alias (only the fields given).
    #[arg(long, value_name = "ALIAS", conflicts_with = "choices")]
    pub set: Option<String>,
    /// Models and efforts offered for an agent.
    #[arg(long, value_name = "EXECUTOR")]
    pub choices: Option<String>,
    /// Refresh the native catalog immediately, bypassing its cache lifetime.
    #[arg(long, requires = "choices")]
    pub refresh: bool,
    #[arg(long, visible_alias = "agent")]
    pub executor: Option<String>,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, visible_alias = "variant")]
    pub effort: Option<String>,
    /// Who made the change, recorded in the journal.
    #[arg(long)]
    pub actor: Option<String>,
}

const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const OTHER_EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];

fn efforts(executor: &str) -> &'static [&'static str] {
    match executor {
        "claude" => &CLAUDE_EFFORTS,
        "opencode" => &[],
        _ => &OTHER_EFFORTS,
    }
}

fn fixed_view(configuration: &ProfileConfig) -> Value {
    json!({"executor":configuration.executor,"provider":configuration.provider,"model":configuration.model,
        "effort":configuration.effort.as_ref().or(configuration.variant.as_ref()),"variant":configuration.variant})
}

/// Aliases named by the run's tasks (role tasks) and observers, in plan order.
pub fn list(workspace: &Workspace, run: &str) -> Result<Value> {
    let conn = projections::open(workspace)?;
    let mut query =
        conn.prepare("SELECT id,spec,state FROM tasks WHERE run_id=?1 ORDER BY ordinal")?;
    let rows = query
        .query_map([run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(!rows.is_empty(), "unknown run {run}");
    let mut order: Vec<(String, &'static str)> = vec![];
    let mut tasks: BTreeMap<(String, &str), Vec<(String, String)>> = BTreeMap::new();
    for (id, spec, state) in rows {
        let spec: Value = serde_json::from_str(&spec)?;
        let mut found = vec![];
        if spec["executor"]["kind"] == "profile" {
            if let Some(alias) = spec["executor"]["alias"].as_str() {
                found.push((alias.to_string(), "tasks"));
            }
        }
        if spec["reviews"]["observer"]["kind"] == "profile" {
            if let Some(alias) = spec["reviews"]["observer"]["alias"].as_str() {
                found.push((alias.to_string(), "observer"));
            }
        }
        for (alias, role) in found {
            let key = (alias, role);
            if !order.contains(&key) {
                order.push(key.clone());
            }
            tasks
                .entry(key)
                .or_default()
                .push((id.clone(), state.clone()));
        }
    }
    let known = profiles::aliases(workspace)?;
    let mut aliases = vec![];
    for key in order {
        let used = &tasks[&key];
        let mut states: BTreeMap<&str, u64> = BTreeMap::new();
        for (_, state) in used {
            *states.entry(state.as_str()).or_default() += 1;
        }
        let (alias, role) = key;
        let (configuration, source) = match known.get(&alias) {
            Some((config, source)) => (
                fixed_view(config),
                json!(source.split(':').next().unwrap_or("project")),
            ),
            None => (Value::Null, Value::Null),
        };
        aliases.push(json!({"alias":alias,"role":role,"tasks":used.iter().map(|t| &t.0).collect::<Vec<_>>(),"states":states,"configuration":configuration,"source":source}));
    }
    Ok(json!({"run":run,"aliases":aliases}))
}

pub fn choices(workspace: &Workspace, executor: &str, refresh: bool) -> Result<Value> {
    let mut models: Vec<String> = vec![];
    let mut in_use: Vec<String> = vec![];
    let mut model_options = vec![];
    let mut providers = vec![];
    let mut effort_options = vec![];
    let snapshot = catalog::default_choices(workspace, executor, refresh)?;
    if let Some(snapshot) = &snapshot {
        if refresh {
            ensure!(
                snapshot.availability == "available_partial",
                "catalog discovery failed: {}",
                snapshot.availability
            );
            ensure!(
                matches!(snapshot.metadata_status.as_str(), "observed" | "partial")
                    || (executor == "claude"
                        && snapshot.metadata_status == "native_interface_unavailable"),
                "catalog discovery failed: {}",
                snapshot.metadata_status
            );
        }
    }
    let available = match snapshot {
        Some(snapshot) if snapshot.availability == "available_partial" => {
            for choice in snapshot.choices.iter().filter(|c| c.field == "model") {
                if !models.contains(&choice.value) {
                    models.push(choice.value.clone());
                }
                model_options.push(json!({"provider":choice.provider,"model":choice.value}));
            }
            providers.extend(
                snapshot
                    .choices
                    .iter()
                    .filter(|c| c.field == "provider")
                    .map(|c| c.value.clone()),
            );
            effort_options.extend(
                snapshot
                    .choices
                    .iter()
                    .filter(|c| c.field == "effort")
                    .map(|c| json!({"provider":c.provider,"model":c.model,"effort":c.value})),
            );
            true
        }
        _ => false,
    };
    if executor == "claude" {
        providers.extend(["anthropic", "bedrock", "vertex", "foundry"].map(str::to_string));
    }
    for (config, _) in profiles::aliases(workspace)?.values() {
        if config.executor.as_deref() == Some(executor) {
            if let Some(model) = &config.model {
                if !in_use.contains(model) {
                    in_use.push(model.clone());
                }
            }
        }
    }
    Ok(
        json!({"executor":executor,"catalogue":if available {"available"} else {"unavailable"},
        "models":models,"model_options":model_options,"providers":providers,"in_use":in_use,"efforts":efforts(executor),"effort_options":effort_options,"free_text":!available || models.is_empty()}),
    )
}

struct Change {
    alias: String,
    old: ProfileConfig,
    new: ProfileConfig,
}

fn prepare(
    workspace: &Workspace,
    alias: &str,
    executor: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    actor: &str,
) -> Result<Change> {
    let old = profiles::aliases(workspace)?
        .remove(alias)
        .with_context(|| format!("unknown jtasks alias {alias}"))?
        .0;
    let mut new = old.clone();
    if let Some(executor) = executor {
        if old.executor.as_deref() != Some(executor.as_str()) {
            // Options of another agent do not carry over.
            new.variant = None;
            new.native_profile = None;
            new.native_agent = None;
            new.effort = None;
            new.provider = None;
            new.model = None;
            new.executable = None;
            new.launcher_args = None;
        }
        new.executor = Some(executor);
    }
    if let Some(provider) = provider {
        new.provider = (provider != "native-default").then_some(provider);
    }
    if model.is_some() {
        new.model = model;
    }
    if let Some(effort) = effort {
        new.effort = (effort != "native-default").then_some(effort);
        new.variant = None;
    }
    profiles::validate(&new)?;
    ensure!(new.model.is_some(), "select a model for the agent");
    let mut values = profiles::executor_value(&new)?;
    if let Some(effort) = values.get("reasoning_effort").cloned() {
        values["effort"] = effort;
    }
    let executor = new.executor.clone().context("alias has no executor")?;
    let exception = catalog::UnknownException {
        actor: actor.into(),
        reason: "set through jtasks configure".into(),
    };
    catalog::validate_values(
        workspace,
        &executor,
        &values,
        "native-default",
        Some(&exception),
    )?;
    Ok(Change {
        alias: alias.into(),
        old,
        new,
    })
}

fn apply(workspace: &Workspace, run: &str, change: &Change, actor: &str) -> Result<Value> {
    let path = profiles::project_file(workspace);
    profiles::update(
        &path,
        BTreeMap::from([(change.alias.clone(), change.new.clone())]),
    )?;
    let detail = json!({"alias":change.alias,"actor":actor,
        "old":fixed_view(&change.old),"new":fixed_view(&change.new)});
    let conn = Connection::open_with_flags(
        workspace.database(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(crate::storage::WRITER_BUSY_TIMEOUT)?;
    conn.execute(
        "INSERT INTO journal(run_id,event,detail) VALUES(?1,'alias_changed',?2)",
        params![run, detail.to_string()],
    )?;
    Ok(json!({"run":run,"alias":change.alias,"changed":detail,"saved":path}))
}

/// Same as `jtasks configure --set`; answers the alias's new `--list` entry.
pub fn set_alias(
    workspace: &Workspace,
    run: &str,
    alias: &str,
    fields: [Option<String>; 4],
    actor: &str,
) -> Result<Value> {
    let [executor, provider, model, effort] = fields;
    let change = prepare(workspace, alias, executor, provider, model, effort, actor)?;
    let applied = apply(workspace, run, &change, actor)?;
    let entry = list(workspace, run).ok().and_then(|l| {
        l["aliases"]
            .as_array()?
            .iter()
            .find(|a| a["alias"] == alias)
            .cloned()
    });
    Ok(entry.unwrap_or(applied))
}

pub fn execute(workspace: &Workspace, args: Configure) -> Result<i32> {
    if let Some(executor) = &args.choices {
        println!("{}", choices(workspace, executor, args.refresh)?);
        return Ok(0);
    }
    let run = match projections::resolve_current(workspace, args.run.clone()) {
        Ok(run) => run,
        Err(error) => {
            println!("{error}");
            return Ok(1);
        }
    };
    let actor = args.actor.clone().unwrap_or_else(|| "cli".into());
    if let Some(alias) = &args.set {
        let change = prepare(
            workspace,
            alias,
            args.executor,
            args.provider,
            args.model,
            args.effort,
            &actor,
        )?;
        println!("{}", apply(workspace, &run, &change, &actor)?);
        return Ok(0);
    }
    if args.list {
        println!("{}", list(workspace, &run)?);
        return Ok(0);
    }
    menu(
        workspace,
        &run,
        &actor,
        &mut io::stdin().lock(),
        &mut io::stdout(),
    )
}

/// One answer; `None` means cancel (end of input, Esc or an empty answer).
fn ask(input: &mut impl BufRead, out: &mut impl Write, prompt: &str) -> Result<Option<String>> {
    write!(out, "{prompt} ")?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let line = line.trim();
    if line.is_empty() || line.contains('\x1b') {
        return Ok(None);
    }
    Ok(Some(line.to_string()))
}

fn menu(
    workspace: &Workspace,
    run: &str,
    actor: &str,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<i32> {
    let listing = list(workspace, run)?;
    let aliases = listing["aliases"].as_array().cloned().unwrap_or_default();
    if aliases.is_empty() {
        writeln!(out, "run {run} names no aliases")?;
        return Ok(0);
    }
    for (index, item) in aliases.iter().enumerate() {
        let c = &item["configuration"];
        writeln!(
            out,
            "{}. {} ({}) {} {} {}",
            index + 1,
            item["alias"].as_str().unwrap_or_default(),
            item["role"].as_str().unwrap_or_default(),
            c["executor"].as_str().unwrap_or("?"),
            c["model"].as_str().unwrap_or("?"),
            c["effort"].as_str().unwrap_or("-")
        )?;
    }
    let cancelled = |out: &mut dyn Write| -> Result<i32> {
        writeln!(out, "cancelled; nothing written")?;
        Ok(0)
    };
    let Some(answer) = ask(input, out, "Alias number or name:")? else {
        return cancelled(out);
    };
    let alias = match answer.parse::<usize>() {
        Ok(n) if (1..=aliases.len()).contains(&n) => aliases[n - 1]["alias"].as_str(),
        _ => aliases
            .iter()
            .find_map(|a| a["alias"].as_str().filter(|name| *name == answer)),
    };
    let Some(alias) = alias else {
        bail!("unknown alias {answer}");
    };
    let Some(executor) = ask(input, out, "Agent (claude, codex, opencode):")? else {
        return cancelled(out);
    };
    let offered = choices(workspace, &executor, false)?;
    writeln!(
        out,
        "Models ({}): {}",
        offered["catalogue"].as_str().unwrap_or_default(),
        offered["models"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(offered["in_use"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    let Some(model) = ask(input, out, "Model:")? else {
        return cancelled(out);
    };
    writeln!(out, "Efforts: {}", efforts(&executor).join(", "))?;
    let level_prompt = if executor == "opencode" {
        "Variant (or native-default):"
    } else {
        "Effort (or native-default):"
    };
    let Some(effort) = ask(input, out, level_prompt)? else {
        return cancelled(out);
    };
    let Some(provider) = ask(input, out, "Provider (or native-default):")? else {
        return cancelled(out);
    };
    let change = prepare(
        workspace,
        alias,
        Some(executor),
        Some(provider),
        Some(model),
        Some(effort),
        actor,
    )?;
    writeln!(
        out,
        "Change {}: {} -> {}",
        change.alias,
        fixed_view(&change.old),
        fixed_view(&change.new)
    )?;
    match ask(input, out, "Confirm (y/N):")? {
        Some(answer) if answer.eq_ignore_ascii_case("y") => {
            writeln!(out, "{}", apply(workspace, run, &change, actor)?)?;
            Ok(0)
        }
        _ => cancelled(out),
    }
}
