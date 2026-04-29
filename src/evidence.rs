//! Evidence gathering — collect file contents within a spec's scope.
//!
//! v1: gitignore-aware walk via the `ignore` crate; respect the spec's
//! `default_scope` (or a CLI `--scope` override); skip large or non-UTF8
//! files. No chunking — return a single `EvidenceChunk` containing every
//! eligible file. Run-time chunking against the model's context window
//! is deferred; if the bundle is too large for the model, the failure
//! surfaces as an API error from the LLM call layer (chunk E owns
//! turning that into a friendlier message).
//!
//! Skips are counted in the returned `GatherStats` so the CLI layer can
//! eventually surface them to the user before the verdict ("skipped 12
//! files: 3 too large, 9 binary"). The infrastructure ships here; the CLI
//! presentation is wired up in the run/output module (chunk E in v1).
//! Audit results that silently ignore half the repo aren't useful results
//! — `GatherStats` exists so we don't ship that case.

use anyhow::{Context, Result, bail};
use glob::Pattern;
use std::path::Path;

use crate::spec::Spec;
use crate::subject::Subject;

/// Files larger than this are skipped (counted in `GatherStats`).
/// 256 KB covers ~95% of source files; bigger files are usually generated
/// or vendored and would only burn context tokens. Hard-coded for v1;
/// expose as a spec field or `--max-file-bytes` flag later if real users
/// hit the limit on legitimate content.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug)]
pub(crate) struct GatherResult {
    pub chunks: Vec<EvidenceChunk>,
    pub stats: GatherStats,
}

#[derive(Debug, Default)]
pub(crate) struct GatherStats {
    pub skipped_too_large: u32,
    pub skipped_binary: u32,
    /// Walker entry errors AND per-file metadata failures. Both cases
    /// share this counter because they're the same failure category from
    /// the user's perspective: "couldn't read this file." Samples below
    /// capture context for the first few.
    pub skipped_io_error: u32,
    /// First few I/O error messages captured so the user has something
    /// to act on when `skipped_io_error > 0`. Cap is small on purpose —
    /// a sample, not a flood.
    pub io_error_samples: Vec<String>,
}

const IO_ERROR_SAMPLE_CAP: usize = 5;

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
) -> Result<GatherResult> {
    let root = subject.root();

    // Single-file subject: no walk, no scope filter. The user pointed at
    // exactly one file and said "audit it" — gather should give them
    // exactly that file. Avoids the empty-rel-path corner of WalkBuilder
    // where the walk root and the only entry are the same path.
    //
    // --scope is meaningless here (there's nothing to filter against)
    // and silently ignoring it would surprise users who passed it as a
    // safety filter. Loud-fail instead.
    if root.is_file() {
        if scope_override.is_some() {
            bail!(
                "--scope has no effect when the target is a single file. \
                 Remove --scope, or pass a directory instead."
            );
        }
        return single_file_chunk(root);
    }

    let scope = effective_scope(spec, scope_override)?;

    let mut files = Vec::new();
    let mut stats = GatherStats::default();

    for entry in ignore::WalkBuilder::new(root)
        // standard_filters() turns on gitignore + .ignore + global ignore + hidden.
        // We then call hidden(false) to re-enable dotfile traversal — spec
        // excludes are how you drop .git/, not the walker.
        .standard_filters(true)
        .hidden(false)
        .build()
    {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                record_io_error(&mut stats, e.to_string());
                continue;
            }
        };
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let abs = entry.path();
        let Ok(rel) = abs.strip_prefix(root) else {
            continue;
        };
        let rel_str = normalize(rel);

        if !scope.matches(&rel_str) {
            continue;
        }

        // Use the walker's cached metadata where available (saves a stat).
        let len = match entry.metadata() {
            Ok(m) => m.len(),
            Err(e) => {
                record_io_error(&mut stats, format!("{}: {e}", abs.display()));
                continue;
            }
        };
        if len > MAX_FILE_BYTES {
            stats.skipped_too_large += 1;
            continue;
        }

        let Ok(content) = std::fs::read_to_string(abs) else {
            // Non-UTF8 / binary.
            stats.skipped_binary += 1;
            continue;
        };

        files.push(EvidenceFile {
            path: rel_str,
            content,
        });
    }

    if files.is_empty() {
        bail!(
            "no files matched after applying include patterns AND spec excludes under {}.\n  \
             Both default_scope.include + default_scope.exclude (and --scope, if passed) are in play. \
             Check that the includes cover the right files AND that no exclude pattern is clobbering them.",
            root.display()
        );
    }

    Ok(GatherResult {
        chunks: vec![EvidenceChunk { files }],
        stats,
    })
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
            .any(|p| p.matches_with(rel_path, GLOB_OPTS));
        if !included {
            return false;
        }
        !self.exclude.iter().any(|p| p.matches_with(rel_path, GLOB_OPTS))
    }
}

