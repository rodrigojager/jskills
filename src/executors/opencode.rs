//! OpenCode 2.x direct runner. Verified against installed 2.3.0 help and
//! anomalyco/opencode v2.3.0 packages/cli/src/{run,session-target.ts}.
//! JSON text/tool/step records have no designated final-result authority.
use crate::{
    domain::types::Task,
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::BufReader,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub use super::codex::PermissionMode;
fn executable() -> String {
    "opencode".into()
}
fn agent() -> String {
    "build".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCodeConfig {
    #[serde(default = "executable")]
    pub executable: String,
    #[serde(default)]
    pub launcher_args: Vec<String>,
    #[serde(default)]
    pub prompt: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    #[serde(default = "agent")]
    pub native_agent: String,
    #[serde(default)]
    pub permissions: PermissionMode,
    pub resume_attempt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Observation {
    #[serde(default)]
    pub decoding: crate::read::output::DecodeProvenance,
    pub task_id: String,
    pub attempt_id: String,
    pub session_id: Option<String>,
    pub step_finishes: usize,
    pub activity_records: usize,
    pub unknown_records: usize,
    pub invalid_records: usize,
    pub protocol_error: Option<String>,
    pub failure_class: Option<String>,
    pub result_source: Option<String>,
    pub raw_stdout: PathBuf,
    pub raw_stderr: PathBuf,
}

fn command(config: &OpenCodeConfig) -> Result<Command> {
    let configured = PathBuf::from(&config.executable);
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
            .unwrap_or(configured)
    } else {
        configured
    };
    #[cfg(not(windows))]
    let resolved = configured;
    // npm's installed 2.x shims all point to this native launcher. Resolve the
    // verified target without executing a batch shell or interpolating arguments.
    let extension = resolved.extension().and_then(|s| s.to_str()).unwrap_or("");
    let npm_native = resolved
        .parent()
        .unwrap_or(Path::new("."))
        .join("node_modules/@opencode/cli/bin/opencode.exe");
    let resolved = if ["cmd", "ps1"]
        .iter()
        .any(|ext| extension.eq_ignore_ascii_case(ext))
        && resolved.file_stem().and_then(|s| s.to_str()) == Some("opencode")
        && npm_native.is_file()
    {
        npm_native
    } else {
        resolved
    };
    let extension = resolved.extension().and_then(|s| s.to_str()).unwrap_or("");
    ensure!(
        !["cmd", "bat"]
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext)),
        "unsupported OpenCode batch shim; select the native 2.x executable or an explicit PowerShell launcher"
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
    Ok(command)
}

fn validate(config: &OpenCodeConfig) -> Result<()> {
    for (field, value) in [
        ("provider", config.provider.as_str()),
        ("model", &config.model),
        ("native_agent", &config.native_agent),
    ] {
        ensure!(
            !value.is_empty() && !value.chars().any(char::is_whitespace) && !value.contains('#'),
            "invalid OpenCode {field}; select a nonempty explicit native identifier"
        );
    }
    ensure!(
        !config.provider.contains('/'),
        "OpenCode provider must not contain a model separator"
    );
    if let Some(variant) = &config.variant {
        ensure!(
            !variant.is_empty()
                && !variant.chars().any(char::is_whitespace)
                && !variant.contains('#'),
            "invalid OpenCode variant"
        );
    }
    Ok(())
}

fn session_id(value: &str) -> bool {
    value.strip_prefix("ses_").is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    })
}

