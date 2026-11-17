//! Direct Codex exec adapter. Wire contract checked against codex-cli 0.169.0
//! and openai/codex rust-v0.169.0 exec_events.rs/event_processor_with_jsonl_output.rs.
use crate::{
    domain::types::{Receipt, Task},
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command,
};

fn executable() -> String {
    "codex".into()
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    Autonomous,
    Restricted,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodexConfig {
    #[serde(default = "executable")]
    pub executable: String,
    /// Explicit launcher arguments, e.g. a deterministic substitute script.
    #[serde(default)]
    pub launcher_args: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub reasoning_effort: Option<String>,
    /// Native Codex config profile, distinct from a jtasks profile.
    pub native_profile: Option<String>,
    #[serde(default)]
    pub permissions: PermissionMode,
    /// Resume only a recorded same-task attempt, never a generic last session.
    pub resume_attempt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Observation {
    pub task_id: String,
    pub attempt_id: String,
    pub thread_id: Option<String>,
    pub completed_turn: bool,
    pub protocol_error: Option<String>,
    pub unknown_records: usize,
    pub invalid_records: usize,
    pub raw_stdout: PathBuf,
    pub raw_stderr: PathBuf,
    pub failure_class: Option<String>,
}

pub(crate) fn command(config: &CodexConfig) -> Result<Command> {
    let configured = Path::new(&config.executable);
    // Rust does not resolve a PowerShell shim the way an interactive PowerShell
    // session does. Resolve explicitly; prompts never enter a shell command.
    #[cfg(windows)]
    let resolved = if configured.components().count() == 1 && configured.extension().is_none() {
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .flat_map(|dir| {
                        ["exe", "ps1", "cmd"]
                            .map(move |ext| dir.join(format!("{}.{ext}", config.executable)))
                    })
                    .find(|p| p.is_file())
            })
            .unwrap_or_else(|| configured.to_path_buf())
    } else {
        configured.to_path_buf()
    };
    #[cfg(not(windows))]
    let resolved = configured.to_path_buf();
    let path = resolved.as_path();
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    ensure!(
        !extension.eq_ignore_ascii_case("cmd") && !extension.eq_ignore_ascii_case("bat"),
        "Codex batch shims are unsupported; select the native executable or a .ps1 launcher explicitly"
    );
    let mut command = if extension.eq_ignore_ascii_case("ps1") {
        let mut command = Command::new("pwsh");
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(path);
        command
    } else {
        Command::new(path)
    };
    command.args(&config.launcher_args);
    Ok(command)
}

