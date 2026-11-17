//! Explicit, agent-directed corrective stages with durable bounded lineage.
use crate::{
    domain::{
        operation_lineage::{CORRECTION_LIMIT, Recheck, matches_check, overlaps, valid_input},
        operations::{Dependency, Operation, Stage},
        types::Plan,
    },
    execution::commands::CommandSpec,
    storage::Store,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS operation_repair_cycles(run_id TEXT NOT NULL, operation_id TEXT NOT NULL, cycle INTEGER NOT NULL, request_id TEXT NOT NULL UNIQUE, worker TEXT NOT NULL, revision TEXT NOT NULL, summary TEXT, recheck TEXT, outcome TEXT, PRIMARY KEY(run_id,operation_id,cycle), FOREIGN KEY(run_id,operation_id) REFERENCES operations(run_id,id));
CREATE TABLE IF NOT EXISTS operation_repair_commands(command_id TEXT PRIMARY KEY REFERENCES managed_commands(id), run_id TEXT NOT NULL, operation_id TEXT NOT NULL, cycle INTEGER NOT NULL, FOREIGN KEY(run_id,operation_id,cycle) REFERENCES operation_repair_cycles(run_id,operation_id,cycle));
CREATE TABLE IF NOT EXISTS operation_input_changes(request_id TEXT PRIMARY KEY, run_id TEXT NOT NULL, payload TEXT NOT NULL, receipt TEXT NOT NULL);
";

fn operation(conn: &Connection, run: &str, id: &str) -> Result<(Operation, String, String)> {
    let (spec, state, revision): (String, String, String) = conn
        .query_row(
            "SELECT spec,state,revision FROM operations WHERE run_id=?1 AND id=?2",
            [run, id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .context("unknown logical operation")?;
    Ok((serde_json::from_str(&spec)?, state, revision))
}

fn responsible(op: &Operation) -> Result<&str> {
    op.repair_task
        .as_deref()
        .or(match &op.stage {
            Stage::Task { task } => Some(task.as_str()),
            _ => None,
        })
        .context("operation needs explicit repair_task responsibility")
}

pub fn facts(conn: &Connection, run: &str, id: &str) -> Result<Value> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='operation_repair_cycles')", [], |r| r.get(0))?;
    if !exists {
        return Ok(json!({"limit":CORRECTION_LIMIT,"cycles":[],"work_stages_charged":0}));
    }
    let mut statement = conn.prepare("SELECT cycle,worker,revision,summary,recheck,outcome FROM operation_repair_cycles WHERE run_id=?1 AND operation_id=?2 ORDER BY cycle")?;
    let cycles = statement.query_map([run,id], |r| Ok(json!({"cycle":r.get::<_,u32>(0)?,"worker":r.get::<_,String>(1)?,"revision":r.get::<_,String>(2)?,"summary":r.get::<_,Option<String>>(3)?,"recheck":r.get::<_,Option<String>>(4)?,"outcome":r.get::<_,Option<String>>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let charged = cycles.len();
    let mut statement = conn.prepare("SELECT m.id,m.request_id,m.state,m.exit_code,c.cycle FROM managed_commands m LEFT JOIN operation_repair_commands c ON c.command_id=m.id WHERE m.run_id=?1 AND json_extract(m.spec,'$.operation')=?2 ORDER BY m.rowid")?;
    let commands = statement.query_map([run,id],|r| Ok(json!({"command_id":r.get::<_,String>(0)?,"request_id":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"exit_code":r.get::<_,Option<i32>>(3)?,"cycle":r.get::<_,Option<u32>>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(
        json!({"limit":CORRECTION_LIMIT,"cycles":cycles,"work_stages_charged":charged,"commands":commands}),
    )
}

pub fn begin(store: &Store, run: &str, id: &str, worker: &str, request: &str) -> Result<Value> {
    crate::domain::validation::identifier(request)?;
    let (op, state, revision) = operation(&store.conn, run, id)?;
    ensure!(
        responsible(&op)? == worker,
        "worker is not the declared repair responsibility"
    );
    let prior: Option<(String,String,String,u32)> = store.conn.query_row("SELECT run_id,operation_id,worker,cycle FROM operation_repair_cycles WHERE request_id=?1", [request], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if let Some((old_run, old_id, old_worker, cycle)) = prior {
        ensure!(
            old_run == run && old_id == id && old_worker == worker,
            "conflicting repair request id"
        );
        return Ok(json!({"operation":id,"worker":worker,"cycle":cycle,"duplicate":true}));
    }
    ensure!(
        matches!(state.as_str(), "failed" | "invalidated" | "repairing"),
        "operation has no failed or invalidated check to repair"
    );
    for dependency in &op.depends_on {
        let ready: bool = match dependency {
            Dependency::Operation { id } => store.conn.query_row("SELECT state='passed' FROM operations WHERE run_id=?1 AND id=?2",[run,id],|r|r.get(0))?,
            Dependency::TaskCandidate { id } => store.conn.query_row("SELECT state IN ('completed','verification_pending') FROM tasks WHERE run_id=?1 AND id=?2",[run,id],|r|r.get(0))?,
            Dependency::TaskCompleted { id } => store.conn.query_row("SELECT state='completed' FROM tasks WHERE run_id=?1 AND id=?2",[run,id],|r|r.get(0))?,
        };
        ensure!(ready, "operation dependency must be ready before repair");
    }
    let active: u32 = store.conn.query_row("SELECT count(*) FROM operation_repair_cycles WHERE run_id=?1 AND operation_id=?2 AND outcome IS NULL", [run,id], |r| r.get(0))?;
    ensure!(
        active == 0,
        "repair stage already active; retain its cycle across worker replacement"
    );
    let live: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND json_extract(spec,'$.operation')=?2 AND state IN ('intent','running','stopping','unknown'))",[run,id],|r|r.get(0))?;
    ensure!(
        !live,
        "retire operation commands before entering another corrective stage"
    );
    let count: u32 = store.conn.query_row(
        "SELECT count(*) FROM operation_repair_cycles WHERE run_id=?1 AND operation_id=?2",
        [run, id],
        |r| r.get(0),
    )?;
    ensure!(
        count < CORRECTION_LIMIT,
        "logical operation correction allowance exhausted"
    );
    let cycle = count + 1;
    let tx = store.conn.unchecked_transaction()?;
    tx.execute("INSERT INTO operation_repair_cycles(run_id,operation_id,cycle,request_id,worker,revision) VALUES(?1,?2,?3,?4,?5,?6)",params![run,id,cycle,request,worker,revision])?;
    tx.execute(
        "UPDATE operations SET state='repairing' WHERE run_id=?1 AND id=?2",
        [run, id],
    )?;
    // This immutable work-stage event is the integration point for the common task ledger.
    tx.execute(
        "INSERT INTO journal(run_id,task_id,event,detail) VALUES(?1,?2,'operation_work_stage',?3)",
        params![
            run,
            worker,
            json!({"operation":id,"cycle":cycle}).to_string()
        ],
    )?;
    tx.commit()?;
    Ok(json!({"operation":id,"worker":worker,"cycle":cycle,"revision":revision,"duplicate":false}))
}

/// Declared operation follow-ups must enter one corrective stage before launch.
/// Ad-hoc command bindings retain the common executor's existing contract.
pub fn authorize_command(store: &Store, spec: &CommandSpec) -> Result<Option<u32>> {
    let Some(id) = &spec.operation else {
        return Ok(None);
    };
    let declared: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM operations WHERE run_id=?1 AND id=?2)",
        [&spec.run, id],
        |r| r.get(0),
    )?;
    if !declared {
        return Ok(None);
    }
    let previous = store.conn.query_row("SELECT c.cycle FROM operation_repair_commands c JOIN managed_commands m ON m.id=c.command_id WHERE m.request_id=?1",[&spec.request_id],|r|r.get(0)).optional()?;
    if previous.is_some() {
        return Ok(previous);
    }
    let cycle = store.conn.query_row("SELECT cycle FROM operation_repair_cycles WHERE run_id=?1 AND operation_id=?2 AND outcome IS NULL", [&spec.run,id], |r|r.get(0)).optional()?.context("declared operation follow-up requires an active repair stage")?;
    Ok(Some(cycle))
}

pub fn record_command(store: &Store, spec: &CommandSpec, cycle: u32, command: &str) -> Result<()> {
    store.conn.execute("INSERT OR IGNORE INTO operation_repair_commands(command_id,run_id,operation_id,cycle) VALUES(?1,?2,?3,?4)",params![command,spec.run,spec.operation,cycle])?;
    Ok(())
}

pub fn finish(store: &Store, report: &Recheck) -> Result<Value> {
    ensure!(
        !report.summary.trim().is_empty(),
        "repair summary is required"
    );
    let (op, _, revision) = operation(&store.conn, &report.run, &report.operation)?;
    ensure!(
        responsible(&op)? == report.worker,
        "worker is not the declared repair responsibility"
    );
    let (worker, started, summary, recheck, outcome): (String,String,Option<String>,Option<String>,Option<String>) = store.conn.query_row("SELECT worker,revision,summary,recheck,outcome FROM operation_repair_cycles WHERE run_id=?1 AND operation_id=?2 AND cycle=?3",params![report.run,report.operation,report.cycle],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    ensure!(worker == report.worker, "repair worker mismatch");
    if let Some(outcome) = outcome {
        ensure!(
            summary.as_deref() == Some(&report.summary)
                && recheck.as_deref() == Some(&report.command_id)
                && started == report.revision,
            "conflicting repair report"
        );
        return Ok(json!({"operation":report.operation,"state":outcome,"duplicate":true}));
    }
    ensure!(
        revision == report.revision && started == report.revision,
        "stale check input revision; start a stage for current inputs"
    );
    let (spec, state, exit): (String,String,Option<i32>) = store.conn.query_row("SELECT m.spec,m.state,m.exit_code FROM managed_commands m JOIN operation_repair_commands c ON c.command_id=m.id WHERE m.id=?1 AND c.run_id=?2 AND c.operation_id=?3 AND c.cycle=?4",params![report.command_id,report.run,report.operation,report.cycle],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).context("recheck must belong to this operation correction stage")?;
    ensure!(
        matches!(state.as_str(), "exited" | "launch_failed" | "stopped"),
        "recheck has not retired with a known outcome"
    );
    let unsettled: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM operation_repair_commands c JOIN managed_commands m ON m.id=c.command_id WHERE c.run_id=?1 AND c.operation_id=?2 AND c.cycle=?3 AND m.state NOT IN ('exited','launch_failed','stopped'))",params![report.run,report.operation,report.cycle],|r|r.get(0))?;
    ensure!(
        !unsettled,
        "retire every command in the corrective stage before certifying its check"
    );
    let command: CommandSpec = serde_json::from_str(&spec)?;
    let checkout: String = store.conn.query_row(
        "SELECT checkout FROM runs WHERE id=?1",
        [&report.run],
        |r| r.get(0),
    )?;
    ensure!(
        matches_check(&op.initial, &command, Path::new(&checkout)),
        "command does not match the original check specification"
    );
    let outcome = if state == "exited" && exit == Some(0) {
        "passed"
    } else {
        "failed"
    };
    let tx = store.conn.unchecked_transaction()?;
    tx.execute("UPDATE operation_repair_cycles SET summary=?4,recheck=?5,outcome=?6 WHERE run_id=?1 AND operation_id=?2 AND cycle=?3",params![report.run,report.operation,report.cycle,report.summary,report.command_id,outcome])?;
    tx.execute(
        "UPDATE operations SET state=?3,command_id=?4 WHERE run_id=?1 AND id=?2",
        params![report.run, report.operation, outcome, report.command_id],
    )?;
    if outcome == "passed" {
        tx.execute("UPDATE operations SET state='pending' WHERE run_id=?1 AND state='blocked' AND command_id IS NULL",[&report.run])?;
    }
    tx.commit()?;
    Ok(json!({"operation":report.operation,"state":outcome,"duplicate":false}))
}

pub fn invalidate(
    store: &Store,
    run: &str,
    request: &str,
    revision: &str,
    inputs: &[String],
    reason: &str,
) -> Result<Value> {
    crate::domain::validation::identifier(request)?;
    ensure!(
        !revision.trim().is_empty() && !reason.trim().is_empty() && !inputs.is_empty(),
        "input change requires revision, inputs and reason"
    );
    ensure!(
        inputs.iter().all(|input| valid_input(input)),
        "input changes must use explicit checkout-relative paths without parent traversal or globs"
    );
    let payload =
        json!({"run":run,"revision":revision,"inputs":inputs,"reason":reason}).to_string();
    let prior: Option<(String, String)> = store
        .conn
        .query_row(
            "SELECT payload,receipt FROM operation_input_changes WHERE request_id=?1",
            [request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((previous, receipt)) = prior {
        ensure!(previous == payload, "conflicting input change request id");
        return Ok(serde_json::from_str(&receipt)?);
    }
    let plan: String = store
        .conn
        .query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    let plan: Plan = serde_json::from_str(&plan)?;
    let mut affected: HashSet<String> = plan
        .operations
        .iter()
        .filter(|op| {
            op.inputs
                .iter()
                .any(|declared| inputs.iter().any(|changed| overlaps(declared, changed)))
        })
        .map(|op| op.id.clone())
        .collect();
    loop {
        let before = affected.len();
        for op in &plan.operations {
            if op
                .depends_on
                .iter()
                .any(|dep| matches!(dep,Dependency::Operation { id } if affected.contains(id)))
            {
                affected.insert(op.id.clone());
            }
        }
        if affected.len() == before {
            break;
        }
    }
    let mut ids: Vec<_> = affected.into_iter().collect();
    ids.sort();
    let tx = store.conn.unchecked_transaction()?;
    for id in &ids {
        let (_, state, old_revision) = operation(&tx, run, id)?;
        ensure!(
            old_revision != revision,
            "changed inputs require a new revision"
        );
        let live: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND json_extract(spec,'$.operation')=?2 AND state IN ('intent','running','stopping','unknown'))",[run,id],|r|r.get(0))?;
        ensure!(
            !live && state != "running",
            "retire affected managed commands before changing inputs"
        );
        tx.execute("UPDATE operation_repair_cycles SET outcome='invalidated',summary='inputs changed during corrective stage' WHERE run_id=?1 AND operation_id=?2 AND outcome IS NULL",[run,id])?;
        tx.execute(
            "UPDATE operations SET state=CASE WHEN command_id IS NULL AND state IN ('pending','blocked') THEN 'pending' ELSE 'invalidated' END,revision=?3 WHERE run_id=?1 AND id=?2",
            params![run, id, revision],
        )?;
        let (op, _, _) = operation(&tx, run, id)?;
        if let Stage::Task { task } = op.stage {
            if op.required {
                tx.execute("UPDATE tasks SET state='verification_pending' WHERE run_id=?1 AND id=?2 AND state='completed'",[run,task.as_str()])?;
            }
        }
    }
    let receipt = json!({"run":run,"revision":revision,"invalidated":ids});
    tx.execute("INSERT INTO operation_input_changes(request_id,run_id,payload,receipt) VALUES(?1,?2,?3,?4)",params![request,run,payload,receipt.to_string()])?;
    tx.execute(
        "INSERT INTO journal(run_id,event,detail) VALUES(?1,'operation_inputs_changed',?2)",
        params![run, payload],
    )?;
    tx.commit()?;
    Ok(receipt)
}
