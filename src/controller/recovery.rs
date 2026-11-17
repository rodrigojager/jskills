//! Durable allowance admission. Only the foreground writer calls mutation functions.
use crate::{
    domain::{
        recovery::{Allowance, Ledger, Policy},
        types::{Executor, Task},
    },
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{fs, io::Read};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS recovery_lineages(run_id TEXT NOT NULL,task_id TEXT NOT NULL,policy TEXT NOT NULL,next_kind TEXT NOT NULL DEFAULT 'initial',resume_attempt TEXT,work_grants INTEGER NOT NULL DEFAULT 0,infrastructure_grants INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(run_id,task_id),FOREIGN KEY(run_id,task_id) REFERENCES tasks(run_id,id));
CREATE TABLE IF NOT EXISTS recovery_invocations(attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),run_id TEXT NOT NULL,task_id TEXT NOT NULL,kind TEXT NOT NULL,state TEXT NOT NULL,source_attempt TEXT,detail TEXT NOT NULL DEFAULT '',FOREIGN KEY(run_id,task_id) REFERENCES recovery_lineages(run_id,task_id));
CREATE TRIGGER IF NOT EXISTS immutable_recovery_identity BEFORE UPDATE OF attempt_id,run_id,task_id,kind,source_attempt ON recovery_invocations BEGIN SELECT RAISE(ABORT,'recovery invocation identity is immutable'); END;
";

/// Called by Store::open only, never a read projection. Old attempt identities are
/// conservatively adopted rather than granting a new initial invocation.
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    let mut stmt = conn.prepare("SELECT run_id,id,spec FROM tasks")?;
    let tasks = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (run, id, spec) in tasks {
        let task: Task = serde_json::from_str(&spec)?;
        ensure_lineage(conn, &run, &id, &task.recovery)?;
        let mut attempts = conn.prepare(
            "SELECT id,state FROM attempts WHERE run_id=?1 AND task_id=?2 ORDER BY rowid",
        )?;
        for (index, row) in attempts
            .query_map([&run, &id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .enumerate()
        {
            let (attempt, state) = row?;
            conn.execute("INSERT OR IGNORE INTO recovery_invocations(attempt_id,run_id,task_id,kind,state,detail) VALUES(?1,?2,?3,?4,?5,'adopted existing immutable attempt')",params![attempt,run,id,if index==0 {"initial"} else {"work"},if matches!(state.as_str(),"launch_intent"|"uncertain") {"uncertain"} else {"dispatched"}])?;
        }
    }
    Ok(())
}

pub fn ensure_lineage(conn: &Connection, run: &str, task: &str, policy: &Policy) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO recovery_lineages(run_id,task_id,policy) VALUES(?1,?2,?3)",
        params![run, task, serde_json::to_string(policy)?],
    )?;
    Ok(())
}

/// Reserved and uncertain launches count against available allowance until a
/// definite prelaunch failure releases them. Stage entries share the work count.
pub fn ledger(conn: &Connection, run: &str, task: &str) -> Result<Ledger> {
    let (policy,work_grants,infra_grants): (String,u32,u32) = conn.query_row("SELECT policy,work_grants,infrastructure_grants FROM recovery_lineages WHERE run_id=?1 AND task_id=?2",[run,task],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let policy: Policy = serde_json::from_str(&policy)?;
    let count = |kind: &str| -> Result<u32> {
        Ok(conn.query_row("SELECT count(*) FROM recovery_invocations WHERE run_id=?1 AND task_id=?2 AND kind=?3 AND state<>'prelaunch_failed'",[run,task,kind],|r|r.get(0))?)
    };
    let stages: u32 = conn.query_row("SELECT count(*) FROM journal WHERE run_id=?1 AND task_id=?2 AND event='operation_work_stage'",[run,task],|r|r.get(0))?;
    Ok(Ledger {
        initial: count("initial")?,
        work: count("work")?.saturating_add(stages),
        infrastructure: count("infrastructure")?,
        work_limit: policy.work_retries.saturating_add(work_grants),
        infrastructure_limit: policy.infrastructure_retries.saturating_add(infra_grants),
    })
}

pub fn work_available(conn: &Connection, run: &str, task: &str) -> Result<bool> {
    let ledger = ledger(conn, run, task)?;
    Ok(ledger.work < ledger.work_limit)
}

pub fn eligible(conn: &Connection, run: &str, task: &str) -> Result<bool> {
    let Some(kind): Option<String> = conn
        .query_row(
            "SELECT next_kind FROM recovery_lineages WHERE run_id=?1 AND task_id=?2",
            [run, task],
            |r| r.get(0),
        )
        .optional()?
    else {
        return Ok(true);
    };
    let ledger = ledger(conn, run, task)?;
    Ok(match kind.as_str() {
        "initial" => ledger.initial == 0,
        "work" => ledger.work < ledger.work_limit,
        "infrastructure" => ledger.infrastructure < ledger.infrastructure_limit,
        _ => false,
    })
}

/// Runs inside the same short transaction as insertion of the launch intent.
pub fn reserve(conn: &Connection, run: &str, task: &str, attempt: &str) -> Result<()> {
    ensure!(
        eligible(conn, run, task)?,
        "task lineage recovery allowance exhausted or blocked"
    );
    let has_commands: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='managed_commands')",
        [],
        |r| r.get(0),
    )?;
    if has_commands {
        let live: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND task_id=?2 AND state IN ('intent','running','stopping','unknown'))",[run,task],|r|r.get(0))?;
        ensure!(
            !live,
            "reconcile owned task commands before worker replacement"
        );
    }
    conn.execute("INSERT INTO recovery_invocations(attempt_id,run_id,task_id,kind,state,source_attempt) SELECT ?3,run_id,task_id,next_kind,'reserved',resume_attempt FROM recovery_lineages WHERE run_id=?1 AND task_id=?2",[run,task,attempt])?;
    Ok(())
}

