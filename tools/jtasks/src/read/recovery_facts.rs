//! Read-only, bounded recovery evidence for one task lineage. No transcript inference.
use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};

pub const MAX_ATTEMPTS: usize = 4;
pub const MAX_FACTS: usize = 8;
pub const MAX_TEXT: usize = 800;
const MAX_LOG_BYTES: u64 = 32 * 1024;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerFacts {
    pub findings: Vec<String>,
    pub decisions: Vec<String>,
    pub obstacles: Vec<String>,
    pub remaining_work: Vec<String>,
    pub artifacts: Vec<String>,
    pub commands: Vec<CommandFact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandFact {
    pub command: String,
    pub exit_code: Option<i32>,
    pub outcome: String,
    pub receipt: String,
}

#[derive(Debug)]
pub struct AttemptFacts {
    pub attempt: String,
    pub state: String,
    pub outcome: Option<String>,
    pub summary: Option<String>,
    pub exit_code: Option<i32>,
    pub receipt: Option<String>,
    pub recorded_at: Option<String>,
    pub worker: WorkerFacts,
    pub commands: Vec<CommandFact>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct RecoveryFacts {
    pub attempts: Vec<AttemptFacts>,
    pub omitted_attempts: usize,
}

pub fn bounded(text: &str) -> String {
    let mut result: String = text.chars().take(MAX_TEXT).collect();
    if text.chars().count() > MAX_TEXT {
        result.push_str(" [truncated]");
    }
    result
}

/// An absent/invalid explicit fact log is uncertainty, never an empty-checkout assertion.
fn worker_facts(path: &Path) -> (WorkerFacts, Vec<String>) {
    let result = (|| -> Result<WorkerFacts> {
        let mut data = Vec::new();
        File::open(path)?
            .take(MAX_LOG_BYTES + 1)
            .read_to_end(&mut data)?;
        ensure!(
            data.len() as u64 <= MAX_LOG_BYTES,
            "fact log exceeds size limit"
        );
        Ok(serde_json::from_slice(&data)?)
    })();
    match result {
        Ok(mut facts) => {
            let mut warnings = Vec::new();
            for values in [
                &mut facts.findings,
                &mut facts.decisions,
                &mut facts.obstacles,
                &mut facts.remaining_work,
                &mut facts.artifacts,
            ] {
                if values.len() > MAX_FACTS {
                    warnings.push("Additional explicit facts omitted by limit".into());
                }
                values.truncate(MAX_FACTS);
                for value in values {
                    *value = bounded(value);
                }
            }
            if facts.commands.len() > MAX_FACTS {
                warnings.push("Additional command facts omitted by limit".into());
            }
            facts.commands.truncate(MAX_FACTS);
            for command in &mut facts.commands {
                command.command = bounded(&command.command);
                command.outcome = bounded(&command.outcome);
                command.receipt = bounded(&command.receipt);
            }
            (facts, warnings)
        }
        Err(_) => (
            WorkerFacts::default(),
            vec![
                "Explicit recovery fact log unavailable or invalid; inspect preserved files/output"
                    .into(),
            ],
        ),
    }
}

/// Facts are constrained by run AND task and exclude the new attempt, including other runs.
pub fn read(
    database: &Path,
    data: &Path,
    run: &str,
    task: &str,
    current: &str,
) -> Result<RecoveryFacts> {
    let conn = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(crate::storage::READER_BUSY_TIMEOUT)?;
    conn.execute_batch("BEGIN")?;
    let total: i64 = conn.query_row(
        "SELECT count(*) FROM attempts WHERE run_id=?1 AND task_id=?2 AND id<>?3",
        params![run, task, current],
        |row| row.get(0),
    )?;
    let mut stmt = conn.prepare("SELECT a.id,a.state,a.outcome,a.summary,a.exit_code,r.id,(SELECT recorded_at FROM journal j WHERE j.attempt_id=a.id AND j.run_id=a.run_id AND j.task_id=a.task_id ORDER BY sequence DESC LIMIT 1) FROM attempts a LEFT JOIN receipts r ON r.attempt_id=a.id WHERE a.run_id=?1 AND a.task_id=?2 AND a.id<>?3 ORDER BY a.rowid DESC LIMIT ?4")?;
    let mut attempts = stmt
        .query_map(params![run, task, current, MAX_ATTEMPTS as i64], |row| {
            Ok(AttemptFacts {
                attempt: row.get(0)?,
                state: row.get(1)?,
                outcome: row.get(2)?,
                summary: row.get::<_, Option<String>>(3)?.map(|text| bounded(&text)),
                exit_code: row.get(4)?,
                receipt: row.get(5)?,
                recorded_at: row.get(6)?,
                worker: WorkerFacts::default(),
                commands: vec![],
                warnings: vec![],
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for attempt in &mut attempts {
        let has_commands: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='managed_commands')", [], |row| row.get(0))?;
        if has_commands {
            let mut commands = conn.prepare("SELECT id,request_id,state,exit_code,reason FROM managed_commands WHERE run_id=?1 AND task_id=?2 AND attempt_id=?3 ORDER BY rowid DESC LIMIT ?4")?;
            attempt.commands = commands
                .query_map(
                    params![run, task, attempt.attempt, MAX_FACTS as i64],
                    |row| {
                        let id: String = row.get(0)?;
                        let request: String = row.get(1)?;
                        let state: String = row.get(2)?;
                        let reason: Option<String> = row.get(4)?;
                        Ok(CommandFact {
                            command: bounded(&id),
                            exit_code: row.get(3)?,
                            outcome: bounded(&format!(
                                "state={state}; reason={}",
                                reason.as_deref().unwrap_or("none")
                            )),
                            receipt: bounded(&request),
                        })
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            attempt.commands.reverse();
        }
        // IDs are normally validated on import/creation; still refuse path traversal in evidence reads.
        if crate::domain::validation::identifier(&attempt.attempt).is_err() {
            attempt
                .warnings
                .push("Invalid attempt artifact path; no files read".into());
            continue;
        }
        (attempt.worker, attempt.warnings) = worker_facts(
            &data
                .join("attempts")
                .join(&attempt.attempt)
                .join("recovery-facts.json"),
        );
    }
    attempts.reverse();
    Ok(RecoveryFacts {
        omitted_attempts: (total as usize).saturating_sub(attempts.len()),
        attempts,
    })
}