/// Gitignore-style separator semantics: `*` does NOT cross `/`, `**` does.
/// `src/*.rs` matches `src/foo.rs` only; `**/*` matches files at any depth.
/// Diverges from the glob crate's default so spec authors get the rule
/// they expect from gitignore / ripgrep / fd.
const GLOB_OPTS: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

/// Excludes applied when a spec doesn't declare its own scope. Just enough
/// to keep the worst footguns (`.git/`, `node_modules/`, `target/`) out of
/// the prompt — specs SHOULD declare their own scope, but if they don't,
/// we shouldn't ship `.git/objects/` to the LLM.
const FALLBACK_EXCLUDES: &[&str] = &[".git/**", "node_modules/**", "target/**"];

/// Build a single-file `GatherResult` directly. Skip the WalkBuilder /
/// scope-glob path entirely; the user pointed at exactly one file.
fn single_file_chunk(path: &std::path::Path) -> Result<GatherResult> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    if meta.len() > MAX_FILE_BYTES {
        bail!(
            "{} is {} bytes — over the {} byte per-file limit. Pass a smaller file.",
            path.display(),
            meta.len(),
            MAX_FILE_BYTES
        );
    }
    let content = std::fs::read_to_string(path).with_context(|| {
        format!(
            "reading {} as UTF-8 (binary files aren't supported as a single-file subject)",
            path.display()
        )
    })?;
    // canonicalize() above guarantees file_name() is Some; to_str() only
    // fails on non-UTF8 filenames, which we treat as out-of-scope for v1.
    let rel = path
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("file name {} is not valid UTF-8", path.display()))?
        .to_string();
    Ok(GatherResult {
        chunks: vec![EvidenceChunk {
            files: vec![EvidenceFile { path: rel, content }],
        }],
        stats: GatherStats::default(),
    })
}

fn record_io_error(stats: &mut GatherStats, msg: String) {
    stats.skipped_io_error += 1;
    if stats.io_error_samples.len() < IO_ERROR_SAMPLE_CAP {
        stats.io_error_samples.push(msg);
    }
}

