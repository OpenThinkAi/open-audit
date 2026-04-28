//! Per-spec run orchestration.
//!
//! For each spec: gather evidence → call claude session with system=spec body,
//! user=evidence chunks → parse JSON findings → dedup by `id` across chunks.

use crate::claude_session::ClaudeSession;
use crate::finding::AuditReport;
use crate::spec::Spec;
use crate::subject::Subject;

pub async fn run(
    _subject: &Subject,
    _specs: &[Spec],
    _scope_override: Option<&str>,
    _session: &mut ClaudeSession,
) -> anyhow::Result<AuditReport> {
    anyhow::bail!("run::run not yet implemented")
}
