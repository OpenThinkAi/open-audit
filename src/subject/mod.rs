//! Audit subjects — what's being audited.
//!
//! For v1: `Repo` (git URL or local git dir) and `File` (file or non-git dir).
//! Future: `Pkg`, `Image`, `Pr`.

pub mod file;
pub mod repo;

use std::path::PathBuf;

/// A resolved subject ready for evidence gathering.
pub enum Subject {
    Repo(repo::Repo),
    File(file::File),
}

impl Subject {
    pub fn root(&self) -> &PathBuf {
        match self {
            Subject::Repo(r) => &r.root,
            Subject::File(f) => &f.root,
        }
    }
}
