//! Parse `.oaudit/config.yml`.
//!
//! Schema (v1, all fields optional):
//!   default_against: ["untrusted/security", "untrusted/supply-chain"]
//!   severity_threshold: high   # exit non-zero at this severity or above

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_against: Vec<String>,
    #[serde(default)]
    pub severity_threshold: Option<String>,
}

pub fn load(_repo_root: &std::path::Path) -> anyhow::Result<Option<Config>> {
    Ok(None) // TODO
}
