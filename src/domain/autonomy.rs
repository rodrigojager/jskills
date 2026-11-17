//! Effective native permission policy is durable; it is not an OS sandbox.
use crate::{
    domain::{
        profiles::EffectiveProfile,
        types::{Executor, Task},
    },
    executors::codex::PermissionMode,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Autonomous,
    Restricted,
}
impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Autonomous => "autonomous",
            Self::Restricted => "restricted",
        }
    }
}
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS run_autonomy(run_id TEXT PRIMARY KEY REFERENCES runs(id), restricted INTEGER NOT NULL CHECK(restricted IN (0,1)));")?;
    Ok(())
}
/// A launch option can establish policy, never silently change an existing run.
pub fn launch(conn: &Connection, run: &str, restricted: bool) -> Result<()> {
    let previous: Option<bool> = conn
        .query_row(
            "SELECT restricted FROM run_autonomy WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        ensure!(
            !restricted || previous,
            "run autonomy is immutable after launch"
        );
    } else {
        conn.execute(
            "INSERT INTO run_autonomy(run_id,restricted) VALUES(?1,?2)",
            rusqlite::params![run, restricted],
        )?;
    }
    Ok(())
}
pub fn effective(
    conn: &Connection,
    run: &str,
    task: &mut Task,
    snapshot: &mut EffectiveProfile,
) -> Result<()> {
    let restricted: bool = conn
        .query_row(
            "SELECT restricted FROM run_autonomy WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    let prior: Option<String> = conn.query_row("SELECT effective_profile FROM attempts WHERE run_id=?1 AND task_id=?2 ORDER BY rowid LIMIT 1",[run,&task.id],|r|r.get(0)).optional()?;
    let prior = prior
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
        .and_then(|v| v["values"]["autonomy"].as_str().map(str::to_owned));
    let mode = if restricted {
        "restricted"
    } else {
        prior
            .as_deref()
            .unwrap_or_else(|| snapshot.values["autonomy"].as_str().unwrap_or("autonomous"))
    };
    let permissions = if mode == "restricted" {
        PermissionMode::Restricted
    } else {
        PermissionMode::Autonomous
    };
    match &mut task.executor {
        Executor::Codex { config } => config.permissions = permissions,
        Executor::Claude { config } => config.permissions = permissions,
        Executor::OpenCode { config } => config.permissions = permissions,
        _ => {}
    }
    let mode = mode.to_owned();
    snapshot.values["autonomy"] = json!(mode);
    snapshot.values["permissions"] = json!(mode);
    snapshot.values["autonomy_limitations"] = json!([
        "Native permissions are not an OS sandbox",
        "Scope, budgets and required outcomes remain enforced",
        "Native helpers are harness-managed; inherited policy is not guaranteed by jtasks",
        "No credentials or facts may be invented"
    ]);
    let origin = if restricted {
        "persisted_run_restricted"
    } else if prior.is_some() {
        "persisted_task_autonomy"
    } else {
        "resolved_native_permissions"
    };
    snapshot.origins.insert("autonomy".into(), origin.into());
    snapshot.origins.insert("permissions".into(), origin.into());
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub request_id: String,
    pub situation: String,
    pub decision: String,
    pub deviation: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub request_id: String,
    pub question: String,
    pub blocked_requirement: String,
    #[serde(default)]
    pub options: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub request_id: String,
    pub question_id: String,
    pub actor: String,
    pub text: String,
}
