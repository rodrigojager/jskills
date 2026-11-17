use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(default)]
    pub scheduling: crate::controller::admission::Scheduling,
    #[serde(default)]
    pub profile_defaults: crate::domain::profiles::ProfileConfig,
    pub schema_version: u32,
    pub completion: CompletionPolicy,
    pub operation_placement: crate::domain::operations::Placement,
    #[serde(default)]
    pub operations: Vec<crate::domain::operations::Operation>,
    #[serde(default)]
    pub context: crate::domain::tickets::TicketContext,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionPolicy {
    Judgment,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(default)]
    pub admission: crate::controller::admission::Requirements,
    pub id: String,
    pub title: String,
    #[serde(default, alias = "dependsOn")]
    pub depends_on: Vec<String>,
    pub executor: Executor,
    #[serde(default)]
    pub context: crate::domain::tickets::TicketContext,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Executor {
    Profile {
        alias: String,
        #[serde(default)]
        overrides: crate::domain::profiles::ProfileConfig,
    },
    Codex {
        #[serde(flatten)]
        config: crate::executors::codex::CodexConfig,
    },
    Fixture {
        outcome: FixtureOutcome,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        delay_ms: u64,
        #[serde(default)]
        linger_ms: u64,
        #[serde(default)]
        exit_code: i32,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum FixtureOutcome {
    Completed,
    Blocked,
    Failed,
    Silent,
    Prose,
    HelperJson,
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Blocked,
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub receipt_id: String,
    pub attempt_id: String,
    pub task_id: String,
    pub outcome: Outcome,
    pub summary: String,
}