/// Build the compiled include/exclude patterns from the spec + override.
///
/// Override semantics: `scope_override` (CLI `--scope`) REPLACES the spec's
/// include list but PRESERVES the spec's exclude list. The intent is "limit
/// to this subtree, but keep the spec's safety excludes (target/, etc.)
/// active." A user who wants a true clean slate should set both via a
/// custom spec file rather than `--scope`.
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
        (None, None) => (
            vec!["**/*".to_string()],
            FALLBACK_EXCLUDES.iter().map(|s| s.to_string()).collect(),
        ),
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
        let res = gather(&subject, &spec, None).unwrap();
        assert_eq!(res.chunks.len(), 1);
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"README.md"));
        assert_eq!(res.stats.skipped_too_large, 0);
        assert_eq!(res.stats.skipped_binary, 0);
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
        let res = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
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
        let res = gather(&subject, &spec, Some("src/**")).unwrap();
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/a.rs"));
        assert!(!paths.iter().any(|p| p.starts_with("docs/")));
    }

    #[test]
    fn scope_override_preserves_spec_exclude() {
        // --scope src/** + spec excludes target/** → target/foo.rs still
        // excluded even though --scope just says "src/**" (which doesn't
        // match it anyway, but tests the merge rule).
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/a.rs"), "");
        write(&tmp.path().join("src/sub/b.rs"), "");
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: [\"src/sub/**\"]\n---\n",
        );
        let res = gather(&subject, &spec, Some("src/**")).unwrap();
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/a.rs"));
        assert!(!paths.iter().any(|p| p.starts_with("src/sub/")));
    }

    #[test]
    fn skips_files_larger_than_limit_and_counts_them() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("small.txt"), "tiny");
        let big = "x".repeat((MAX_FILE_BYTES + 1024) as usize);
        write(&tmp.path().join("big.txt"), &big);
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: []\n---\n",
        );
        let res = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"small.txt"));
        assert!(!paths.contains(&"big.txt"));
        assert_eq!(res.stats.skipped_too_large, 1);
    }

    #[test]
    fn skips_binary_files_and_counts_them() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path());
        write(&tmp.path().join("src/text.rs"), "fn x() {}");
        std::fs::write(tmp.path().join("data.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let subject = make_subject(tmp.path());
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*\"]\n  exclude: []\n---\n",
        );
        let res = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/text.rs"));
        assert!(!paths.contains(&"data.bin"));
        assert_eq!(res.stats.skipped_binary, 1);
    }

    #[test]
    fn fallback_excludes_drop_git_dir_when_spec_omits_scope() {
        let tmp = tempdir().unwrap();
        init_git(tmp.path()); // creates .git/
        write(&tmp.path().join("src/lib.rs"), "fn x() {}");
        let subject = make_subject(tmp.path());
        // Spec with NO default_scope at all — exercises the (None, None)
        // branch in effective_scope which now ships fallback excludes.
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\n---\n",
        );
        let res = gather(&subject, &spec, None).unwrap();
        let paths: Vec<&str> = res.chunks[0].files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/lib.rs"));
        assert!(
            !paths.iter().any(|p| p.starts_with(".git/")),
            "fallback excludes should drop .git/, got: {paths:?}"
        );
    }

    #[test]
    fn single_file_subject_includes_the_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("foo.rs");
        write(&path, "fn x() {}");
        let subject = Subject::File(crate::subject::file::File {
            root: path.clone(),
        });
        let spec = parse_spec(
            "---\nname: t\nmode: trusted\nkind: prompt\ndefault_scope:\n  include: [\"**/*.tsx\"]\n  exclude: []\n---\n",
        );
        // Even though the spec scope wouldn't match a `.rs` file under
        // a directory walk, single-file mode bypasses scope entirely.
        let res = gather(&subject, &spec, None).unwrap();
        assert_eq!(res.chunks.len(), 1);
        assert_eq!(res.chunks[0].files.len(), 1);
        assert_eq!(res.chunks[0].files[0].path, "foo.rs");
        assert_eq!(res.chunks[0].files[0].content, "fn x() {}");
    }

    #[test]
    fn single_file_subject_rejects_scope_override() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("foo.rs");
        write(&path, "fn x() {}");
        let subject = Subject::File(crate::subject::file::File { root: path });
        let spec = parse_spec("---\nname: t\nmode: trusted\nkind: prompt\n---\n");
        let err = gather(&subject, &spec, Some("**/*.tsx")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--scope"), "got: {msg}");
        assert!(msg.contains("single file"), "got: {msg}");
    }

    #[test]
    fn single_file_subject_rejects_oversize() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("big.txt");
        let big = "x".repeat((MAX_FILE_BYTES + 1024) as usize);
        write(&path, &big);
        let subject = Subject::File(crate::subject::file::File {
            root: path,
        });
        let spec = parse_spec("---\nname: t\nmode: trusted\nkind: prompt\n---\n");
        let err = gather(&subject, &spec, None).unwrap_err();
        assert!(err.to_string().contains("over the"), "got: {err}");
    }

    #[test]
    fn single_file_subject_rejects_binary() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("bin.dat");
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let subject = Subject::File(crate::subject::file::File {
            root: path,
        });
        let spec = parse_spec("---\nname: t\nmode: trusted\nkind: prompt\n---\n");
        let err = gather(&subject, &spec, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("UTF-8"), "got: {msg}");
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
