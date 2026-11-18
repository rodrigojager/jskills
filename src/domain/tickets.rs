//! Fresh worker briefs with explicit context and factual same-task recovery.
use crate::{
    domain::types::{Plan, Task},
    read::recovery_facts::{self, RecoveryFacts},
    storage::Workspace,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{fmt::Write, fs, path::PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TicketContext {
    pub brief: String,
    pub requirements: Vec<String>,
    pub references: Vec<String>,
    pub dependency_artifacts: Vec<String>,
    pub operations: Vec<String>,
    pub configuration: Vec<String>,
    pub limits: Vec<String>,
    pub resources: Vec<String>,
}

fn list(out: &mut String, label: &str, values: &[String]) {
    if !values.is_empty() {
        let _ = writeln!(out, "\n### {label}");
        for value in values {
            let _ = writeln!(out, "- {value}");
        }
    }
}

fn context(out: &mut String, name: &str, context: &TicketContext) {
    let _ = writeln!(out, "\n## {name}\n{}", context.brief);
    for (label, values) in [
        ("Requirements", &context.requirements),
        ("References", &context.references),
        (
            "Explicit dependency artifacts (named inputs only)",
            &context.dependency_artifacts,
        ),
        ("Selected operation schedule", &context.operations),
        (
            "Effective configuration (no secret values)",
            &context.configuration,
        ),
        ("Limits", &context.limits),
        ("Present resources", &context.resources),
    ] {
        list(out, label, values);
    }
}

pub fn render(plan: &Plan, task: &Task, attempt: &str, facts: &RecoveryFacts) -> String {
    let mut out = format!(
        "# Task {}: {}\n\nAttempt: {attempt}\nCompletion policy: {:?}\n",
        task.id, task.title, plan.completion
    );
    context(&mut out, "Common context", &plan.context);
    context(&mut out, "Task context", &task.context);
    if !task.depends_on.is_empty() {
        let _ = writeln!(
            out,
            "\nDeclared dependencies: {}. Other-task histories are not injected.",
            task.depends_on.join(", ")
        );
    }
    out.push_str("\n## Same-task previous attempts\nInspect the current checkout and reacquired resources before trusting prior facts. Missing facts never mean a pristine checkout. Recorded claims retain their provenance; no conversations were summarized.\n");
    if facts.attempts.is_empty() {
        out.push_str("No prior facts recorded for this task; existing edits and output may still be present.\n");
    }
    if facts.omitted_attempts > 0 {
        let _ = writeln!(
            out,
            "{} older attempts omitted by limit.",
            facts.omitted_attempts
        );
    }
    for prior in &facts.attempts {
        let _ = writeln!(
            out,
            "\n### Attempt {}\nSource: runtime attempts/receipts/journal; recorded_at={}; state={}; designated_outcome={}; process_exit={:?}; receipt={}",
            prior.attempt,
            prior.recorded_at.as_deref().unwrap_or("unknown"),
            prior.state,
            prior.outcome.as_deref().unwrap_or("not recorded"),
            prior.exit_code,
            prior.receipt.as_deref().unwrap_or("none")
        );
        if let Some(summary) = &prior.summary {
            let _ = writeln!(out, "Recorded result summary: {summary}");
        }
        for command in &prior.commands {
            let _ = writeln!(
                out,
                "- Managed command receipt (runtime managed_commands): handle={}; exit={:?}; {}; request={}",
                command.command, command.exit_code, command.outcome, command.receipt
            );
        }
        let _ = writeln!(
            out,
            "Preserved output: attempts/{}/stdout.log, attempts/{}/stderr.log",
            prior.attempt, prior.attempt
        );
        let _ = writeln!(
            out,
            "Explicit worker-log provenance (recorded claims): attempts/{}/recovery-facts.json",
            prior.attempt
        );
        for (label, values) in [
            ("Findings", &prior.worker.findings),
            ("Recorded decisions", &prior.worker.decisions),
            ("Obstacles", &prior.worker.obstacles),
            ("Remaining work", &prior.worker.remaining_work),
            ("Existing edits/output references", &prior.worker.artifacts),
        ] {
            list(&mut out, label, values);
        }
        for command in &prior.worker.commands {
            let _ = writeln!(
                out,
                "- Recorded command: {}; exit={:?}; outcome={}; receipt={}",
                command.command, command.exit_code, command.outcome, command.receipt
            );
        }
        for warning in &prior.warnings {
            let _ = writeln!(out, "Uncertainty: {warning}");
        }
    }
    out.push_str("\nRecord compact explicit facts as recovery-facts.json beside this ticket when useful. Use this JSON shape (all arrays optional):\n```json\n{\"findings\":[],\"decisions\":[],\"obstacles\":[],\"remaining_work\":[],\"artifacts\":[],\"commands\":[{\"command\":\"literal command\",\"exit_code\":null,\"outcome\":\"actual observed result or unknown\",\"receipt\":\"existing receipt/path\"}]}\n```\nKeep secret values and transcripts out. Only report outcomes you observed; unknown exit codes stay null. These are attributable worker claims, separate from controller command receipts.\n");
    out
}

/// Every worker reads this: the end of a native turn demands the designated
/// result, so waiting for background work must stay inside one turn.
pub const LONG_COMMANDS: &str = "## Long commands\nRun long commands (builds, test suites, images) with `jtasks exec run --request-id <stable-id> -- <program> <args>` (or `--shell \"<script>\"`) in the foreground, calling the jtasks executable at `JTASKS_BIN` (PowerShell `& $env:JTASKS_BIN exec run ...`, POSIX `\"$JTASKS_BIN\" exec run ...`): the jtasks controller owns the command, it has no deadline unless you give one, it outlives your session, and the call waits up to `--wait-seconds` (default 300). While the receipt says `wait_expired: true`, repeat the same `exec run` call (the same request ID waits for the same command; nothing starts twice) or `jtasks exec wait <command_id> --wait-seconds <n>`, in the foreground; read the output in pages with `jtasks exec output <command_id> --stream stdout|stderr --offset <next_offset> --text`. Reuse the same request ID after a retry: it returns the same command instead of starting it again. Short commands may run in the foreground directly. Never run a command in the background and never end your turn to wait for background work (a monitor, a scheduled wake-up or a background task notification): the end of your turn demands your result, and jtasks refuses those tools. Submit your outcome only when the work is done.";

/// Newest operator notes shown to a worker; older ones are counted, not shown.
const NOTE_LIMIT: usize = 10;
/// Bytes of one note shown to a worker; the journal keeps the full text.
const NOTE_BYTES: usize = 2000;

fn bounded(text: &str) -> String {
    let text = text.trim();
    if text.len() <= NOTE_BYTES {
        return text.into();
    }
    let mut end = NOTE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [truncated; full text in the run journal]", &text[..end])
}

/// Operator notes (manual retry reasons and recorded answers) given to this
/// task's lineage since its previous attempt started, so a retried worker
/// receives the answer in both autonomous and restricted mode.
fn operator_notes(conn: &Connection, run: &str, task: &str, attempt: &str) -> Result<String> {
    let root = crate::controller::graph::root(conn, run, task)?;
    let mut lineage = vec![task.to_string(), root.clone()];
    let ancestry: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_ancestry')",
        [],
        |r| r.get(0),
    )?;
    if ancestry {
        let mut query =
            conn.prepare("SELECT task_id FROM graph_ancestry WHERE run_id=?1 AND root_id=?2")?;
        for id in query.query_map([run, root.as_str()], |r| r.get::<_, String>(0))? {
            lineage.push(id?);
        }
    }
    let mut launches = conn.prepare(
        "SELECT sequence,task_id,attempt_id FROM journal WHERE run_id=?1 AND event='launch_intent' ORDER BY sequence",
    )?;
    let (mut since, mut until) = (0i64, i64::MAX);
    for row in launches.query_map([run], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })? {
        let (sequence, task_id, attempt_id) = row?;
        if attempt_id.as_deref() == Some(attempt) {
            until = until.min(sequence);
        } else if task_id.is_some_and(|id| lineage.contains(&id)) && sequence < until {
            since = since.max(sequence);
        }
    }
    let records = crate::controller::questions::read(conn, run)?;
    let records = records.as_array().unwrap();
    let mut events = conn.prepare(
        "SELECT sequence,task_id,event,detail,recorded_at FROM journal WHERE run_id=?1 AND event IN ('manual_retry','restricted_answer') AND sequence>?2 AND sequence<?3 ORDER BY sequence",
    )?;
    let mut notes = Vec::new();
    for row in events.query_map(rusqlite::params![run, since, until], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })? {
        let (sequence, task_id, event, detail, recorded_at) = row?;
        if !task_id.is_some_and(|id| lineage.contains(&id)) {
            continue;
        }
        let detail: serde_json::Value = serde_json::from_str(&detail).unwrap_or_default();
        let note = if event == "manual_retry" {
            format!(
                "- Retry reason (sequence {sequence}, {recorded_at}): {}",
                bounded(detail["reason"].as_str().unwrap_or(""))
            )
        } else {
            let question = records
                .iter()
                .find(|q| {
                    q["event"] == "restricted_question"
                        && q["detail"]["request_id"] == detail["question_id"]
                })
                .and_then(|q| q["detail"]["question"].as_str())
                .unwrap_or("unknown question");
            format!(
                "- Answer by {} (sequence {sequence}, {recorded_at}) to \"{}\": {}",
                bounded(detail["actor"].as_str().unwrap_or("unknown")),
                bounded(question),
                bounded(detail["text"].as_str().unwrap_or(""))
            )
        };
        notes.push(note);
    }
    let mut out = String::from(
        "## Notes from the operator since the last attempt\nThe operator gave these notes (`jtasks retry --reason`, `jtasks answer`) after the previous attempt started. They answer or resolve what that attempt reported: act on them and do not report the same blocker or question again unless a note leaves it unresolved.\n",
    );
    if notes.is_empty() {
        out.push_str("None recorded.\n");
    }
    let omitted = notes.len().saturating_sub(NOTE_LIMIT);
    if omitted > 0 {
        let _ = writeln!(out, "{omitted} older notes omitted by limit.");
    }
    for note in &notes[omitted..] {
        let _ = writeln!(out, "{note}");
    }
    Ok(out)
}

