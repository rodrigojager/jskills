//! Authored graph changes carry a base revision and explicit ancestry/scope.
use super::types::Plan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub request_id: String,
    pub base_revision: u32,
    pub reason: String,
    pub plan: Plan,
    /// New task -> pending task whose unresolved work it continues.
    #[serde(default)]
    pub ancestry: BTreeMap<String, String>,
    /// Truly new requirement ID -> reason for the visible scope addition.
    #[serde(default)]
    pub scope_additions: BTreeMap<String, String>,
}
