//! jtasks aliases are independent of native harness profile/agent identities.
use crate::{
    domain::types::{Executor, Task},
    storage::Workspace,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileConfig {
    pub executor: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub variant: Option<String>,
    pub native_profile: Option<String>,
    pub native_agent: Option<String>,
    pub autonomy: Option<String>,
    pub executable: Option<String>,
    pub launcher_args: Option<Vec<String>>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub schema_version: u32,
    pub profiles: BTreeMap<String, ProfileConfig>,
}
impl Default for ProfileFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profiles: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EffectiveProfile {
    pub alias: Option<String>,
    pub values: Value,
    pub origins: BTreeMap<String, String>,
    /// Discovery/catalog validation is supplied by the scoped catalog feature.
    pub executor_version: Option<String>,
    pub catalog_reference: Option<String>,
    pub discovery: String,
    #[serde(default)]
    pub catalog_validation: Option<crate::executors::catalog::Validation>,
}

pub fn user_file() -> Result<PathBuf> {
    let home = if let Some(home) = std::env::var_os("JTASKS_CONFIG_HOME") {
        PathBuf::from(home)
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA").context("set JTASKS_CONFIG_HOME or APPDATA")?)
            .join("jtasks")
    } else if let Some(home) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(home).join("jtasks")
    } else {
        PathBuf::from(std::env::var_os("HOME").context("set JTASKS_CONFIG_HOME or HOME")?)
            .join(".config/jtasks")
    };
    Ok(home.join("profiles.json"))
}
pub fn project_file(workspace: &Workspace) -> PathBuf {
    workspace.data.join("config/profiles.json")
}

pub fn validate(config: &ProfileConfig) -> Result<()> {
    let value = serde_json::to_value(config)?;
    for (field, value) in value.as_object().unwrap() {
        if let Some(text) = value.as_str() {
            ensure!(
                !text.trim().is_empty() || field == "prompt",
                "profile field {field} cannot be empty"
            );
        }
    }
    if let Some(executor) = &config.executor {
        ensure!(
            matches!(executor.as_str(), "codex" | "claude" | "opencode"),
            "known-invalid executor {executor}; expected codex, claude or opencode"
        );
    }
    if let Some(autonomy) = &config.autonomy {
        ensure!(
            matches!(autonomy.as_str(), "autonomous" | "restricted"),
            "known-invalid autonomy {autonomy}"
        );
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<ProfileFile> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProfileFile::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read profiles {}", path.display()));
        }
    };
    let file: ProfileFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid profiles {}", path.display()))?;
    validate_file(&file)?;
    Ok(file)
}
pub fn validate_file(file: &ProfileFile) -> Result<()> {
    ensure!(
        file.schema_version == 1,
        "unsupported profile schema_version (expected 1)"
    );
    for (alias, config) in &file.profiles {
        super::validation::identifier(alias)?;
        validate(config)?;
    }
    Ok(())
}

/// A project alias replaces the entire user alias; only run defaults fill gaps.
pub fn aliases(workspace: &Workspace) -> Result<BTreeMap<String, (ProfileConfig, String)>> {
    let user = user_file()?;
    let project = project_file(workspace);
    let mut aliases = BTreeMap::new();
    for (path, scope) in [(&user, "user"), (&project, "project")] {
        for (alias, config) in read(path)?.profiles {
            aliases.insert(alias, (config, format!("{scope}:{}", path.display())));
        }
    }
    Ok(aliases)
}

