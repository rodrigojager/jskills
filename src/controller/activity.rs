//! Attributable events and their activity projection share one transaction.
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Worker,
    Reviewer,
    Controller,
    Reader,
    Unattributed,
}
#[derive(Debug, Clone, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}
#[derive(Debug, Clone, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Message,
    Tool,
    Other,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub actor: Actor,
    pub source: String,
    pub kind: Kind,
    pub severity: Severity,
    /// Logical content only; timestamps and transport metadata are excluded.
    pub content: Option<String>,
}

pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS attempt_activity(attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),start_ms INTEGER NOT NULL,last_sequence INTEGER,last_log_ms INTEGER,last_activity_ms INTEGER,last_distinct_content_ms INTEGER,last_tool_activity_ms INTEGER);
CREATE TABLE IF NOT EXISTS activity_events(sequence INTEGER PRIMARY KEY REFERENCES journal(sequence),attempt_id TEXT NOT NULL REFERENCES attempts(id),observed_ms INTEGER NOT NULL,event TEXT NOT NULL,eligible INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS activity_content(attempt_id TEXT NOT NULL REFERENCES attempts(id),content TEXT NOT NULL,PRIMARY KEY(attempt_id,content));")?;
    Ok(())
}

pub fn begin(conn: &Connection, attempt: &str, now_ms: u64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO attempt_activity(attempt_id,start_ms) VALUES(?1,?2)",
        params![attempt, i64::try_from(now_ms)?],
    )?;
    Ok(())
}

/// Caller supplies a controller observation time, never a worker timestamp.
/// Historical/other actors remain logs but cannot advance worker progress.
pub fn commit(
    conn: &Connection,
    run: &str,
    task: &str,
    attempt: &str,
    event: &Event,
    now_ms: u64,
) -> Result<i64> {
    ensure!(
        event.source.len() <= 128 && event.content.as_ref().is_none_or(|s| s.len() <= 65536),
        "oversized activity event"
    );
    let tx = conn.unchecked_transaction()?;
    let current: Option<bool> = tx.query_row("SELECT state='running' AND outcome IS NULL FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3",[attempt,run,task],|r|r.get(0)).optional()?;
    ensure!(current.is_some(), "unknown activity attempt identity");
    begin(&tx, attempt, now_ms)?;
    let eligible = current == Some(true)
        && event.actor == Actor::Worker
        && event.severity != Severity::Error
        && event.kind != Kind::Other;
    let serialized = serde_json::to_string(event)?;
    tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'activity',?4)",params![run,task,attempt,serialized])?;
    let seq = tx.last_insert_rowid();
    let now = i64::try_from(now_ms)?;
    tx.execute("INSERT INTO activity_events(sequence,attempt_id,observed_ms,event,eligible) VALUES(?1,?2,?3,?4,?5)",params![seq,attempt,now,serialized,eligible])?;
    let distinct = if eligible && event.kind == Kind::Message {
        if let Some(content) = event.content.as_deref().filter(|s| !s.trim().is_empty()) {
            tx.execute(
                "INSERT OR IGNORE INTO activity_content(attempt_id,content) VALUES(?1,?2)",
                [attempt, content],
            )? > 0
        } else {
            false
        }
    } else {
        false
    };
    tx.execute("UPDATE attempt_activity SET last_sequence=?2,last_log_ms=?3,last_activity_ms=CASE WHEN ?4 THEN ?3 ELSE last_activity_ms END,last_distinct_content_ms=CASE WHEN ?5 THEN ?3 ELSE last_distinct_content_ms END,last_tool_activity_ms=CASE WHEN ?6 THEN ?3 ELSE last_tool_activity_ms END WHERE attempt_id=?1",params![attempt,seq,now,eligible,distinct,eligible && event.kind==Kind::Tool])?;
    tx.commit()?;
    Ok(seq)
}

