//! File subject — single file or non-git directory.
//!
//! Distinct from `Repo` because there's no git history to read and no
//! worktree-discovery walk: the path the user passed IS the root.
//! `ignore::WalkBuilder` accepts a file as the walk root and yields just
//! that file as a single entry, so `evidence::gather` works against
//! either shape with no special-casing.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct File {
    pub root: PathBuf,
    pub is_dir: bool,
}

pub(crate) async fn open(target: &Path) -> Result<File> {
    let canonical = target
        .canonicalize()
        .with_context(|| format!("path `{}` does not exist or is not accessible", target.display()))?;

    let meta = std::fs::metadata(&canonical)
        .with_context(|| format!("reading metadata for {}", canonical.display()))?;

    if !meta.is_file() && !meta.is_dir() {
        bail!(
            "`{}` is neither a file nor a directory (symlink-to-nowhere, special file, etc.)",
            target.display()
        );
    }

    Ok(File {
        root: canonical,
        is_dir: meta.is_dir(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn opens_a_directory() {
        let tmp = tempdir().unwrap();
        let f = open(tmp.path()).await.unwrap();
        assert!(f.is_dir);
        assert_eq!(f.root.canonicalize().unwrap(), tmp.path().canonicalize().unwrap());
    }

    #[tokio::test]
    async fn opens_a_single_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("a.rs");
        std::fs::write(&path, "fn x() {}").unwrap();
        let f = open(&path).await.unwrap();
        assert!(!f.is_dir);
        assert_eq!(f.root, path.canonicalize().unwrap());
    }

    #[tokio::test]
    async fn errors_on_non_existent_path() {
        let err = open(Path::new("/definitely/not/a/real/path/here"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }
}
