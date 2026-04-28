//! Output formatters — JSON (default, machine-readable) + human (terminal).

use crate::cli::Format;
use crate::evidence::GatherStats;
use crate::finding::{AuditReport, Finding, Severity};
use anyhow::Result;
use console::{Style, style};

pub(crate) fn emit(report: &AuditReport, stats: &GatherStats, format: Format) -> Result<()> {
    match format {
        Format::Json => emit_json(report, stats),
        Format::Human => emit_human(report, stats),
    }
}

fn emit_json(report: &AuditReport, stats: &GatherStats) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Output<'a> {
        #[serde(flatten)]
        report: &'a AuditReport,
        gather: GatherStatsJson<'a>,
    }
    #[derive(serde::Serialize)]
    struct GatherStatsJson<'a> {
        skipped_too_large: u32,
        skipped_binary: u32,
        skipped_io_error: u32,
        io_error_samples: &'a [String],
        /// True when `skipped_io_error > io_error_samples.len()`. Lets
        /// downstream tooling know the sample list isn't the full picture.
        io_error_samples_truncated: bool,
    }
    let truncated = stats.skipped_io_error as usize > stats.io_error_samples.len();
    let out = Output {
        report,
        gather: GatherStatsJson {
            skipped_too_large: stats.skipped_too_large,
            skipped_binary: stats.skipped_binary,
            skipped_io_error: stats.skipped_io_error,
            io_error_samples: &stats.io_error_samples,
            io_error_samples_truncated: truncated,
        },
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn emit_human(report: &AuditReport, stats: &GatherStats) -> Result<()> {
    let dim = Style::new().dim();

    // Header
    println!("{}", style(format!("audit: {}", report.subject)).bold());
    println!(
        "{}",
        dim.apply_to(format!(
            "specs: {}",
            if report.specs_run.is_empty() {
                "(none)".to_string()
            } else {
                report.specs_run.join(", ")
            }
        ))
    );

    // Skip stats — surface BEFORE the verdict so users know what wasn't read.
    if has_skips(stats) {
        let mut parts = Vec::new();
        if stats.skipped_too_large > 0 {
            parts.push(format!("{} too large", stats.skipped_too_large));
        }
        if stats.skipped_binary > 0 {
            parts.push(format!("{} binary", stats.skipped_binary));
        }
        if stats.skipped_io_error > 0 {
            parts.push(format!("{} I/O error", stats.skipped_io_error));
        }
        println!("{}", dim.apply_to(format!("skipped: {}", parts.join(", "))));
        for sample in &stats.io_error_samples {
            println!("{}", dim.apply_to(format!("  - {sample}")));
        }
    }
    println!();

    if report.findings.is_empty() {
        println!("{}", style("no findings").green().bold());
        return Ok(());
    }

    // Sort by severity descending then title for stable output.
    let mut sorted: Vec<&Finding> = report.findings.iter().collect();
    sorted.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.title.cmp(&b.title)));

    for f in &sorted {
        print_finding(f);
    }

    // Summary line
    println!();
    let counts = severity_counts(&report.findings);
    let parts: Vec<String> = [
        ("critical", counts.critical, Style::new().red().bold()),
        ("high", counts.high, Style::new().red()),
        ("medium", counts.medium, Style::new().yellow()),
        ("low", counts.low, Style::new().cyan()),
        ("info", counts.info, Style::new().dim()),
    ]
    .into_iter()
    .filter(|(_, n, _)| *n > 0)
    .map(|(label, n, style)| style.apply_to(format!("{n} {label}")).to_string())
    .collect();
    println!("{}", parts.join(", "));

    // Be explicit about the v1 gate rule so users (especially in CI) know
    // why oaudit just exited 0 with 5 medium findings, or 1 with one high.
    println!("{}", dim.apply_to("gate: high or critical → exit 1"));

    Ok(())
}

fn print_finding(f: &Finding) {
    let sev = severity_tag(f.severity);
    let location = format!("{}:{}", f.location.file, f.location.line);
    println!("{} {} ({})", sev, style(&f.title).bold(), Style::new().dim().apply_to(location));
    if let Some(spec) = &f.spec {
        println!("  {}", Style::new().dim().apply_to(format!("from: {spec}")));
    }
    println!("  {}", f.explanation);
    if !f.suggestion.is_empty() {
        println!("  {}: {}", Style::new().green().apply_to("→"), f.suggestion);
    }
    println!();
}

