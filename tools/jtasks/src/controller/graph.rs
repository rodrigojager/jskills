//! Serialized, idempotent pending-graph mutation; historical identities never move.
use crate::{
    domain::{
        graph::Proposal,
        operations::{Dependency, Operation, Stage},
        types::Plan,
    },
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Data-folder directory holding proposals too large for one control message.
pub const PROPOSALS: &str = "proposals";
/// Upper bound on a proposal passed by reference.
const MAX_REFERENCED: u64 = 64 * 1024 * 1024;

/// Write a proposal for a live controller to read by reference; returns the
/// file and the SHA-256 of its bytes. The caller removes the file afterwards.
pub fn write_referenced(workspace: &Workspace, proposal: &Proposal) -> Result<(PathBuf, String)> {
    let bytes = serde_json::to_vec(proposal)?;
    ensure!(
        bytes.len() as u64 <= MAX_REFERENCED,
        "graph proposal exceeds {MAX_REFERENCED} bytes"
    );
    let dir = workspace.data.join(PROPOSALS);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", uuid::Uuid::new_v4()));
    fs::write(&path, &bytes)?;
    Ok((path, format!("{:x}", Sha256::digest(&bytes))))
}

/// Read a proposal passed by reference: the file must sit directly in this
/// data folder's proposal directory and its bytes must match the hash.
pub fn referenced(workspace: &Workspace, path: &Path, sha256: &str) -> Result<Proposal> {
    ensure!(
        sha256.len() == 64
            && sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid graph proposal hash"
    );
    let dir = fs::canonicalize(workspace.data.join(PROPOSALS))
        .context("graph proposal folder missing")?;
    let file = fs::canonicalize(path).context("graph proposal file missing")?;
    ensure!(
        file.parent() == Some(dir.as_path())
            && file.extension().is_some_and(|e| e == "json")
            && file.is_file(),
        "graph proposal file outside the data folder's proposal directory"
    );
    let mut bytes = Vec::new();
    fs::File::open(&file)?
        .take(MAX_REFERENCED + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_REFERENCED,
        "graph proposal exceeds {MAX_REFERENCED} bytes"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == sha256,
        "graph proposal file does not match its hash"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS graph_baselines(run_id TEXT PRIMARY KEY REFERENCES runs(id), original_plan TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0);
CREATE TRIGGER IF NOT EXISTS immutable_graph_baseline BEFORE UPDATE OF original_plan ON graph_baselines BEGIN SELECT RAISE(ABORT,'original graph is immutable'); END;
CREATE TABLE IF NOT EXISTS graph_requests(request_id TEXT PRIMARY KEY,run_id TEXT NOT NULL,payload TEXT NOT NULL,receipt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS graph_ancestry(run_id TEXT NOT NULL,task_id TEXT NOT NULL,parent_id TEXT NOT NULL,root_id TEXT NOT NULL,PRIMARY KEY(run_id,task_id),FOREIGN KEY(run_id,task_id) REFERENCES tasks(run_id,id));
CREATE TRIGGER IF NOT EXISTS immutable_graph_ancestry BEFORE UPDATE ON graph_ancestry BEGIN SELECT RAISE(ABORT,'graph ancestry is immutable'); END;
";
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    conn.execute(
        "INSERT OR IGNORE INTO graph_baselines(run_id,original_plan) SELECT id,plan FROM runs",
        [],
    )?;
    Ok(())
}
pub fn root(conn: &Connection, run: &str, task: &str) -> Result<String> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_ancestry')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(task.into());
    }
    Ok(conn
        .query_row(
            "SELECT root_id FROM graph_ancestry WHERE run_id=?1 AND task_id=?2",
            [run, task],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| task.into()))
}
fn producers(op: &Operation) -> Vec<String> {
    match &op.stage {
        Stage::Task { task } => vec![task.clone()],
        Stage::Prerequisite { consumers } => consumers.clone(),
        Stage::Group { tasks } | Stage::Final { tasks } => tasks.clone(),
    }
}
fn expanded(ids: &[String], replacements: &HashMap<String, Vec<String>>) -> HashSet<String> {
    ids.iter()
        .flat_map(|id| {
            replacements
                .get(id)
                .cloned()
                .unwrap_or_else(|| vec![id.clone()])
        })
        .collect()
}
fn task_check_signature(operation: &Operation) -> Result<Value> {
    let mut value = serde_json::to_value(operation)?;
    for key in ["id", "stage", "repair_task", "revision"] {
        value.as_object_mut().unwrap().remove(key);
    }
    Ok(value)
}

pub fn propose(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    request: &Proposal,
) -> Result<Value> {
    store.check_checkout(run, &workspace.checkout)?;
    crate::domain::validation::identifier(&request.request_id)?;
    ensure!(
        !request.reason.trim().is_empty(),
        "graph change requires a reason"
    );
    let payload = serde_json::to_string(request)?;
    let tx = store.conn.unchecked_transaction()?;
    if let Some((old_run, old_payload, receipt)) = tx
        .query_row(
            "SELECT run_id,payload,receipt FROM graph_requests WHERE request_id=?1",
            [&request.request_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    {
        ensure!(
            old_run == run && old_payload == payload,
            "conflicting graph request id"
        );
        return Ok(serde_json::from_str(&receipt)?);
    }
    let (baseline, revision): (String, u32) = tx.query_row(
        "SELECT original_plan,revision FROM graph_baselines WHERE run_id=?1",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        revision == request.base_revision,
        "stale base graph revision: current {revision}"
    );
    let old: String = tx.query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    let old: Plan = serde_json::from_str(&old)?;
    let new = &request.plan;
    crate::domain::validation::validate(new)?;
    crate::domain::reviews::check_observer_aliases(workspace, new)?;
    // Run defaults, original shared context and completion policy are not a
    // pending-task rewrite. In particular context.requirements stays immutable.
    let mut old_globals = serde_json::to_value(&old)?;
    let mut new_globals = serde_json::to_value(new)?;
    for key in ["tasks", "operations", "requirements", "operation_placement"] {
        old_globals.as_object_mut().unwrap().remove(key);
        new_globals.as_object_mut().unwrap().remove(key);
    }
    ensure!(
        old_globals == new_globals,
        "graph change cannot rewrite run policy or original context"
    );
    let old_requirements: HashMap<_, _> =
        old.requirements.iter().map(|r| (&r.id, &r.text)).collect();
    let new_requirements: HashMap<_, _> =
        new.requirements.iter().map(|r| (&r.id, &r.text)).collect();
    for (id, text) in &old_requirements {
        ensure!(
            new_requirements.get(id) == Some(text),
            "original requirement {id} cannot be removed or rewritten"
        );
    }
    let added: HashSet<_> = new
        .requirements
        .iter()
        .filter(|r| !old_requirements.contains_key(&r.id))
        .map(|r| r.id.clone())
        .collect();
    ensure!(
        added == request.scope_additions.keys().cloned().collect()
            && request
                .scope_additions
                .values()
                .all(|r| !r.trim().is_empty()),
        "new requirements need separate IDs and explicit scope addition reasons"
    );
    let previous: HashMap<_, _> = old.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let current: HashMap<_, _> = new.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut replacements: HashMap<String, Vec<String>> = HashMap::new();
    let mut changed = HashSet::new();
    for (child, parent) in &request.ancestry {
        ensure!(
            !previous.contains_key(child.as_str())
                && current.contains_key(child.as_str())
                && previous.contains_key(parent.as_str()),
            "ancestry must link a new task to existing pending work"
        );
        replacements
            .entry(parent.clone())
            .or_default()
            .push(child.clone());
    }
    for task in &old.tasks {
        let same = current
            .get(task.id.as_str())
            .map(|t| serde_json::to_value(t))
            .transpose()?
            == Some(serde_json::to_value(task)?);
        if same && !replacements.contains_key(&task.id) {
            continue;
        }
        let state: String = tx.query_row(
            "SELECT state FROM tasks WHERE run_id=?1 AND id=?2",
            [run, &task.id],
            |r| r.get(0),
        )?;
        ensure!(
            state == "pending",
            "only pending task structure can change: {} is {state}",
            task.id
        );
        ensure!(
            !super::recovery::live_commands(&tx, run, &task.id)?,
            "pending task still has live managed commands"
        );
        let live: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE run_id=?1 AND task_id=?2 AND state IN ('launch_intent','running','uncertain'))",[run,&task.id],|r|r.get(0))?;
        ensure!(!live, "task has active or uncertain history");
        changed.insert(task.id.clone());
        if let Some(next) = current.get(task.id.as_str()) {
            ensure!(
                next.requirements == task.requirements
                    && next.context.requirements == task.context.requirements
                    && serde_json::to_value(&next.recovery)?
                        == serde_json::to_value(&task.recovery)?,
                "adjustment cannot remap original requirements or recovery policy"
            );
        } else {
            ensure!(
                replacements.contains_key(&task.id),
                "pending tasks can only be removed through explicit splits"
            );
        }
    }
    for task in &new.tasks {
        if previous.contains_key(task.id.as_str()) {
            continue;
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE run_id=?1 AND id=?2)",
            [run, &task.id],
            |r| r.get(0),
        )?;
        ensure!(!exists, "historical task IDs cannot be reused");
        if let Some(parent) = request.ancestry.get(&task.id) {
            let source = previous[parent.as_str()];
            ensure!(
                changed.contains(parent),
                "split source must be changed pending work"
            );
            ensure!(
                task.requirements == source.requirements
                    && task.context.requirements == source.context.requirements
                    && serde_json::to_value(&task.recovery)?
                        == serde_json::to_value(&source.recovery)?,
                "split must preserve requirement text/mappings and recovery policy"
            );
        } else {
            ensure!(
                !task.requirements.is_empty()
                    && task.requirements.iter().all(|r| added.contains(r)),
                "new work on existing requirements requires explicit repair ancestry"
            );
        }
        changed.insert(task.id.clone());
    }
    // A retained parent counts as one continuation too, never as removed work.
    for (parent, children) in &mut replacements {
        if current.contains_key(parent.as_str()) {
            children.push(parent.clone());
        }
    }
    // Mandatory checks retain identity and every original (expanded) producer.
    // Changes invalidate current applicability, preserving the receipt/journal.
    let ops: HashMap<_, _> = new
        .operations
        .iter()
        .map(|op| (op.id.as_str(), op))
        .collect();
    let mut invalidated = HashSet::new();
    for op in &old.operations {
        let next = ops.get(op.id.as_str());
        ensure!(next.is_some(), "historical operations cannot be removed");
        let next = *next.unwrap();
        ensure!(
            !op.required || next.required,
            "required checks cannot be waived"
        );
        if op.required {
            let mut original_call = serde_json::to_value(&op.initial)?;
            let mut next_call = serde_json::to_value(&next.initial)?;
            original_call.as_object_mut().unwrap().remove("resources");
            next_call.as_object_mut().unwrap().remove("resources");
            ensure!(
                original_call == next_call
                    && op.inputs.iter().all(|input| next.inputs.contains(input)),
                "required check command and original inputs cannot be waived"
            );
            for dependency in &op.depends_on {
                match dependency {
                    Dependency::Operation { id } => ensure!(
                        next.depends_on
                            .iter()
                            .any(|d| matches!(d,Dependency::Operation{id:next_id} if id==next_id)),
                        "required operation dependency cannot be waived"
                    ),
                    Dependency::TaskCandidate { id } | Dependency::TaskCompleted { id } => {
                        let expected = expanded(std::slice::from_ref(id), &replacements);
                        for id in expected {
                            ensure!(
                                next.depends_on.iter().any(|d| match (dependency, d) {
                                    (
                                        Dependency::TaskCandidate { .. },
                                        Dependency::TaskCandidate { id: next_id },
                                    )
                                    | (
                                        Dependency::TaskCompleted { .. },
                                        Dependency::TaskCompleted { id: next_id },
                                    ) => next_id == &id,
                                    _ => false,
                                }),
                                "required task dependency cannot be waived"
                            );
                        }
                    }
                }
            }
        }
        let expected = expanded(&producers(op), &replacements);
        let actual: HashSet<_> = producers(next).into_iter().collect();
        if op.required {
            ensure!(
                std::mem::discriminant(&op.stage) == std::mem::discriminant(&next.stage),
                "required operation placement cannot be waived"
            );
            if matches!(op.stage, Stage::Task { .. }) {
                ensure!(
                    actual.is_subset(&expected),
                    "task check must remain on its original continuation"
                );
                let signature = task_check_signature(next)?;
                for task in &expected {
                    let covered = new
                        .operations
                        .iter()
                        .filter(
                            |candidate| matches!(&candidate.stage,Stage::Task{task:id} if id==task),
                        )
                        .any(|candidate| {
                            task_check_signature(candidate).is_ok_and(|s| s == signature)
                        });
                    ensure!(
                        covered,
                        "split task {task} needs its original required task check"
                    );
                }
            } else {
                ensure!(
                    expected.is_subset(&actual),
                    "required check {} lost original producers/consumers",
                    op.id
                );
            }
        }
        let mut affected = serde_json::to_value(op)? != serde_json::to_value(next)?
            || producers(op).iter().any(|id| changed.contains(id));
        for dep in &op.depends_on {
            if let Dependency::TaskCandidate { id } | Dependency::TaskCompleted { id } = dep {
                affected |= changed.contains(id);
            }
        }
        if affected {
            let state: String = tx.query_row(
                "SELECT state FROM operations WHERE run_id=?1 AND id=?2",
                [run, &op.id],
                |r| r.get(0),
            )?;
            ensure!(
                !matches!(state.as_str(), "running" | "repairing"),
                "active operation cannot change applicability"
            );
            // Changing a task check on completed/active work would rewrite its
            // accepted completion semantics; group/final verification is separate.
            if let Stage::Task { task } = &op.stage {
                let state: String = tx.query_row(
                    "SELECT state FROM tasks WHERE run_id=?1 AND id=?2",
                    [run, task],
                    |r| r.get(0),
                )?;
                ensure!(
                    state == "pending",
                    "task check changes require pending work"
                );
            }
            invalidated.insert(op.id.clone());
        }
    }
    // Transitive checks cannot retain a pass after an upstream input changed.
    loop {
        let before = invalidated.len();
        for op in &new.operations {
            if op
                .depends_on
                .iter()
                .any(|d| matches!(d,Dependency::Operation{id} if invalidated.contains(id)))
            {
                invalidated.insert(op.id.clone());
            }
        }
        if before == invalidated.len() {
            break;
        }
    }
    let held = crate::resources::registry::list(&super::admission::registry_path(workspace))?;
    let mut resource_effects = Vec::new();
    for task in &new.tasks {
        let wanted = super::admission::resources(workspace, &task.admission.resources)?;
        if changed.contains(&task.id) {
            crate::domain::profiles::resolve(workspace, task, &new.profile_defaults)?;
            let conflicts: Vec<_> = held
                .iter()
                .filter(|h| wanted.iter().any(|r| r.identity == h.resource.identity))
                .map(|h| json!({"reservation":h.id,"owner":h.owner}))
                .collect();
            if !conflicts.is_empty() {
                resource_effects.push(json!({"task":task.id,"effect":"wait_for_normal_admission","conflicts":conflicts}));
            }
        }
    }
    for op in &new.operations {
        super::admission::resources(workspace, &op.initial.resources)?;
    }
    // All validation above is read-only; one transaction records graph, ancestry,
    // applicability, request receipt and journal below.
    for task in &old.tasks {
        if !current.contains_key(task.id.as_str()) {
            tx.execute(
                "UPDATE tasks SET state='superseded' WHERE run_id=?1 AND id=?2",
                [run, &task.id],
            )?;
            tx.execute("UPDATE due_work SET state='cancelled' WHERE run_id=?1 AND task_id=?2 AND state IN ('pending','queued','admitted')",[run,&task.id])?;
        }
    }
    for (ordinal, task) in new.tasks.iter().enumerate() {
        if previous.contains_key(task.id.as_str()) {
            tx.execute(
                "UPDATE tasks SET ordinal=?3,spec=?4 WHERE run_id=?1 AND id=?2",
                params![run, task.id, ordinal as i64, serde_json::to_string(task)?],
            )?;
        } else {
            tx.execute(
                "INSERT INTO tasks(run_id,id,ordinal,spec,state) VALUES(?1,?2,?3,?4,'pending')",
                params![run, task.id, ordinal as i64, serde_json::to_string(task)?],
            )?;
            if let Some(parent) = request.ancestry.get(&task.id) {
                let root = root(&tx, run, parent)?;
                tx.execute(
                    "INSERT INTO graph_ancestry VALUES(?1,?2,?3,?4)",
                    [run, &task.id, parent, &root],
                )?;
            } else {
                super::recovery::ensure_lineage(&tx, run, &task.id, &task.recovery)?;
            }
        }
    }
    let next_revision = revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("graph revision exhausted"))?;
    for (ordinal, op) in new.operations.iter().enumerate() {
        let old_op = old.operations.iter().any(|old| old.id == op.id);
        if old_op {
            if invalidated.contains(&op.id) {
                let live: bool = tx.query_row("SELECT state IN ('running','repairing') FROM operations WHERE run_id=?1 AND id=?2",[run,&op.id],|r|r.get(0))?;
                ensure!(!live, "dependent operation is active");
                tx.execute("UPDATE operations SET spec=?3,ordinal=?4,required=?5,revision=?6,state='pending',command_id=NULL,request_id=?7 WHERE run_id=?1 AND id=?2",params![run,op.id,serde_json::to_string(op)?,ordinal as i64,op.required,format!("graph-{next_revision}:{}",op.revision),format!("graph-{}-{}",request.request_id,op.id)])?;
            }
        } else {
            if let Stage::Task { task } = &op.stage {
                let pending = if previous.contains_key(task.as_str()) {
                    tx.query_row(
                        "SELECT state='pending' FROM tasks WHERE run_id=?1 AND id=?2",
                        [run, task],
                        |r| r.get::<_, bool>(0),
                    )?
                } else {
                    true
                };
                ensure!(pending, "new task checks require pending work");
            }
            tx.execute("INSERT INTO operations(run_id,id,ordinal,spec,required,revision,state,request_id) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7)",params![run,op.id,ordinal as i64,serde_json::to_string(op)?,op.required,op.revision,format!("graph-{}-{}",request.request_id,op.id)])?;
        }
    }
    tx.execute("UPDATE runs SET plan=?2,state=CASE WHEN state='completed' THEN 'ready' ELSE state END WHERE id=?1",[run,&serde_json::to_string(new)?])?;
    tx.execute(
        "UPDATE graph_baselines SET revision=?2 WHERE run_id=?1",
        params![run, next_revision],
    )?;
    let original: Plan = serde_json::from_str(&baseline)?;
    let receipt = json!({"request_id":request.request_id,"run":run,"revision":next_revision,"reason":request.reason,"original_total":original.tasks.len(),"previous_total":old.tasks.len(),"current_total":new.tasks.len(),"delta":new.tasks.len() as i64-old.tasks.len() as i64,"scope_additions":request.scope_additions,"resource_effects":resource_effects,"invalidated_operations":invalidated.into_iter().collect::<std::collections::BTreeSet<_>>()});
    tx.execute(
        "INSERT INTO graph_requests VALUES(?1,?2,?3,?4)",
        params![request.request_id, run, payload, receipt.to_string()],
    )?;
    tx.execute(
        "INSERT INTO journal(run_id,event,detail) VALUES(?1,'graph_revised',?2)",
        params![run, receipt.to_string()],
    )?;
    tx.commit()?;
    Ok(receipt)
}

