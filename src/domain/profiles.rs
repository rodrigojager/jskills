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
            return Ok((
                task.clone(),
                EffectiveProfile {
                    alias: None,
                    values,
                    origins,
                    executor_version: None,
                    catalog_reference: None,
                    discovery: "explicit_executor; native defaults/access not discovered".into(),
                },
            ));
        }
    };
    let snapshot = effective(workspace, alias, overrides, defaults)?;
    let config: ProfileConfig = serde_json::from_value(snapshot.values.clone())?;
    ensure!(
        matches!(config.executor.as_deref(), Some("codex" | "claude")),
        "executor {} is configured; its native adapter is not available in this build",
        config.executor.as_deref().unwrap()
    );
    let mut value = if config.executor.as_deref() == Some("claude") {
        ensure!(
            config.variant.is_none() && config.native_profile.is_none(),
            "Claude adapter does not support variant/native_profile; use native_agent for a native Claude agent"
        );
        json!({"kind":"claude","model":config.model,"provider":config.provider,"effort":config.effort,
            "native_agent":config.native_agent,"permissions":config.autonomy.unwrap()})
    } else {
        ensure!(
            config.variant.is_none() && config.native_agent.is_none(),
            "Codex adapter does not support variant/native_agent; use native_profile for a native Codex profile"
        );
        json!({"kind":"codex", "model":config.model,"provider":config.provider,"reasoning_effort":config.effort,"native_profile":config.native_profile,"permissions":config.autonomy.unwrap()})
    };
    for (key, item) in [
        ("executable", config.executable.map(Value::String)),
        ("launcher_args", config.launcher_args.map(|v| json!(v))),
        ("prompt", config.prompt.map(Value::String)),
    ] {
        if let Some(item) = item {
            value[key] = item;
        }
    }
    let mut task = task.clone();
    task.executor = serde_json::from_value(value)?;
    Ok((task, snapshot))
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
