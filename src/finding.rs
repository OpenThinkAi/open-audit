//! Finding type — the structured output from each auditor run.
//!
//! Schema mirrors the auditor specs' output contract. Some fields are optional
//! because the contract varies between trusted/untrusted/privacy specs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub file: String,
    pub line: u32,
    #[serde(default, rename = "endLine", alias = "end_line")]
    pub end_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub title: String,
    pub location: Location,
    #[serde(default)]
    pub additional_locations: Vec<Location>,
    pub evidence: String,
    pub explanation: String,
    #[serde(default)]
    pub attack_path: Option<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub impact: Option<String>,
    #[serde(default)]
    pub user_input: Option<String>,
    pub suggestion: String,
    #[serde(default)]
    pub see_also: Vec<String>,

    // Untrusted-mode additions
    #[serde(default)]
    pub benign_explanation: Option<String>,
    #[serde(default)]
    pub activation: Option<String>,
    #[serde(default)]
    pub impact_if_malicious: Option<String>,

    // Privacy-spec additions
    #[serde(default)]
    pub data_categories: Vec<String>,
    #[serde(default)]
    pub destinations: Vec<String>,
    #[serde(default)]
    pub regulatory_relevance: Vec<String>,
    #[serde(default)]
    pub policy_alignment: Option<String>,

    /// Spec that produced this finding (filled in by the runner).
    #[serde(default)]
    pub spec: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    pub findings: Vec<Finding>,
    pub specs_run: Vec<String>,
    pub subject: String,
}
