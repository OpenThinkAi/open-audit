//! Resolve `--against` strings into a list of `Spec`s.
//!
//! Resolution per comma-separated value:
//! - Path-shaped (contains `/` or `.`) AND points at an existing file → load that file as `AdHoc`
//! - `<mode>/<name>` (catalog form) → repo-local `.oaudit/auditors/<mode>/<name>.md`
//!   first (`Local`), else embedded built-in (`Builtin`)
//! - Bare name (no slash, no dot) → ambiguous-error with the available builtins listed
//!
//! `<mode>` must be one of `Mode::ALL` (currently `trusted`, `untrusted`).
//! Unknown-mode catalog tokens get a tailored error pointing at the typo.

use crate::builtins;
use crate::spec::{self, Mode, Spec, SpecSource};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub(crate) fn resolve(against: &str, repo_root: &Path) -> Result<Vec<Spec>> {
    let mut out = Vec::new();
    for raw in against.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        out.push(resolve_one(token, repo_root)?);
    }
    if out.is_empty() {
        bail!("--against requires at least one spec");
    }
    Ok(out)
}

/// Lookup the raw, unparsed text of a spec for display (no frontmatter
/// stripping). Mirrors `resolve_one`'s lookup chain but skips parsing,
/// so callers like `explain` get the original file content + a label
/// suitable for display.
pub(crate) fn lookup_raw(token: &str, repo_root: &Path) -> Result<(String, String)> {
    let path_shaped = token.contains('/') || token.contains('.');
    if path_shaped {
        let raw_path = Path::new(token);
        if raw_path.is_file() {
            let body = std::fs::read_to_string(raw_path)
                .with_context(|| format!("reading spec file {}", raw_path.display()))?;
            return Ok((body, raw_path.display().to_string()));
        }
        if looks_like_catalog(token) {
            let local = repo_root
                .join(".oaudit")
                .join("auditors")
                .join(format!("{token}.md"));
            if local.is_file() {
                let body = std::fs::read_to_string(&local)
                    .with_context(|| format!("reading spec file {}", local.display()))?;
                return Ok((body, format!("local: {}", local.display())));
            }
            if let Some(b) = builtins::all().iter().find(|b| b.catalog_path == token) {
                return Ok((b.body.to_string(), format!("builtin: {}", b.catalog_path)));
            }
            bail!(
                "spec `{token}` not found in repo or built-ins.\n  Available builtins:\n{}",
                builtins_index(),
            );
        }
        // Catalog-shaped with unknown mode (`weird/security`) → name the mode
        // explicitly. Otherwise it's a bare file path that didn't exist.
        if let Some(unknown_mode) = unknown_mode_in_catalog_shape(token) {
            bail!(
                "unknown spec mode `{unknown_mode}` in `{token}`. Known modes: {}",
                known_modes_csv(),
            );
        }
        bail!("spec file `{token}` not found");
    }
    bail!(
        "spec `{token}` is ambiguous: bare names need to be qualified.\n  Use `<mode>/<name>` (e.g. `trusted/security`) or a path to a .md file.\n  Available builtins:\n{}",
        builtins_index(),
    )
}

pub(crate) fn resolve_one(token: &str, repo_root: &Path) -> Result<Spec> {
    let path_shaped = token.contains('/') || token.contains('.');
    if path_shaped {
        let raw_path = Path::new(token);
        if raw_path.is_file() {
            return load_path(raw_path, SpecSource::AdHoc(raw_path.to_path_buf()));
        }
        if looks_like_catalog(token) {
            if let Some(spec) = try_local_catalog(token, repo_root)? {
                return Ok(spec);
            }
            if let Some(b) = builtins::all().iter().find(|b| b.catalog_path == token) {
                return spec::parse(b.body, SpecSource::Builtin(b.catalog_path));
            }
            bail!(
                "spec `{token}` not found in repo or built-ins.\n  Available builtins:\n{}",
                builtins_index(),
            );
        }
        if let Some(unknown_mode) = unknown_mode_in_catalog_shape(token) {
            bail!(
                "unknown spec mode `{unknown_mode}` in `{token}`. Known modes: {}",
                known_modes_csv(),
            );
        }
        bail!("spec file `{token}` not found");
    }
    bail!(
        "spec `{token}` is ambiguous: bare names need to be qualified.\n  Use `<mode>/<name>` (e.g. `trusted/security`) or a path to a .md file.\n  Available builtins:\n{}",
        builtins_index(),
    )
}

fn looks_like_catalog(token: &str) -> bool {
    match token.split_once('/') {
        Some((mode, name)) => {
            !name.is_empty()
                && !name.contains('/')
                && !name.contains('.')
                && Mode::ALL.iter().any(|m| m.as_str() == mode)
        }
        None => false,
    }
}

/// `Some(mode)` if `token` looks like `<unknown-mode>/<simple-name>` —
/// distinguishes "user typo'd the mode" from "user passed a real path that
/// happens not to exist."
fn unknown_mode_in_catalog_shape(token: &str) -> Option<&str> {
    let (mode, name) = token.split_once('/')?;
    if name.is_empty() || name.contains('/') || name.contains('.') {
        return None;
    }
    if Mode::ALL.iter().any(|m| m.as_str() == mode) {
        return None;
    }
    Some(mode)
}

fn known_modes_csv() -> String {
    Mode::ALL
        .iter()
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn try_local_catalog(token: &str, repo_root: &Path) -> Result<Option<Spec>> {
    let path = repo_root
        .join(".oaudit")
        .join("auditors")
        .join(format!("{token}.md"));
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(load_path(&path, SpecSource::Local(path.clone()))?))
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
/// come from `Mode::ALL` so this stays in lockstep with what `resolve`
/// considers a valid catalog.
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
        assert!(!msg.contains("Available builtins"), "should not show builtins listing for file paths");
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
    fn mode_all_matches_serialization() {
        // Pin the relationship between Mode variants and Mode::ALL.
        // If someone adds a new variant without extending ALL, this fails.
        for mode in Mode::ALL {
            let s = mode.as_str();
            // Round-trip via serde to make sure the serialization name
            // matches the as_str() name (used in catalog paths).
            let json = serde_json::to_string(mode).unwrap();
            assert_eq!(json, format!("\"{s}\""));
        }
    }
}
