//! Discovery is evidence about a scoped installation, never account permission.
//! Refresh uses version/help and bounded native metadata APIs, never a turn.
use crate::{domain::types::Task, storage::Workspace};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const FIELDS: [&str; 6] = [
    "provider",
    "model",
    "effort",
    "variant",
    "native_profile",
    "native_agent",
];
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn version(value: &str) -> bool {
    value.len() < 64
        && value.split('.').count() == 3
        && value
            .split('.')
            .all(|s| !s.is_empty() && s.parse::<u64>().is_ok())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownException {
    pub actor: String,
    pub reason: String,
}
impl UnknownException {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.actor.trim().is_empty() && !self.reason.trim().is_empty(),
            "unknown capability exception requires actor and reason"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub checkout: PathBuf,
    pub executable: String,
    #[serde(default)]
    pub launcher_args: Vec<String>,
    /// Caller-owned configuration identity; no credentials/config files are read.
    pub configuration: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub field: String,
    pub value: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}
/// Completeness may describe one model's options without claiming that every
/// model/provider in the installation exposes the same option set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteScope {
    pub field: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    NativeHelp,
    NativeMetadata,
    ExplicitSnapshot,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub executor: String,
    pub scope: Scope,
    pub executor_version: Option<String>,
    pub source: Source,
    pub observed_at: u64,
    pub recommended_refresh_at: u64,
    /// Only complete dimensions can establish negative catalog membership.
    pub complete: Vec<String>,
    #[serde(default)]
    pub complete_scopes: Vec<CompleteScope>,
    pub choices: Vec<Choice>,
    pub known_invalid: Vec<Choice>,
    /// Completeness/choices apply only to this provider when set.
    pub provider_scope: Option<String>,
    pub supported_options: Vec<String>,
    pub native_flags: Vec<String>,
    pub unknown_fields: Vec<String>,
    pub availability: String,
    #[serde(default = "metadata_unavailable")]
    pub metadata_status: String,
    pub account_access: String,
}
fn metadata_unavailable() -> String {
    "not_requested".into()
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cache {
    schema_version: u32,
    snapshots: Vec<Snapshot>,
}
pub fn name(executor: &str) -> Result<()> {
    ensure!(
        ["codex", "claude", "opencode"].contains(&executor),
        "known-invalid executor; expected codex, claude or opencode"
    );
    Ok(())
}
fn path(workspace: &Workspace) -> PathBuf {
    workspace.data.join("config/catalog.json")
}
fn cache(workspace: &Workspace) -> Result<Cache> {
    match fs::read(path(workspace)) {
        Ok(bytes) => {
            let cache: Cache =
                serde_json::from_slice(&bytes).context("invalid scoped catalog cache")?;
            ensure!(cache.schema_version == 1, "unsupported catalog schema");
            for item in &cache.snapshots {
                validate_snapshot(item)?;
            }
            Ok(cache)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Cache {
            schema_version: 1,
            snapshots: vec![],
        }),
        Err(e) => Err(e.into()),
    }
}
pub fn read(workspace: &Workspace, executor: &str, scope: &Scope) -> Result<Option<Snapshot>> {
    name(executor)?;
    Ok(cache(workspace)?
        .snapshots
        .into_iter()
        .find(|s| s.executor == executor && s.scope == *scope))
}
fn selector(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}
pub fn validate_snapshot(snapshot: &Snapshot) -> Result<()> {
    name(&snapshot.executor)?;
    ensure!(snapshot.schema_version == 1, "unsupported catalog schema");
    ensure!(
        snapshot.scope.checkout.is_absolute()
            && !snapshot.scope.executable.trim().is_empty()
            && selector(&snapshot.scope.configuration),
        "invalid catalog scope"
    );
    ensure!(
        snapshot.recommended_refresh_at >= snapshot.observed_at,
        "invalid catalog freshness interval"
    );
    ensure!(
        snapshot.account_access == "not_verified",
        "catalog discovery cannot establish account access"
    );
    ensure!(
        [
            "not_requested",
            "observed",
            "partial",
            "provider_not_configured",
            "native_interface_unavailable",
            "probe_failed",
            "probe_timeout",
            "cleanup_unknown",
            "capture_incomplete",
            "executable_missing",
            "unsupported_launcher"
        ]
        .contains(&snapshot.metadata_status.as_str()),
        "invalid metadata status"
    );
    ensure!(
        [
            "not_discovered",
            "available_partial",
            "executable_missing",
            "unsupported_launcher",
            "probe_failed",
            "probe_timeout",
            "cleanup_unknown",
            "capture_incomplete",
            "version_unavailable",
            "version_unsupported"
        ]
        .contains(&snapshot.availability.as_str()),
        "invalid discovery availability"
    );
    ensure!(
        snapshot
            .unknown_fields
            .iter()
            .all(|f| FIELDS.contains(&f.as_str()))
            && snapshot
                .supported_options
                .iter()
                .all(|f| options(&snapshot.executor).contains(f))
            && snapshot.native_flags.iter().all(|f| f.starts_with("--")
                && f.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))),
        "invalid catalog option metadata"
    );
    ensure!(
        snapshot.executor_version.as_deref().is_none_or(version),
        "invalid executor version metadata"
    );
    ensure!(
        snapshot
            .complete
            .iter()
            .all(|f| FIELDS.contains(&f.as_str())),
        "invalid completeness dimension"
    );
    for choice in snapshot.choices.iter().chain(&snapshot.known_invalid) {
        ensure!(
            FIELDS.contains(&choice.field.as_str()) && selector(&choice.value),
            "invalid catalog selector"
        );
        ensure!(
            choice.provider.as_deref().is_none_or(selector)
                && choice.model.as_deref().is_none_or(selector),
            "invalid catalog selector scope"
        );
    }
    for scope in &snapshot.complete_scopes {
        ensure!(
            FIELDS.contains(&scope.field.as_str())
                && scope.provider.as_deref().is_none_or(selector)
                && scope.model.as_deref().is_none_or(selector),
            "invalid scoped completeness"
        );
    }
    ensure!(
        snapshot.provider_scope.as_deref().is_none_or(selector),
        "invalid provider scope"
    );
    Ok(())
}
/// Configuration updates have their own lock; they never open the runtime DB.
pub fn save(workspace: &Workspace, mut snapshot: Snapshot, explicit: bool) -> Result<Snapshot> {
    validate_snapshot(&snapshot)?;
    ensure!(
        snapshot.scope.checkout == workspace.checkout,
        "catalog belongs to another checkout"
    );
    if explicit {
        snapshot.source = Source::ExplicitSnapshot;
    }
    let target = path(workspace);
    let parent = target.parent().unwrap();
    fs::create_dir_all(parent)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join("catalog.lock"))?;
    lock.try_lock()
        .context("another catalog refresh is in progress")?;
    let mut cache = cache(workspace)?;
    cache
        .snapshots
        .retain(|s| !(s.executor == snapshot.executor && s.scope == snapshot.scope));
    cache.snapshots.push(snapshot.clone());
    let pending = parent.join("catalog.pending.json");
    use std::io::Write;
    let mut file = fs::File::create(&pending)?;
    file.write_all(&serde_json::to_vec_pretty(&cache)?)?;
    file.sync_all()?;
    fs::rename(pending, target)?;
    Ok(snapshot)
}
pub fn scope(
    workspace: &Workspace,
    executable: &str,
    launcher_args: &[String],
    configuration: &str,
) -> Scope {
    Scope {
        checkout: workspace.checkout.clone(),
        executable: executable.into(),
        launcher_args: launcher_args.to_vec(),
        configuration: configuration.into(),
    }
}
pub fn unavailable(executor: &str, scope: Scope) -> Snapshot {
    Snapshot {
        schema_version: 1,
        executor: executor.into(),
        scope,
        executor_version: None,
        source: Source::Unavailable,
        observed_at: 0,
        recommended_refresh_at: 0,
        complete: vec![],
        complete_scopes: vec![],
        choices: vec![],
        known_invalid: vec![],
        provider_scope: None,
        supported_options: options(executor),
        native_flags: vec![],
        unknown_fields: FIELDS.iter().map(|s| s.to_string()).collect(),
        availability: "not_discovered".into(),
        metadata_status: metadata_unavailable(),
        account_access: "not_verified".into(),
    }
}
fn options(executor: &str) -> Vec<String> {
    let mut fields = vec![
        "provider",
        "model",
        "autonomy",
        "executable",
        "launcher_args",
        "prompt",
    ];
    match executor {
        "codex" => fields.extend(["effort", "native_profile"]),
        "claude" => fields.extend(["effort", "native_agent"]),
        "opencode" => fields.extend(["variant", "native_agent"]),
        _ => {}
    }
    fields.into_iter().map(str::to_string).collect()
}

