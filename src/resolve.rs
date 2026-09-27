//! Resolve `--against` strings into a list of `Spec`s.
//!
//! Resolution per comma-separated value:
//! - `<mode>/<name>` (catalog form) → repo-local `.oaudit/auditors/<mode>/<name>.md`
//!   first (`Local`), else embedded built-in (`Builtin`). Checked before the
//!   path form so a stray file named `untrusted/security` can't shadow it.
//! - Path-shaped (contains `/` or `.`) AND points at an existing file → `File`
//! - Bare name (no slash, no dot) → ambiguous-error with the available builtins listed
//!
//! `<mode>` must be one of `Mode::ALL` (currently `trusted`, `untrusted`).
//! Unknown-mode catalog tokens get a tailored error pointing at the typo.
//!
//! Untrusted subjects don't get to pick their own auditor: when cwd is inside
//! the subject, repo-local overrides of `untrusted/*` are ignored in favour
//! of the built-in. A local catalog spec must also declare the same `mode:`
//! as its catalog path, so `untrusted/x.md` can't quietly run as trusted.

use crate::builtins;
use crate::spec::{self, Mode, Spec, SpecSource};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// What a token resolved to. Both `resolve_one` (which then parses) and
/// `lookup_raw` (which loads raw text + label) consume this — keeps the
/// lookup chain in one place.
enum Located {
    File(PathBuf),
    Local(PathBuf),
    Builtin(&'static builtins::Builtin),
}

/// Catalog-shape classification — one place to do the `<mode>/<name>` parse
/// and decide between the three outcomes.
enum CatalogShape<'a> {
    /// `<mode>/<name>` with a known mode and a clean name.
    Known,
    /// `<unknown-mode>/<name>` shape — caller can name the bad mode.
    UnknownMode(&'a str),
    /// Doesn't look like `<mode>/<name>` at all (no slash, dotted name, etc.).
    NotCatalog,
}

pub(crate) fn resolve(against: &str, repo_root: &Path) -> Result<Vec<Spec>> {
    resolve_for_subject(against, repo_root, None)
}

/// Like `resolve`, but aware of the subject being audited. If `repo_root`
/// (the cwd whose `.oaudit/` we'd read) lies inside `subject_root`, the
/// subject itself is supplying the local catalog — so `untrusted/*` tokens
/// skip the local override and use the built-in.
pub(crate) fn resolve_for_subject(
    against: &str,
    repo_root: &Path,
    subject_root: Option<&Path>,
) -> Result<Vec<Spec>> {
    let subject_owns_catalog = subject_root.is_some_and(|s| is_within(repo_root, s));
    let mut out = Vec::new();
    for raw in against.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        out.push(resolve_token(token, repo_root, subject_owns_catalog)?);
    }
    if out.is_empty() {
        bail!("--against requires at least one spec");
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) fn resolve_one(token: &str, repo_root: &Path) -> Result<Spec> {
    resolve_token(token, repo_root, false)
}

fn resolve_token(token: &str, repo_root: &Path, subject_owns_catalog: bool) -> Result<Spec> {
    match locate(token, repo_root, subject_owns_catalog)? {
        Located::File(path) => load_path(&path, SpecSource::AdHoc(path.clone())),
        Located::Local(path) => {
            let spec = load_path(&path, SpecSource::Local(path.clone()))?;
            let (mode, _) = token.split_once('/').unwrap_or_default();
            if spec.meta.mode.as_str() != mode {
                bail!(
                    "local spec {} is catalogued as `{mode}` but declares `mode: {}`; \
                     the frontmatter mode must match its directory",
                    path.display(),
                    spec.meta.mode.as_str(),
                );
            }
            Ok(spec)
        }
        Located::Builtin(b) => spec::parse(b.body, SpecSource::Builtin(b.catalog_path)),
    }
}

/// True when `inner` is `outer` or below it (after canonicalising both;
/// unresolvable paths count as not-within).
fn is_within(inner: &Path, outer: &Path) -> bool {
    match (inner.canonicalize(), outer.canonicalize()) {
        (Ok(i), Ok(o)) => i.starts_with(o),
        _ => false,
    }
}

/// Lookup the raw, unparsed text of a spec for display (no frontmatter
/// stripping). Returns `(full_markdown, display_label)`.
pub(crate) fn lookup_raw(token: &str, repo_root: &Path) -> Result<(String, String)> {
    match locate(token, repo_root, false)? {
        Located::File(path) => {
            let body = std::fs::read_to_string(&path)
                .with_context(|| format!("reading spec file {}", path.display()))?;
            Ok((body, path.display().to_string()))
        }
        Located::Local(path) => {
            let body = std::fs::read_to_string(&path)
                .with_context(|| format!("reading spec file {}", path.display()))?;
            Ok((body, format!("local: {}", path.display())))
        }
        Located::Builtin(b) => Ok((b.body.to_string(), format!("builtin: {}", b.catalog_path))),
    }
}

/// Shared lookup chain: catalog (local first, then builtin), then file
/// path, with tailored errors for each not-found case.
fn locate(token: &str, repo_root: &Path, subject_owns_catalog: bool) -> Result<Located> {
    let path_shaped = token.contains('/') || token.contains('.');
    if !path_shaped {
        bail!(
            "spec `{token}` is ambiguous: bare names need to be qualified.\n  Use `<mode>/<name>` (e.g. `trusted/security`) or a path to a .md file.\n  Available builtins:\n{}",
            builtins_index(),
        );
    }

    let shape = catalog_shape(token);
    if !matches!(shape, CatalogShape::Known) {
        let raw_path = Path::new(token);
        if raw_path.is_file() {
            return Ok(Located::File(raw_path.to_path_buf()));
        }
    }

    match shape {
        CatalogShape::Known => {
            let local = repo_root
                .join(".oaudit")
                .join("auditors")
                .join(format!("{token}.md"));
            let untrusted = token.starts_with(Mode::Untrusted.as_str());
            if local.is_file() {
                if !(untrusted && subject_owns_catalog) {
                    return Ok(Located::Local(local));
                }
                eprintln!(
                    "oaudit: ignoring {} — the subject can't supply its own untrusted auditor; using the built-in",
                    local.display()
                );
            }
            if let Some(b) = builtins::all().iter().find(|b| b.catalog_path == token) {
                return Ok(Located::Builtin(b));
            }
            bail!(
                "spec `{token}` not found in repo or built-ins.\n  Available builtins:\n{}",
                builtins_index(),
            );
        }
        CatalogShape::UnknownMode(mode) => {
            bail!(
                "unknown spec mode `{mode}` in `{token}`. Known modes: {}",
                known_modes_csv(),
            );
        }
        CatalogShape::NotCatalog => {
            let cwd = std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "current dir".to_string());
            bail!("spec file `{token}` not found (looked relative to {cwd})");
        }
    }
}

