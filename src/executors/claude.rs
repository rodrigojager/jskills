//! Direct Claude Code print adapter, checked against 2.1.326 --help and
//! https://code.claude.com/docs/en/headless and /agent-sdk/typescript.
//! Only a root result's structured_output is a native designated submission.
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

pub use super::codex::PermissionMode;
fn executable() -> String {
    "claude".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeConfig {
    #[serde(default = "executable")]
    pub executable: String,
    #[serde(default)]
    pub launcher_args: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    pub model: Option<String>,
    /// Explicit provider routes use documented invocation-local environment switches.
    /// None keeps the native installation's provider configuration, recorded as unknown.
    pub provider: Option<String>,
    pub effort: Option<String>,
    pub native_agent: Option<String>,
    #[serde(default)]
    pub permissions: PermissionMode,
    pub resume_attempt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Observation {
    pub task_id: String,
    pub attempt_id: String,
    pub session_id: Option<String>,
    pub completed_turn: bool,
    pub protocol_error: Option<String>,
    pub unknown_records: usize,
    pub invalid_records: usize,
    pub activity_records: usize,
    pub raw_stdout: PathBuf,
    pub raw_stderr: PathBuf,
    pub failure_class: Option<String>,
    pub native_model: Option<String>,
    pub native_permissions: Option<String>,
    pub result_source: Option<String>,
}

fn command(config: &ClaudeConfig) -> Result<Command> {
    let configured = Path::new(&config.executable);
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
    let extension = resolved.extension().and_then(|s| s.to_str()).unwrap_or("");
    ensure!(
        !extension.eq_ignore_ascii_case("cmd") && !extension.eq_ignore_ascii_case("bat"),
        "Claude batch shims are unsupported; select the native executable or a .ps1 launcher explicitly"
    );
    let mut command = if extension.eq_ignore_ascii_case("ps1") {
        let mut command = Command::new("pwsh");
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(&resolved);
        command
    } else {
        Command::new(&resolved)
    };
    command.args(&config.launcher_args);
    for (name, option) in [
        ("model", &config.model),
        ("effort", &config.effort),
        ("native_agent", &config.native_agent),
    ] {
        if let Some(value) = option {
            ensure!(!value.trim().is_empty(), "empty Claude {name}");
        }
    }
    if let Some(effort) = &config.effort {
        ensure!(
            ["low", "medium", "high", "xhigh", "max"].contains(&effort.as_str()),
            "known-invalid Claude effort {effort}"
        );
    }
    if let Some(provider) = &config.provider {
        ensure!(
            ["anthropic", "bedrock", "vertex", "foundry"].contains(&provider.as_str()),
            "unsupported Claude provider {provider}; configure anthropic, bedrock, vertex or foundry explicitly"
        );
        for (name, selected) in [
            ("CLAUDE_CODE_USE_BEDROCK", "bedrock"),
            ("CLAUDE_CODE_USE_VERTEX", "vertex"),
            ("CLAUDE_CODE_USE_FOUNDRY", "foundry"),
        ] {
            command.env(name, if provider == selected { "1" } else { "0" });
        }
    }
    Ok(command)
}

#[allow(clippy::too_many_arguments)]
pub fn launch(
    workspace: &Workspace,
    task: &Task,
    config: &ClaudeConfig,
    attempt: &str,
    endpoint: SocketAddr,
    token: &str,
    ticket: &Path,
) -> Result<crate::execution::custody::OwnedProcess> {
    let dir = workspace.data.join("attempts").join(attempt);
    fs::create_dir_all(&dir)?;
    let schema = json!({"type":"object","additionalProperties":false,"properties":{
        "receipt_id":{"type":"string","const":format!("native-{attempt}")},
        "attempt_id":{"type":"string","const":attempt},"task_id":{"type":"string","const":task.id},
        "outcome":{"type":"string","enum":["completed","blocked","failed"]},"summary":{"type":"string","minLength":1}},
        "required":["receipt_id","attempt_id","task_id","outcome","summary"]});
    fs::write(dir.join("result-schema.json"), serde_json::to_vec(&schema)?)?;
    fs::write(
        dir.join("prompt.txt"),
        format!(
            "{}\n\n{}\n\nReturn the designated final result matching this JSON schema:\n{}\nThe scoped jtasks result client is available through JTASKS_* environment variables. Helper/tool messages are evidence only.\n",
            fs::read_to_string(ticket)?,
            config.prompt,
            schema
        ),
    )?;
    let mut command = command(config)?;
    command
        .args([
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--permission-prompts",
            "none",
        ])
        .arg("--json-schema")
        .arg(serde_json::to_string(&schema)?);
    for (flag, value) in [
        ("--model", &config.model),
        ("--effort", &config.effort),
        ("--agent", &config.native_agent),
    ] {
        if let Some(value) = value {
            command.arg(flag).arg(value);
        }
    }
    let permission = match config.permissions {
        PermissionMode::Autonomous => "bypassPermissions",
        PermissionMode::Restricted => "dontAsk",
    };
    command.arg("--permission-mode").arg(permission);
    if matches!(config.permissions, PermissionMode::Autonomous) {
        command.arg("--dangerously-skip-permissions");
    }
    let (session, resumed) = if let Some(previous) = &config.resume_attempt {
        crate::domain::validation::identifier(previous)?;
        ensure!(previous != attempt, "cannot resume the current attempt");
        let same_task: bool = rusqlite::Connection::open_with_flags(workspace.database(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?
            .query_row("SELECT EXISTS(SELECT 1 FROM attempts p JOIN attempts n ON p.run_id=n.run_id AND p.task_id=n.task_id WHERE p.id=?1 AND n.id=?2 AND p.state IN ('retired','interrupted'))", [previous,attempt], |r|r.get(0))?;
        ensure!(
            same_task,
            "resume attempt is not a retired attempt in this task lineage"
        );
        let observation: Observation = serde_json::from_slice(&fs::read(
            workspace
                .data
                .join("attempts")
                .join(previous)
                .join("claude-observation.json"),
        )?)?;
        ensure!(
            observation.task_id == task.id && observation.attempt_id == *previous,
            "resume session belongs to another task"
        );
        let session = observation
            .session_id
            .context("previous attempt has no exact Claude session")?;
        uuid::Uuid::parse_str(&session).context("recorded Claude session is not an exact UUID")?;
        command.arg("--resume").arg(&session);
        (session, true)
    } else {
        let session = uuid::Uuid::new_v4().to_string();
        command.arg("--session-id").arg(&session);
        (session, false)
    };
    command
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
        dir.join("claude-launch.json"),
        serde_json::to_vec_pretty(&json!({
        "task_id":task.id,"attempt_id":attempt,"configuration":config,"session_id":session,"resumed":resumed,
        "capabilities":{"direct_execution":true,"raw_output_capture":true,"structured_native_final":true,"exact_session_resume":true,
        "permissions":permission,"permission_prompts":"none","provider":config.provider,
        "effective_configuration":"invocation overrides recorded; native init fields observed when supplied; account access unknown",
        "limitations":["native helpers are harness-managed", "native settings/hooks and managed policy still apply", "restricted dontAsk is permission denial, not an OS sandbox", "provider credentials and endpoint configuration remain native", "batch shims require explicit native or PowerShell executable", "Linux deliberately detached sessions escape process-group custody"]}}))?,
    )?;
    crate::execution::custody::OwnedProcess::spawn(&mut command, &dir)
        .context("could not launch the configured Claude executable")
}

/// Captures bytes verbatim; only complete NDJSON records are decoded. Helper
/// records never affect root identity or designate a parent result.
pub fn observe(dir: &Path, task: &str, attempt: &str) -> Result<(Observation, Option<Value>)> {
    let mut observation = Observation {
        task_id: task.into(),
        attempt_id: attempt.into(),
        session_id: None,
        completed_turn: false,
        protocol_error: None,
        unknown_records: 0,
        invalid_records: 0,
        activity_records: 0,
        raw_stdout: dir.join("stdout.log"),
        raw_stderr: dir.join("stderr.log"),
        failure_class: None,
        native_model: None,
        native_permissions: None,
        result_source: None,
    };
    let mut final_value = None;
    let mut initialized = false;
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
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                observation.invalid_records += 1;
                continue;
            }
        };
        if value
            .get("parent_tool_use_id")
            .is_some_and(|v| !v.is_null())
        {
            observation.activity_records += 1;
            continue;
        }
        match (value["type"].as_str(), value["subtype"].as_str()) {
            (Some("system"), Some("init")) => {
                if initialized {
                    observation.protocol_error =
                        Some("multiple native initialization records".into());
                }
                initialized = true;
                observation.session_id = value["session_id"]
                    .as_str()
                    .filter(|s| uuid::Uuid::parse_str(s).is_ok())
                    .map(str::to_owned);
                if observation.session_id.is_none() {
                    observation.protocol_error =
                        Some("missing exact native session identity".into());
                }
                observation.native_model = value["model"].as_str().map(str::to_owned);
                observation.native_permissions =
                    value["permissionMode"].as_str().map(str::to_owned);
            }
            (Some("result"), _) => {
                if observation.completed_turn {
                    observation.protocol_error = Some("multiple root native result records".into());
                }
                if !initialized || value["session_id"].as_str() != observation.session_id.as_deref()
                {
                    observation.protocol_error =
                        Some("native result differs from initialized exact session".into());
                }
                if value["subtype"] != "success" || value["is_error"] != false {
                    observation.protocol_error = Some("native turn failed".into());
                    observation.failure_class =
                        Some(super::codex::classify_error(&value.to_string()).into());
                }
                observation.completed_turn = true;
                final_value = value
                    .get("structured_output")
                    .filter(|v| !v.is_null())
                    .cloned();
            }
            (
                Some("assistant" | "user" | "stream_event" | "tool_progress" | "tool_use_summary"),
                _,
            ) => observation.activity_records += 1,
            _ => observation.unknown_records += 1,
        }
    }
    Ok((observation, final_value))
}

