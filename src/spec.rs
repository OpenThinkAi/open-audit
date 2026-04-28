//! Spec doc parsing — markdown with YAML frontmatter.

use anyhow::{Context, Result, bail};
use gray_matter::Matter;
use gray_matter::engine::YAML;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Prompt,
    Deterministic,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultScope {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecMeta {
    pub name: String,
    pub mode: Mode,
    pub kind: Kind,
    #[serde(default)]
    pub default_scope: Option<DefaultScope>,
    #[serde(default)]
    pub deterministic_checks: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SpecSource {
    Builtin,
    Local(PathBuf),
    AdHoc(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub meta: SpecMeta,
    pub body: String,
    pub source: SpecSource,
}

pub fn parse(text: &str, source: SpecSource) -> Result<Spec> {
    let matter = Matter::<YAML>::new();
    let parsed = matter
        .parse::<SpecMeta>(text)
        .with_context(|| match &source {
            SpecSource::Builtin => "parsing builtin spec".to_string(),
            SpecSource::Local(p) | SpecSource::AdHoc(p) => {
                format!("parsing spec at {}", p.display())
            }
        })?;

    let Some(meta) = parsed.data else {
        bail!("spec missing YAML frontmatter (expected `---`-delimited block at top of file)");
    };

    Ok(Spec { meta, body: parsed.content, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "---\nname: test\nmode: trusted\nkind: prompt\n---\nbody text\n";

    #[test]
    fn parses_minimal_frontmatter() {
        let spec = parse(MINIMAL, SpecSource::Builtin).unwrap();
        assert_eq!(spec.meta.name, "test");
        assert_eq!(spec.meta.mode, Mode::Trusted);
        assert_eq!(spec.meta.kind, Kind::Prompt);
        assert!(spec.meta.default_scope.is_none());
        assert!(spec.meta.deterministic_checks.is_empty());
        assert_eq!(spec.body.trim(), "body text");
    }

    #[test]
    fn parses_full_frontmatter() {
        let text = "---\n\
            name: security\n\
            mode: untrusted\n\
            kind: hybrid\n\
            default_scope:\n  \
              include: [\"**/*\"]\n  \
              exclude: [\".git/**\"]\n\
            deterministic_checks:\n  \
              - secret-scan-tree\n  \
              - obfuscation-scan\n\
            ---\n\
            # body\n";
        let spec = parse(text, SpecSource::Builtin).unwrap();
        assert_eq!(spec.meta.mode, Mode::Untrusted);
        assert_eq!(spec.meta.kind, Kind::Hybrid);
        let scope = spec.meta.default_scope.expect("scope");
        assert_eq!(scope.include, vec!["**/*"]);
        assert_eq!(scope.exclude, vec![".git/**"]);
        assert_eq!(spec.meta.deterministic_checks.len(), 2);
    }

    #[test]
    fn missing_frontmatter_errors() {
        let err = parse("just a body, no frontmatter\n", SpecSource::Builtin).unwrap_err();
        assert!(err.to_string().contains("missing YAML frontmatter"));
    }

    #[test]
    fn missing_required_field_errors() {
        // missing `kind`
        let text = "---\nname: test\nmode: trusted\n---\nbody\n";
        let err = parse(text, SpecSource::Builtin).unwrap_err();
        assert!(format!("{err:#}").to_lowercase().contains("kind"));
    }

    #[test]
    fn parses_every_builtin() {
        for b in crate::builtins::all() {
            let spec = parse(b.body, SpecSource::Builtin)
                .unwrap_or_else(|e| panic!("failed to parse builtin {}: {e:#}", b.catalog_path));
            // catalog_path = "<mode>/<name>"; meta.mode and meta.name should agree.
            let (mode_seg, name_seg) = b.catalog_path.split_once('/').unwrap();
            assert_eq!(spec.meta.name, name_seg, "name mismatch in {}", b.catalog_path);
            let mode_str = match spec.meta.mode {
                Mode::Trusted => "trusted",
                Mode::Untrusted => "untrusted",
            };
            assert_eq!(mode_str, mode_seg, "mode mismatch in {}", b.catalog_path);
        }
    }
}
