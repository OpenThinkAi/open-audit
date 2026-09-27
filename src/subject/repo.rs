//! Repo subject — local git directory. URL-clone-to-tempdir is deferred.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Repo {
    pub root: PathBuf,
    /// `Some` when we cloned to a tempdir; cleaned up on drop.
    /// v1 is local-path only, so this is always `None`.
    pub _tempdir: Option<tempfile::TempDir>,
    /// Original target as the user provided it.
    pub origin: String,
}

/// Open a git repository at `target`. v1 only handles local paths — URL
/// targets (`http://...`, `git@...`) bail with a clear "not yet implemented"
/// rather than fall through to a path lookup that would silently fail.
pub(crate) async fn open(target: &str) -> Result<Repo> {
    if looks_like_url(target) {
        bail!(
            "URL clone is not yet implemented in v1. Pass a local path to a git repository instead.\n  Got: {target}"
        );
    }

    let path = Path::new(target);
    let canonical = path
        .canonicalize()
        .with_context(|| format!("path `{target}` does not exist or is not accessible"))?;

    if !canonical.is_dir() {
        bail!("`{target}` is not a directory");
    }

    // git2::Repository::open succeeds for bare repos AND for paths inside
    // a worktree (it walks up to find .git). Discover() is the explicit
    // form; we use it so the error names "no git repository found" cleanly
    // when the user pointed at a non-git directory.
    let repo = git2::Repository::discover(&canonical).with_context(|| {
        format!("`{target}` is not a git repository (no .git found here or in any parent)")
    })?;

    // Anchor `root` at the working tree (or the bare repo path if there's
    // no worktree). Evidence gathering walks from this root.
    let root = repo
        .workdir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo.path().to_path_buf());

    // discover() walks UPWARD, so a plain directory inside a larger repo
    // (e.g. ~/Downloads/x under a dotfiles-managed $HOME) would silently
    // widen the audit to that whole repo. Refuse unless the path the user
    // gave IS the repo root.
    let root_canonical = root
        .canonicalize()
        .with_context(|| format!("resolving repository root {}", root.display()))?;
    if root_canonical != canonical {
        bail!(
            "`{target}` is inside a larger git repository rooted at {root}; auditing it as a repo \
             would audit that whole repository.\n  \
             To audit the whole repository: oaudit repo {root}\n  \
             To audit just this directory: oaudit file {target}",
            root = root_canonical.display()
        );
    }

    Ok(Repo {
        root,
        _tempdir: None,
        origin: target.to_string(),
    })
}

fn looks_like_url(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("git://")
        || target.starts_with("ssh://")
        || target.contains("@") && target.contains(":") && !target.starts_with('/')
    // ^ matches `git@github.com:user/repo.git` SSH shorthand
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn init_git_repo(dir: &Path) {
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(dir)
            .status()
            .expect("git init");
        assert!(status.success(), "git init failed");
    }

    #[tokio::test]
    async fn opens_a_local_git_repo() {
        let tmp = tempdir().unwrap();
        init_git_repo(tmp.path());
        let repo = open(tmp.path().to_str().unwrap()).await.unwrap();
        assert_eq!(
            repo.root.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
        assert!(repo._tempdir.is_none());
    }

    #[tokio::test]
    async fn errors_on_non_existent_path() {
        let err = open("/definitely/does/not/exist/oaudit-test")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn errors_on_non_git_directory() {
        let tmp = tempdir().unwrap();
        // exists, is a directory, but no `git init`
        let err = open(tmp.path().to_str().unwrap()).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("not a git repository"),
            "expected 'not a git repository', got: {msg}"
        );
    }

    #[tokio::test]
    async fn errors_on_url_with_clear_message() {
        let err = open("https://github.com/example/repo.git")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("URL clone is not yet implemented"));
    }

    #[tokio::test]
    async fn errors_on_ssh_shorthand_url() {
        let err = open("git@github.com:example/repo.git").await.unwrap_err();
        assert!(err.to_string().contains("URL clone is not yet implemented"));
    }

    #[tokio::test]
    async fn rejects_subdir_of_a_larger_repo() {
        let tmp = tempdir().unwrap();
        init_git_repo(tmp.path());
        let subdir = tmp.path().join("nested");
        std::fs::create_dir(&subdir).unwrap();
        let err = open(subdir.to_str().unwrap()).await.unwrap_err();
        let msg = err.to_string();
        let root = tmp.path().canonicalize().unwrap();
        assert!(msg.contains("inside a larger git repository"), "got: {msg}");
        assert!(msg.contains(&format!("oaudit repo {}", root.display())), "got: {msg}");
        assert!(msg.contains("oaudit file"), "got: {msg}");
    }

    #[tokio::test]
    async fn opens_nested_repo_at_its_own_root() {
        // A repo nested inside another repo is fine when pointed at directly:
        // discover() stops at the nearest .git, which is the given path.
        let tmp = tempdir().unwrap();
        init_git_repo(tmp.path());
        let inner = tmp.path().join("inner");
        std::fs::create_dir(&inner).unwrap();
        init_git_repo(&inner);
        let repo = open(inner.to_str().unwrap()).await.unwrap();
        assert_eq!(repo.root.canonicalize().unwrap(), inner.canonicalize().unwrap());
    }
}
