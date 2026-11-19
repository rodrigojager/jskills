//! Persisted due work, not per-task timers. Scope aliases are explicitly authored,
//! never inferred from credentials or a single provider error.
use crate::{
    domain::recovery::Policy,
    storage::{Store, Workspace},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS recovery_waits(attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),run_id TEXT NOT NULL,task_id TEXT NOT NULL,due_ms INTEGER NOT NULL,source TEXT NOT NULL,reason TEXT NOT NULL,scope TEXT,state TEXT NOT NULL,FOREIGN KEY(run_id,task_id) REFERENCES recovery_lineages(run_id,task_id));
CREATE TABLE IF NOT EXISTS manual_retries(request_id TEXT PRIMARY KEY,run_id TEXT NOT NULL,task_id TEXT NOT NULL,reason TEXT NOT NULL,granted_kind TEXT,consumed_by TEXT,receipt TEXT NOT NULL);
";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub provider: String,
    /// A user supplied account alias, never an API key/token.
    pub account_alias: String,
    pub model: Option<String>,
}

/// Timing normalized by the native adapter from a root failure. Agent prose,
/// helper output, and arbitrary stderr never authorize waits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hint {
    pub reset_at_ms: Option<u64>,
    pub retry_after_ms: Option<u64>,
}
impl Hint {
    /// Native Claude stream contract, not prose or invented generic fields.
    /// https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/message_parser.py
    pub fn claude_rate_limit(value: &Value) -> Self {
        let info = &value["rate_limit_info"];
        if value["type"] != "rate_limit_event"
            || info["status"] != "rejected"
            || matches!(
                info["overageStatus"].as_str(),
                Some("allowed" | "allowed_warning")
            )
        {
            return Self::default();
        }
        Self {
            reset_at_ms: info["resetsAt"].as_u64().and_then(|v| v.checked_mul(1000)),
            retry_after_ms: None,
        }
    }
    pub fn explicit(&self) -> bool {
        self.reset_at_ms.is_some() || self.retry_after_ms.is_some()
    }
}

pub fn due(now: u64, retries: u32, hint: &Hint) -> (u64, &'static str) {
    if let Some(reset) = hint.reset_at_ms {
        return (reset.max(now), "provider_reset");
    }
    if let Some(delay) = hint.retry_after_ms {
        return (now.saturating_add(delay), "retry_after");
    }
    let delay = [10, 20, 40, 80, 120][(retries as usize).min(4)];
    (now.saturating_add(delay * 1000), "connection_backoff")
}

/// Called in the retirement/recovery transaction, after terminal custody proof.
pub fn schedule(
    conn: &Connection,
    run: &str,
    task: &str,
    attempt: &str,
    hint: &Hint,
    now: u64,
) -> Result<bool> {
    let root = super::graph::root(conn, run, task)?;
    let task = root.as_str();
    let policy: String = conn.query_row(
        "SELECT policy FROM recovery_lineages WHERE run_id=?1 AND task_id=?2",
        [run, task],
        |r| r.get(0),
    )?;
    let policy: Policy = serde_json::from_str(&policy)?;
    let retries = super::recovery::ledger(conn, run, task)?.infrastructure;
    let (due_ms, source) = due(now, retries, hint);
    let paused = policy
        .max_wait_seconds
        .is_some_and(|limit| due_ms.saturating_sub(now) > limit.saturating_mul(1000));
    conn.execute("INSERT OR IGNORE INTO recovery_waits(attempt_id,run_id,task_id,due_ms,source,reason,scope,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![attempt,run,task,i64::try_from(due_ms)?,source,"normalized native infrastructure failure",policy.cooldown_scope.map(|s|serde_json::to_string(&s)).transpose()?,if paused {"paused"} else {"waiting"}])?;
    Ok(paused)
}

pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
fn exists(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='recovery_waits')",
        [],
        |r| r.get(0),
    )?)
}