pub fn projection(conn: &Connection, attempt: &str) -> Result<Value> {
    let unavailable = json!({"attempt":attempt,"start_ms":null,"silence_baseline":"not_initialized","last_sequence":null,"last_log_ms":null,"last_activity_ms":null,"last_distinct_content_ms":null,"last_tool_activity_ms":null});
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='attempt_activity')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(unavailable);
    }
    Ok(conn.query_row("SELECT start_ms,last_sequence,last_log_ms,last_activity_ms,last_distinct_content_ms,last_tool_activity_ms FROM attempt_activity WHERE attempt_id=?1",[attempt],|r|Ok(json!({"attempt":attempt,"start_ms":r.get::<_,i64>(0)?,"silence_baseline":"attempt_start","last_sequence":r.get::<_,Option<i64>>(1)?,"last_log_ms":r.get::<_,Option<i64>>(2)?,"last_activity_ms":r.get::<_,Option<i64>>(3)?,"last_distinct_content_ms":r.get::<_,Option<i64>>(4)?,"last_tool_activity_ms":r.get::<_,Option<i64>>(5)?}))).optional()?.unwrap_or(unavailable))
}

/// One incremental I/O cursor per owned process, not a timer. Partial NDJSON is
/// retained until complete; invalid/child/foreign-session records are only logs.
pub struct NativeCursor {
    executor: &'static str,
    offset: u64,
    stderr_offset: u64,
    pending: Vec<u8>,
    session: Option<String>,
    expected: Option<String>,
    invalid_identity: bool,
    discarding: bool,
}
impl NativeCursor {
    pub fn new(dir: &Path, executor: &'static str) -> Result<Self> {
        let launch: Value =
            serde_json::from_slice(&std::fs::read(dir.join(format!("{executor}-launch.json")))?)?;
        let key = match executor {
            "claude" => "session_id",
            "codex" => "resume_thread",
            _ => "resume_session",
        };
        Ok(Self {
            executor,
            offset: 0,
            stderr_offset: 0,
            pending: Vec::new(),
            session: None,
            expected: launch[key].as_str().map(str::to_owned),
            invalid_identity: false,
            discarding: false,
        })
    }
    pub fn poll(
        &mut self,
        conn: &Connection,
        dir: &Path,
        run: &str,
        task: &str,
        attempt: &str,
        now_ms: u64,
    ) -> Result<()> {
        let mut file = File::open(dir.join("stdout.log"))?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.take(65536).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        for byte in bytes {
            if byte == b'\n' {
                let event = if self.discarding {
                    Event {
                        actor: Actor::Unattributed,
                        source: format!("native:{}", self.executor),
                        kind: Kind::Other,
                        severity: Severity::Warning,
                        content: Some("oversized native record; raw capture retained".into()),
                    }
                } else {
                    self.normalize()
                };
                commit(conn, run, task, attempt, &event, now_ms)?;
                self.pending.clear();
                self.discarding = false;
            } else if !self.discarding {
                if self.pending.len() >= 65536 {
                    self.pending.clear();
                    self.discarding = true;
                } else {
                    self.pending.push(byte);
                }
            }
        }
        let mut stderr = File::open(dir.join("stderr.log"))?;
        stderr.seek(SeekFrom::Start(self.stderr_offset))?;
        let mut bytes = Vec::new();
        stderr.take(16384).read_to_end(&mut bytes)?;
        self.stderr_offset += bytes.len() as u64;
        if !bytes.is_empty() {
            // Stream selection is not an error classifier or proof of progress.
            commit(
                conn,
                run,
                task,
                attempt,
                &Event {
                    actor: Actor::Unattributed,
                    source: format!("native:{}:stderr", self.executor),
                    kind: Kind::Other,
                    severity: Severity::Info,
                    content: Some(String::from_utf8_lossy(&bytes).into_owned()),
                },
                now_ms,
            )?;
        }
        Ok(())
    }
    pub fn drain(
        &mut self,
        conn: &Connection,
        dir: &Path,
        run: &str,
        task: &str,
        attempt: &str,
        now_ms: u64,
    ) -> Result<()> {
        let end = std::fs::metadata(dir.join("stdout.log"))?.len();
        let stderr_end = std::fs::metadata(dir.join("stderr.log"))?.len();
        // Bounded retirement catch-up; raw capture remains authoritative.
        for _ in 0..16 {
            if self.offset >= end && self.stderr_offset >= stderr_end {
                break;
            }
            self.poll(conn, dir, run, task, attempt, now_ms)?;
        }
        if self.offset < end
            || self.stderr_offset < stderr_end
            || !self.pending.is_empty()
            || self.discarding
        {
            commit(
                conn,
                run,
                task,
                attempt,
                &Event {
                    actor: Actor::Unattributed,
                    source: format!("native:{}", self.executor),
                    kind: Kind::Other,
                    severity: Severity::Warning,
                    content: Some(
                        "native activity capture gap; inspect retained raw stream".into(),
                    ),
                },
                now_ms,
            )?;
        }
        Ok(())
    }
    fn normalize(&mut self) -> Event {
        let value: Value = serde_json::from_slice(&self.pending).unwrap_or(Value::Null);
        let typ = value["type"].as_str().unwrap_or("");
        let mut actor = Actor::Unattributed;
        let mut kind = Kind::Other;
        let mut content = None;
        let mut severity = Severity::Info;
        let child = value
            .get("parent_tool_use_id")
            .is_some_and(|v| !v.is_null())
            || value.get("parentID").is_some_and(|v| !v.is_null());
        let session = match self.executor {
            "codex" if typ == "thread.started" => value["thread_id"].as_str(),
            "claude" => value["session_id"].as_str(),
            "opencode" => value["sessionID"].as_str(),
            _ => None,
        }
        .filter(|s| !s.is_empty());
        if !child {
            if let Some(id) = session {
                if self.expected.as_deref().is_some_and(|e| e != id)
                    || self.session.as_deref().is_some_and(|e| e != id)
                {
                    self.invalid_identity = true;
                } else if self.executor != "claude"
                    || (typ == "system" && value["subtype"] == "init")
                    || self.session.is_some()
                {
                    self.session = Some(id.to_owned());
                }
            }
            if self.session.is_some() && !self.invalid_identity {
                actor = Actor::Worker;
            }
        }
        match self.executor {
            "codex" => {
                if matches!(typ, "error" | "turn.failed") || value["item"]["type"] == "error" {
                    severity = Severity::Error;
                }
                if matches!(typ, "item.started" | "item.updated" | "item.completed") {
                    match value["item"]["type"].as_str() {
                        Some("agent_message" | "reasoning") => {
                            kind = Kind::Message;
                            if typ == "item.completed" {
                                content = value["item"]["text"].as_str().map(str::to_owned);
                            }
                        }
                        Some(
                            "command_execution" | "mcp_tool_call" | "web_search" | "file_change",
                        ) => kind = Kind::Tool,
                        _ => {}
                    }
                }
            }
            "claude" => {
                if typ == "result" && value["is_error"] == true || typ == "error" {
                    severity = Severity::Error;
                }
                if matches!(typ, "tool_progress" | "tool_use_summary") {
                    kind = Kind::Tool;
                }
                if typ == "assistant" {
                    let blocks = value["message"]["content"].as_array();
                    if blocks.is_some_and(|b| b.iter().any(|v| v["type"] == "tool_use")) {
                        kind = Kind::Tool;
                    } else if let Some(blocks) = blocks {
                        let text = blocks
                            .iter()
                            .filter_map(|v| v["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !text.is_empty() {
                            kind = Kind::Message;
                            content = Some(text);
                        }
                    }
                    if value["error"].is_string() {
                        severity = Severity::Error;
                    }
                }
                if typ == "stream_event" {
                    let delta = &value["event"]["delta"];
                    if delta["text"].is_string() {
                        kind = Kind::Message;
                    } else if matches!(
                        value["event"]["content_block"]["type"].as_str(),
                        Some("tool_use")
                    ) || delta["type"] == "input_json_delta"
                    {
                        kind = Kind::Tool;
                    }
                }
            }
            "opencode" => {
                if typ == "error" {
                    severity = Severity::Error;
                }
                if matches!(typ, "text" | "reasoning") {
                    kind = Kind::Message;
                    content = value["part"]["text"].as_str().map(str::to_owned);
                }
                if typ == "tool_use" {
                    kind = Kind::Tool;
                }
                if session.is_none()
                    || value["part"]
                        .get("sessionID")
                        .is_some_and(|id| id.as_str() != session)
                {
                    actor = Actor::Unattributed;
                }
            }
            _ => {}
        }
        Event {
            actor,
            source: format!("native:{}", self.executor),
            kind,
            severity,
            content,
        }
    }
}
