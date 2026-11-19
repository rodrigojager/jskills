//! Global configuration: `<config-dir>/jproject/config.toml`.

use crate::util;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_EMBEDDING: &str = "local/potion-code-16m-v2";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Optional default parent folder suggested by `jproject init`.
    pub projects_root: String,
    /// Name of the versioned folder.
    pub product_dir: String,
    pub work_dir: String,
    pub secrets_dir: String,
    /// jtasks run data, relative to the container.
    pub jtasks_data: String,
    /// Global folder for generic tools (may stay empty).
    pub tools_dir: String,
    /// Path of the global STORAGE.md; empty means `<config-dir>/jproject/STORAGE.md`.
    pub storage_map: String,
    /// Optional folder whose files override the embedded templates.
    pub templates_dir: String,
    /// Create the zvec-grep index on init (only when zvec-grep is installed).
    pub default_index: bool,
    /// Use jtasks in new containers (creates the jtasks data folder).
    pub use_jtasks: bool,
    /// Embedding model for new zvec-grep indexes.
    pub zvec_embedding: String,
    /// Where `jproject tools install jtasks` keeps the jtasks clone.
    pub jtasks_repo: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            projects_root: String::new(),
            product_dir: "product".into(),
            work_dir: "_work".into(),
            secrets_dir: "_secrets".into(),
            jtasks_data: "_work/30-plan/jtasks".into(),
            tools_dir: String::new(),
            storage_map: String::new(),
            templates_dir: String::new(),
            default_index: true,
            use_jtasks: true,
            zvec_embedding: DEFAULT_EMBEDDING.into(),
            jtasks_repo: String::new(),
        }
    }
}

/// `<config-dir>/jproject`, overridable with `JPROJECT_CONFIG_DIR`.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("JPROJECT_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("jproject")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

impl Config {
    pub fn load() -> Result<Option<Config>> {
        let path = config_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let cfg = toml::from_str(&text)
            .with_context(|| format!("invalid config {}; fix it or run `jproject setup`", path.display()))?;
        Ok(Some(cfg))
    }

    /// The saved config, or defaults when there is none (for read-only commands).
    pub fn load_or_default() -> Result<Config> {
        Ok(Self::load()?.unwrap_or_default())
    }

    /// The saved config; runs the setup first when there is none.
    pub fn load_or_setup() -> Result<Config> {
        if let Some(cfg) = Self::load()? {
            return Ok(cfg);
        }
        println!("No jproject configuration found; running `jproject setup` first.");
        crate::setup::run(&crate::setup::SetupArgs::default())
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = config_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let body = toml::to_string_pretty(self)?;
        std::fs::write(&path, format!("# jproject configuration. Edit by hand or run `jproject setup`.\n{body}"))?;
        Ok(path)
    }

    pub fn storage_map_path(&self) -> PathBuf {
        if self.storage_map.trim().is_empty() {
            config_dir().join("STORAGE.md")
        } else {
            PathBuf::from(&self.storage_map)
        }
    }

    pub fn templates_dir(&self) -> Option<&Path> {
        let t = self.templates_dir.trim();
        (!t.is_empty()).then(|| Path::new(t))
    }
}

pub fn has_zvec_grep() -> bool {
    util::which("zg").is_some()
}

/// jtasks counts as installed when its CLI is on PATH or the Claude Code plugin is installed.
pub fn has_jtasks() -> bool {
    util::which("jtasks").is_some() || jtasks_plugin_installed()
}

/// Reads Claude Code's plugin registry (cheap; no process spawned).
pub fn jtasks_plugin_installed() -> bool {
    dirs::home_dir()
        .map(|h| h.join(".claude").join("plugins").join("installed_plugins.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|t| t.contains("\"jtasks@"))
}
