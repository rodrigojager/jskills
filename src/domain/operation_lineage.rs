//! Stable repair facts belong to the logical operation, never a command handle.
use super::operations::Invocation;
use crate::execution::commands::CommandSpec;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const CORRECTION_LIMIT: u32 = 2;

/// Inputs are explicit checkout-relative files/directories; `.` means all inputs.
pub fn valid_input(path: &str) -> bool {
    !path.trim().is_empty()
        && path == path.trim()
        && !path.starts_with(['/', '\\'])
        && !path.contains([':', '*', '?', '[', ']', '\0'])
        && !path.replace('\\', "/").split('/').any(|part| part == "..")
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recheck {
    pub run: String,
    pub operation: String,
    pub cycle: u32,
    pub worker: String,
    pub command_id: String,
    pub revision: String,
    pub summary: String,
}

/// An agent designates the recheck, but cannot substitute an unrelated success.
pub fn matches_check(initial: &Invocation, command: &CommandSpec, checkout: &Path) -> bool {
    let cwd = if initial.cwd.is_absolute() {
        initial.cwd.clone()
    } else {
        checkout.join(&initial.cwd)
    };
    initial.program == command.program
        && initial.args == command.args
        && initial.script == command.script
        && std::fs::canonicalize(cwd)
            .ok()
            .is_some_and(|cwd| std::fs::canonicalize(&command.cwd).ok().as_ref() == Some(&cwd))
        && initial.environment_refs == command.environment_refs
        && initial.resources == command.resources
        && initial.deadline_ms == command.deadline_ms
}

fn normalized(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| {
            if cfg!(windows) {
                part.to_lowercase()
            } else {
                part.into()
            }
        })
        .collect()
}

pub fn overlaps(left: &str, right: &str) -> bool {
    let left = normalized(left);
    let right = normalized(right);
    left.starts_with(&right) || right.starts_with(&left)
}
