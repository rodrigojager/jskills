//! Container discovery (§10.7) and the `_work/WORKSPACE.md` front matter.

use crate::config::Config;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The fixed work folders, in order.
pub const WORK_FOLDERS: [&str; 10] = [
    "00-tmp",
    "10-docs",
    "20-design",
    "25-source-assets",
    "30-plan",
    "40-handoff",
    "50-tools",
    "60-intake",
    "70-proposals",
    "80-ideas",
];

#[derive(Debug, Clone, Serialize)]
pub struct Container {
    pub root: PathBuf,
    pub project: String,
    pub product_dir: String,
    pub work_dir: String,
    pub secrets_dir: String,
    pub jtasks_data: String,
}

impl Container {
    pub fn product(&self) -> PathBuf {
        self.root.join(&self.product_dir)
    }
    pub fn work(&self) -> PathBuf {
        self.root.join(&self.work_dir)
    }
    pub fn secrets(&self) -> PathBuf {
        self.root.join(&self.secrets_dir)
    }
    pub fn jtasks(&self) -> PathBuf {
        self.root.join(&self.jtasks_data)
    }
    pub fn workspace_md(&self) -> PathBuf {
        self.work().join("WORKSPACE.md")
    }
    pub fn work_folder(&self, name: &str) -> PathBuf {
        self.work().join(name)
    }

    /// Builds a container description from a front matter map, falling back to config values.
    pub fn from_front_matter(root: &Path, fm: &BTreeMap<String, String>, cfg: &Config) -> Self {
        let get = |k: &str, d: &str| fm.get(k).cloned().filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_string());
        let dir_name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Container {
            root: root.to_path_buf(),
            project: get("project", &dir_name),
            product_dir: get("product_dir", &cfg.product_dir),
            work_dir: get("work_dir", &cfg.work_dir),
            secrets_dir: get("secrets_dir", &cfg.secrets_dir),
            jtasks_data: get("jtasks_data", &cfg.jtasks_data),
        }
    }
}

/// Parses a leading `---` YAML front matter made of `key: value` lines.
pub fn parse_front_matter(text: &str) -> Option<BTreeMap<String, String>> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut map = BTreeMap::new();
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            return Some(map);
        }
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            map.insert(k.trim().to_string(), v);
        }
    }
    None
}

/// Reads a WORKSPACE.md and returns its front matter if it marks a jproject container.
pub fn read_marker(workspace_md: &Path) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(workspace_md).ok()?;
    let fm = parse_front_matter(&text)?;
    (fm.get("jproject").map(|v| v.as_str()) == Some("1")).then_some(fm)
}

/// Checks whether `dir` itself is a container root.
pub fn container_at(dir: &Path, cfg: &Config) -> Option<Container> {
    // Fast path: the configured work folder name.
    if let Some(fm) = read_marker(&dir.join(&cfg.work_dir).join("WORKSPACE.md")) {
        if fm.get("work_dir").map_or(true, |w| w == &cfg.work_dir) {
            return Some(Container::from_front_matter(dir, &fm, cfg));
        }
    }
    // Any child folder whose WORKSPACE.md names it as the work folder (custom names).
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(fm) = read_marker(&entry.path().join("WORKSPACE.md")) {
            if fm.get("work_dir").map_or(name == cfg.work_dir, |w| w == &name) {
                return Some(Container::from_front_matter(dir, &fm, cfg));
            }
        }
    }
    None
}

/// Walks up from `start` to the first container (so `product/` and `_work/` resolve to their parent).
pub fn discover(start: &Path, cfg: &Config) -> Option<Container> {
    let start = std::path::absolute(start).ok()?;
    start.ancestors().find_map(|dir| container_at(dir, cfg))
}

/// The container for a command: `--path` if given, else the current directory.
pub fn locate(path: Option<&Path>, cfg: &Config) -> Result<Container> {
    let start = match path {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    match discover(&start, cfg) {
        Some(c) => Ok(c),
        None => bail!(
            "Not inside a jproject container ({}). Run `jproject init` or pass `--path <container>`.",
            start.display()
        ),
    }
}
