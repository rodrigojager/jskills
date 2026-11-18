//! Fresh, finite observers, consumed by the existing foreground due-work scan.
//! No reviewer owns a result token or a resource/dispatch control capability.
use super::due::{self, Checker, Kind, Work};
use crate::{
    domain::{
        reviews::{Assessment, Decision, Observer, Policy},
        types::Task,
    },
    execution::custody::OwnedProcess,
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

pub const MAX_EVIDENCE_BYTES: u64 = 16 * 1024;
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;
pub const DEFAULT_CONCURRENCY: usize = 2;

pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS execution_reviews(attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),run_id TEXT NOT NULL,task_id TEXT NOT NULL,policy TEXT NOT NULL,estimate_seconds INTEGER,state TEXT NOT NULL,generation INTEGER NOT NULL DEFAULT 1,infrastructure_retries INTEGER NOT NULL DEFAULT 0,action TEXT,reason TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS reviewer_calls(id TEXT PRIMARY KEY,run_id TEXT NOT NULL,task_id TEXT NOT NULL,attempt_id TEXT NOT NULL REFERENCES execution_reviews(attempt_id),generation INTEGER NOT NULL,state TEXT NOT NULL,invoked INTEGER NOT NULL DEFAULT 0,started_ms INTEGER,deadline_ms INTEGER,session TEXT,evidence TEXT NOT NULL,response TEXT,detail TEXT NOT NULL DEFAULT '');
CREATE UNIQUE INDEX IF NOT EXISTS one_live_reviewer ON reviewer_calls(attempt_id) WHERE state IN ('intent','running','stopping','unknown');
CREATE TRIGGER IF NOT EXISTS immutable_reviewer_identity BEFORE UPDATE OF id,run_id,task_id,attempt_id,generation,evidence ON reviewer_calls BEGIN SELECT RAISE(ABORT,'review identity and evidence are immutable'); END;
CREATE TABLE IF NOT EXISTS reviewer_selections(review_id TEXT PRIMARY KEY REFERENCES reviewer_calls(id),selection TEXT NOT NULL);
CREATE TRIGGER IF NOT EXISTS immutable_reviewer_selection BEFORE UPDATE ON reviewer_selections BEGIN SELECT RAISE(ABORT,'review observer selection is immutable'); END;")?;
    Ok(())
}

/// The observer a due review launches: a direct observer as written, or an
/// alias resolved from the project's aliases at this moment.
pub struct Selection {
    pub observer: Observer,
    pub record: Value,
}

/// Resolve the policy's observer now. An alias uses the run's profile defaults
/// and refuses an executor the observer does not support.
pub fn resolve_observer(
    workspace: &Workspace,
    conn: &Connection,
    run: &str,
    policy: &Policy,
) -> Result<Selection> {
    let observer = policy
        .observer
        .as_ref()
        .context("execution observer is unconfigured")?;
    let Observer::Profile { alias, overrides } = observer else {
        let native = serde_json::to_value(observer)?;
        let mut values = native.clone();
        let mut origins = native
            .as_object()
            .context("observer selection must be an object")?
            .keys()
            .map(|key| (key.clone(), "observer".into()))
            .collect();
        crate::domain::profiles::view_selection(&mut values, &mut origins, &native);
        return Ok(Selection {
            observer: observer.clone(),
            record: json!({"form":"direct","alias":null,"observer":native,"values":values,"origins":origins}),
        });
    };
    let plan: Option<String> = conn
        .query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))
        .optional()?;
    let defaults = match plan
        .and_then(|plan| serde_json::from_str::<Value>(&plan).ok())
        .and_then(|mut plan| plan.get_mut("profile_defaults").map(Value::take))
    {
        Some(defaults) => serde_json::from_value(defaults)?,
        None => crate::domain::profiles::ProfileConfig::default(),
    };
    let (value, effective) =
        crate::domain::profiles::resolve_observer(workspace, alias, overrides, &defaults)?;
    let resolved: Observer = serde_json::from_value(value)?;
    Ok(Selection {
        record: json!({"form":"alias","alias":alias,"observer":resolved,"values":effective.values,"origins":effective.origins}),
        observer: resolved,
    })
}

fn review_work(run: &str, task: &str, attempt: &str, generation: u32, due_ms: u64) -> Work {
    Work {
        id: format!("review-{attempt}"),
        run: run.into(),
        task: Some(task.into()),
        attempt: Some(attempt.into()),
        kind: Kind::Review,
        generation,
        due_ms,
    }
}

