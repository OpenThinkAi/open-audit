//! Evidence gathering — collect file contents within a spec's scope.
//!
//! v1: gitignore-aware walk; respect spec `default_scope` (or `--scope` override);
//! batch into context-window-sized chunks. Error on overflow rather than truncate.

use crate::spec::Spec;
use crate::subject::Subject;

pub struct EvidenceChunk {
    pub files: Vec<EvidenceFile>,
}

pub struct EvidenceFile {
    pub path: String, // relative to subject root
    pub content: String,
}

pub fn gather(_subject: &Subject, _spec: &Spec, _scope_override: Option<&str>) -> anyhow::Result<Vec<EvidenceChunk>> {
    anyhow::bail!("evidence::gather not yet implemented")
}
