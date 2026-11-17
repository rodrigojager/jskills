//! Shared command input and capture boundary. Environment values never enter receipts.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub request_id: String,
    pub run: String,
    pub task: Option<String>,
    pub attempt: Option<String>,
    pub operation: Option<String>,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Explicit real-shell script, appended after the supplied shell arguments.
    pub script: Option<String>,
    pub cwd: PathBuf,
    /// Child variable -> controller variable name. Values are resolved only at spawn.
    #[serde(default)]
    pub environment_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub resources: Vec<String>,
    pub deadline_ms: Option<u64>,
}

impl CommandSpec {
    pub fn validate(&self) -> Result<()> {
        crate::domain::validation::identifier(&self.request_id)?;
        crate::domain::validation::identifier(&self.run)?;
        ensure!(!self.program.trim().is_empty(), "program is required");
        ensure!(
            self.cwd.is_absolute() && self.cwd.is_dir(),
            "cwd must be an existing absolute directory"
        );
        ensure!(
            self.task.is_some() == self.attempt.is_some(),
            "task and attempt must be supplied together"
        );
        ensure!(
            self.task.is_some()
                || self
                    .operation
                    .as_ref()
                    .is_some_and(|v| !v.trim().is_empty()),
            "a task attempt or operation binding is required"
        );
        ensure!(self.deadline_ms != Some(0), "deadline must be positive");
        for (name, reference) in &self.environment_refs {
            ensure!(
                !name.is_empty() && !name.contains(['=', '\0']) && !reference.is_empty(),
                "invalid environment reference"
            );
        }
        Ok(())
    }
}

pub fn spawn(spec: &CommandSpec, output_dir: &Path) -> Result<super::custody::OwnedProcess> {
    spec.validate()?;
    fs::create_dir_all(output_dir)?;
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(script) = &spec.script {
        command.arg(script);
    }
    for (name, reference) in &spec.environment_refs {
        command.env(
            name,
            std::env::var_os(reference)
                .context("referenced environment variable is unavailable")?,
        );
    }
    command
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(File::create(output_dir.join("stdout.log"))?)
        .stderr(File::create(output_dir.join("stderr.log"))?);
    // Direct file handles drain both streams without blocking the controller on pipes.
    super::custody::OwnedProcess::spawn(&mut command, output_dir)
        .context("command program could not launch")
}