/// In the launch transaction, alongside the actual attempt start observation.
pub fn begin(conn: &Connection, attempt: &str, start_ms: u64) -> Result<()> {
    let (run, task, spec): (String,String,String) = conn.query_row("SELECT a.run_id,a.task_id,t.spec FROM attempts a JOIN tasks t ON t.run_id=a.run_id AND t.id=a.task_id WHERE a.id=?1",[attempt],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let spec: Task = serde_json::from_str(&spec)?;
    let changed = conn.execute("INSERT OR IGNORE INTO execution_reviews(attempt_id,run_id,task_id,policy,estimate_seconds,state) VALUES(?1,?2,?3,?4,?5,?6)",params![attempt,run,task,serde_json::to_string(&spec.reviews)?,spec.estimate_seconds.map(i64::try_from).transpose()?,if spec.estimate_seconds.is_some(){"scheduled"}else{"unconfigured"}])?;
    if changed > 0 {
        if let Some(due_ms) = crate::domain::reviews::first_due(start_ms, spec.estimate_seconds) {
            due::schedule(conn, &review_work(&run, &task, attempt, 1, due_ms))?;
        }
    }
    Ok(())
}

pub fn projection(conn: &Connection, attempt: &str) -> Result<Value> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='execution_reviews')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(json!({"state":"not_initialized"}));
    }
    let row: Option<(String,Option<i64>,String,u32,u32,Option<String>,String)> = conn.query_row("SELECT state,estimate_seconds,policy,generation,infrastructure_retries,action,reason FROM execution_reviews WHERE attempt_id=?1",[attempt],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    let Some((state, estimate, policy, generation, retries, action, reason)) = row else {
        return Ok(json!({"state":"not_initialized"}));
    };
    let estimate = estimate.map(u64::try_from).transpose()?;
    let policy: Policy = serde_json::from_str(&policy)?;
    let calls: u32 = conn.query_row(
        "SELECT count(*) FROM reviewer_calls WHERE attempt_id=?1 AND invoked=1",
        [attempt],
        |r| r.get(0),
    )?;
    let last: Option<String> = conn
        .query_row(
            "SELECT response FROM reviewer_calls WHERE attempt_id=?1 ORDER BY rowid DESC LIMIT 1",
            [attempt],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(
        json!({"state":state,"estimate_seconds":estimate,"estimate_trigger":if estimate.is_some(){"estimate_times_1_5"}else{"unconfigured"},"worker_hard_timeout":null,"generation":generation,"actual_calls":calls,"max_calls":policy.max_calls,"infrastructure_retries":retries,"observer_configured":policy.observer.is_some(),"action":action,"reason":reason,"last_review":last.and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
    )
}

fn current(conn: &Connection, work: &Work) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM execution_reviews r JOIN attempts a ON a.id=r.attempt_id WHERE r.attempt_id=?1 AND r.run_id=?2 AND r.task_id=?3 AND r.generation=?4 AND r.action IS NULL AND a.state='running' AND a.outcome IS NULL AND a.rowid=(SELECT max(b.rowid) FROM attempts b WHERE b.run_id=a.run_id AND b.task_id=a.task_id))",params![work.attempt,work.run,work.task,work.generation],|r|r.get(0))?)
}

/// Stops are requests, never declarations of worker death or source rollback.
fn request_action(conn: &Connection, work: &Work, action: &str, reason: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if current(&tx, work)? {
        tx.execute(
            "UPDATE execution_reviews SET state='stopping',action=?2,reason=?3 WHERE attempt_id=?1",
            params![work.attempt, action, reason],
        )?;
        due::cancel(&tx, &work.id)?;
        tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'review_stop_requested',?4)",params![work.run,work.task,work.attempt,json!({"action":action,"reason":reason}).to_string()])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn stop_requested(conn: &Connection, attempt: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM execution_reviews r JOIN attempts a ON a.id=r.attempt_id WHERE r.attempt_id=?1 AND r.action IS NOT NULL AND a.outcome IS NULL)",[attempt],|r|r.get(0))?)
}

/// Called before normal retirement recovery, and again after owned commands end.
/// Completion wins even when its receipt arrives while stopping is in flight.
pub fn reconcile_actions(workspace: &Workspace, store: &Store, run: &str) -> Result<()> {
    let rows = {
        let mut q=store.conn.prepare("SELECT r.task_id,r.attempt_id,r.action,a.state,a.outcome FROM execution_reviews r JOIN attempts a ON a.id=r.attempt_id WHERE r.run_id=?1 AND r.action IS NOT NULL")?;
        q.query_map([run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (task, attempt, action, state, outcome) in rows {
        if outcome.is_some() {
            store.conn.execute("UPDATE execution_reviews SET state='obsolete',action=NULL,reason='accepted result invalidated review action' WHERE attempt_id=?1",[&attempt])?;
            continue;
        }
        let unknown: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM reviewer_calls WHERE attempt_id=?1 AND state='unknown')",
            [&attempt],
            |r| r.get(0),
        )?;
        if unknown {
            continue;
        }
        if !matches!(state.as_str(), "retired" | "interrupted")
            || super::recovery::live_commands(&store.conn, run, &task)?
        {
            continue;
        }
        if action == "pause" {
            let tx = store.conn.unchecked_transaction()?;
            tx.execute("UPDATE recovery_lineages SET next_kind='blocked',resume_attempt=NULL WHERE run_id=?1 AND task_id=?2",[run,&task])?;
            tx.execute(
                "UPDATE tasks SET state='needs_intervention' WHERE run_id=?1 AND id=?2",
                [run, &task],
            )?;
            tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) SELECT ?1,?2,?3,'recovery_decided','review exhausted/inconclusive; stopped custody confirmed; edits preserved' WHERE NOT EXISTS(SELECT 1 FROM journal WHERE run_id=?1 AND task_id=?2 AND attempt_id=?3 AND event='recovery_decided')",[run,&task,&attempt])?;
            tx.execute(
                "UPDATE execution_reviews SET state='paused',action=NULL WHERE attempt_id=?1",
                [&attempt],
            )?;
            tx.commit()?;
            super::admission::release(workspace, run, &task, &attempt)?;
        } else {
            // Existing work allowance and cooldown logic is the only replacement path.
            super::recovery::after_retire(workspace, store, run, &task, &attempt)?;
            store.conn.execute("UPDATE execution_reviews SET state='retry_requested',action=NULL WHERE attempt_id=?1",[&attempt])?;
        }
    }
    Ok(())
}

