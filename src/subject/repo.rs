//! Repo subject — git URL (clone to tempdir) or local git dir.

use std::path::PathBuf;

pub struct Repo {
    pub root: PathBuf,
    /// `Some` when we cloned to a tempdir; cleaned up on drop.
    pub _tempdir: Option<tempfile::TempDir>,
    /// Original target as the user provided it.
    pub origin: String,
}

pub async fn open(_target: &str) -> anyhow::Result<Repo> {
    anyhow::bail!("subject::repo::open not yet implemented")
}
