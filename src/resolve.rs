//! Resolve `--against` strings into a list of `Spec`s.
//!
//! Resolution rules:
//! - Path-shaped (contains `/`, `.`, or starts with `~`) → file path
//! - `mode/name` (single `/`) → catalog: repo-local `.oaudit/auditors/...` first, then built-in
//! - bare name → ambiguous, error

use crate::spec::Spec;
use std::path::Path;

pub fn resolve(_against: &str, _repo_root: &Path) -> anyhow::Result<Vec<Spec>> {
    anyhow::bail!("resolve::resolve not yet implemented")
}
