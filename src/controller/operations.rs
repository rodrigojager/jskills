//! Declared operations share command custody; their outcomes never fabricate task reports.
use crate::{
    domain::{
        operations::{Dependency, Operation, Stage},
        types::Plan,
    },
    execution::commands::CommandSpec,
    storage::{Store, Workspace},
};
use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct OperationStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<serde_json::Value>,
    pub id: String,
    pub state: String,
    pub required: bool,
    pub revision: String,
    pub inputs: Vec<String>,
    pub stage: Stage,
    pub repair_task: Option<String>,
    pub command_id: Option<String>,
    pub exit_code: Option<i32>,
    pub initial: crate::domain::operations::Invocation,
    pub repair: serde_json::Value,
}

pub fn read(conn: &Connection, run: &str) -> Result<Vec<OperationStatus>> {
    read_page(conn, run, 0, i64::MAX)
}

/// Only the requested page is loaded from storage.
pub fn read_page(
    conn: &Connection,
    run: &str,
    after: i64,
    limit: i64,
) -> Result<Vec<OperationStatus>> {
    let mut statement = conn.prepare(
        "SELECT id,state,spec,command_id,revision FROM operations WHERE run_id=?1 ORDER BY ordinal LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement
        .query_map(params![run, limit, after], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, state, spec, command_id, revision)| {
            let op: Operation = serde_json::from_str(&spec)?;
            let exit_code = if let Some(command) = &command_id {
                conn.query_row(
                    "SELECT exit_code FROM managed_commands WHERE id=?1",
                    [command],
                    |r| r.get(0),
                )?
            } else {
                None
            };
            let repair = super::operation_repair::facts(conn, run, &id)?;
            let repair_task = op.repair_task.clone().or_else(|| match &op.stage {
                Stage::Task { task } => Some(task.clone()),
                _ => None,
            });
            Ok(OperationStatus {
                checkpoint: op
                    .checkpoint
                    .as_ref()
                    .map(|declaration| super::checkpoints::status(conn, run, &id, declaration))
                    .transpose()?,
                id,
                state,
                required: op.required,
                revision,
                inputs: op.inputs,
                stage: op.stage,
                repair_task,
                command_id,
                exit_code,
                initial: op.initial,
                repair,
            })
        })
        .collect()
}