#[allow(clippy::too_many_arguments)]
pub fn launch(
    workspace: &Workspace,
    task: &Task,
    config: &CodexConfig,
    attempt: &str,
    endpoint: SocketAddr,
    token: &str,
    ticket: &Path,
) -> Result<crate::execution::custody::OwnedProcess> {
    let dir = workspace.data.join("attempts").join(attempt);
    fs::create_dir_all(&dir)?;
    let receipt = format!("native-{attempt}");
    let schema = json!({"type":"object","additionalProperties":false,
        "properties":{"receipt_id":{"type":"string","const":receipt},
            "attempt_id":{"type":"string","const":attempt},"task_id":{"type":"string","const":task.id},
            "outcome":{"type":"string","enum":["completed","blocked","failed"]},
            "summary":{"type":"string","minLength":1}},
        "required":["receipt_id","attempt_id","task_id","outcome","summary"]});
    fs::write(dir.join("result-schema.json"), serde_json::to_vec(&schema)?)?;
    let prompt = format!(
        "{}\n\n{}\n\nReturn your designated final result matching this JSON schema exactly:\n{}\nThe scoped jtasks result client is also available through JTASKS_* environment variables. Helper/tool output is evidence only.\n",
        fs::read_to_string(ticket)?,
        config.prompt,
        schema
    );
    fs::write(dir.join("prompt.txt"), prompt)?;
    let mut command = command(config)?;
    command.arg("exec").arg("--json");
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
            ensure!(!value.trim().is_empty(), "empty Codex {key}");
            command
                .arg("--config")
                .arg(format!("{key}={}", serde_json::to_string(value)?));
        }
    }
    match config.permissions {
        PermissionMode::Autonomous => {
            command.arg("--dangerously-bypass-approvals-and-sandbox");
        }
        PermissionMode::Restricted => {
            command.args([
                "--sandbox",
                "workspace-write",
                "--config",
                "approval_policy=\"never\"",
            ]);
        }
    }
    // Override inherited profiles without changing user configuration. Hook trust
    // is deliberately not bypassed: installed hooks may still block invocation.
    command
        .arg("--output-schema")
        .arg(dir.join("result-schema.json"))
        .arg("--output-last-message")
        .arg(dir.join("final.json"));
    let resumed = if let Some(previous) = &config.resume_attempt {
        crate::domain::validation::identifier(previous)?;
        ensure!(previous != attempt, "cannot resume the current attempt");
        let same_task: bool = rusqlite::Connection::open_with_flags(workspace.database(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?
            .query_row("SELECT EXISTS(SELECT 1 FROM attempts p JOIN attempts n ON p.run_id=n.run_id AND p.task_id=n.task_id WHERE p.id=?1 AND n.id=?2 AND p.state IN ('retired','interrupted'))",[previous,attempt],|r|r.get(0))?;
        ensure!(
            same_task,
            "resume attempt is not a retired attempt in this task lineage"
        );
        let observation: Observation = serde_json::from_slice(&fs::read(
            workspace
                .data
                .join("attempts")
                .join(previous)
                .join("codex-observation.json"),
        )?)?;
        ensure!(
            observation.task_id == task.id && observation.attempt_id == *previous,
            "resume session belongs to another task"
        );
        let thread = observation
            .thread_id
            .context("previous attempt has no exact Codex thread")?;
        ensure!(
            !thread.is_empty() && !thread.starts_with('-'),
            "invalid recorded Codex thread"
        );
        // Exact recorded identity, supplied as an argument. Never --last.
        command.arg("resume").arg(&thread);
        Some(thread)
    } else {
        None
    };
    command
        .arg("-")
        .current_dir(&workspace.checkout)
        .env("JTASKS_CONTROL_ENDPOINT", endpoint.to_string())
        .env("JTASKS_RESULT_TOKEN", token)
        .env("JTASKS_ATTEMPT_ID", attempt)
        .env("JTASKS_TASK_ID", &task.id)
        .env("JTASKS_TICKET_PATH", ticket)
        .env("JTASKS_BIN", std::env::current_exe()?)
        .stdin(File::open(dir.join("prompt.txt"))?)
        .stdout(File::create(dir.join("stdout.log"))?)
        .stderr(File::create(dir.join("stderr.log"))?);
    fs::write(
        dir.join("codex-launch.json"),
        serde_json::to_vec_pretty(&json!({
            "task_id":task.id,"attempt_id":attempt,"configuration":config,"resume_thread":resumed,
            "capabilities":{"direct_execution":true,"raw_output_capture":true,"native_last_message":true,
            "exact_session_resume":true,"observable_commands":"native output only; not managed command custody",
            "permissions":match config.permissions { PermissionMode::Autonomous=>"bypass approvals and sandbox", PermissionMode::Restricted=>"workspace-write, never approve" },
            "effective_configuration":"invocation overrides recorded; native resolved model/account access unknown",
            "limitations":["native helpers are harness-managed", "hook trust may require prior native setup", "batch shims require explicit native or PowerShell executable", "Linux deliberately detached sessions escape process-group custody"]}
        }))?,
    )?;
    crate::execution::custody::OwnedProcess::spawn(&mut command, &dir)
        .context("could not launch the configured Codex executable")
}

/// Parse complete newline-delimited records; source bytes are never rewritten.
/// Chunk boundaries disappear in the capture file. A trailing partial record is
/// preserved and counted as invalid, and cannot become a designated result.
pub fn observe(dir: &Path, task: &str, attempt: &str) -> Result<(Observation, Option<String>)> {
    let mut observation = Observation {
        task_id: task.into(),
        attempt_id: attempt.into(),
        thread_id: None,
        completed_turn: false,
        protocol_error: None,
        unknown_records: 0,
        invalid_records: 0,
        raw_stdout: dir.join("stdout.log"),
        raw_stderr: dir.join("stderr.log"),
        failure_class: None,
    };
    let mut final_text = None;
    let mut started = false;
    let mut reader = BufReader::new(File::open(&observation.raw_stdout)?);
    loop {
        let mut bytes = Vec::new();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            break;
        }
        if !bytes.ends_with(b"\n") {
            observation.invalid_records += 1;
            break;
        }
        bytes.pop();
        if bytes.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                observation.invalid_records += 1;
                continue;
            }
        };
        match value["type"].as_str() {
            Some("thread.started") => {
                if let Some(thread) = value["thread_id"].as_str().filter(|s| !s.is_empty()) {
                    if observation
                        .thread_id
                        .as_deref()
                        .is_some_and(|s| s != thread)
                    {
                        observation.protocol_error =
                            Some("multiple native thread identities".into());
                    } else {
                        observation.thread_id = Some(thread.into());
                    }
                } else {
                    observation.invalid_records += 1;
                }
            }
            Some("turn.started") => {
                started = true;
                observation.completed_turn = false;
                final_text = None;
            }
            Some("item.completed")
                if value["item"]["type"] == "agent_message"
                    && started
                    && !observation.completed_turn =>
            {
                final_text = value["item"]["text"].as_str().map(str::to_owned);
            }
            Some("turn.completed") if started => {
                observation.completed_turn = true;
            }
            Some("turn.failed") | Some("error") => {
                observation.protocol_error = Some("native turn failed".into());
                observation.failure_class = Some(classify_error(&value.to_string()).into());
            }
            Some("item.started" | "item.updated" | "item.completed") => {}
            _ => observation.unknown_records += 1,
        }
    }
    Ok((observation, final_text))
}