/// Called after durable launch intent and before spawning any worker.
pub fn write_attempt_ticket(workspace: &Workspace, task: &Task, attempt: &str) -> Result<PathBuf> {
    crate::domain::validation::identifier(attempt)?;
    let conn = Connection::open_with_flags(
        workspace.database(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(crate::storage::READER_BUSY_TIMEOUT)?;
    let (run, task_id, plan): (String, String, String) = conn.query_row("SELECT a.run_id,a.task_id,r.plan FROM attempts a JOIN runs r ON r.id=a.run_id WHERE a.id=?1", [attempt], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
    ensure!(
        task_id == task.id,
        "ticket task does not match attempt identity"
    );
    let plan: Plan = serde_json::from_str(&plan)?;
    let facts = recovery_facts::read(
        &workspace.database(),
        &workspace.data,
        &run,
        &task.id,
        attempt,
    )?;
    let path = workspace
        .data
        .join("attempts")
        .join(attempt)
        .join("ticket.md");
    fs::create_dir_all(path.parent().unwrap())?;
    let profile: String = conn.query_row(
        "SELECT effective_profile FROM attempts WHERE id=?1",
        [attempt],
        |row| row.get(0),
    )?;
    let profile: serde_json::Value = serde_json::from_str(&profile)?;
    let mode = profile["values"]["autonomy"]
        .as_str()
        .unwrap_or("autonomous");
    let instructions = if mode == "restricted" {
        "For a genuine blocked requirement, submit `jtasks ask --request-id <stable-id> --question <text> --blocked-requirement <requirement> [--option <choice>]`, then submit the designated blocked result and retire. A question alone is not a resumable wait. Management records answers using `jtasks answer`; explicit bounded retry after custody retirement resumes work."
    } else {
        "Choose your recommended solution to routine questions and continue. Record concise situation/decision/deviation with `jtasks decisions record --request-id <stable-id> --situation <text> --decision <text> --deviation <text>` (use 'none' for no deviation). Do not wait for routine approval."
    };
    let records = crate::controller::questions::read(&conn, &run)?;
    let same_task: Vec<_> = records
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["task"] == task.id)
        .collect();
    fs::write(
        &path,
        format!(
            "{}\n{}\n## Effective autonomy: {mode}\n{instructions}\nNative permissions are not an OS sandbox. Scope, required outcomes and budgets remain enforced. Do not invent credentials or facts. Native helper policy inheritance is harness-managed and not guaranteed by jtasks. Keep private reasoning and secrets out of records.\n\n{LONG_COMMANDS}\n\nSame-task explicit decisions/questions/answers (journal sequence; attributable claims):\n```json\n{}\n```\n",
            render(&plan, task, attempt, &facts),
            operator_notes(&conn, &run, &task.id, attempt)?,
            serde_json::to_string_pretty(&same_task)?
        ),
    )?;
    Ok(path)
}