pub fn effective(
    workspace: &Workspace,
    alias: &str,
    overrides: &ProfileConfig,
    defaults: &ProfileConfig,
) -> Result<EffectiveProfile> {
    super::validation::identifier(alias)?;
    let (selected, origin) = aliases(workspace)?.remove(alias).with_context(|| format!("unknown jtasks alias {alias}; native profiles/agents must use native_profile/native_agent"))?;
    let mut values = serde_json::Map::new();
    let mut origins = BTreeMap::new();
    for (config, source) in [
        (defaults, "run_defaults"),
        (&selected, origin.as_str()),
        (overrides, "task"),
    ] {
        validate(config)?;
        for (key, value) in serde_json::to_value(config)?.as_object().unwrap() {
            if !value.is_null() {
                values.insert(key.clone(), value.clone());
                origins.insert(key.clone(), source.into());
            }
        }
    }
    if !values.contains_key("autonomy") {
        values.insert("autonomy".into(), json!("autonomous"));
        origins.insert("autonomy".into(), "supported_default".into());
    }
    if values.get("executor").and_then(Value::as_str) == Some("opencode")
        && !values.contains_key("native_agent")
    {
        values.insert("native_agent".into(), json!("build"));
        origins.insert("native_agent".into(), "supported_default".into());
    }
    let missing: Vec<_> = ["executor", "provider", "model"]
        .into_iter()
        .filter(|key| !values.contains_key(*key))
        .collect();
    ensure!(
        missing.is_empty(),
        "jtasks alias {alias} missing required fields: {}; set alias fields, task overrides or run defaults",
        missing.join(", ")
    );
    for (field, value) in [
        ("executable", values.get("executor").cloned().unwrap()),
        ("launcher_args", json!([])),
        ("prompt", json!("")),
    ] {
        if !values.contains_key(field) {
            values.insert(field.into(), value);
            origins.insert(field.into(), "supported_default".into());
        }
    }
    validate(&serde_json::from_value(Value::Object(values.clone()))?)?;
    Ok(EffectiveProfile {
        alias: Some(alias.into()),
        values: Value::Object(values),
        origins,
        executor_version: None,
        catalog_reference: None,
        discovery: "not_discovered; configuration does not prove account/model access".into(),
        catalog_validation: None,
    })
}

pub fn resolve(
    workspace: &Workspace,
    task: &Task,
    defaults: &ProfileConfig,
) -> Result<(Task, EffectiveProfile)> {
    let (alias, overrides) = match &task.executor {
        Executor::Profile { alias, overrides } => (alias, overrides),
        executor => {
            let values = serde_json::to_value(executor)?;
            let mut origins: BTreeMap<_, _> = values
                .as_object()
                .unwrap()
                .keys()
                .map(|key| (key.clone(), "task".into()))
                .collect();
            let mut values = values;
            if let Executor::Codex { config } = executor {
                let autonomy = match config.permissions {
                    crate::executors::codex::PermissionMode::Autonomous => "autonomous",
                    crate::executors::codex::PermissionMode::Restricted => "restricted",
                };
                values["autonomy"] = json!(autonomy);
                origins.insert("autonomy".into(), "task_or_supported_default".into());
            }
            if let Executor::Claude { config } = executor {
                let autonomy = match config.permissions {
                    crate::executors::claude::PermissionMode::Autonomous => "autonomous",
                    crate::executors::claude::PermissionMode::Restricted => "restricted",
                };
                values["autonomy"] = json!(autonomy);
                origins.insert("autonomy".into(), "task_or_supported_default".into());
            }
            if let Executor::OpenCode { config } = executor {
                values["autonomy"] = serde_json::to_value(&config.permissions)?;
                origins.insert("autonomy".into(), "task_or_supported_default".into());
            }
            let validation = if matches!(executor, Executor::Fixture { .. }) {
                None
            } else {
                Some(crate::executors::catalog::annotate(
                    workspace, task, &values,
                )?)
            };
            let version = validation
                .as_ref()
                .and_then(|v| v.catalog.as_ref())
                .and_then(|c| c.executor_version.clone());
            let reference = validation
                .as_ref()
                .and_then(|v| v.catalog.as_ref())
                .map(|c| format!("{}:{}:{}", c.executor, c.scope.configuration, c.observed_at));
            return Ok((
                task.clone(),
                EffectiveProfile {
                    alias: None,
                    values,
                    origins,
                    executor_version: version,
                    catalog_reference: reference,
                    discovery: "explicit_executor; native defaults/access not discovered".into(),
                    catalog_validation: validation,
                },
            ));
        }
    };
    let mut snapshot = effective(workspace, alias, overrides, defaults)?;
    let config: ProfileConfig = serde_json::from_value(snapshot.values.clone())?;
    let mut task = task.clone();
    task.executor = serde_json::from_value(executor_value(&config)?)?;
    let validation = crate::executors::catalog::annotate(workspace, &task, &snapshot.values)?;
    snapshot.executor_version = validation
        .catalog
        .as_ref()
        .and_then(|c| c.executor_version.clone());
    snapshot.catalog_reference = validation
        .catalog
        .as_ref()
        .map(|c| format!("{}:{}:{}", c.executor, c.scope.configuration, c.observed_at));
    snapshot.discovery = validation.state.clone();
    snapshot.catalog_validation = Some(validation);
    Ok((task, snapshot))
}