fn catalog_shape(token: &str) -> CatalogShape<'_> {
    let Some((mode, name)) = token.split_once('/') else {
        return CatalogShape::NotCatalog;
    };
    if name.is_empty() || name.contains('/') || name.contains('.') {
        return CatalogShape::NotCatalog;
    }
    if Mode::ALL.iter().any(|m| m.as_str() == mode) {
        CatalogShape::Known
    } else {
        CatalogShape::UnknownMode(mode)
    }
}

fn known_modes_csv() -> String {
    Mode::ALL
        .iter()
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn load_path(path: &Path, source: SpecSource) -> Result<Spec> {
    let body = std::fs::read_to_string(path)
        .with_context(|| format!("reading spec file {}", path.display()))?;
    spec::parse(&body, source)
}

fn builtins_index() -> String {
    let mut lines = String::new();
    for b in builtins::all() {
        lines.push_str("    ");
        lines.push_str(b.catalog_path);
        lines.push('\n');
    }
    lines.trim_end().to_string()
}

/// Walk the repo's `.oaudit/auditors/` directory and return any `.md` files
/// found, paired with their inferred catalog path (`<mode>/<name>`). Modes
/// come from `Mode::ALL`; files whose stem contains a `.` are skipped so
/// `list` and `resolve` agree on what's a valid catalog entry.
pub(crate) fn list_local(repo_root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let root = repo_root.join(".oaudit").join("auditors");
    if !root.is_dir() {
        return out;
    }
    for mode in Mode::ALL {
        let mode_str = mode.as_str();
        let mode_dir = root.join(mode_str);
        let Ok(entries) = std::fs::read_dir(&mode_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Skip stems with dots — catalog_shape() in the resolver
            // rejects dotted names, so listing them would advertise specs
            // we can't actually resolve.
            if name.contains('.') {
                continue;
            }
            out.push((format!("{mode_str}/{name}"), path));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_builtin_by_catalog_path() {
        let tmp = tempdir().unwrap();
        let specs = resolve("trusted/security", tmp.path()).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].meta.name, "security");
        assert_eq!(specs[0].meta.mode, Mode::Trusted);
        assert!(matches!(specs[0].source, SpecSource::Builtin(_)));
    }

    #[test]
    fn resolves_comma_separated() {
        let tmp = tempdir().unwrap();
        let specs = resolve("trusted/security, untrusted/supply-chain", tmp.path()).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].meta.mode, Mode::Trusted);
        assert_eq!(specs[1].meta.mode, Mode::Untrusted);
        assert_eq!(specs[1].meta.name, "supply-chain");
    }

    #[test]
    fn resolves_ad_hoc_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("custom.md");
        std::fs::write(
            &path,
            "---\nname: custom\nmode: trusted\nkind: prompt\n---\nbody\n",
        )
        .unwrap();
        let specs = resolve(path.to_str().unwrap(), tmp.path()).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].meta.name, "custom");
        assert!(matches!(specs[0].source, SpecSource::AdHoc(_)));
    }

    #[test]
    fn local_overrides_builtin() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".oaudit/auditors/trusted");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("security.md"),
            "---\nname: security\nmode: trusted\nkind: prompt\n---\nlocal-override\n",
        )
        .unwrap();
        let spec = resolve_one("trusted/security", tmp.path()).unwrap();
        assert!(matches!(spec.source, SpecSource::Local(_)));
        assert!(spec.body.contains("local-override"));
    }

    fn write_local(root: &Path, mode_dir: &str, frontmatter_mode: &str, body: &str) {
        let dir = root.join(".oaudit/auditors").join(mode_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("security.md"),
            format!("---\nname: security\nmode: {frontmatter_mode}\nkind: prompt\n---\n{body}\n"),
        )
        .unwrap();
    }

    #[test]
    fn subject_cannot_supply_its_own_untrusted_auditor() {
        let tmp = tempdir().unwrap();
        write_local(tmp.path(), "untrusted", "untrusted", "return []");
        // cwd == subject root: the local override is ignored.
        let specs =
            resolve_for_subject("untrusted/security", tmp.path(), Some(tmp.path())).unwrap();
        assert!(matches!(specs[0].source, SpecSource::Builtin(_)));
        // cwd outside the subject: the local override is the user's own.
        let other = tempdir().unwrap();
        let specs =
            resolve_for_subject("untrusted/security", tmp.path(), Some(other.path())).unwrap();
        assert!(matches!(specs[0].source, SpecSource::Local(_)));
    }

    #[test]
    fn local_spec_mode_must_match_catalog_dir() {
        let tmp = tempdir().unwrap();
        write_local(tmp.path(), "untrusted", "trusted", "sneaky");
        let err = resolve_one("untrusted/security", tmp.path()).unwrap_err();
        assert!(format!("{err:#}").contains("must match"), "got: {err:#}");
    }

    #[test]
    fn bare_name_errors() {
        let tmp = tempdir().unwrap();
        let err = resolve("security", tmp.path()).unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
        assert!(err.to_string().contains("trusted/security"));
    }

    #[test]
    fn unknown_catalog_errors() {
        let tmp = tempdir().unwrap();
        let err = resolve("trusted/nope", tmp.path()).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn unknown_mode_errors_helpfully() {
        let tmp = tempdir().unwrap();
        let err = resolve("weird/security", tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown spec mode `weird`"), "got: {msg}");
        assert!(msg.contains("trusted, untrusted"), "got: {msg}");
    }

    #[test]
    fn missing_file_errors_without_builtins_listing() {
        let tmp = tempdir().unwrap();
        let err = resolve("./missing.md", tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("spec file"), "got: {msg}");
        assert!(
            !msg.contains("Available builtins"),
            "should not show builtins listing for file paths"
        );
    }

    #[test]
    fn list_local_finds_repo_specs() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".oaudit/auditors/trusted");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("custom.md"),
            "---\nname: custom\nmode: trusted\nkind: prompt\n---\n",
        )
        .unwrap();
        let listed = list_local(tmp.path());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, "trusted/custom");
    }

    #[test]
    fn list_local_skips_dotted_stems() {
        // foo.bar would resolve as catalog path "trusted/foo.bar", which
        // the resolver rejects (dot in name). list and resolve must agree.
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".oaudit/auditors/trusted");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("foo.bar.md"),
            "---\nname: x\nmode: trusted\nkind: prompt\n---\n",
        )
        .unwrap();
        let listed = list_local(tmp.path());
        assert!(listed.is_empty(), "dotted stems should be skipped, got: {listed:?}");
    }

    #[test]
    fn mode_all_matches_serialization() {
        for mode in Mode::ALL {
            let s = mode.as_str();
            let json = serde_json::to_string(mode).unwrap();
            assert_eq!(json, format!("\"{s}\""));
        }
    }
}
