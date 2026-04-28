//! Spec doc parsing — markdown with YAML frontmatter.
//!
//! TODO: parse frontmatter (gray_matter) into `SpecMeta`, expose `Spec { meta, body, source }`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Prompt,
    Deterministic,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultScope {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecMeta {
    pub name: String,
    pub mode: Mode,
    pub kind: Kind,
    #[serde(default)]
    pub default_scope: Option<DefaultScope>,
    #[serde(default)]
    pub deterministic_checks: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SpecSource {
    Builtin,
    Local(PathBuf),
    AdHoc(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub meta: SpecMeta,
    pub body: String,
    pub source: SpecSource,
}

// TODO
pub fn parse(_text: &str, _source: SpecSource) -> anyhow::Result<Spec> {
    anyhow::bail!("spec::parse not yet implemented")
}