pub fn dispatched(conn: &Connection, attempt: &str) -> Result<()> {
    super::cooldowns::consumed(conn, attempt)?;
    conn.execute("UPDATE recovery_invocations SET state='dispatched' WHERE attempt_id=?1 AND state IN ('reserved','uncertain')",[attempt])?;
    Ok(())
}

/// Override only controller-managed recovery. Explicit first-invocation resume
/// stays subject to the adapter's exact same-task verification.
pub fn configure(conn: &Connection, run: &str, task: &mut Task) -> Result<()> {
    let Some((kind, previous)): Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT next_kind,resume_attempt FROM recovery_lineages WHERE run_id=?1 AND task_id=?2",
            [run, &task.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
    else {
        return Ok(());
    };
    if kind == "initial" {
        return Ok(());
    }
    let resume = if kind == "infrastructure" {
        previous
    } else {
        None
    };
    match &mut task.executor {
        Executor::Codex { config } => config.resume_attempt = resume,
        Executor::Claude { config } => config.resume_attempt = resume,
        Executor::OpenCode { config } => config.resume_attempt = resume,
        _ => {}
    }
    Ok(())
}

/// Terminal scope proof precedes replacement. Raw bytes remain in attempt logs;
/// classification uses only the adapter's attributable normalized observation.
pub fn after_retire(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    task: &str,
    attempt: &str,
) -> Result<()> {
    let (state, outcome): (String, Option<String>) = store.conn.query_row(
        "SELECT state,outcome FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3",
        [attempt, run, task],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        !matches!(state.as_str(), "running" | "launch_intent" | "uncertain"),
        "retire owned work before recovery"
    );
    if matches!(outcome.as_deref(), Some("completed" | "blocked")) {
        return Ok(());
    }
    let launch_state: Option<String> = store
        .conn
        .query_row(
            "SELECT state FROM recovery_invocations WHERE attempt_id=?1",
            [attempt],
            |r| r.get(0),
        )
        .optional()?;
    if launch_state.as_deref() == Some("prelaunch_failed") {
        return Ok(());
    }
    if live_commands(&store.conn, run, task)? {
        return Ok(());
    }
    let mut kind = Allowance::Work;
    let mut resume = None;
    let mut evidence = json!({"reason":"unfinished work or unknown failure","attempt":attempt});
    let mut blocked = false;
    let mut wait_hint = None;
    for (file, identity) in [
        ("codex-observation.json", "thread_id"),
        ("claude-observation.json", "session_id"),
        ("opencode-observation.json", "session_id"),
    ] {
        let path = workspace.data.join("attempts").join(attempt).join(file);
        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        let mut bytes = Vec::new();
        file.take(128 * 1024).read_to_end(&mut bytes)?;
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if value["task_id"] != task || value["attempt_id"] != attempt {
            continue;
        }
        evidence = json!({"observation":value,"attempt":attempt});
        let hint = serde_json::from_value::<super::cooldowns::Hint>(value["cooldown_hint"].clone())
            .unwrap_or_default();
        match value["failure_class"].as_str() {
            Some("transient_service_or_connection") => {
                kind = Allowance::Infrastructure;
                wait_hint = Some(hint.clone());
                if value[identity].as_str().is_some_and(|s| !s.is_empty()) {
                    resume = Some(attempt);
                }
            }
            Some("quota_reset_unknown") if hint.explicit() => {
                kind = Allowance::Infrastructure;
                wait_hint = Some(hint);
                if value[identity].as_str().is_some_and(|s| !s.is_empty()) {
                    resume = Some(attempt);
                }
            }
            Some("authentication_or_access" | "quota_reset_unknown") => blocked = true,
            _ => {}
        }
        break;
    }
    let tx = store.conn.unchecked_transaction()?;
    // A repeated reconciliation must not create a different decision.
    let recorded: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM journal WHERE run_id=?1 AND task_id=?2 AND attempt_id=?3 AND event='recovery_decided')",[run,task,attempt],|r|r.get(0))?;
    if !recorded {
        tx.execute("UPDATE recovery_lineages SET next_kind=?3,resume_attempt=?4 WHERE run_id=?1 AND task_id=?2",params![run,task,if blocked {"blocked"} else {kind.as_str()},resume])?;
        if let Some(hint) = wait_hint {
            blocked |= super::cooldowns::schedule(
                &tx,
                run,
                task,
                attempt,
                &hint,
                super::admission::now_ms(),
            )?;
        }
        let eligible = !blocked && eligible(&tx, run, task)?;
        tx.execute(
            "UPDATE tasks SET state=?3 WHERE run_id=?1 AND id=?2",
            [
                run,
                task,
                if eligible {
                    "pending"
                } else if blocked {
                    "needs_intervention"
                } else {
                    "retry_exhausted"
                },
            ],
        )?;
        tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'recovery_decided',?4)",params![run,task,attempt,json!({"kind":kind,"blocked":blocked,"resume_attempt":resume,"evidence":evidence}).to_string()])?;
    }
    tx.commit()?;
    super::admission::release(workspace, run, task, attempt)?;
    Ok(())
}

