//! One allowance policy for the entire task, independent of attempts and operations.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub work_retries: u32,
    pub infrastructure_retries: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            work_retries: 2,
            infrastructure_retries: 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Allowance {
    Initial,
    Work,
    Infrastructure,
}
impl Allowance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Work => "work",
            Self::Infrastructure => "infrastructure",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Ledger {
    pub initial: u32,
    pub work: u32,
    pub infrastructure: u32,
    pub work_limit: u32,
    pub infrastructure_limit: u32,
}
