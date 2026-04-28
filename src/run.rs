//! Per-spec run orchestration. Stubbed until chunk E.

use crate::finding::AuditReport;
use crate::spec::Spec;
use crate::subject::Subject;

pub(crate) async fn run(
    _subject: &Subject,
    _specs: &[Spec],
    _scope_override: Option<&str>,
) -> anyhow::Result<AuditReport> {
    anyhow::bail!("run::run not yet implemented")
}