fn command(executor: &str, scope: &Scope) -> Result<Command> {
    let configured = PathBuf::from(&scope.executable);
    #[cfg(windows)]
    let resolved = if configured.components().count() == 1 && configured.extension().is_none() {
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .flat_map(|dir| {
                        ["exe", "ps1", "cmd"]
                            .map(move |ext| dir.join(format!("{}.{ext}", scope.executable)))
                    })
                    .find(|p| p.is_file())
            })
            .unwrap_or(configured)
    } else {
        configured
    };
    #[cfg(not(windows))]
    let resolved = configured;
    let npm_native = resolved
        .parent()
        .unwrap_or(Path::new("."))
        .join("node_modules/@opencode/cli/bin/opencode.exe");
    let resolved = if executor == "opencode"
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
            .any(|e| extension.eq_ignore_ascii_case(e)),
        "unsupported_launcher"
    );
    let mut command = if extension.eq_ignore_ascii_case("ps1") {
        let mut c = Command::new("pwsh");
        c.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(resolved);
        c
    } else {
        Command::new(resolved)
    };
    command
        .args(&scope.launcher_args)
        .current_dir(&scope.checkout)
        .stdin(Stdio::null());
    Ok(command)
}
/// Pipe readers retain bounded data but drain all output. No raw output/credentials
/// are persisted or returned. Tree custody stops descendants even after leader exit.
pub(crate) fn probe(
    mut command: Command,
    args: &[&str],
    timeout: Duration,
) -> std::result::Result<String, &'static str> {
    let (stdout, writer) = std::io::pipe().map_err(|_| "probe_failed")?;
    let (stderr, errors) = std::io::pipe().map_err(|_| "probe_failed")?;
    command.args(args).stdout(writer).stderr(errors);
    let mut tree = crate::execution::platform::Tree::spawn(
        &mut command,
        &format!("jtasks-discovery-{}", uuid::Uuid::new_v4()),
        |_, _| Ok(()),
    )
    .map_err(|e| {
        if !e.empty {
            "cleanup_unknown"
        } else if e.error.kind() == std::io::ErrorKind::NotFound {
            "executable_missing"
        } else {
            "probe_failed"
        }
    })?;
    drop(command);
    let (send, receive) = std::sync::mpsc::channel();
    for (mut reader, output) in [(stdout, true), (stderr, false)] {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buffer = [0u8; 4096];
            let mut complete = true;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        if output {
                            let take = n.min(262144usize.saturating_sub(kept.len()));
                            kept.extend_from_slice(&buffer[..take]);
                            if take < n {
                                complete = false;
                            }
                        }
                    }
                    Err(_) => {
                        complete = false;
                        break;
                    }
                }
            }
            let _ = send.send((output, kept, complete));
        });
    }
    drop(send);
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut code = None;
    while Instant::now() < deadline {
        match tree.leader_exit() {
            Ok(Some(exit)) => {
                code = exit;
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if code.is_none() {
        timed_out = true;
    }
    let _ = tree.force();
    let cleanup = Instant::now() + Duration::from_secs(2);
    let mut empty = false;
    while Instant::now() < cleanup {
        if tree.leader_exit().ok().flatten().is_some() && tree.empty().ok() == Some(true) {
            empty = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !empty {
        return Err("cleanup_unknown");
    }
    let _ = tree.reap();
    if timed_out {
        return Err("probe_timeout");
    }
    if code != Some(0) {
        return Err("probe_failed");
    }
    let mut text = String::new();
    for _ in 0..2 {
        let (output, bytes, complete) = receive
            .recv_timeout(Duration::from_millis(500))
            .map_err(|_| "capture_incomplete")?;
        if !complete {
            return Err("capture_incomplete");
        }
        if output {
            text = String::from_utf8(bytes).map_err(|_| "capture_incomplete")?;
        }
    }
    Ok(text)
}
/// Retire a discovery scope on every result, including malformed metadata and
/// deadline expiry. A leader's exit alone never proves its helpers have stopped.
fn retire(tree: &mut crate::execution::platform::Tree) -> std::result::Result<(), &'static str> {
    let _ = tree.force();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if tree.leader_exit().ok().flatten().is_some() && tree.empty().ok() == Some(true) {
            return tree.reap().map_err(|_| "cleanup_unknown");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err("cleanup_unknown")
}

/// Stable App Server stdio: initialize, initialized, model/list only. Reader
/// memory, accepted responses, pagination and total elapsed time are bounded.
fn codex_models(
    snapshot: &mut Snapshot,
    provider: Option<&str>,
    timeout: Duration,
) -> std::result::Result<(), &'static str> {
    use std::io::Write;
    let mut command = command("codex", &snapshot.scope).map_err(|_| "unsupported_launcher")?;
    let (input, mut send) = std::io::pipe().map_err(|_| "probe_failed")?;
    let (mut output, writer) = std::io::pipe().map_err(|_| "probe_failed")?;
    command
        .args(["app-server", "--stdio"])
        .stdin(input)
        .stdout(writer)
        .stderr(Stdio::null());
    let mut tree = crate::execution::platform::Tree::spawn(
        &mut command,
        &format!("jtasks-discovery-{}", uuid::Uuid::new_v4()),
        |_, _| Ok(()),
    )
    .map_err(|e| {
        if e.empty {
            "probe_failed"
        } else {
            "cleanup_unknown"
        }
    })?;
    drop(command);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut pending = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut total = 0usize;
        let mut accepted = 0;
        loop {
            let n = match output.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            total += n;
            if total > 2097152 {
                let _ = tx.send(Err("capture_incomplete"));
                break;
            }
            for b in &buffer[..n] {
                if *b == b'\n' {
                    if let Ok(value) = serde_json::from_slice::<Value>(&pending) {
                        if value["id"].as_u64().is_some_and(|id| id <= 8) && accepted < 16 {
                            accepted += 1;
                            let _ = tx.send(Ok(value));
                        }
                    }
                    pending.clear();
                } else {
                    pending.push(*b);
                    if pending.len() > 262144 {
                        let _ = tx.send(Err("capture_incomplete"));
                        return;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let result = (|| {
        let mut request = |value: Value| -> std::result::Result<(), &'static str> {
            let bytes = serde_json::to_vec(&value).map_err(|_| "probe_failed")?;
            send.write_all(&bytes)
                .and_then(|_| send.write_all(b"\n"))
                .and_then(|_| send.flush())
                .map_err(|_| "probe_failed")
        };
        let response = |id: u64| -> std::result::Result<Value, &'static str> {
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err("probe_timeout");
                }
                let value = rx.recv_timeout(left).map_err(|e| {
                    if matches!(e, std::sync::mpsc::RecvTimeoutError::Timeout) {
                        "probe_timeout"
                    } else {
                        "probe_failed"
                    }
                })??;
                if value["id"].as_u64() == Some(id) {
                    return Ok(value);
                }
            }
        };
        request(
            json!({"id":0,"method":"initialize","params":{"clientInfo":{"name":"jtasks","version":env!("CARGO_PKG_VERSION")}}}),
        )?;
        let initialized = response(0)?;
        if !initialized["result"].is_object() || !initialized["error"].is_null() {
            return Err("native_interface_unavailable");
        }
        request(json!({"method":"initialized"}))?;
        let mut cursor = Value::Null;
        snapshot.provider_scope = Some(provider.unwrap_or("native-default").into());
        for id in 1..=8 {
            let mut params = json!({"limit":100,"includeHidden":true,"cursor":cursor});
            if let Some(provider) = provider {
                params["modelProvider"] = json!(provider);
            }
            request(json!({"id":id,"method":"model/list","params":params}))?;
            let value = response(id)?;
            if value["error"]["data"]["type"] == "modelProviderNotConfigured" {
                if let Some(provider) = provider {
                    snapshot.known_invalid.push(Choice {
                        field: "provider".into(),
                        value: provider.into(),
                        provider: None,
                        model: None,
                    });
                }
                return Err("provider_not_configured");
            }
            if !value["error"].is_null() {
                return Err("probe_failed");
            }
            let data = value["result"]["data"]
                .as_array()
                .ok_or("capture_incomplete")?;
            for model in data {
                let model_id = model["model"]
                    .as_str()
                    .filter(|v| selector(v))
                    .ok_or("capture_incomplete")?;
                snapshot.choices.push(Choice {
                    field: "model".into(),
                    value: model_id.into(),
                    provider: provider.map(str::to_string),
                    model: None,
                });
                if let Some(efforts) = model["supportedReasoningEfforts"].as_array() {
                    for effort in efforts {
                        let value = effort["reasoningEffort"]
                            .as_str()
                            .filter(|v| selector(v))
                            .ok_or("capture_incomplete")?;
                        snapshot.choices.push(Choice {
                            field: "effort".into(),
                            value: value.into(),
                            provider: provider.map(str::to_string),
                            model: Some(model_id.into()),
                        });
                    }
                    snapshot.complete_scopes.push(CompleteScope {
                        field: "effort".into(),
                        provider: provider.map(str::to_string),
                        model: Some(model_id.into()),
                    });
                }
            }
            cursor = value["result"]
                .get("nextCursor")
                .cloned()
                .ok_or("capture_incomplete")?;
            if cursor.is_null() {
                snapshot.complete.push("model".into());
                if let Some(provider) = provider {
                    snapshot.choices.push(Choice {
                        field: "provider".into(),
                        value: provider.into(),
                        provider: None,
                        model: None,
                    });
                }
                return Ok(());
            }
            if !cursor.is_string() {
                return Err("capture_incomplete");
            }
        }
        Ok(()) // page cap: preserve partial choices, never claim completeness
    })();
    drop(send);
    retire(&mut tree)?;
    result
}

fn opencode_models(
    snapshot: &mut Snapshot,
    timeout: Duration,
) -> std::result::Result<(), &'static str> {
    // v2.3.0 models handler calls client.model.list(location.cwd); standalone
    // prevents connection to a background/shared service. Output is provider/id.
    let output = probe(
        command("opencode", &snapshot.scope).map_err(|_| "unsupported_launcher")?,
        &["models", "--standalone"],
        timeout,
    )?;
    let mut providers = BTreeSet::new();
    for line in output.lines().filter(|l| !l.trim().is_empty()) {
        let (provider, model) = line.trim().split_once('/').ok_or("capture_incomplete")?;
        if !selector(provider) || provider.contains('/') || !selector(model) {
            return Err("capture_incomplete");
        }
        providers.insert(provider.to_string());
        snapshot.choices.push(Choice {
            field: "model".into(),
            value: model.into(),
            provider: Some(provider.into()),
            model: None,
        });
    }
    for provider in providers {
        snapshot.choices.push(Choice {
            field: "provider".into(),
            value: provider,
            provider: None,
            model: None,
        });
    }
    snapshot
        .complete
        .extend(["provider".into(), "model".into()]);
    Ok(())
}

pub fn refresh(
    executor: &str,
    scope: Scope,
    timeout: Duration,
    provider: Option<&str>,
) -> Result<Snapshot> {
    name(executor)?;
    ensure!(
        provider.is_none_or(selector),
        "invalid provider discovery selector"
    );
    let mut snapshot = unavailable(executor, scope);
    snapshot.observed_at = now();
    snapshot.recommended_refresh_at = now() + 86400;
    let result = (|| -> std::result::Result<(), &'static str> {
        let output = probe(
            command(executor, &snapshot.scope).map_err(|_| "unsupported_launcher")?,
            &["--version"],
            timeout,
        )?;
        snapshot.executor_version = output
            .split_whitespace()
            .find(|part| version(part))
            .map(str::to_string);
        if snapshot.executor_version.is_none() {
            return Err("version_unavailable");
        }
        let parts: Vec<u64> = snapshot
            .executor_version
            .as_ref()
            .unwrap()
            .split('.')
            .filter_map(|v| v.parse().ok())
            .collect();
        let supported = match executor {
            "codex" => parts[0] > 0 || parts[1] >= 169,
            "claude" | "opencode" => parts[0] >= 2,
            _ => false,
        };
        if !supported {
            return Err("version_unsupported");
        }
        let args: &[&str] = match executor {
            "codex" => &["exec", "--help"],
            "opencode" => &["run", "--help"],
            _ => &["--help"],
        };
        let help = probe(
            command(executor, &snapshot.scope).map_err(|_| "unsupported_launcher")?,
            args,
            timeout,
        )?;
        snapshot.native_flags = help
            .split_whitespace()
            .map(|s| s.trim_matches([',', '[', ']']))
            .filter(|s| {
                s.starts_with("--")
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            })
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(())
    })();
    snapshot.availability = match result {
        Ok(()) => {
            snapshot.source = Source::NativeHelp;
            "available_partial".into()
        }
        Err(error) => error.into(),
    };
    if snapshot.availability == "available_partial" {
        let result = match executor {
            "codex" => codex_models(&mut snapshot, provider, timeout),
            "opencode" => opencode_models(&mut snapshot, timeout),
            // Installed Claude help exposes no metadata-only model/agent list;
            // `agents` manages background sessions, not reusable agent names.
            _ => Err("native_interface_unavailable"),
        };
        snapshot.metadata_status = match result {
            Ok(()) => {
                snapshot.source = Source::NativeMetadata;
                if snapshot.complete.is_empty() {
                    "partial"
                } else {
                    "observed"
                }
            }
            Err(error) => error,
        }
        .into();
        if result.is_err() {
            snapshot.choices.clear();
            snapshot.complete.clear();
            snapshot.complete_scopes.clear();
        }
        snapshot
            .unknown_fields
            .retain(|field| !snapshot.complete.contains(field));
    }
    Ok(snapshot)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validation {
    pub state: String,
    pub unknown_fields: Vec<String>,
    pub exception: Option<UnknownException>,
    pub catalog: Option<Snapshot>,
    pub account_access: String,
}
fn matches(choice: &Choice, field: &str, value: &str, values: &Value) -> bool {
    choice.field == field
        && choice.value == value
        && choice
            .provider
            .as_deref()
            .is_none_or(|p| values["provider"].as_str() == Some(p))
        && choice
            .model
            .as_deref()
            .is_none_or(|m| values["model"].as_str() == Some(m))
}
pub fn validate_values(
    workspace: &Workspace,
    executor: &str,
    values: &Value,
    configuration: &str,
    exception: Option<&UnknownException>,
) -> Result<Validation> {
    name(executor)?;
    if let Some(e) = exception {
        e.validate()?;
    }
    let executable = values["executable"].as_str().unwrap_or(executor);
    let launcher_args: Vec<String> =
        serde_json::from_value(values.get("launcher_args").cloned().unwrap_or(json!([])))?;
    let snapshot = read(
        workspace,
        executor,
        &scope(workspace, executable, &launcher_args, configuration),
    )?;
    if let Some(catalog) = &snapshot {
        ensure!(
            ![
                "executable_missing",
                "unsupported_launcher",
                "version_unsupported"
            ]
            .contains(&catalog.availability.as_str()),
            "known-invalid executor installation: {}",
            catalog.availability
        );
    }
    let supported = options(executor);
    // Adapter structural rules precede unknown exceptions and even cached positives.
    for field in FIELDS {
        if values.get(field).is_some_and(|v| !v.is_null()) {
            ensure!(
                supported.iter().any(|f| f == field),
                "known-invalid {executor} option {field}"
            );
        }
    }
    if let Some(effort) = values["effort"].as_str() {
        let choices: &[&str] = if executor == "claude" {
            &["low", "medium", "high", "xhigh", "max"]
        } else {
            &["none", "minimal", "low", "medium", "high", "xhigh"]
        };
        let advertised = executor == "codex"
            && snapshot.as_ref().is_some_and(|catalog| {
                now() < catalog.recommended_refresh_at
                    && catalog
                        .choices
                        .iter()
                        .any(|choice| matches(choice, "effort", effort, values))
            });
        ensure!(
            choices.contains(&effort) || advertised,
            "known-invalid {executor} effort"
        );
    }
    if executor == "claude" {
        if let Some(provider) = values["provider"].as_str() {
            ensure!(
                ["anthropic", "bedrock", "vertex", "foundry"].contains(&provider),
                "known-invalid Claude provider"
            );
        }
    }
    if executor == "opencode" {
        if let Some(provider) = values["provider"].as_str() {
            ensure!(
                selector(provider) && !provider.contains('/'),
                "known-invalid OpenCode provider"
            );
        }
    }
    let mut unknown = vec![];
    for field in FIELDS {
        if !supported.iter().any(|s| s == field) {
            continue;
        }
        let value = values[field].as_str();
        if value.is_none() {
            if ["model", "provider"].contains(&field) {
                unknown.push(field.into());
            }
            continue;
        }
        let value = value.unwrap();
        ensure!(selector(value), "known-invalid {field} selector");
        let mut known = false;
        if let Some(catalog) = &snapshot {
            ensure!(
                !catalog
                    .known_invalid
                    .iter()
                    .any(|c| matches(c, field, value, values)),
                "known-invalid cached {field}"
            );
            let relevant = catalog.provider_scope.as_deref().is_none_or(|p| {
                values["provider"].as_str() == Some(p)
                    || (p == "native-default" && values["provider"].is_null())
            });
            if relevant
                && now() < catalog.recommended_refresh_at
                && catalog.availability == "available_partial"
            {
                known = catalog
                    .choices
                    .iter()
                    .any(|c| matches(c, field, value, values));
                ensure!(
                    known
                        || !(catalog.complete.iter().any(|f| f == field)
                            || catalog.complete_scopes.iter().any(|s| s.field == field
                                && s.provider
                                    .as_deref()
                                    .is_none_or(|p| values["provider"].as_str() == Some(p))
                                && s.model
                                    .as_deref()
                                    .is_none_or(|m| values["model"].as_str() == Some(m)))),
                    "known-invalid {field}: absent from complete scoped catalog"
                );
            }
        }
        if !known {
            unknown.push(field.into());
        }
    }
    ensure!(
        unknown.is_empty() || exception.is_some(),
        "unknown capabilities require recorded unknown_capabilities actor/reason (fields: {})",
        unknown.join(", ")
    );
    Ok(Validation {
        state: if unknown.is_empty() {
            "known"
        } else {
            "unknown_allowed"
        }
        .into(),
        unknown_fields: unknown,
        exception: exception.cloned(),
        catalog: snapshot,
        account_access: "not_verified".into(),
    })
}
pub fn annotate(workspace: &Workspace, task: &Task, values: &Value) -> Result<Validation> {
    let executor = values["executor"]
        .as_str()
        .or_else(|| values["kind"].as_str())
        .context("executor selector missing")?;
    let mut values = values.clone();
    if let Some(effort) = values.get("reasoning_effort").cloned() {
        values["effort"] = effort;
    }
    validate_values(
        workspace,
        executor,
        &values,
        &task.catalog_scope,
        task.unknown_capabilities.as_ref(),
    )
}