/// Resolve an execution observer alias now, from the project's current aliases.
/// Returns the direct observer executor JSON and the effective values with origins.
pub fn resolve_observer(
    workspace: &Workspace,
    alias: &str,
    overrides: &ProfileConfig,
    defaults: &ProfileConfig,
) -> Result<(Value, EffectiveProfile)> {
    let snapshot = effective(workspace, alias, overrides, defaults)?;
    let config: ProfileConfig = serde_json::from_value(snapshot.values.clone())?;
    let executor = config.executor.as_deref().unwrap_or_default();
    ensure!(
        crate::domain::reviews::OBSERVER_EXECUTORS.contains(&executor),
        "execution observer alias {alias} resolves to executor {executor}, which the observer does not support (supported: {})",
        crate::domain::reviews::OBSERVER_EXECUTORS.join(", ")
    );
    Ok((executor_value(&config)?, snapshot))
}

/// The native executor JSON that effective alias values launch.
fn executor_value(config: &ProfileConfig) -> Result<Value> {
    ensure!(
        matches!(
            config.executor.as_deref(),
            Some("codex" | "claude" | "opencode")
        ),
        "executor {} is configured; its native adapter is not available in this build",
        config.executor.as_deref().unwrap_or_default()
    );
    let mut value = if config.executor.as_deref() == Some("opencode") {
        ensure!(
            config.effort.is_none() && config.native_profile.is_none(),
            "OpenCode adapter does not support effort/native_profile; use variant/native_agent"
        );
        json!({"kind":"opencode","provider":config.provider,"model":config.model,"variant":config.variant,
            "native_agent":config.native_agent,"permissions":config.autonomy})
    } else if config.executor.as_deref() == Some("claude") {
        ensure!(
            config.variant.is_none() && config.native_profile.is_none(),
            "Claude adapter does not support variant/native_profile; use native_agent for a native Claude agent"
        );
        json!({"kind":"claude","model":config.model,"provider":config.provider,"effort":config.effort,
            "native_agent":config.native_agent,"permissions":config.autonomy})
    } else {
        ensure!(
            config.variant.is_none() && config.native_agent.is_none(),
            "Codex adapter does not support variant/native_agent; use native_profile for a native Codex profile"
        );
        json!({"kind":"codex", "model":config.model,"provider":config.provider,"reasoning_effort":config.effort,"native_profile":config.native_profile,"permissions":config.autonomy})
    };
    for (key, item) in [
        ("executable", config.executable.clone().map(Value::String)),
        (
            "launcher_args",
            config.launcher_args.clone().map(|v| json!(v)),
        ),
        ("prompt", config.prompt.clone().map(Value::String)),
    ] {
        if let Some(item) = item {
            value[key] = item;
        }
    }
    Ok(value)
}

/// Serialize configuration updates independently of runtime/controller custody.
pub fn update(path: &Path, changes: BTreeMap<String, ProfileConfig>) -> Result<()> {
    for (alias, config) in &changes {
        super::validation::identifier(alias)?;
        validate(config)?;
    }
    let parent = path.parent().context("profile file needs a parent")?;
    fs::create_dir_all(parent)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join("profiles.lock"))?;
    lock.try_lock()
        .context("another profile update is in progress")?;
    let mut file = read(path)?;
    file.profiles.extend(changes);
    let pending = parent.join("profiles.pending.json");
    let result: Result<()> = (|| {
        use std::io::Write;
        let mut output = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&pending)?;
        output.write_all(&serde_json::to_vec_pretty(&file)?)?;
        output.sync_all()?;
        fs::rename(&pending, path)?;
        Ok(())
    })();
    if let Err(error) = result {
        bail!("profile update failed without accepting a partial file: {error}");
    }
    Ok(())
}