pub fn classify_error(text: &str) -> &'static str {
    let text = text.to_ascii_lowercase();
    if text.contains("unauthorized") || text.contains("authentication") {
        "authentication_or_access"
    } else if text.contains("rate_limit") || text.contains("quota") {
        "quota_reset_unknown"
    } else if text.contains("connection") || text.contains("overloaded") {
        "transient_service_or_connection"
    } else {
        "unknown"
    }
}

/// Retain exact session observations even when an attempt is explicitly stopped;
/// cancellation does not make its last available bytes into a result.
pub fn capture_interrupted(workspace: &Workspace, task: &str, attempt: &str) -> Result<()> {
    let dir = workspace.data.join("attempts").join(attempt);
    let (observation, _) = observe(&dir, task, attempt)?;
    fs::write(
        dir.join("codex-observation.json"),
        serde_json::to_vec_pretty(&observation)?,
    )?;
    Ok(())
}

/// Controller-only completion: only the final-message file corroborated by the
/// root exec stream may enter the same receipt transaction as the scoped client.
pub fn finish(
    workspace: &Workspace,
    store: &mut Store,
    run: &str,
    task: &str,
    attempt: &str,
    success: bool,
) -> Result<()> {
    let dir = workspace.data.join("attempts").join(attempt);
    let (mut observation, final_text) = observe(&dir, task, attempt)?;
    let previous: Option<String> = {
        use rusqlite::OptionalExtension;
        store
            .conn
            .query_row(
                "SELECT payload FROM receipts WHERE attempt_id=?1",
                [attempt],
                |r| r.get(0),
            )
            .optional()?
    };
    let mut conflicting_reports = false;
    let result: Result<()> = (|| {
        ensure!(
            observation.protocol_error.is_none(),
            "native stream reported a protocol failure"
        );
        let launch: Value = serde_json::from_slice(&fs::read(dir.join("codex-launch.json"))?)?;
        if let Some(expected) = launch["resume_thread"].as_str() {
            ensure!(
                observation.thread_id.as_deref() == Some(expected),
                "resumed Codex thread differs from exact recorded session"
            );
        }
        // A scoped-client result remains valid without a native final payload.
        if !dir.join("final.json").is_file() {
            return Ok(());
        }
        ensure!(
            success && observation.completed_turn && observation.thread_id.is_some(),
            "native final lacks a successful exact-thread terminal turn"
        );
        let text = fs::read_to_string(dir.join("final.json"))?;
        ensure!(
            final_text.as_deref().map(str::trim) == Some(text.trim()),
            "native last-message differs from terminal root agent payload"
        );
        let receipt: Receipt =
            serde_json::from_str(&text).context("native final is not a designated result")?;
        ensure!(
            receipt.attempt_id == attempt
                && receipt.task_id == task
                && receipt.receipt_id == format!("native-{attempt}"),
            "native result identity differs from launch binding"
        );
        if let Some(previous) = &previous {
            let previous: Receipt = serde_json::from_str(previous)?;
            conflicting_reports =
                previous.outcome != receipt.outcome || previous.summary != receipt.summary;
            ensure!(
                !conflicting_reports,
                "conflicting native and scoped-client reports"
            );
        } else {
            store.accept(run, attempt, &receipt)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let message = error.to_string();
        observation.protocol_error = Some(message.clone());
        // Conflicts invalidate successful completion, while retaining both raw
        // reports and the immutable original receipt for diagnosis.
        if previous.is_none() || conflicting_reports {
            store.conn.execute(
                "UPDATE attempts SET outcome='failed',summary=?2 WHERE id=?1",
                rusqlite::params![attempt, message],
            )?;
        }
        store.conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'native_result_rejected',?4)",rusqlite::params![run,task,attempt,message])?;
    }
    fs::write(
        dir.join("codex-observation.json"),
        serde_json::to_vec_pretty(&observation)?,
    )?;
    Ok(())
}