pub fn capture_interrupted(workspace: &Workspace, task: &str, attempt: &str) -> Result<()> {
    let dir = workspace.data.join("attempts").join(attempt);
    let (observation, _) = observe(&dir, task, attempt)?;
    fs::write(
        dir.join("claude-observation.json"),
        serde_json::to_vec_pretty(&observation)?,
    )?;
    Ok(())
}

pub fn finish(
    workspace: &Workspace,
    store: &mut Store,
    run: &str,
    task: &str,
    attempt: &str,
    success: bool,
) -> Result<()> {
    use rusqlite::OptionalExtension;
    let dir = workspace.data.join("attempts").join(attempt);
    let (mut observation, final_value) = observe(&dir, task, attempt)?;
    let previous: Option<String> = store
        .conn
        .query_row(
            "SELECT payload FROM receipts WHERE attempt_id=?1",
            [attempt],
            |r| r.get(0),
        )
        .optional()?;
    observation.result_source = previous.as_ref().map(|_| "scoped_client".into());
    let mut conflict = false;
    let result: Result<()> = (|| {
        ensure!(
            observation.protocol_error.is_none(),
            "native stream reported a protocol failure"
        );
        let launch: Value = serde_json::from_slice(&fs::read(dir.join("claude-launch.json"))?)?;
        if let Some(session) = &observation.session_id {
            ensure!(
                Some(session.as_str()) == launch["session_id"].as_str(),
                "native Claude session differs from exact launch binding"
            );
        }
        if let Some(mode) = &observation.native_permissions {
            ensure!(
                Some(mode.as_str()) == launch["capabilities"]["permissions"].as_str(),
                "native Claude permission mode differs from invocation"
            );
        }
        let Some(value) = final_value else {
            return Ok(());
        };
        ensure!(
            success && observation.completed_turn && observation.session_id.is_some(),
            "native final lacks successful exact-session terminal result"
        );
        let receipt: Receipt = serde_json::from_value(value)
            .context("native structured_output is not a designated result")?;
        ensure!(
            receipt.task_id == task
                && receipt.attempt_id == attempt
                && receipt.receipt_id == format!("native-{attempt}"),
            "native result identity differs from launch binding"
        );
        if let Some(previous) = &previous {
            let previous: Receipt = serde_json::from_str(previous)?;
            conflict = previous.outcome != receipt.outcome || previous.summary != receipt.summary;
            ensure!(!conflict, "conflicting native and scoped-client reports");
            observation.result_source = Some("deduplicated_native_and_scoped_client".into());
        } else {
            store.accept(run, attempt, &receipt)?;
            observation.result_source = Some("native_structured_output".into());
        }
        Ok(())
    })();
    if let Err(error) = result {
        let message = error.to_string();
        observation.protocol_error = Some(message.clone());
        if previous.is_none() || conflict {
            store.conn.execute(
                "UPDATE attempts SET outcome='failed',summary=?2 WHERE id=?1",
                rusqlite::params![attempt, message],
            )?;
        }
        store.conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'native_result_rejected',?4)",rusqlite::params![run,task,attempt,message])?;
    }
    fs::write(
        dir.join("claude-observation.json"),
        serde_json::to_vec_pretty(&observation)?,
    )?;
    Ok(())
}
