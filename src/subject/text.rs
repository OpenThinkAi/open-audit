//! Text subject — an in-memory string with no backing file.
//!
//! Built for auditing untrusted text inputs (issue bodies, RAG snippets,
//! support tickets) without forcing callers to round-trip through a
//! tempfile. The label replaces what would otherwise be `location.file`
//! in findings; default is `stdin` when callers don't pass `--label`.

use anyhow::{Result, bail};

/// Hard ceiling for stdin payloads. Mirrors `evidence::MAX_FILE_BYTES`
/// (256 KB) — same rationale: a single text input larger than that is
/// almost certainly an integration mistake (someone piping a binary or
/// a whole repo through stdin), not a legitimate audit target.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub struct Text {
    pub label: String,
    pub content: String,
}

pub fn new(label: impl Into<String>, content: impl Into<String>) -> Result<Text> {
    let label = label.into();
    let content = content.into();
    if label.is_empty() {
        bail!("--label cannot be empty");
    }
    if content.is_empty() {
        bail!("no input on stdin (read 0 bytes). Pipe a non-empty string to audit.");
    }
    if content.len() > MAX_TEXT_BYTES {
        bail!(
            "stdin input is {} bytes — over the {} byte limit. Audit smaller chunks.",
            content.len(),
            MAX_TEXT_BYTES
        );
    }
    Ok(Text { label, content })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_content() {
        let err = new("stdin", "").unwrap_err();
        assert!(err.to_string().contains("no input on stdin"));
    }

    #[test]
    fn rejects_empty_label() {
        let err = new("", "hello").unwrap_err();
        assert!(err.to_string().contains("--label cannot be empty"));
    }

    #[test]
    fn rejects_oversize_content() {
        let big = "x".repeat(MAX_TEXT_BYTES + 1);
        let err = new("stdin", big).unwrap_err();
        assert!(err.to_string().contains("over the"), "got: {err}");
    }

    #[test]
    fn accepts_valid_input() {
        let t = new("stdin", "audit me").unwrap();
        assert_eq!(t.label, "stdin");
        assert_eq!(t.content, "audit me");
    }
}
