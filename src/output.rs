//! Output formatters — JSON (default) + human (terminal-pretty).

use crate::cli::Format;
use crate::finding::AuditReport;

pub fn emit(_report: &AuditReport, _format: Format) -> anyhow::Result<()> {
    anyhow::bail!("output::emit not yet implemented")
}