/// Startup is a custody barrier. An interrupted observer never gets a new cap.
pub fn reconcile(workspace: &Workspace, store: &Store, run: &str) -> Result<()> {
    let calls = {
        let mut q=store.conn.prepare("SELECT id,attempt_id FROM reviewer_calls WHERE run_id=?1 AND state IN ('intent','running','stopping','unknown')")?;
        q.query_map([run], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, attempt) in calls {
        let dir = workspace.data.join("reviews").join(&id);
        // The intent first owns a diff-capture scope; observer launch is later.
        // Missing observer custody cannot prove the snapshot's scope empty.
        let mut empty = true;
        for scope in [&dir, &dir.join("snapshot")] {
            match fs::metadata(scope.join("custody.json")) {
                Ok(_) => empty &= crate::execution::custody::reconcile(scope).unwrap_or(false),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => empty = false,
            }
        }
        let bound = fs::read(dir.join("custody.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<crate::execution::custody::Record>(&b).ok())
            .is_some_and(|r| r.bound);
        if bound {
            store
                .conn
                .execute("UPDATE reviewer_calls SET invoked=1 WHERE id=?1", [&id])?;
        }
        store.conn.execute("UPDATE reviewer_calls SET state=?2,detail='controller gap; no timing or invocation allowance reconstructed' WHERE id=?1",params![id,if empty {"failed"}else{"unknown"}])?;
        store.conn.execute("UPDATE execution_reviews SET state='stopping',action='pause',reason=?2 WHERE attempt_id=?1",params![attempt,if empty {"reviewer interrupted by controller gap"}else{"reviewer custody unknown; ownership retained"}])?;
    }
    // A worker stopped during startup cannot silently get a new review allowance.
    store.conn.execute("UPDATE execution_reviews SET state='stopping',action='pause',reason='worker timing/custody gap requires intervention' WHERE run_id=?1 AND state IN ('scheduled','running') AND EXISTS(SELECT 1 FROM attempts a WHERE a.id=execution_reviews.attempt_id AND a.outcome IS NULL AND a.state IN ('retired','interrupted'))",[run])?;
    // Unknown reviewer scope forbids task replacement even if the worker ended.
    store.conn.execute("UPDATE recovery_lineages SET next_kind='blocked' WHERE run_id=?1 AND task_id IN (SELECT task_id FROM reviewer_calls WHERE run_id=?1 AND state='unknown')",[run])?;
    reconcile_actions(workspace, store, run)
}

struct Live {
    work: Work,
    id: String,
    observer: Observer,
    session: String,
    child: OwnedProcess,
    stopping: Option<String>,
    deadline_ms: u64,
    snapshot: bool,
    evidence: Value,
    budget: Option<Instant>,
}
pub struct Service {
    live: Vec<Live>,
    capacity: usize,
    budgets: HashMap<(String, u32), (Instant, u64)>,
}
impl Default for Service {
    fn default() -> Self {
        Self {
            live: Vec::new(),
            capacity: DEFAULT_CONCURRENCY,
            budgets: HashMap::new(),
        }
    }
}
impl Service {
    pub fn configured() -> Result<Self> {
        let capacity = std::env::var("JTASKS_REVIEW_CONCURRENCY")
            .ok()
            .map(|s| s.parse::<usize>())
            .transpose()?
            .unwrap_or(DEFAULT_CONCURRENCY);
        ensure!(
            (1..=32).contains(&capacity),
            "JTASKS_REVIEW_CONCURRENCY must be 1..32"
        );
        Ok(Self {
            live: Vec::new(),
            capacity,
            budgets: HashMap::new(),
        })
    }
    pub fn has_live(&self) -> bool {
        !self.live.is_empty()
    }
    pub fn stop_all(&mut self) -> Result<()> {
        for live in &mut self.live {
            live.stopping = Some("controller stopped".into());
            live.child.request_stop()?;
        }
        Ok(())
    }
    /// Only called with snapshots from the single instance checker. No wait here.
    pub fn consume(
        &mut self,
        workspace: &Workspace,
        store: &Store,
        run: &str,
        queued: &[Work],
        checker: &Checker,
        _clocks: &mut super::clock::Clock,
    ) -> Result<()> {
        for work in queued {
            if work.kind == Kind::ReviewerDeadline
                || (work.kind == Kind::CommandDeadline && work.id.starts_with("snapshot-deadline-"))
            {
                if checker.admit(&store.conn, work)?.is_some() {
                    if let Some(live) = self.live.iter_mut().find(|l| {
                        format!("deadline-{}", l.id) == work.id
                            || format!("snapshot-deadline-{}", l.id) == work.id
                    }) {
                        live.stopping = Some("reviewer finite deadline elapsed".into());
                        live.child.request_stop()?;
                    }
                    due::complete(&store.conn, &work.id, work.generation)?;
                }
                continue;
            }
            if work.kind != Kind::Review || self.live.len() >= self.capacity {
                continue;
            }
            let Some(queue_delay_ms) = checker.admit(&store.conn, work)? else {
                continue;
            };
            if !current(&store.conn, work)? {
                due::cancel(&store.conn, &work.id)?;
                continue;
            }
            let policy = policy(&store.conn, work.attempt.as_deref().unwrap())?;
            let count = call_count(&store.conn, work.attempt.as_deref().unwrap())?;
            if count >= policy.max_calls {
                request_action(
                    &store.conn,
                    work,
                    "pause",
                    "reviewer actual-call cap exhausted",
                )?;
                continue;
            }
            if policy.observer.is_none() {
                request_action(
                    &store.conn,
                    work,
                    "pause",
                    "estimate due but execution observer is unconfigured",
                )?;
                continue;
            }
            // An alias is resolved here, when the review is due, never at import.
            let selection = resolve_observer(workspace, &store.conn, run, &policy).and_then(|s| {
                validate_resolved(workspace, &policy, &s.observer)?;
                Ok(s)
            });
            let selection = match selection {
                Ok(selection) => selection,
                Err(error) => {
                    request_action(
                        &store.conn,
                        work,
                        "pause",
                        &format!("observer selection rejected: {error:#}"),
                    )?;
                    continue;
                }
            };
            let observer = selection.observer;
            let id = uuid::Uuid::new_v4().to_string();
            let session = uuid::Uuid::new_v4().to_string();
            let mut evidence = evidence(workspace, &store.conn, work)?;
            evidence["queue_delay_ms"] = json!(queue_delay_ms);
            // A retry shares the original group's finite deadline, not a reset.
            let inherited=store.conn.query_row("SELECT deadline_ms FROM reviewer_calls WHERE attempt_id=?1 AND generation=?2 ORDER BY rowid DESC LIMIT 1",params![work.attempt,work.generation],|r|r.get::<_,Option<i64>>(0)).optional()?.flatten().map(u64::try_from).transpose()?;
            let now = super::clock::utc_ms();
            let budget = self
                .budgets
                .get(&(work.attempt.clone().unwrap(), work.generation))
                .copied();
            if budget
                .map(|(d, _)| Instant::now() >= d)
                .unwrap_or_else(|| inherited.is_some_and(|d| d <= now))
            {
                request_action(
                    &store.conn,
                    work,
                    "pause",
                    "review infrastructure retry deadline exhausted",
                )?;
                continue;
            }
            store.conn.execute("INSERT INTO reviewer_calls(id,run_id,task_id,attempt_id,generation,state,session,evidence,deadline_ms) VALUES(?1,?2,?3,?4,?5,'intent',?6,?7,?8)",params![id,work.run,work.task,work.attempt,work.generation,session,serde_json::to_string(&evidence)?,inherited.map(i64::try_from).transpose()?])?;
            store.conn.execute(
                "INSERT INTO reviewer_selections(review_id,selection) VALUES(?1,?2)",
                params![id, selection.record.to_string()],
            )?;
            let dir = workspace.data.join("reviews").join(&id);
            let launched = launch_snapshot(workspace, &dir);
            match launched {
                Ok(child) => {
                    self.live.push(Live {
                        work: work.clone(),
                        id: id.clone(),
                        observer,
                        session,
                        child,
                        stopping: None,
                        deadline_ms: inherited.unwrap_or(0),
                        snapshot: true,
                        evidence,
                        budget: budget.map(|b| b.0),
                    });
                    due::schedule(
                        &store.conn,
                        &Work {
                            id: format!("snapshot-deadline-{id}"),
                            run: run.into(),
                            task: work.task.clone(),
                            attempt: work.attempt.clone(),
                            kind: Kind::CommandDeadline,
                            generation: work.generation,
                            due_ms: now.saturating_add(5000),
                        },
                    )?;
                }
                Err(error) => {
                    let invoked = false;
                    let empty = scope_empty(&dir.join("snapshot"));
                    store.conn.execute(
                        "UPDATE reviewer_calls SET state=?2,invoked=?3,detail=?4 WHERE id=?1",
                        params![
                            id,
                            if empty { "failed" } else { "unknown" },
                            invoked,
                            format!("{error:#}")
                        ],
                    )?;
                    request_action(
                        &store.conn,
                        work,
                        "pause",
                        if empty {
                            "observer launch failed; no fallback"
                        } else {
                            "observer launch custody unknown; preserve ownership"
                        },
                    )?;
                }
            }
        }
        Ok(())
    }
    /// I/O observation is asynchronous with worker and managed-command handling.
    pub fn poll(
        &mut self,
        workspace: &Workspace,
        store: &Store,
        run: &str,
        clocks: &mut super::clock::Clock,
    ) -> Result<()> {
        let mut retired = Vec::new();
        for live in &mut self.live {
            let current = current(&store.conn, &live.work)?;
            if !current && live.stopping.is_none() {
                live.stopping = Some("stale review: current result/generation changed".into());
                live.child.request_stop()?;
            }
            let observed = live.child.poll();
            let exit = match observed {
                Ok(exit) => exit,
                Err(error) => {
                    store.conn.execute(
                        "UPDATE reviewer_calls SET state='unknown',detail=?2 WHERE id=?1",
                        params![live.id, error.to_string()],
                    )?;
                    request_action(
                        &store.conn,
                        &live.work,
                        "pause",
                        "reviewer cleanup unknown; no replacement",
                    )?;
                    return Err(error);
                }
            };
            let Some(exit) = exit else {
                continue;
            };
            if live.snapshot {
                due::cancel(&store.conn, &format!("snapshot-deadline-{}", live.id))?;
                let dir = workspace.data.join("reviews").join(&live.id);
                if !current {
                    store.conn.execute("UPDATE reviewer_calls SET state='stale',detail='snapshot obsolete; cleanup confirmed' WHERE id=?1",[&live.id])?;
                    retired.push((live.id.clone(), live.work.task.clone().unwrap()));
                    continue;
                }
                let remaining = live
                    .budget
                    .map(|d| d.saturating_duration_since(Instant::now()));
                if live.stopping.is_some()
                    || exit != Some(0)
                    || remaining.is_some_and(|d| d.is_zero())
                {
                    store.conn.execute("UPDATE reviewer_calls SET state='failed',detail='bounded diff snapshot failed or deadline exhausted; no observer call' WHERE id=?1",[&live.id])?;
                    request_action(
                        &store.conn,
                        &live.work,
                        "pause",
                        "bounded observer evidence unavailable",
                    )?;
                    retired.push((live.id.clone(), live.work.task.clone().unwrap()));
                    continue;
                }
                let policy = policy(&store.conn, live.work.attempt.as_deref().unwrap())?;
                let selected = validate_resolved(workspace, &policy, &live.observer);
                if let Err(error) = selected {
                    store.conn.execute(
                        "UPDATE reviewer_calls SET state='failed',detail=?2 WHERE id=?1",
                        params![live.id, error.to_string()],
                    )?;
                    request_action(
                        &store.conn,
                        &live.work,
                        "pause",
                        "observer selection changed or rejected before invocation",
                    )?;
                    retired.push((live.id.clone(), live.work.task.clone().unwrap()));
                    continue;
                }
                fs::write(dir.join("selection.json"), serde_json::to_vec(&selected?)?)?;
                let launched = launch(
                    workspace,
                    &dir,
                    &live.work,
                    &live.id,
                    &live.session,
                    &live.observer,
                    &live.evidence,
                );
                match launched {
                    Ok(child) => {
                        live.child = child;
                        live.snapshot = false;
                        let now = super::clock::utc_ms();
                        let budget = live.budget.unwrap_or_else(|| {
                            Instant::now() + Duration::from_secs(policy.deadline_seconds)
                        });
                        let remaining = budget.saturating_duration_since(Instant::now());
                        if live.deadline_ms == 0 {
                            live.deadline_ms = now.saturating_add(policy.deadline_seconds * 1000);
                        }
                        live.budget = Some(budget);
                        self.budgets.insert(
                            (live.work.attempt.clone().unwrap(), live.work.generation),
                            (budget, live.deadline_ms),
                        );
                        store.conn.execute("UPDATE reviewer_calls SET state='running',invoked=1,started_ms=?2,deadline_ms=?3 WHERE id=?1",params![live.id,i64::try_from(now)?,i64::try_from(live.deadline_ms)?])?;
                        store.conn.execute(
                            "UPDATE execution_reviews SET state='running' WHERE attempt_id=?1",
                            [&live.work.attempt],
                        )?;
                        due::schedule(
                            &store.conn,
                            &Work {
                                id: format!("deadline-{}", live.id),
                                run: run.into(),
                                task: live.work.task.clone(),
                                attempt: live.work.attempt.clone(),
                                kind: Kind::ReviewerDeadline,
                                generation: live.work.generation,
                                due_ms: now.saturating_add(remaining.as_millis() as u64),
                            },
                        )?;
                        clocks.checkpoint(&store.conn, run)?;
                        clocks.set_reviewing(live.work.task.as_deref().unwrap(), true);
                        clocks.checkpoint(&store.conn, run)?;
                        continue;
                    }
                    Err(error) => {
                        let record = fs::read(dir.join("custody.json")).ok().and_then(|b| {
                            serde_json::from_slice::<crate::execution::custody::Record>(&b).ok()
                        });
                        let empty = scope_empty(&dir);
                        store.conn.execute(
                            "UPDATE reviewer_calls SET state=?2,invoked=?3,detail=?4 WHERE id=?1",
                            params![
                                live.id,
                                if empty { "failed" } else { "unknown" },
                                record.is_some_and(|r| r.bound),
                                error.to_string()
                            ],
                        )?;
                        request_action(
                            &store.conn,
                            &live.work,
                            "pause",
                            "observer launch failed or custody unknown; no fallback",
                        )?;
                        retired.push((live.id.clone(), live.work.task.clone().unwrap()));
                        continue;
                    }
                }
            }
            due::cancel(&store.conn, &format!("deadline-{}", live.id))?;
            if !current {
                store.conn.execute("UPDATE reviewer_calls SET state='stale',detail='completion or generation invalidated observer; cleanup confirmed' WHERE id=?1",[&live.id])?;
                due::complete(&store.conn, &live.work.id, live.work.generation)?;
            } else {
                let parsed = if live.stopping.is_some() {
                    Err(anyhow::anyhow!(live.stopping.clone().unwrap()))
                } else {
                    response(
                        &workspace.data.join("reviews").join(&live.id),
                        live,
                        exit == Some(0),
                    )
                };
                let (assessment, infra, detail) = match parsed {
                    Ok((assessment, infra, detail)) => (assessment, infra, detail),
                    Err(error) => (None, false, format!("{error:#}")),
                };
                let now = super::clock::utc_ms();
                // Validation uses the continuous monotonic budget. New due work
                // is relative to actual accepted UTC, not an old wall anchor.
                let effective_deadline = live
                    .budget
                    .map(|d| {
                        if Instant::now() >= d {
                            now.saturating_sub(1)
                        } else {
                            now.saturating_add(
                                d.saturating_duration_since(Instant::now()).as_millis() as u64,
                            )
                        }
                    })
                    .unwrap_or(live.deadline_ms);
                finish_call(
                    &store.conn,
                    &live.work,
                    &live.id,
                    assessment.as_ref(),
                    infra,
                    effective_deadline,
                    now,
                    &detail,
                )?;
            }
            retired.push((live.id.clone(), live.work.task.clone().unwrap()));
        }
        for (id, task) in retired {
            self.live.retain(|l| l.id != id);
            clocks.checkpoint(&store.conn, run)?;
            clocks.set_reviewing(&task, false);
            clocks.checkpoint(&store.conn, run)?;
        }
        reconcile_actions(workspace, store, run)
    }
}

fn call_count(conn: &Connection, attempt: &str) -> Result<u32> {
    // Intent/unknown entries reserve allowance across a crash until reconciled.
    Ok(conn.query_row("SELECT count(*) FROM reviewer_calls WHERE attempt_id=?1 AND (invoked=1 OR state IN ('intent','unknown'))",[attempt],|r|r.get(0))?)
}
fn policy(conn: &Connection, attempt: &str) -> Result<Policy> {
    let value: String = conn.query_row(
        "SELECT policy FROM execution_reviews WHERE attempt_id=?1",
        [attempt],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&value)?)
}

/// Catalog reads never discover accounts or create/refresh metadata. The
/// observer has its own selectors, installation scope and explicit exception.
/// An alias resolves from the project's aliases without run defaults here.
pub fn validate_observer(
    workspace: &Workspace,
    policy: &Policy,
) -> Result<crate::executors::catalog::Validation> {
    let observer = policy
        .observer
        .as_ref()
        .context("execution observer is unconfigured")?;
    let observer = match observer {
        Observer::Profile { alias, overrides } => {
            let (value, _) = crate::domain::profiles::resolve_observer(
                workspace,
                alias,
                overrides,
                &Default::default(),
            )?;
            serde_json::from_value(value)?
        }
        direct => direct.clone(),
    };
    validate_resolved(workspace, policy, &observer)
}

/// The capability exception applies to resolved alias values as to direct ones.
pub fn validate_resolved(
    workspace: &Workspace,
    policy: &Policy,
    observer: &Observer,
) -> Result<crate::executors::catalog::Validation> {
    ensure!(
        !policy.observer_catalog_scope.trim().is_empty(),
        "observer catalog scope must be nonempty"
    );
    let (kind, mut values) = match observer {
        Observer::Codex { config } => {
            ensure!(
                config.resume_attempt.is_none(),
                "observer must use a fresh session"
            );
            ("codex", serde_json::to_value(config)?)
        }
        Observer::Claude { config } => {
            ensure!(
                config.resume_attempt.is_none() && config.native_agent.is_none(),
                "observer requires a fresh session without a native developer agent"
            );
            ("claude", serde_json::to_value(config)?)
        }
        Observer::OpenCode { config } => {
            ensure!(
                config.resume_attempt.is_none(),
                "observer must use a fresh session"
            );
            crate::executors::opencode::validate(config)?;
            ("opencode", serde_json::to_value(config)?)
        }
        Observer::Profile { alias, .. } => {
            anyhow::bail!("execution observer alias {alias} must be resolved before validation")
        }
    };
    if let Some(effort) = values.get("reasoning_effort").cloned() {
        values["effort"] = effort;
    }
    crate::executors::catalog::validate_values(
        workspace,
        kind,
        &values,
        &policy.observer_catalog_scope,
        policy.observer_unknown_capabilities.as_ref(),
    )
}

/// Atomic provenance/current-generation validation and finite decision action.
/// Exposed for deterministic synthetic-clock coverage; only controller calls it.
pub fn finish_call(
    conn: &Connection,
    work: &Work,
    id: &str,
    assessment: Option<&Assessment>,
    infrastructure_failure: bool,
    deadline_ms: u64,
    now_ms: u64,
    detail: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let bound:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM reviewer_calls WHERE id=?1 AND run_id=?2 AND task_id=?3 AND attempt_id=?4 AND generation=?5 AND state IN ('running','stopping'))",params![id,work.run,work.task,work.attempt,work.generation],|r|r.get(0))?;
    ensure!(bound, "review response lacks invocation provenance");
    if !current(&tx, work)? {
        tx.execute(
            "UPDATE reviewer_calls SET state='stale',response=?2,detail=?3 WHERE id=?1",
            params![
                id,
                assessment.map(serde_json::to_string).transpose()?,
                detail
            ],
        )?;
        tx.commit()?;
        return Ok(());
    }
    let policy = policy(&tx, work.attempt.as_deref().unwrap())?;
    let valid = assessment.filter(|a| {
        a.run == work.run
            && Some(&a.task) == work.task.as_ref()
            && Some(&a.attempt) == work.attempt.as_ref()
            && a.review == id
            && a.generation == work.generation
            && a.validate(&policy).is_ok()
            && now_ms <= deadline_ms
    });
    tx.execute(
        "UPDATE reviewer_calls SET state=?2,response=?3,detail=?4 WHERE id=?1",
        params![
            id,
            if valid.is_some() {
                "finished"
            } else {
                "failed"
            },
            assessment.map(serde_json::to_string).transpose()?,
            detail
        ],
    )?;
    due::complete(&tx, &work.id, work.generation)?;
    let count = call_count(&tx, work.attempt.as_deref().unwrap())?;
    if let Some(assessment) = valid {
        match assessment.decision {
            Decision::Continue if count < policy.max_calls => {
                let next = work
                    .generation
                    .checked_add(1)
                    .context("review generation exhausted")?;
                tx.execute("UPDATE execution_reviews SET state='scheduled',generation=?2 WHERE attempt_id=?1",params![work.attempt,next])?;
                due::schedule(
                    &tx,
                    &review_work(
                        &work.run,
                        work.task.as_deref().unwrap(),
                        work.attempt.as_deref().unwrap(),
                        next,
                        now_ms.saturating_add(assessment.next_review_after_seconds.unwrap() * 1000),
                    ),
                )?;
            }
            Decision::Retry => set_action(
                &tx,
                work,
                "retry",
                "observer requested normal bounded replacement",
            )?,
            Decision::Continue => set_action(
                &tx,
                work,
                "pause",
                "last allowed observer call requested more time",
            )?,
            Decision::Inconclusive => {
                set_action(&tx, work, "pause", "observer assessment inconclusive")?
            }
        }
    } else {
        let retried: u32 = tx.query_row(
            "SELECT infrastructure_retries FROM execution_reviews WHERE attempt_id=?1",
            [&work.attempt],
            |r| r.get(0),
        )?;
        if infrastructure_failure
            && retried == 0
            && count < policy.max_calls
            && now_ms < deadline_ms
        {
            tx.execute("UPDATE execution_reviews SET infrastructure_retries=infrastructure_retries+1,state='scheduled' WHERE attempt_id=?1",[&work.attempt])?;
            // Same generation/group deadline; schedule requires generation advance,
            // so requeue this already-admitted immutable work explicitly.
            tx.execute("UPDATE due_work SET state='queued',queued_ms=?2,admitted_ms=NULL WHERE id=?1 AND generation=?3",params![work.id,i64::try_from(now_ms)?,work.generation])?;
        } else {
            set_action(
                &tx,
                work,
                "pause",
                "observer failed, invalid, late or infrastructure allowance exhausted",
            )?;
        }
    }
    let observer = tx
        .query_row(
            "SELECT selection FROM reviewer_selections WHERE review_id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'execution_review',?4)",params![work.run,work.task,work.attempt,json!({"review":id,"generation":work.generation,"actual_calls":count,"valid":valid.is_some(),"detail":detail,"observer":observer}).to_string()])?;
    tx.commit()?;
    Ok(())
}
fn set_action(conn: &Connection, work: &Work, action: &str, reason: &str) -> Result<()> {
    conn.execute(
        "UPDATE execution_reviews SET state='stopping',action=?2,reason=?3 WHERE attempt_id=?1",
        params![work.attempt, action, reason],
    )?;
    Ok(())
}

fn tail(path: &Path, limit: u64) -> Result<Value> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"available":false}));
        }
        Err(e) => return Err(e.into()),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(limit);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(
        json!({"available":true,"offset":start,"total_bytes":len,"truncated":start>0,"text":String::from_utf8_lossy(&bytes)}),
    )
}