// --session creates a new session if it is absent. Export the exact requested
// session first, through bounded custody, so missing history cannot silently
// become a fresh retry. Native data remains subject to external deletion races.
fn verify_resume(
    workspace: &Workspace,
    config: &OpenCodeConfig,
    dir: &Path,
    session: &str,
) -> Result<()> {
    let probe = dir.join("resume-check");
    fs::create_dir_all(&probe)?;
    let mut command = command(config)?;
    command
        .args(["session", "export", "--standalone", "--sanitize", session])
        .current_dir(&workspace.checkout)
        .env("PWD", &workspace.checkout)
        .stdin(Stdio::null())
        .stdout(File::create(probe.join("stdout.log"))?)
        .stderr(File::create(probe.join("stderr.log"))?);
    let mut process = crate::execution::custody::OwnedProcess::spawn(&mut command, dir)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(exit) = process.poll()? {
            ensure!(
                exit == Some(0),
                "exact OpenCode session unavailable; resume refused without fallback"
            );
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "OpenCode exact-session availability check timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn launch(
    workspace: &Workspace,
    task: &Task,
    config: &OpenCodeConfig,
    attempt: &str,
    endpoint: SocketAddr,
    token: &str,
    ticket: &Path,
) -> Result<crate::execution::custody::OwnedProcess> {
    validate(config)?;
    let dir = workspace.data.join("attempts").join(attempt);
    fs::create_dir_all(&dir)?;
    let resume = if let Some(previous) = &config.resume_attempt {
        crate::domain::validation::identifier(previous)?;
        ensure!(previous != attempt, "cannot resume the current attempt");
        let same_task: bool = rusqlite::Connection::open_with_flags(workspace.database(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?
            .query_row("SELECT EXISTS(SELECT 1 FROM attempts p JOIN attempts n ON p.run_id=n.run_id AND p.task_id=n.task_id WHERE p.id=?1 AND n.id=?2 AND p.state IN ('retired','interrupted'))", [previous, attempt], |r| r.get(0))?;
        ensure!(
            same_task,
            "resume attempt is not a retired attempt in this task lineage"
        );
        let observation: Observation = serde_json::from_slice(&fs::read(
            workspace
                .data
                .join("attempts")
                .join(previous)
                .join("opencode-observation.json"),
        )?)?;
        ensure!(
            observation.task_id == task.id && observation.attempt_id == *previous,
            "resume session belongs to another task"
        );
        let session = observation
            .session_id
            .context("previous attempt has no exact OpenCode session")?;
        ensure!(
            session_id(&session),
            "invalid exact OpenCode session identity"
        );
        verify_resume(workspace, config, &dir, &session)?;
        Some(session)
    } else {
        None
    };
    fs::write(
        dir.join("prompt.txt"),
        format!(
            "{}\n\n{}\n\nSubmit the designated outcome using the scoped jtasks client: JTASKS_BIN result --receipt <unique-id> --outcome completed|blocked|failed --summary <nonempty-summary>. JTASKS_* environment variables bind it to this attempt. Native prose, helper output and JSON step records do not submit a result.\n",
            fs::read_to_string(ticket)?,
            config.prompt
        ),
    )?;
    let mut command = command(config)?;
    command
        .args([
            "run",
            "--standalone",
            "--format",
            "json",
            "--agent",
            &config.native_agent,
            "--model",
        ])
        .arg(format!(
            "{}/{}{}",
            config.provider,
            config.model,
            config
                .variant
                .as_ref()
                .map(|v| format!("#{v}"))
                .unwrap_or_default()
        ));
    if matches!(config.permissions, PermissionMode::Autonomous) {
        command.arg("--auto");
    }
    if let Some(session) = &resume {
        command.arg("--session").arg(session);
    }
    command
        .current_dir(&workspace.checkout)
        .env("PWD", &workspace.checkout)
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
        dir.join("opencode-launch.json"),
        serde_json::to_vec_pretty(&json!({
        "task_id":task.id,"attempt_id":attempt,"configuration":config,"resume_session":resume,
        "capabilities":{"direct_execution":true,"private_server":true,"raw_output_capture":true,
            "structured_native_final":false,"exact_session_resume":true,"result_channel":"scoped_client",
            "permission_asks":if matches!(config.permissions,PermissionMode::Autonomous){"auto-approve unless configured deny"}else{"auto-reject"},
            "limitations":["native configured deny and managed policies remain enforced", "restricted run is permission denial, not an OS sandbox", "native questions auto-cancel; use scoped blocked reporting", "native helpers use their own policies and are not tracked", "step_finish is a step hint, not a task or session outcome", "session availability check cannot prevent external deletion after export", "account/model access unknown", "Linux deliberately detached sessions escape process-group custody"]}}))?,
    )?;
    crate::execution::custody::OwnedProcess::spawn(&mut command, &dir)
        .context("could not launch the configured OpenCode executable")
}

/// Raw bytes stay intact. Complete JSONL records are evidence only; v2.3.0
/// emits step hints and activity, not a constrained designated final envelope.
pub fn observe(dir: &Path, task: &str, attempt: &str) -> Result<Observation> {
    let mut result = Observation {
        decoding: Default::default(),
        task_id: task.into(),
        attempt_id: attempt.into(),
        session_id: None,
        step_finishes: 0,
        activity_records: 0,
        unknown_records: 0,
        invalid_records: 0,
        protocol_error: None,
        failure_class: None,
        result_source: None,
        raw_stdout: dir.join("stdout.log"),
        raw_stderr: dir.join("stderr.log"),
    };
    let mut reader =
        crate::read::output::Records::new(BufReader::new(File::open(&result.raw_stdout)?));
    while let Some(record) = reader.next_record()? {
        let Some(bytes) = record.bytes else {
            result.invalid_records += 1;
            result.protocol_error = Some("native decoding gap; raw capture retained".into());
            continue;
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                reader.gap(record.start, record.end, "invalid_json");
                result.invalid_records += 1;
                continue;
            }
        };
        let kind = value["type"].as_str().unwrap_or("");
        if ![
            "step_start",
            "step_finish",
            "text",
            "reasoning",
            "tool_use",
            "error",
            "retry",
            "background_wait",
        ]
        .contains(&kind)
        {
            result.unknown_records += 1;
            continue;
        }
        // Children can appear in a future format. They never bind root identity.
        if value.get("parentID").is_some_and(|v| !v.is_null()) {
            result.activity_records += 1;
            continue;
        }
        let session = value["sessionID"].as_str().filter(|id| session_id(id));
        if let Some(session) = session {
            if result
                .session_id
                .as_deref()
                .is_some_and(|root| root != session)
            {
                result.protocol_error = Some("conflicting root native session identities".into());
            } else {
                result.session_id = Some(session.into());
            }
            if value["part"]
                .get("sessionID")
                .is_some_and(|id| id.as_str() != Some(session))
            {
                result.protocol_error = Some("native part differs from root session".into());
            }
        } else if kind != "error" {
            result.protocol_error = Some("missing exact native session identity".into());
        }
        if kind == "error" {
            result.failure_class =
                Some(super::codex::classify_error(&value["error"].to_string()).into());
            result.protocol_error = Some("native OpenCode run reported an error".into());
        } else if kind == "step_finish" {
            result.step_finishes += 1;
        } else {
            result.activity_records += 1;
        }
    }
    result.decoding = reader.provenance();
    Ok(result)
}

pub fn capture_interrupted(workspace: &Workspace, task: &str, attempt: &str) -> Result<()> {
    let dir = workspace.data.join("attempts").join(attempt);
    fs::write(
        dir.join("opencode-observation.json"),
        serde_json::to_vec_pretty(&observe(&dir, task, attempt)?)?,
    )?;
    Ok(())
}

pub fn finish(
    workspace: &Workspace,
    store: &mut Store,
    run: &str,
    task: &str,
    attempt: &str,
    _success: bool,
) -> Result<()> {
    let dir = workspace.data.join("attempts").join(attempt);
    let mut observation = observe(&dir, task, attempt)?;
    let accepted: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM receipts WHERE attempt_id=?1)",
        [attempt],
        |r| r.get(0),
    )?;
    if accepted {
        observation.result_source = Some("scoped_client".into());
    }
    let launch: Value = serde_json::from_slice(&fs::read(dir.join("opencode-launch.json"))?)?;
    if let Some(resume) = launch["resume_session"].as_str() {
        if observation
            .session_id
            .as_deref()
            .is_some_and(|id| id != resume)
        {
            observation.protocol_error =
                Some("native session differs from exact resume binding".into());
        }
    }
    // An independently accepted scoped outcome and native retirement are distinct.
    // A malformed or failed native stream cannot revoke that designated receipt.
    if let Some(error) = &observation.protocol_error {
        if !accepted {
            store.conn.execute(
                "UPDATE attempts SET outcome='failed',summary=?2 WHERE id=?1",
                rusqlite::params![attempt, error],
            )?;
        }
        store.conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'native_protocol_observed',?4)", rusqlite::params![run,task,attempt,error])?;
    }
    fs::write(
        dir.join("opencode-observation.json"),
        serde_json::to_vec_pretty(&observation)?,
    )?;
    Ok(())
}