/// Includes matching explicit account/model scopes across tasks of this run only.
pub fn deadline(conn: &Connection, run: &str, task: &str) -> Result<Option<u64>> {
    let root = super::graph::root(conn, run, task)?;
    let task = root.as_str();
    if !exists(conn)? {
        return Ok(None);
    }
    let policy: Option<String> = conn
        .query_row(
            "SELECT policy FROM recovery_lineages WHERE run_id=?1 AND task_id=?2",
            [run, task],
            |r| r.get(0),
        )
        .optional()?;
    let scope = policy
        .map(|v| serde_json::from_str::<Policy>(&v))
        .transpose()?
        .and_then(|p| p.cooldown_scope);
    let mut q = conn.prepare(
        "SELECT task_id,due_ms,scope FROM recovery_waits WHERE run_id=?1 AND state='waiting'",
    )?;
    let mut deadline = None;
    for row in q.query_map([run], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })? {
        let (owner, due, source) = row?;
        let source = source
            .map(|v| serde_json::from_str::<Scope>(&v))
            .transpose()?;
        let covered = source
            .as_ref()
            .zip(scope.as_ref())
            .is_some_and(|(from, to)| {
                from.provider == to.provider
                    && from.account_alias == to.account_alias
                    && (from.model.is_none() || from.model == to.model)
            });
        if owner == task || covered {
            deadline = Some(deadline.unwrap_or(0).max(u64::try_from(due)?));
        }
    }
    Ok(deadline)
}
pub fn eligible(conn: &Connection, run: &str, task: &str, now: u64) -> Result<bool> {
    Ok(deadline(conn, run, task)?.is_none_or(|due| due <= now))
}

pub fn read(conn: &Connection, run: &str, task: &str, now: u64) -> Result<Value> {
    let root = super::graph::root(conn, run, task)?;
    let task = root.as_str();
    if !exists(conn)? {
        return Ok(json!({"state":"not_initialized"}));
    }
    let mut q=conn.prepare("SELECT attempt_id,due_ms,source,reason,scope,state FROM recovery_waits WHERE run_id=?1 AND task_id=?2 ORDER BY rowid DESC LIMIT 1")?;
    let own = q
        .query_row([run, task], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .optional()?;
    if let Some((_, due, _, _, _, _)) = &own {
        ensure!(*due >= 0, "negative persisted cooldown");
    }
    let deadline = deadline(conn, run, task)?;
    Ok(
        json!({"effective_due_ms":deadline,"remaining_ms":deadline.map(|d|d.saturating_sub(now)),"sharing":"within_run_only","latest":own.map(|(attempt,due,source,reason,scope,state)|json!({"attempt":attempt,"due_ms":due,"source":source,"reason":reason,"scope":scope.as_ref().and_then(|s|serde_json::from_str::<Value>(s).ok()),"scope_certainty":if scope.is_some(){"known_explicit_alias"}else{"unknown_task_local"},"state":state}))}),
    )
}

/// The writer lock/controller serializes retries. The request ID is a durable
/// idempotency key; repeated invocations neither replenish nor erase history.
pub fn retry(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    task: &str,
    reason: &str,
    request: &str,
    active: bool,
) -> Result<Value> {
    ensure!(!reason.trim().is_empty(), "manual retry requires a reason");
    crate::domain::validation::identifier(request)?;
    store.check_checkout(run, &workspace.checkout)?;
    let tx = store.conn.unchecked_transaction()?;
    let previous: Option<(String, String, String, String)> = tx
        .query_row(
            "SELECT run_id,task_id,reason,receipt FROM manual_retries WHERE request_id=?1",
            [request],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    if let Some((r, t, why, receipt)) = previous {
        ensure!(
            r == run && t == task && why == reason,
            "conflicting retry request ID"
        );
        return Ok(serde_json::from_str(&receipt)?);
    }
    let state: String = tx.query_row(
        "SELECT state FROM tasks WHERE run_id=?1 AND id=?2",
        [run, task],
        |r| r.get(0),
    )?;
    ensure!(
        !matches!(
            state.as_str(),
            "completed" | "verification_pending" | "superseded"
        ),
        "completed work cannot be retried"
    );
    let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE run_id=?1 AND task_id=?2 AND state IN ('launch_intent','running','uncertain'))",[run,task],|r|r.get(0))?;
    ensure!(
        !live,
        "live or uncertain task attempt must stop and reconcile first"
    );
    let has_commands: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='managed_commands')",
        [],
        |r| r.get(0),
    )?;
    if has_commands {
        let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND task_id=?2 AND state IN ('intent','running','stopping','unknown'))",[run,task],|r|r.get(0))?;
        ensure!(!live, "owned commands must stop and reconcile first");
    }
    let root = super::graph::root(&tx, run, task)?;
    let mut kind = super::recovery::next_kind(&tx, run, task)?;
    if kind == "blocked" {
        kind = "infrastructure".into();
    }
    tx.execute(
        "UPDATE recovery_lineages SET next_kind=?3 WHERE run_id=?1 AND task_id=?2",
        [run, root.as_str(), &kind],
    )?;
    let grant = if !super::recovery::eligible(&tx, run, task)? {
        ensure!(
            matches!(kind.as_str(), "work" | "infrastructure"),
            "initial invocation is already reserved"
        );
        let sql = if kind == "work" {
            "UPDATE recovery_lineages SET work_grants=work_grants+1 WHERE run_id=?1 AND task_id=?2"
        } else {
            "UPDATE recovery_lineages SET infrastructure_grants=infrastructure_grants+1 WHERE run_id=?1 AND task_id=?2"
        };
        tx.execute(sql, [run, root.as_str()])?;
        Some(kind.clone())
    } else {
        None
    };
    tx.execute("UPDATE recovery_waits SET state='manual_advanced' WHERE run_id=?1 AND task_id=?2 AND state IN ('waiting','paused')",[run,root.as_str()])?;
    tx.execute(
        "UPDATE tasks SET state='pending' WHERE run_id=?1 AND id=?2",
        [run, task],
    )?;
    let spec: String = tx.query_row(
        "SELECT spec FROM tasks WHERE run_id=?1 AND id=?2",
        [run, task],
        |r| r.get(0),
    )?;
    let spec: crate::domain::types::Task = serde_json::from_str(&spec)?;
    let mut dependencies = true;
    for dep in &spec.depends_on {
        let completed: bool = tx.query_row(
            "SELECT state='completed' FROM tasks WHERE run_id=?1 AND id=?2",
            [run, dep],
            |r| r.get(0),
        )?;
        dependencies &= completed;
    }
    let wanted = super::admission::resources(workspace, &spec.admission.resources)?;
    let held = crate::resources::registry::list(&super::admission::registry_path(workspace))?;
    let resource_blocked = held.iter().any(|held| {
        wanted.iter().any(|r| {
            r.identity == held.resource.identity
                && (r.mode == crate::resources::registry::Mode::Exclusive
                    || held.resource.mode == crate::resources::registry::Mode::Exclusive)
        })
    });
    let status = if !active {
        "requires_foreground_resume"
    } else if !dependencies {
        "dependency_blocked"
    } else if !eligible(&tx, run, task, super::admission::now_ms())? {
        "cooldown_blocked"
    } else if resource_blocked {
        "resource_blocked"
    } else {
        "queued"
    };
    let receipt = json!({"request_id":request,"run":run,"task":task,"reason":reason,"granted_kind":grant,"status":status,"history_preserved":true});
    tx.execute("INSERT INTO manual_retries(request_id,run_id,task_id,reason,granted_kind,receipt) VALUES(?1,?2,?3,?4,?5,?6)",params![request,run,task,reason,grant,receipt.to_string()])?;
    tx.execute(
        "INSERT INTO journal(run_id,task_id,event,detail) VALUES(?1,?2,'manual_retry',?3)",
        params![run, task, receipt.to_string()],
    )?;
    tx.commit()?;
    Ok(receipt)
}

