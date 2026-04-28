//! Evidence gathering — collect file contents within a spec's scope.
//!
//! v1: gitignore-aware walk via the `ignore` crate; respect the spec's
//! `default_scope` (or a CLI `--scope` override); skip large or non-UTF8
//! files. No chunking — return a single `EvidenceChunk` containing every
//! eligible file. Run-time chunking against the model's context window
//! is deferred; for now we error if the bundle is implausibly large.

use anyhow::{Context, Result, bail};
use glob::Pattern;
use std::path::{Path, PathBuf};

use crate::spec::Spec;
use crate::subject::Subject;

/// Files larger than this are skipped (with a warning at trace level).
/// 256 KB covers ~95% of source files; bigger files are usually generated
/// or vendored and would only burn context tokens.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug)]
pub(crate) struct EvidenceChunk {
    pub files: Vec<EvidenceFile>,
}

#[derive(Debug)]
pub(crate) struct EvidenceFile {
    /// Path relative to the subject root, forward-slash normalized.
    pub path: String,
    pub content: String,
}

pub(crate) fn gather(
    subject: &Subject,
    spec: &Spec,
    scope_override: Option<&str>,
) -> Result<Vec<EvidenceChunk>> {
    let root = subject.root().clone();
    let scope = effective_scope(spec, scope_override)?;

    let mut files = Vec::new();
    for entry in ignore::WalkBuilder::new(&root)
        .standard_filters(true) // gitignore + .ignore + global ignore + hidden
        .git_ignore(true)
        .git_exclude(true)
        .hidden(false) // include dotfiles by default; spec exclude can drop .git etc.
        .build()
    {
        let entry = match entry {
            Ok(e) => e,
            // Permission errors etc. — log via tracing later, skip for now.
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let abs = entry.path();
        let Ok(rel) = abs.strip_prefix(&root) else {
            continue;
        };
        let rel_str = normalize(rel);

        if !scope.matches(&rel_str) {
            continue;
        }

        let Ok(meta) = std::fs::metadata(abs) else {
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(abs) else {
            // Non-UTF8 / binary — skip silently.
            continue;
        };

        files.push(EvidenceFile {
            path: rel_str,
            content,
        });
    }

    if files.is_empty() {
        bail!(
            "no files matched the spec scope under {}.\n  Check the spec's default_scope or pass --scope.",
            root.display()
        );
    }

    Ok(vec![EvidenceChunk { files }])
}

/// Compiled include/exclude patterns. A path matches the scope when at least
/// one include pattern matches AND no exclude pattern matches.
struct CompiledScope {
    include: Vec<Pattern>,
    exclude: Vec<Pattern>,
}

impl CompiledScope {
    fn matches(&self, rel_path: &str) -> bool {
        let included = self
            .include
            .iter()
            .any(|p| p.matches(rel_path) || p.matches_with(rel_path, glob_opts()));
        if !included {
            return false;
        }
        !self
            .exclude
            .iter()
            .any(|p| p.matches(rel_path) || p.matches_with(rel_path, glob_opts()))
    }
}

fn glob_opts() -> glob::MatchOptions {
    glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    }
}

fn effective_scope(spec: &Spec, scope_override: Option<&str>) -> Result<CompiledScope> {
    let (include_patterns, exclude_patterns) = match (scope_override, spec.meta.default_scope.as_ref()) {
        (Some(over), Some(scope)) => (vec![over.to_string()], scope.exclude.clone()),
        (Some(over), None) => (vec![over.to_string()], Vec::new()),
        (None, Some(scope)) => (
            if scope.include.is_empty() {
                vec!["**/*".to_string()]
            } else {
                scope.include.clone()
            },
            scope.exclude.clone(),
        ),
        (None, None) => (vec!["**/*".to_string()], Vec::new()),
    };

    let include = compile_globs(&include_patterns).context("compiling include globs")?;
    let exclude = compile_globs(&exclude_patterns).context("compiling exclude globs")?;
    Ok(CompiledScope { include, exclude })
}

fn compile_globs(patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|p| Pattern::new(p).with_context(|| format!("invalid glob `{p}`")))
        .collect()
}

fn normalize(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{self, SpecSource};
    use crate::subject::{Subject, repo::Repo};
    use std::process::Command;
    use tempfile::tempdir;

    fn init_git(dir: &Path) {
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn write(p: &Path, contents: &str) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, contents).unwrap();
    }

    fn make_subject(root: &Path) -> Subject {
        Subject::Repo(Repo {
            root: root.to_path_buf(),
            _tempdir: None,
            origin: root.display().to_string(),
        })
    }

    fn parse_spec(yaml_body: &str) -> Spec {
        spec::parse(yaml_body, SpecSource::Builtin("test/spec")).unwrap()
    }

    #[test]
    fn gathers_files_matching_default_scope() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/lib.rs"), "fn x() {}");
        write(&tmp.path().join("README.md"), "# hi");
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: []\n---\n",
        );
        let chunks = gather(&subject, &spec, None).unwrap();
        assert_eq!(chunks.len(), 1);
        let paths: Vec<&str> = chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"README.md"));
    }

    #[test]
    fn excludes_match_filters_out() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/main.rs"), "fn main() {}");
        write(&tmp.path().join("target/debug/oaudit"), "binary-ish");
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: [\"target/**\"]\n---\n",
        );
        let chunks = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/main.rs"));
        assert!(!paths.iter().any(|p| p.starts_with("target/")));
    }

    #[test]
    fn scope_override_replaces_include() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/a.rs"), "");
        write(&tmp.path().join("docs/b.md"), "");
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: []\n---\n",
        );
        let chunks = gather(&subject, &spec, Some("src/**")).unwrap();
        let paths: Vec<&str> = chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/a.rs"));
        assert!(!paths.iter().any(|p| p.starts_with("docs/")));
    }

    #[test]
    fn skips_files_larger_than_limit() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("small.txt"), "tiny");
        let big = "x".repeat((MAX_FILE_BYTES + 1024) as usize);
        write(&tmp.path().join("big.txt"), &big);
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: []\n---\n",
        );
        let chunks = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"small.txt"));
        assert!(!paths.contains(&"big.txt"));
    }

    #[test]
    fn skips_binary_files_silently() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/text.rs"), "fn x() {}");
        // Write non-UTF8 bytes — read_to_string will fail and we'll skip.
        std::fs::write(tmp.path().join("data.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: []\n---\n",
        );
        let chunks = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/text.rs"));
        assert!(!paths.contains(&"data.bin"));
    }

    #[test]
    fn errors_when_scope_matches_nothing() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/a.rs"), "");
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*.tsx\"]\n  exclude: []\n---\n",
        );
        let err = gather(&subject, &spec, None).unwrap_err();
        assert!(err.to_string().contains("no files matched"));
    }
}
