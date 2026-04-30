//! Audit subjects — what's being audited.
//!
//! For v1: `Repo` (git URL or local git dir), `File` (file or non-git dir),
//! and `Text` (an in-memory string with no filesystem). Future: `Pkg`,
//! `Image`, `Pr`.

pub mod file;
pub mod repo;
pub mod text;

use std::path::PathBuf;

/// A resolved subject ready for evidence gathering.
pub enum Subject {
    Repo(repo::Repo),
    File(file::File),
    Text(text::Text),
}

impl Subject {
    /// Filesystem anchor for evidence gathering. Only meaningful for
    /// `Repo` and `File`; `Text` has no path on disk and the gather
    /// pipeline must short-circuit before calling this on it.
    pub fn root(&self) -> &PathBuf {
        match self {
            Subject::Repo(r) => &r.root,
            Subject::File(f) => &f.root,
            Subject::Text(_) => panic!(
                "Subject::root() is not defined for Text subjects; \
                 gather must short-circuit on Subject::Text before reaching here"
            ),
        }
    }

    /// Display label for the subject in audit output (`AuditReport.subject`).
    /// Path-shaped for `Repo`/`File`, the user-supplied label for `Text`.
    pub fn label(&self) -> String {
        match self {
            Subject::Repo(r) => r.root.display().to_string(),
            Subject::File(f) => f.root.display().to_string(),
            Subject::Text(t) => t.label.clone(),
        }
    }
}
