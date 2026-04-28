//! File subject — single file or non-git directory.

use std::path::PathBuf;

pub struct File {
    pub root: PathBuf,
    pub is_dir: bool,
}

pub async fn open(_target: &std::path::Path) -> anyhow::Result<File> {
    anyhow::bail!("subject::file::open not yet implemented")
}