/// Bounded immutable evidence snapshot. Local Git output is piped to a file
/// under the review directory by launch; no source files are ever changed.
fn scope_empty(scope: &Path) -> bool {
    match fs::metadata(scope.join("custody.json")) {
        Ok(_) => crate::execution::custody::reconcile(scope).unwrap_or(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

fn evidence(workspace: &Workspace, conn: &Connection, work: &Work) -> Result<Value> {
    let attempt = work.attempt.as_deref().unwrap();
    let spec: String = conn.query_row(
        "SELECT spec FROM tasks WHERE run_id=?1 AND id=?2",
        params![work.run, work.task],
        |r| r.get(0),
    )?;
    let task: Task = serde_json::from_str(&spec)?;
    let dir = workspace.data.join("attempts").join(attempt);
    let process: (String, Option<u32>, Option<String>) = conn.query_row(
        "SELECT state,pid,outcome FROM attempts WHERE id=?1",
        [attempt],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let previous: Option<String> = conn
        .query_row(
            "SELECT response FROM reviewer_calls WHERE attempt_id=?1 ORDER BY rowid DESC LIMIT 1",
            [attempt],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let previous = previous.map(|s| {
        s.chars()
            .take(MAX_EVIDENCE_BYTES as usize)
            .collect::<String>()
    });
    let mut commands=conn.prepare("SELECT id,state,pid,exit_code,reason FROM managed_commands WHERE run_id=?1 AND task_id=?2 ORDER BY rowid DESC LIMIT 8")?;
    let mut commands=commands.query_map(params![work.run,work.task],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"pid":r.get::<_,Option<u32>>(2)?,"exit_code":r.get::<_,Option<i32>>(3)?,"reason":r.get::<_,Option<String>>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for command in &mut commands {
        let dir = workspace
            .data
            .join("commands")
            .join(command["id"].as_str().unwrap());
        command["stdout"] = tail(&dir.join("stdout.log"), 4096)?;
        command["stderr"] = tail(&dir.join("stderr.log"), 4096)?;
    }
    let plan: String = conn.query_row("SELECT plan FROM runs WHERE id=?1", [&work.run], |r| {
        r.get(0)
    })?;
    let plan: crate::domain::types::Plan = serde_json::from_str(&plan)?;
    let operations = serde_json::to_string(&plan.operations)?;
    let context = serde_json::to_string(&json!({"common":plan.context,"task":task.context}))?;
    Ok(
        json!({"task":{"id":task.id,"title":task.title.chars().take(4096).collect::<String>(),"estimate_seconds":task.estimate_seconds,"context":context.chars().take(MAX_EVIDENCE_BYTES as usize).collect::<String>()},"elapsed":super::clock::projection(conn,&work.run,work.task.as_deref().unwrap())?,"activity":super::activity::projection(conn,attempt)?,"stdout":tail(&dir.join("stdout.log"),MAX_EVIDENCE_BYTES)?,"stderr":tail(&dir.join("stderr.log"),MAX_EVIDENCE_BYTES)?,"process":{"state":process.0,"pid":process.1,"outcome":process.2,"observation":"controller-owned scope; PID alone does not prove progress"},"commands":commands,"previous_review":previous,"operations":operations.chars().take(MAX_EVIDENCE_BYTES as usize).collect::<String>(),"diff":{"source":"bounded git diff supplied at invocation","limitations":"current checkout includes concurrent tasks; unchanged old diff is not new progress"}}),
    )
}

fn schema(work: &Work, id: &str) -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "run":{"type":"string","const":work.run},"task":{"type":"string","const":work.task},"attempt":{"type":"string","const":work.attempt},"review":{"type":"string","const":id},"generation":{"type":"integer","const":work.generation},"decision":{"type":"string","enum":["continue","retry","inconclusive"]},"rationale":{"type":"string"},"evidence":{"type":"array","items":{"type":"string"}},"delay_cause":{"type":"string"},"next_review_after_seconds":{"type":["integer","null"]}},"required":["run","task","attempt","review","generation","decision","rationale","evidence","delay_cause","next_review_after_seconds"]})
}

fn launch_snapshot(workspace: &Workspace, dir: &Path) -> Result<OwnedProcess> {
    fs::create_dir_all(dir)?;
    let mut git = std::process::Command::new("git");
    // No external diff/text conversion hooks; process waits stay in poll().
    git.arg("-C")
        .arg(&workspace.checkout)
        .args([
            "--no-optional-locks",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            ".",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(File::create(dir.join("diff.log"))?)
        .stderr(File::create(dir.join("diff-stderr.log"))?);
    OwnedProcess::spawn(&mut git, &dir.join("snapshot"))
}

fn launch(
    workspace: &Workspace,
    dir: &Path,
    work: &Work,
    id: &str,
    session: &str,
    observer: &Observer,
    evidence: &Value,
) -> Result<OwnedProcess> {
    fs::create_dir_all(dir)?;
    let schema = schema(work, id);
    fs::write(dir.join("schema.json"), serde_json::to_vec(&schema)?)?;
    let mut diff = Vec::new();
    File::open(dir.join("diff.log"))?
        .take(MAX_EVIDENCE_BYTES)
        .read_to_end(&mut diff)?;
    let mut evidence = evidence.clone();
    evidence["diff"]["text"] = json!(String::from_utf8_lossy(&diff));
    evidence["diff"]["capture_limit_bytes"] = json!(MAX_EVIDENCE_BYTES);
    fs::write(dir.join("evidence.json"), serde_json::to_vec(&evidence)?)?;
    let prompt = format!(
        "You are a fresh execution observer, never a developer or completion checker. Read the immutable evidence below. Do not edit source, run tools, release resources, report task completion, or launch replacements. Compare previous_review with current evidence. A PID, heartbeat, timestamp, repeated old diff or unsupported worker claim alone is not progress. Consider the chosen operation schedule and plausible delay cause. Return continue with a justified finite interval, retry for normal stop-before-replace, or inconclusive. Native read-only/tool denial is not a universal OS security boundary. Do not claim unavailable evidence.\nEvidence references are task, elapsed, activity, stdout, stderr, commands, process, diff, previous_review, operations.\nEVIDENCE:\n{}\nRESPONSE SCHEMA:\n{}\n",
        evidence, schema
    );
    fs::write(dir.join("prompt.txt"), prompt)?;
    let (mut command, limits) = match observer {
        Observer::Codex { config } => {
            ensure!(
                config.resume_attempt.is_none(),
                "execution reviewer must use a fresh session"
            );
            let mut command = crate::executors::codex::command(config)?;
            command.args([
                "exec",
                "--json",
                "--sandbox",
                "read-only",
                "--config",
                "approval_policy=\"never\"",
                "--config",
                "features.shell_tool=false",
                "--config",
                "web_search=\"disabled\"",
                "--config",
                "mcp_servers={}",
            ]);
            if let Some(profile) = &config.native_profile {
                command.arg("--profile").arg(profile);
            }
            if let Some(model) = &config.model {
                command.arg("--model").arg(model);
            }
            for (key, value) in [
                ("model_provider", &config.provider),
                ("model_reasoning_effort", &config.reasoning_effort),
            ] {
                if let Some(value) = value {
                    command
                        .arg("--config")
                        .arg(format!("{key}={}", serde_json::to_string(value)?));
                }
            }
            command
                .arg("--output-schema")
                .arg(dir.join("schema.json"))
                .arg("--output-last-message")
                .arg(dir.join("final.json"))
                .arg("-");
            (
                command,
                "native read-only sandbox, never approve, shell disabled; platform/native enforcement and configured external integrations are not an absolute security boundary",
            )
        }
        Observer::Claude { config } => {
            ensure!(
                config.resume_attempt.is_none(),
                "execution reviewer must use a fresh session"
            );
            let mut command = crate::executors::claude::command(config)?;
            command
                .args([
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--permission-mode",
                    "dontAsk",
                    "--permission-prompts",
                    "none",
                    "--tools",
                    "",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "{\"mcpServers\":{}}",
                    "--session-id",
                ])
                .arg(session)
                .arg("--json-schema")
                .arg(serde_json::to_string(&schema)?);
            for (flag, value) in [("--model", &config.model), ("--effort", &config.effort)] {
                if let Some(value) = value {
                    command.arg(flag).arg(value);
                }
            }
            (
                command,
                "native tools disabled, strict empty MCP config and dontAsk; permission denial is not an OS sandbox",
            )
        }
        Observer::OpenCode { config } => (
            crate::executors::opencode::reviewer_command(config)?,
            "standalone fresh session with native tool, MCP, skill and web search policies denied",
        ),
        Observer::Profile { alias, .. } => {
            anyhow::bail!("execution observer alias {alias} was not resolved before launch")
        }
    };
    // Remove all inherited scoped clients. Reviewer cannot impersonate a worker.
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("JTASKS_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(&workspace.checkout)
        .stdin(File::open(dir.join("prompt.txt"))?)
        .stdout(File::create(dir.join("stdout.log"))?)
        .stderr(File::create(dir.join("stderr.log"))?);
    fs::write(
        dir.join("permissions.json"),
        serde_json::to_vec(
            &json!({"actor":"execution_reviewer","fresh":true,"scope":{"run":work.run,"task":work.task,"attempt":work.attempt,"review":id,"generation":work.generation},"session":session,"native_read_only":limits,"control_capabilities":[],"evidence_limits":{"per_stream_bytes":MAX_EVIDENCE_BYTES,"commands":8,"response_bytes":MAX_RESPONSE_BYTES},"account_access":"unknown; invocation selection does not establish provider availability"}),
        )?,
    )?;
    OwnedProcess::spawn(&mut command, dir)
}

fn response(dir: &Path, live: &Live, success: bool) -> Result<(Option<Assessment>, bool, String)> {
    // Native decoders operate only on bounded files; raw capture remains intact.
    for name in ["stdout.log", "final.json"] {
        if let Ok(meta) = fs::metadata(dir.join(name)) {
            ensure!(
                meta.len() <= MAX_RESPONSE_BYTES,
                "review native response exceeds bounded decoding limit"
            );
        }
    }
    let (value, infra) = match &live.observer {
        Observer::Codex { .. } => {
            let (obs, text) = crate::executors::codex::observe(
                dir,
                live.work.task.as_deref().unwrap(),
                live.work.attempt.as_deref().unwrap(),
            )?;
            if obs.failure_class.as_deref() == Some("transient_service_or_connection") {
                return Ok((None, true, "native infrastructure failure".into()));
            }
            ensure!(
                success
                    && obs.protocol_error.is_none()
                    && obs.completed_turn
                    && obs.thread_id.is_some(),
                "review lacks successful exact-root Codex terminal turn"
            );
            let final_text = fs::read_to_string(dir.join("final.json"))?;
            ensure!(
                text.as_deref().map(str::trim) == Some(final_text.trim()),
                "review final message differs from root native stream"
            );
            (serde_json::from_str::<Value>(&final_text)?, false)
        }
        Observer::Claude { .. } => {
            let (obs, value) = crate::executors::claude::observe(
                dir,
                live.work.task.as_deref().unwrap(),
                live.work.attempt.as_deref().unwrap(),
            )?;
            if obs.failure_class.as_deref() == Some("transient_service_or_connection") {
                return Ok((None, true, "native infrastructure failure".into()));
            }
            ensure!(
                success
                    && obs.completed_turn
                    && obs.protocol_error.is_none()
                    && obs.session_id.as_deref() == Some(&live.session),
                "review lacks successful launch-bound Claude result"
            );
            ensure!(
                obs.native_permissions
                    .as_deref()
                    .is_none_or(|p| p == "dontAsk"),
                "reviewer native permissions differ from invocation"
            );
            (
                value.context("review lacks native structured_output")?,
                false,
            )
        }
        Observer::OpenCode { .. } => {
            let (obs, text) = crate::executors::opencode::observe_review(
                dir,
                live.work.task.as_deref().unwrap(),
                live.work.attempt.as_deref().unwrap(),
            )?;
            if obs.failure_class.as_deref() == Some("transient_service_or_connection") {
                return Ok((None, true, "native infrastructure failure".into()));
            }
            ensure!(
                success
                    && obs.protocol_error.is_none()
                    && obs.invalid_records == 0
                    && obs.session_id.is_some(),
                "review lacks a successful root OpenCode response"
            );
            (serde_json::from_str(&text)?, false)
        }
        Observer::Profile { .. } => anyhow::bail!("review observer was not resolved"),
    };
    Ok((
        Some(serde_json::from_value(value)?),
        infra,
        "native designated execution assessment".into(),
    ))
}
