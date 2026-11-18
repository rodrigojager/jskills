//! Finite execution observation is independent of work-retry allowance.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

fn calls() -> u32 {
    3
}
fn deadline() -> u64 {
    120
}
fn extension() -> u64 {
    3600
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default = "observer_scope")]
    pub observer_catalog_scope: String,
    #[serde(default)]
    pub observer_unknown_capabilities: Option<crate::executors::catalog::UnknownException>,
    #[serde(default = "calls")]
    pub max_calls: u32,
    #[serde(default = "deadline")]
    pub deadline_seconds: u64,
    #[serde(default = "extension")]
    pub max_extension_seconds: u64,
    #[serde(
        default,
        deserialize_with = "super::profiles::deserialize_optional_selection"
    )]
    pub observer: Option<Observer>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            observer_catalog_scope: observer_scope(),
            observer_unknown_capabilities: None,
            max_calls: calls(),
            deadline_seconds: deadline(),
            max_extension_seconds: extension(),
            observer: None,
        }
    }
}
fn observer_scope() -> String {
    "execution-observer-read-only".into()
}

/// Observer selection is explicit. Never inherit a worker's bypass permissions,
/// resume its session, or silently switch providers when observation fails.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observer {
    Codex {
        #[serde(flatten)]
        config: crate::executors::codex::CodexConfig,
    },
    Claude {
        #[serde(flatten)]
        config: crate::executors::claude::ClaudeConfig,
    },
    #[serde(rename = "opencode")]
    OpenCode {
        #[serde(flatten)]
        config: crate::executors::opencode::OpenCodeConfig,
    },
    /// A jtasks alias, resolved from the project's aliases when a review is due.
    Profile {
        alias: String,
        #[serde(default)]
        overrides: crate::domain::profiles::ProfileConfig,
    },
}

/// Executors the execution observer has a read-only protocol for.
pub const OBSERVER_EXECUTORS: &[&str] = &["codex", "claude", "opencode"];

/// An observer alias that exists now and names an executor the observer does
/// not support is refused at import; a missing alias is resolved at review time.
pub fn check_observer_aliases(
    workspace: &crate::storage::Workspace,
    plan: &super::types::Plan,
) -> Result<()> {
    let mut aliases = None;
    for task in &plan.tasks {
        let Some(Observer::Profile { alias, overrides }) = &task.reviews.observer else {
            continue;
        };
        let aliases = match &aliases {
            Some(aliases) => aliases,
            None => aliases.insert(super::profiles::aliases(workspace)?),
        };
        let Some((config, _)) = aliases.get(alias) else {
            continue;
        };
        let executor = overrides
            .executor
            .as_ref()
            .or(config.executor.as_ref())
            .or(plan.profile_defaults.executor.as_ref());
        if let Some(executor) = executor {
            ensure!(
                OBSERVER_EXECUTORS.contains(&executor.as_str()),
                "task {}: execution observer alias {alias} resolves to executor {executor}, which the observer does not support (supported: {})",
                task.id,
                OBSERVER_EXECUTORS.join(", ")
            );
        }
    }
    Ok(())
}

pub fn validate(estimate: Option<u64>, policy: &Policy) -> Result<()> {
    if let Some(Observer::Profile { alias, overrides }) = &policy.observer {
        super::validation::identifier(alias)?;
        super::profiles::validate(overrides)?;
    }
    ensure!(
        estimate.is_none_or(|s| s > 0 && s <= (i64::MAX as u64) / 3000),
        "estimate_seconds must be positive and representable"
    );
    ensure!(
        policy.max_calls > 0 && policy.max_calls <= 1000,
        "review max_calls must be 1..1000"
    );
    ensure!(
        policy.deadline_seconds > 0 && policy.deadline_seconds <= 86400,
        "review deadline_seconds must be 1..86400"
    );
    ensure!(
        policy.max_extension_seconds > 0 && policy.max_extension_seconds <= 604800,
        "review max_extension_seconds must be 1..604800"
    );
    Ok(())
}

pub fn first_due(start_ms: u64, estimate_seconds: Option<u64>) -> Option<u64> {
    estimate_seconds.map(|s| start_ms.saturating_add(s.saturating_mul(1500)))
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Continue,
    Retry,
    Inconclusive,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub run: String,
    pub task: String,
    pub attempt: String,
    pub review: String,
    pub generation: u32,
    pub decision: Decision,
    pub rationale: String,
    pub evidence: Vec<String>,
    pub delay_cause: String,
    #[serde(default)]
    pub next_review_after_seconds: Option<u64>,
}
impl Assessment {
    pub fn validate(&self, policy: &Policy) -> Result<()> {
        ensure!(
            !self.rationale.trim().is_empty() && self.rationale.len() <= 4096,
            "review rationale must be concise and nonempty"
        );
        ensure!(
            !self.delay_cause.trim().is_empty() && self.delay_cause.len() <= 2048,
            "review needs a bounded plausible delay cause"
        );
        ensure!(
            !self.evidence.is_empty() && self.evidence.len() <= 16,
            "review needs bounded evidence references"
        );
        const REFERENCES: &[&str] = &[
            "task",
            "elapsed",
            "activity",
            "stdout",
            "stderr",
            "commands",
            "process",
            "diff",
            "previous_review",
            "operations",
        ];
        ensure!(
            self.evidence
                .iter()
                .all(|e| REFERENCES.contains(&e.as_str())),
            "unknown review evidence reference"
        );
        if self.decision == Decision::Continue {
            ensure!(
                self.next_review_after_seconds
                    .is_some_and(|s| s > 0 && s <= policy.max_extension_seconds),
                "continue requires a finite positive bounded next_review_after_seconds"
            );
            ensure!(
                self.evidence
                    .iter()
                    .any(|e| !matches!(e.as_str(), "process" | "activity" | "elapsed")),
                "PID, heartbeat or timestamp alone cannot justify extension"
            );
        }
        Ok(())
    }
}
