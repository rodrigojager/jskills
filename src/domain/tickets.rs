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
            "{}\n## Effective autonomy: {mode}\n{instructions}\nNative permissions are not an OS sandbox. Scope, required outcomes and budgets remain enforced. Do not invent credentials or facts. Native helper policy inheritance is harness-managed and not guaranteed by jtasks. Keep private reasoning and secrets out of records.\n\nSame-task explicit decisions/questions/answers (journal sequence; attributable claims):\n```json\n{}\n```\n",
            render(&plan, task, attempt, &facts),
            serde_json::to_string_pretty(&same_task)?
        ),
    )?;
    Ok(path)
}
