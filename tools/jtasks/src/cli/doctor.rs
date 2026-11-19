use crate::storage::Workspace;
use anyhow::Result;

/// A diagnostic projection never repairs config, launches helpers or authenticates.
pub fn execute(workspace: &Workspace) -> Result<i32> {
    let mut output = super::catalog::executors(workspace)?;
    output["runtime_database_exists"] = serde_json::json!(workspace.database().is_file());
    println!("{}", serde_json::to_string(&output)?);
    Ok(0)
}