pub fn consumed(conn: &Connection, attempt: &str) -> Result<()> {
    // A grant becomes consumed only on dispatch; a definite prelaunch failure
    // can reuse it. Invocation rows and grant count still bound uncertain starts.
    let aliases: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_ancestry')",
        [],
        |r| r.get(0),
    )?;
    let sql = if aliases {
        "UPDATE manual_retries SET consumed_by=?1 WHERE request_id=(SELECT m.request_id FROM manual_retries m JOIN recovery_invocations i ON i.run_id=m.run_id AND i.task_id=COALESCE((SELECT root_id FROM graph_ancestry g WHERE g.run_id=m.run_id AND g.task_id=m.task_id),m.task_id) AND i.kind=m.granted_kind WHERE i.attempt_id=?1 AND m.consumed_by IS NULL ORDER BY m.rowid LIMIT 1)"
    } else {
        "UPDATE manual_retries SET consumed_by=?1 WHERE request_id=(SELECT m.request_id FROM manual_retries m JOIN recovery_invocations i ON i.run_id=m.run_id AND i.task_id=m.task_id AND i.kind=m.granted_kind WHERE i.attempt_id=?1 AND m.consumed_by IS NULL ORDER BY m.rowid LIMIT 1)"
    };
    conn.execute(sql, [attempt])?;
    Ok(())
}
