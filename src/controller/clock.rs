//! Monotonic checkpoints, with UTC used only for history and gap uncertainty.
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

pub fn utc_ms() -> u64 {
    super::admission::now_ms()
}
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS task_clocks(run_id TEXT NOT NULL,task_id TEXT NOT NULL,active_ms INTEGER NOT NULL DEFAULT 0,dependency_wait_ms INTEGER NOT NULL DEFAULT 0,resource_wait_ms INTEGER NOT NULL DEFAULT 0,retry_wait_ms INTEGER NOT NULL DEFAULT 0,reviewer_ms INTEGER NOT NULL DEFAULT 0,checkpoint_ms INTEGER NOT NULL,mode TEXT NOT NULL,timing_uncertain INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(run_id,task_id),FOREIGN KEY(run_id,task_id) REFERENCES tasks(run_id,id));
CREATE TABLE IF NOT EXISTS attempt_clocks(attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),active_ms INTEGER NOT NULL DEFAULT 0,checkpoint_ms INTEGER NOT NULL,timing_uncertain INTEGER NOT NULL DEFAULT 0);")?;
    Ok(())
}

#[derive(Clone)]
struct Sample {
    mode: &'static str,
    attempts: Vec<String>,
    reviewing: bool,
}
pub struct Clock {
    origin: Instant,
    last: Duration,
    previous: HashMap<String, Sample>,
    reviewing: HashSet<String>,
}
impl Clock {
    /// Call before startup custody reconciliation. Unobserved downtime is never
    /// added to elapsed counters or mistaken for progress.
    pub fn start(conn: &Connection, run: &str, utc_ms: u64) -> Result<Self> {
        let tx = conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE task_clocks SET timing_uncertain=1 WHERE run_id=?1 AND mode<>'idle'",
            [run],
        )?;
        tx.execute("UPDATE attempt_clocks SET timing_uncertain=1 WHERE attempt_id IN (SELECT id FROM attempts WHERE run_id=?1 AND state IN ('running','launch_intent','uncertain'))",[run])?;
        if changed > 0 {
            tx.execute("INSERT INTO journal(run_id,event,detail) VALUES(?1,'timing_gap',?2)",params![run,json!({"observed_ms":utc_ms,"elapsed":"unobserved gap excluded; reconcile custody before dispatch"}).to_string()])?;
        }
        tx.commit()?;
        Ok(Self {
            origin: Instant::now(),
            last: Duration::ZERO,
            previous: HashMap::new(),
            reviewing: HashSet::new(),
        })
    }
    /// Reviewer time is separate even when it overlaps worker time. Consumers
    /// checkpoint immediately before changing this flag and once after it.
    pub fn set_reviewing(&mut self, task: &str, reviewing: bool) {
        if reviewing {
            self.reviewing.insert(task.into());
        } else {
            self.reviewing.remove(task);
        }
    }
    pub fn checkpoint(&mut self, conn: &Connection, run: &str) -> Result<()> {
        self.sample_at(conn, run, self.origin.elapsed(), utc_ms())
    }
    /// Deterministic synthetic monotonic/UTC inputs; UTC jumps do not alter sums.
    pub fn sample_at(
        &mut self,
        conn: &Connection,
        run: &str,
        elapsed: Duration,
        utc_ms: u64,
    ) -> Result<()> {
        ensure!(elapsed >= self.last, "monotonic clock moved backwards");
        let delta = i64::try_from(elapsed.as_millis() - self.last.as_millis())?;
        let now = i64::try_from(utc_ms)?;
        let tx = conn.unchecked_transaction()?;
        for (task, sample) in &self.previous {
            let column = match sample.mode {
                "active" => "active_ms",
                "dependency_wait" => "dependency_wait_ms",
                "resource_wait" => "resource_wait_ms",
                "retry_wait" => "retry_wait_ms",
                _ => "active_ms",
            };
            let amount = if sample.mode == "idle" { 0 } else { delta };
            tx.execute(&format!("UPDATE task_clocks SET {column}={column}+?3,reviewer_ms=reviewer_ms+?4,checkpoint_ms=?5 WHERE run_id=?1 AND task_id=?2"),params![run,task,amount,if sample.reviewing {delta} else {0},now])?;
            for attempt in &sample.attempts {
                tx.execute("UPDATE attempt_clocks SET active_ms=active_ms+?2,checkpoint_ms=?3 WHERE attempt_id=?1",params![attempt,delta,now])?;
            }
        }
        let rows = {
            let mut q = tx.prepare("SELECT id,state,spec FROM tasks WHERE run_id=?1")?;
            q.query_map([run], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut next = HashMap::new();
        let commands: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='managed_commands')",
            [],
            |r| r.get(0),
        )?;
        for (task, state, spec) in rows {
            let spec: crate::domain::types::Task = serde_json::from_str(&spec)?;
            let attempts = {
                let mut q = tx.prepare(
                    "SELECT id FROM attempts WHERE run_id=?1 AND task_id=?2 AND state='running'",
                )?;
                q.query_map([run, &task], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let command_active=commands && tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND task_id=?2 AND state IN ('intent','running','stopping'))",[run,&task],|r|r.get::<_,bool>(0))?;
            let mode = if !attempts.is_empty() || command_active {
                "active"
            } else if !matches!(state.as_str(), "pending" | "interrupted") {
                "idle"
            } else if !super::cooldowns::eligible(&tx, run, &task, utc_ms)?
                || spec.admission.cooldown_until_ms > utc_ms
            {
                "retry_wait"
            } else {
                let mut waiting = false;
                for dep in &spec.depends_on {
                    let complete: bool = tx.query_row(
                        "SELECT state='completed' FROM tasks WHERE run_id=?1 AND id=?2",
                        [run, dep],
                        |r| r.get(0),
                    )?;
                    waiting |= !complete;
                }
                if waiting {
                    "dependency_wait"
                } else {
                    "resource_wait"
                }
            };
            tx.execute("INSERT INTO task_clocks(run_id,task_id,checkpoint_ms,mode) VALUES(?1,?2,?3,?4) ON CONFLICT(run_id,task_id) DO UPDATE SET checkpoint_ms=excluded.checkpoint_ms,mode=excluded.mode",params![run,task,now,mode])?;
            for attempt in &attempts {
                tx.execute(
                    "INSERT OR IGNORE INTO attempt_clocks(attempt_id,checkpoint_ms) VALUES(?1,?2)",
                    params![attempt, now],
                )?;
            }
            next.insert(
                task.clone(),
                Sample {
                    mode,
                    attempts,
                    reviewing: self.reviewing.contains(&task),
                },
            );
        }
        tx.commit()?;
        self.previous = next;
        self.last = elapsed;
        Ok(())
    }
}

pub fn projection(conn: &Connection, run: &str, task: &str) -> Result<Value> {
    Ok(conn.query_row("SELECT active_ms,dependency_wait_ms,resource_wait_ms,retry_wait_ms,reviewer_ms,checkpoint_ms,mode,timing_uncertain FROM task_clocks WHERE run_id=?1 AND task_id=?2",[run,task],|r|Ok(json!({"task":task,"active_ms":r.get::<_,i64>(0)?,"dependency_wait_ms":r.get::<_,i64>(1)?,"resource_wait_ms":r.get::<_,i64>(2)?,"retry_wait_ms":r.get::<_,i64>(3)?,"reviewer_ms":r.get::<_,i64>(4)?,"checkpoint_ms":r.get::<_,i64>(5)?,"mode":r.get::<_,String>(6)?,"timing_uncertain":r.get::<_,bool>(7)?,"precision":"persisted monotonic checkpoint; unobserved controller gaps excluded"})))?)
}