pub fn projection(conn: &Connection, run: &str) -> Result<Value> {
    let plan: String = conn.query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
    let plan: Plan = serde_json::from_str(&plan)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_baselines')",
        [],
        |r| r.get(0),
    )?;
    let (original, revision) = if exists {
        conn.query_row(
            "SELECT original_plan,revision FROM graph_baselines WHERE run_id=?1",
            [run],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)),
        )
        .optional()?
        .unwrap_or((serde_json::to_string(&plan)?, 0))
    } else {
        (serde_json::to_string(&plan)?, 0)
    };
    let original: Plan = serde_json::from_str(&original)?;
    let completed: u32 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE run_id=?1 AND state='completed'",
        [run],
        |r| r.get(0),
    )?;
    let mut coverage = Vec::new();
    for requirement in &plan.requirements {
        let mut work = Vec::new();
        for task in plan
            .tasks
            .iter()
            .filter(|t| t.requirements.contains(&requirement.id))
        {
            let state: String = conn.query_row(
                "SELECT state FROM tasks WHERE run_id=?1 AND id=?2",
                [run, &task.id],
                |r| r.get(0),
            )?;
            work.push(json!({"task":task.id,"state":state}));
        }
        let ids: HashSet<_> = work.iter().filter_map(|w| w["task"].as_str()).collect();
        let mut checks = Vec::new();
        for op in plan
            .operations
            .iter()
            .filter(|op| op.required && producers(op).iter().any(|t| ids.contains(t.as_str())))
        {
            let state: String = conn.query_row(
                "SELECT state FROM operations WHERE run_id=?1 AND id=?2",
                [run, &op.id],
                |r| r.get(0),
            )?;
            checks.push(json!({"operation":op.id,"state":state}));
        }
        let satisfied = !work.is_empty()
            && work.iter().all(|w| w["state"] == "completed")
            && checks.iter().all(|c| c["state"] == "passed");
        coverage.push(json!({"id":requirement.id,"original_text":requirement.text,"work":work,"checks":checks,"satisfied":satisfied}));
    }
    Ok(
        json!({"revision":revision,"original_total":original.tasks.len(),"current_total":plan.tasks.len(),"completed":completed,"delta":plan.tasks.len() as i64-original.tasks.len() as i64,"coverage":coverage,"original_context_requirements":original.context.requirements}),
    )
}
