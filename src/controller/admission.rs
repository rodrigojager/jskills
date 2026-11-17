//! Foreground admission: stable selection, independent capacities and all-or-none holds.
use crate::{
    domain::types::{Plan, Task},
    resources::{
        identity::Identity,
        registry::{self, Action, Mode, Observation, Owner, Request, Resource},
    },
    storage::{Store, Workspace},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn one() -> usize {
    1
}
fn yes() -> bool {
    true
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scheduling {
    pub task_capacity: usize,
    pub heavy_capacity: usize,
}
impl Default for Scheduling {
    fn default() -> Self {
        Self {
            task_capacity: one(),
            heavy_capacity: one(),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Requirements {
    /// `path:src/file`, `namespace/name`, or `shared:host/environment:namespace/name`.
    pub resources: Vec<String>,
    pub enabled: bool,
    /// Eligibility supplied by the task-lineage allowance policy, never a new retry budget.
    pub allowance: bool,
    pub cooldown_until_ms: u64,
}
impl Default for Requirements {
    fn default() -> Self {
        Self {
            resources: vec![],
            enabled: yes(),
            allowance: yes(),
            cooldown_until_ms: 0,
        }
    }
}
pub fn validate(plan: &Plan) -> Result<()> {
    ensure!(
        plan.scheduling.task_capacity > 0 && plan.scheduling.heavy_capacity > 0,
        "task and heavy capacities must be positive"
    );
    for task in &plan.tasks {
        for resource in &task.admission.resources {
            ensure!(
                !resource.trim().is_empty(),
                "task resource must not be empty"
            );
            if let Some(path) = resource.strip_prefix("path:") {
                ensure!(
                    !path.is_empty() && !path.contains(['*', '?', '[', ']']),
                    "explicit task resource path required"
                );
            } else {
                named_parts(resource)?;
            }
        }
    }
    Ok(())
}
fn named_parts(value: &str) -> Result<(Option<&str>, &str, &str)> {
    let (scope, value) = if let Some(shared) = value.strip_prefix("shared:") {
        let Some((scope, value)) = shared.split_once(':') else {
            anyhow::bail!("shared resource requires shared:<scope>:<namespace>/<name>");
        };
        ensure!(
            !scope.trim().is_empty() && value.contains('/'),
            "shared resource requires a scope, namespace and name"
        );
        (Some(scope), value)
    } else {
        (None, value)
    };
    let (namespace, name) = value.split_once('/').unwrap_or(("resource", value));
    ensure!(
        !namespace.trim().is_empty() && !name.trim().is_empty(),
        "resource namespace and name are required"
    );
    Ok((scope, namespace, name))
}

pub fn plan(store: &Store, run: &str) -> Result<Plan> {
    let json: String = store
        .conn
        .query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    let plan: Plan = serde_json::from_str(&json)?;
    validate(&plan)?;
    Ok(plan)
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
pub fn registry_path(workspace: &Workspace) -> PathBuf {
    let path = std::env::var_os("JTASKS_ADVISORY_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.data.join("advisory.sqlite3"));
    if path.is_absolute() {
        path
    } else {
        workspace.checkout.join(path)
    }
}
pub fn resources(workspace: &Workspace, values: &[String]) -> Result<Vec<Resource>> {
    values
        .iter()
        .map(|value| {
            let identity = if let Some(path) = value.strip_prefix("path:") {
                Identity::path(&workspace.checkout, Path::new(path))?
            } else {
                let (scope, namespace, name) = named_parts(value)?;
                Identity::named(&workspace.checkout, scope, namespace, name)?
            };
            Ok(Resource {
                identity,
                mode: Mode::Exclusive,
            })
        })
        .collect()
}
pub fn owner(workspace: &Workspace, run: &str, task: &str, attempt: &str) -> Owner {
    Owner {
        checkout: workspace.checkout.to_string_lossy().into(),
        run: run.into(),
        task: task.into(),
        attempt: Some(attempt.into()),
    }
}
/// A conflicting complete set acquires nothing. No slot is consumed while waiting.
pub fn reserve(
    workspace: &Workspace,
    owner: Owner,
    values: &[String],
    request: &str,
) -> Result<bool> {
    if values.is_empty() {
        return Ok(true);
    }
    match registry::apply(
        &registry_path(workspace),
        &Request {
            protocol_version: 1,
            request_id: request.into(),
            actor: "foreground-controller".into(),
            reason: "admit owned work".into(),
            operation: Action::Reserve {
                owner,
                observation: Observation::Active,
                resources: resources(workspace, values)?,
            },
        },
    ) {
        Ok(_) => Ok(true),
        Err(error) if error.to_string().starts_with("advisory resource conflict") => Ok(false),
        Err(error) => Err(error),
    }
}
pub fn release(workspace: &Workspace, run: &str, task: &str, attempt: &str) -> Result<()> {
    release_reserved(
        workspace,
        owner(workspace, run, task, attempt),
        &format!("task-{attempt}"),
    )
}
pub fn release_reserved(workspace: &Workspace, mut expected: Owner, request: &str) -> Result<()> {
    use rusqlite::OptionalExtension;
    let path = registry_path(workspace);
    if !path.exists() {
        return Ok(());
    }
    expected.checkout = crate::resources::identity::checkout_key(&workspace.checkout)?;
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let receipt: Option<String> = conn
        .query_row("SELECT receipt FROM receipts WHERE id=?1", [request], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(receipt) = receipt else {
        return Ok(());
    };
    let receipt: registry::Receipt = serde_json::from_str(&receipt)?;
    let current = registry::list(&path)?;
    for held in receipt.current {
        if current
            .iter()
            .any(|r| r.id == held.id && r.owner == expected && !r.overridden)
        {
            registry::apply(
                &path,
                &Request {
                    protocol_version: 1,
                    request_id: format!("retire-{}", held.id),
                    actor: "foreground-controller".into(),
                    reason: "owned scope observed empty".into(),
                    operation: Action::Release {
                        reservation: held.id,
                    },
                },
            )?;
        }
    }
    Ok(())
}

/// Re-read runtime eligibility on every selection; never trust a prior frontier.
pub fn next(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    attempt: &str,
) -> Result<Option<Task>> {
    let plan = plan(store, run)?;
    let used: i64 = store.conn.query_row("SELECT count(*) FROM attempts WHERE run_id=?1 AND state IN ('launch_intent','running','uncertain')", [run], |r| r.get(0))?;
    if usize::try_from(used)? >= plan.scheduling.task_capacity {
        return Ok(None);
    }
    let mut query = store
        .conn
        .prepare("SELECT id,state,spec FROM tasks WHERE run_id=?1 ORDER BY ordinal,id")?;
    let rows = query
        .query_map([run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let states: HashMap<_, _> = rows
        .iter()
        .map(|(id, state, _)| (id.as_str(), state.as_str()))
        .collect();
    for (id, state, spec) in &rows {
        if !matches!(state.as_str(), "pending" | "interrupted") {
            continue;
        }
        let task: Task = serde_json::from_str(spec)?;
        if !task.admission.enabled
            || !task.admission.allowance
            || task.admission.cooldown_until_ms > now_ms()
            || !task
                .depends_on
                .iter()
                .all(|dep| states.get(dep.as_str()) == Some(&"completed"))
            || !super::operations::task_ready(store, run, id)?
        {
            continue;
        }
        let current: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE run_id=?1 AND task_id=?2 AND state IN ('launch_intent','running','uncertain'))", [run,id], |r|r.get(0))?;
        if current {
            continue;
        }
        // Configuration failures stop this task before reserving any resource.
        if let Err(error) =
            crate::domain::profiles::resolve(workspace, &task, &plan.profile_defaults)
        {
            store.conn.execute(
                "UPDATE tasks SET state='needs_intervention' WHERE run_id=?1 AND id=?2",
                [run, id],
            )?;
            store.conn.execute("INSERT INTO journal(run_id,task_id,event,detail) VALUES(?1,?2,'configuration_rejected',?3)", [run,id,&error.to_string()])?;
            continue;
        }
        if reserve(
            workspace,
            owner(workspace, run, id, attempt),
            &task.admission.resources,
            &format!("task-{attempt}"),
        )? {
            return Ok(Some(task));
        }
    }
    Ok(None)
}

/// Startup cleanup follows reconciled custody only, preserving unknown owners.
pub fn reconcile(workspace: &Workspace, store: &Store, run: &str) -> Result<()> {
    let mut query = store.conn.prepare("SELECT id,task_id FROM attempts WHERE run_id=?1 AND state NOT IN ('launch_intent','running','uncertain')")?;
    for row in query.query_map([run], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (id, task) = row?;
        release(workspace, run, &task, &id)?;
    }
    // Reserve can precede durable intent. With no intent there was no spawn.
    for held in registry::list(&registry_path(workspace))? {
        if held.owner.run != run {
            continue;
        }
        if let Some(attempt) = &held.owner.attempt {
            let exists: bool = store.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1)",
                [attempt],
                |r| r.get(0),
            )?;
            if !exists {
                release(workspace, run, &held.owner.task, attempt)?;
            }
        }
    }
    Ok(())
}
pub fn cooling_down(store: &Store, run: &str) -> Result<bool> {
    let plan = plan(store, run)?;
    for task in &plan.tasks {
        if task.admission.enabled
            && task.admission.allowance
            && task.admission.cooldown_until_ms > now_ms()
        {
            let pending: bool = store.conn.query_row(
                "SELECT state IN ('pending','interrupted') FROM tasks WHERE run_id=?1 AND id=?2",
                [run, &task.id],
                |r| r.get(0),
            )?;
            if pending {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