fn severity_tag(s: Severity) -> String {
    match s {
        Severity::Critical => Style::new().red().bold().apply_to("[CRITICAL]").to_string(),
        Severity::High => Style::new().red().apply_to("[HIGH]").to_string(),
        Severity::Medium => Style::new().yellow().apply_to("[MEDIUM]").to_string(),
        Severity::Low => Style::new().cyan().apply_to("[LOW]").to_string(),
        Severity::Info => Style::new().dim().apply_to("[INFO]").to_string(),
    }
}

#[derive(Default)]
struct SeverityCounts {
    critical: u32,
    high: u32,
    medium: u32,
    low: u32,
    info: u32,
}

fn severity_counts(findings: &[Finding]) -> SeverityCounts {
    let mut c = SeverityCounts::default();
    for f in findings {
        match f.severity {
            Severity::Critical => c.critical += 1,
            Severity::High => c.high += 1,
            Severity::Medium => c.medium += 1,
            Severity::Low => c.low += 1,
            Severity::Info => c.info += 1,
        }
    }
    c
}

fn has_skips(stats: &GatherStats) -> bool {
    stats.skipped_too_large > 0 || stats.skipped_binary > 0 || stats.skipped_io_error > 0
}

/// Determine the process exit code from the report. v1 rule: any
/// `high` or `critical` finding closes the gate (exit 1). Otherwise
/// exit 0. Tool errors get exit 2 from main.
pub(crate) fn exit_code(report: &AuditReport) -> u8 {
    let any_blocking = report
        .findings
        .iter()
        .any(|f| matches!(f.severity, Severity::High | Severity::Critical));
    if any_blocking { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Confidence, Location};

    fn finding(severity: Severity) -> Finding {
        Finding {
            id: "x".to_string(),
            severity,
            confidence: Confidence::High,
            title: "t".to_string(),
            location: Location {
                file: "f".to_string(),
                line: 1,
                end_line: None,
            },
            additional_locations: vec![],
            evidence: String::new(),
            explanation: String::new(),
            attack_path: None,
            prerequisites: vec![],
            impact: None,
            user_input: None,
            suggestion: String::new(),
            see_also: vec![],
            benign_explanation: None,
            activation: None,
            impact_if_malicious: None,
            data_categories: vec![],
            destinations: vec![],
            regulatory_relevance: vec![],
            policy_alignment: None,
            spec: None,
        }
    }

    fn report(findings: Vec<Finding>) -> AuditReport {
        AuditReport {
            findings,
            specs_run: vec!["test".to_string()],
            subject: "/tmp/test".to_string(),
        }
    }

    #[test]
    fn exit_code_zero_when_no_blocking_findings() {
        assert_eq!(exit_code(&report(vec![])), 0);
        assert_eq!(exit_code(&report(vec![finding(Severity::Info)])), 0);
        assert_eq!(exit_code(&report(vec![finding(Severity::Low)])), 0);
        assert_eq!(exit_code(&report(vec![finding(Severity::Medium)])), 0);
    }

    #[test]
    fn exit_code_one_on_high_or_critical() {
        assert_eq!(exit_code(&report(vec![finding(Severity::High)])), 1);
        assert_eq!(exit_code(&report(vec![finding(Severity::Critical)])), 1);
        assert_eq!(
            exit_code(&report(vec![finding(Severity::Low), finding(Severity::High)])),
            1
        );
    }

    #[test]
    fn severity_counts_aggregates() {
        let findings = vec![
            finding(Severity::Critical),
            finding(Severity::High),
            finding(Severity::High),
            finding(Severity::Low),
            finding(Severity::Info),
        ];
        let c = severity_counts(&findings);
        assert_eq!(c.critical, 1);
        assert_eq!(c.high, 2);
        assert_eq!(c.medium, 0);
        assert_eq!(c.low, 1);
        assert_eq!(c.info, 1);
    }
}