fn plan(store: &Store, run: &str) -> Result<Plan> {
    let json: String = store
        .conn
        .query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    Ok(serde_json::from_str(&json)?)
}
fn states(store: &Store, run: &str, table: &str) -> Result<HashMap<String, String>> {
    // Table names are internal constants, never user input.
    let mut statement = store
        .conn
        .prepare(&format!("SELECT id,state FROM {table} WHERE run_id=?1"))?;
    Ok(statement
        .query_map([run], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn task_ready(store: &Store, run: &str, task: &str) -> Result<bool> {
    let operations = states(store, run, "operations")?;
    Ok(plan(store,run)?.operations.iter().all(|op| !matches!(&op.stage, Stage::Prerequisite { consumers } if consumers.iter().any(|id| id == task)) || operations.get(&op.id).is_some_and(|state|state == "passed")))
}

fn candidate(state: &str) -> bool {
    matches!(state, "completed" | "verification_pending")
}
fn impossible(state: &str) -> bool {
    matches!(state, "failed" | "blocked" | "needs_intervention")
}

/// Reconcile command outcomes even when explicit stop has disabled admission.
pub fn reconcile(store: &Store, run: &str) -> Result<bool> {
    Ok(store.conn.execute("UPDATE operations SET state=CASE (SELECT state FROM managed_commands WHERE id=operations.command_id) WHEN 'exited' THEN CASE WHEN (SELECT exit_code FROM managed_commands WHERE id=operations.command_id)=0 THEN 'passed' ELSE 'failed' END WHEN 'unknown' THEN 'blocked' ELSE 'failed' END WHERE run_id=?1 AND state='running' AND (SELECT state FROM managed_commands WHERE id=operations.command_id) IN ('exited','launch_failed','stopped','unknown')", [run])? > 0)
}

/// Runs only from the foreground controller after process observations, before admission.
pub fn tick(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    commands: &mut super::commands::Service,
) -> Result<bool> {
    let plan = plan(store, run)?;
    if plan.operations.is_empty() {
        return Ok(false);
    }
    // Bind exactly one stable initial request to each logical operation. A restart
    // reuses the persisted handle; unknown custody remains blocked for intervention.
    let mut progressed = reconcile(store, run)?;
    let operations = states(store, run, "operations")?;
    let mut tasks = states(store, run, "tasks")?;
    for (id, state) in &mut tasks {
        if state == "verification_pending"
            && crate::domain::operations::task_checks(&plan, id)
                .all(|op| operations.get(&op.id).is_some_and(|s| s == "passed"))
        {
            store.conn.execute("UPDATE tasks SET state='completed' WHERE run_id=?1 AND id=?2 AND state='verification_pending'", [run,id.as_str()])?;
            *state = "completed".into();
            progressed = true;
        }
    }
    for operation in &plan.operations {
        if operations
            .get(&operation.id)
            .is_none_or(|state| state != "pending")
        {
            continue;
        }
        let mut needs: Vec<(&str, bool)> = Vec::new();
        match &operation.stage {
            Stage::Prerequisite { .. } => {}
            Stage::Task { task } => needs.push((tasks[task].as_str(), true)),
            Stage::Group { tasks: producers } | Stage::Final { tasks: producers } => {
                for id in producers {
                    needs.push((tasks[id].as_str(), false));
                }
            }
        }
        for dependency in &operation.depends_on {
            match dependency {
                Dependency::Operation { id } => needs.push((operations[id].as_str(), false)),
                Dependency::TaskCandidate { id } => needs.push((tasks[id].as_str(), true)),
                Dependency::TaskCompleted { id } => needs.push((tasks[id].as_str(), false)),
            }
        }
        if needs.iter().any(|(state, _)| impossible(state)) {
            store.conn.execute(
                "UPDATE operations SET state='blocked' WHERE run_id=?1 AND id=?2",
                [run, &operation.id],
            )?;
            progressed = true;
            continue;
        }
        if !needs.iter().all(|(state, is_candidate)| {
            if *is_candidate {
                candidate(state)
            } else {
                matches!(*state, "completed" | "passed")
            }
        }) {
            continue;
        }
        let request: String = store.conn.query_row(
            "SELECT request_id FROM operations WHERE run_id=?1 AND id=?2",
            [run, &operation.id],
            |r| r.get(0),
        )?;
        let invocation = &operation.initial;
        let spec = CommandSpec {
            request_id: request,
            run: run.into(),
            task: None,
            attempt: None,
            operation: Some(operation.id.clone()),
            program: invocation.program.clone(),
            args: invocation.args.clone(),
            script: invocation.script.clone(),
            cwd: if invocation.cwd.is_absolute() {
                invocation.cwd.clone()
            } else {
                workspace.checkout.join(&invocation.cwd)
            },
            environment_refs: invocation.environment_refs.clone(),
            resources: invocation.resources.clone(),
            deadline_ms: invocation.deadline_ms,
        };
        // Malformed/missing directories fail this declared invocation without
        // inventing setup commands, or stopping independent development.
        match commands.start(store, spec) {
            Ok(receipt) => {
                store.conn.execute(
                    "UPDATE operations SET state='running',command_id=?3 WHERE run_id=?1 AND id=?2",
                    params![run, operation.id, receipt["command_id"].as_str()],
                )?;
            }
            Err(_) => {
                store.conn.execute(
                    "UPDATE operations SET state='failed' WHERE run_id=?1 AND id=?2",
                    [run, &operation.id],
                )?;
            }
        }
        progressed = true;
    }
    Ok(progressed)
}
