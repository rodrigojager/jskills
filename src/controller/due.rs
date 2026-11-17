//! One foreground scan queues short decisions; consumers launch asynchronously.
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const INTERVAL: Duration = Duration::from_secs(1);
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Retry,
    Review,
    ReviewerDeadline,
    CommandDeadline,
    ShutdownDeadline,
}
impl Kind {
    fn text(self) -> Result<String> {
        Ok(serde_json::to_value(self)?.as_str().unwrap().into())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Work {
    pub id: String,
    pub run: String,
    pub task: Option<String>,
    pub attempt: Option<String>,
    pub kind: Kind,
    pub generation: u32,
    pub due_ms: u64,
}
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS due_work(id TEXT PRIMARY KEY,run_id TEXT NOT NULL REFERENCES runs(id),task_id TEXT,attempt_id TEXT REFERENCES attempts(id),kind TEXT NOT NULL,generation INTEGER NOT NULL,due_ms INTEGER NOT NULL,state TEXT NOT NULL,queued_ms INTEGER,admitted_ms INTEGER,payload TEXT NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS one_outstanding_review ON due_work(attempt_id) WHERE kind='review' AND state IN ('pending','queued','admitted');")?;
    Ok(())
}
/// Stable ID plus increasing generation invalidates already queued snapshots.
/// Activity/log calls deliberately never call this function.
pub fn schedule(conn: &Connection, work: &Work) -> Result<()> {
    ensure!(!work.id.is_empty(), "due work needs a stable identity");
    let old: Option<(u32, String)> = conn
        .query_row(
            "SELECT generation,payload FROM due_work WHERE id=?1",
            [&work.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let payload = serde_json::to_string(work)?;
    if let Some((generation, previous)) = old {
        let prior: Work = serde_json::from_str(&previous)?;
        ensure!(
            prior.run == work.run
                && prior.task == work.task
                && prior.attempt == work.attempt
                && prior.kind == work.kind,
            "due work identity is immutable"
        );
        ensure!(
            work.generation > generation || previous == payload,
            "due generation must advance"
        );
        if previous == payload {
            return Ok(());
        }
    }
    conn.execute("INSERT INTO due_work(id,run_id,task_id,attempt_id,kind,generation,due_ms,state,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8) ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,due_ms=excluded.due_ms,state='pending',queued_ms=NULL,admitted_ms=NULL,payload=excluded.payload",params![work.id,work.run,work.task,work.attempt,work.kind.text()?,work.generation,i64::try_from(work.due_ms)?,payload])?;
    Ok(())
}
pub fn cancel(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE due_work SET state='cancelled' WHERE id=?1", [id])?;
    Ok(())
}
pub fn complete(conn: &Connection, id: &str, generation: u32) -> Result<()> {
    conn.execute(
        "UPDATE due_work SET state='complete' WHERE id=?1 AND generation=?2",
        params![id, generation],
    )?;
    Ok(())
}

/// Queuing does not start a reviewer deadline. Deadline starts at actual launch.
pub fn scan(conn: &Connection, run: &str, now_ms: u64) -> Result<Vec<Work>> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE due_work SET state='cancelled' WHERE run_id=?1 AND state IN ('pending','queued') AND attempt_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM attempts a WHERE a.id=due_work.attempt_id AND a.run_id=due_work.run_id AND (due_work.task_id IS NULL OR a.task_id=due_work.task_id) AND a.state='running' AND a.outcome IS NULL)",[run])?;
    tx.execute("UPDATE due_work SET state='queued',queued_ms=?2 WHERE run_id=?1 AND state='pending' AND due_ms<=?2",params![run,i64::try_from(now_ms)?])?;
    let work = {
        let mut q = tx.prepare(
            "SELECT payload FROM due_work WHERE run_id=?1 AND state='queued' ORDER BY due_ms,id",
        )?;
        q.query_map([run], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    tx.commit()?;
    work.into_iter()
        .map(|s| Ok(serde_json::from_str(&s)?))
        .collect()
}
/// Recheck snapshot, generation, deadline and current worker at actual admission.
/// Returns queue delay; false/stale snapshots cannot launch work.
pub fn admit(conn: &Connection, work: &Work, now_ms: u64) -> Result<Option<u64>> {
    let tx = conn.unchecked_transaction()?;
    let changed=tx.execute("UPDATE due_work SET state='admitted',admitted_ms=?4 WHERE id=?1 AND generation=?2 AND payload=?3 AND state='queued' AND due_ms<=?4 AND (attempt_id IS NULL OR EXISTS(SELECT 1 FROM attempts a WHERE a.id=due_work.attempt_id AND a.run_id=due_work.run_id AND (due_work.task_id IS NULL OR a.task_id=due_work.task_id) AND a.state='running' AND a.outcome IS NULL))",params![work.id,work.generation,serde_json::to_string(work)?,i64::try_from(now_ms)?])?;
    tx.commit()?;
    Ok((changed > 0).then(|| now_ms.saturating_sub(work.due_ms)))
}

pub struct Checker {
    origin: Instant,
    next: Duration,
    pub scans: u64,
    anchors: HashMap<String, (u32, Duration)>,
}
impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}
impl Checker {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            next: Duration::ZERO,
            scans: 0,
            anchors: HashMap::new(),
        }
    }
    pub fn ready(&mut self) -> bool {
        self.ready_at(self.origin.elapsed())
    }
    pub fn ready_at(&mut self, elapsed: Duration) -> bool {
        if elapsed < self.next {
            return false;
        }
        self.next = elapsed.saturating_add(INTERVAL);
        self.scans += 1;
        true
    }
    /// During continuous supervision UTC adjustments cannot move a due time.
    /// Persisted UTC anchors are adopted once after restart; custody/timing gap
    /// reconciliation runs before this checker is allowed to dispatch.
    pub fn scan(&mut self, conn: &Connection, run: &str) -> Result<Vec<Work>> {
        self.scan_at(conn, run, self.origin.elapsed(), super::clock::utc_ms())
    }
    pub fn scan_at(
        &mut self,
        conn: &Connection,
        run: &str,
        elapsed: Duration,
        utc_ms: u64,
    ) -> Result<Vec<Work>> {
        let rows = {
            let mut q = conn.prepare(
                "SELECT payload FROM due_work WHERE run_id=?1 AND state IN ('pending','queued')",
            )?;
            q.query_map([run], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let tx = conn.unchecked_transaction()?;
        let mut present = std::collections::HashSet::new();
        for payload in rows {
            let work: Work = serde_json::from_str(&payload)?;
            present.insert(work.id.clone());
            let anchor = self.anchors.entry(work.id.clone()).or_insert((
                work.generation,
                elapsed.saturating_add(Duration::from_millis(work.due_ms.saturating_sub(utc_ms))),
            ));
            if anchor.0 != work.generation {
                *anchor = (
                    work.generation,
                    elapsed
                        .saturating_add(Duration::from_millis(work.due_ms.saturating_sub(utc_ms))),
                );
            }
            if elapsed >= anchor.1 {
                tx.execute("UPDATE due_work SET state='queued',queued_ms=COALESCE(queued_ms,?2) WHERE id=?1 AND state='pending'",params![work.id,i64::try_from(utc_ms)?])?;
            }
        }
        self.anchors.retain(|id, _| present.contains(id));
        // Identity is still rechecked at actual admission.
        tx.execute("UPDATE due_work SET state='cancelled' WHERE run_id=?1 AND state IN ('pending','queued') AND attempt_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM attempts a WHERE a.id=due_work.attempt_id AND a.run_id=due_work.run_id AND (due_work.task_id IS NULL OR a.task_id=due_work.task_id) AND a.state='running' AND a.outcome IS NULL)",[run])?;
        let payloads = {
            let mut q=tx.prepare("SELECT payload FROM due_work WHERE run_id=?1 AND state='queued' ORDER BY due_ms,id")?;
            q.query_map([run], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.commit()?;
        payloads
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    /// Monotonic-aware admission for snapshots returned by this checker.
    pub fn admit(&self, conn: &Connection, work: &Work) -> Result<Option<u64>> {
        self.admit_at(conn, work, self.origin.elapsed(), super::clock::utc_ms())
    }
    pub fn admit_at(
        &self,
        conn: &Connection,
        work: &Work,
        elapsed: Duration,
        utc_ms: u64,
    ) -> Result<Option<u64>> {
        let Some((generation, anchor)) = self.anchors.get(&work.id) else {
            return Ok(None);
        };
        if *generation != work.generation || elapsed < *anchor {
            return Ok(None);
        }
        // Use the anchored due UTC for the SQL predicate even after wall-clock
        // rollback; observation UTC remains the actual admitted timestamp.
        let tx = conn.unchecked_transaction()?;
        let changed=tx.execute("UPDATE due_work SET state='admitted',admitted_ms=?4 WHERE id=?1 AND generation=?2 AND payload=?3 AND state='queued' AND (attempt_id IS NULL OR EXISTS(SELECT 1 FROM attempts a WHERE a.id=due_work.attempt_id AND a.run_id=due_work.run_id AND (due_work.task_id IS NULL OR a.task_id=due_work.task_id) AND a.state='running' AND a.outcome IS NULL))",params![work.id,work.generation,serde_json::to_string(work)?,i64::try_from(utc_ms)?])?;
        tx.commit()?;
        Ok((changed > 0)
            .then(|| u64::try_from((elapsed - *anchor).as_millis()).unwrap_or(u64::MAX)))
    }
}