/// Recover the small commit gap between durable retirement and the recovery
/// decision. Only the latest terminal attempt may requeue an unfinished task.
pub fn reconcile(workspace: &Workspace, store: &Store, run: &str) -> Result<()> {
    let mut statement = store.conn.prepare("SELECT a.task_id,a.id FROM attempts a JOIN tasks t ON t.run_id=a.run_id AND t.id=a.task_id WHERE a.run_id=?1 AND a.state IN ('retired','interrupted') AND t.state IN ('failed','needs_intervention','interrupted') AND a.rowid=(SELECT max(b.rowid) FROM attempts b WHERE b.run_id=a.run_id AND b.task_id=a.task_id)")?;
    let rows = statement
        .query_map([run], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (task, attempt) in rows {
        after_retire(workspace, store, run, &task, &attempt)?;
    }
    Ok(())
}

/// An absent pre-spawn record or a confirmed empty, never-bound scope proves
/// prelaunch failure. A bound record proves invocation even if cleanup succeeded.
pub fn launch_failure(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    task: &str,
    attempt: &str,
    error: &str,
) -> Result<bool> {
    let path = workspace
        .data
        .join("attempts")
        .join(attempt)
        .join("custody.json");
    let record = match fs::read(path) {
        Ok(bytes) => Some(
            serde_json::from_slice::<crate::execution::custody::Record>(&bytes)
                .context("unreadable custody launch record")?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let definite = record.as_ref().is_none_or(|r| !r.bound && r.empty);
    let empty = definite
        || crate::execution::custody::reconcile(&workspace.data.join("attempts").join(attempt))
            .unwrap_or(false);
    let tx = store.conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE recovery_invocations SET state=?2,detail=?3 WHERE attempt_id=?1",
        params![
            attempt,
            if definite {
                "prelaunch_failed"
            } else if empty {
                "dispatched"
            } else {
                "uncertain"
            },
            error
        ],
    )?;
    tx.execute(
        "INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,?4,?5)",
        params![
            run,
            task,
            attempt,
            if definite {
                "definite_prelaunch_failure"
            } else if empty {
                "invocation_launch_failure"
            } else {
                "uncertain_launch"
            },
            error
        ],
    )?;
    if definite {
        tx.execute(
            "UPDATE tasks SET state='needs_intervention' WHERE run_id=?1 AND id=?2",
            [run, task],
        )?;
    }
    tx.commit()?;
    Ok(empty)
}

/// Unknown/intent command custody is still live for replacement purposes.
pub fn live_commands(conn: &Connection, run: &str, task: &str) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='managed_commands')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(false);
    }
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND task_id=?2 AND state IN ('intent','running','stopping','unknown'))",[run,task],|r|r.get(0))?)
}
