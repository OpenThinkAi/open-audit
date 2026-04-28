//! Untrusted-mode safety enforcement.
//!
//! Applied if any selected spec has `mode: untrusted`. Most-restrictive-wins.
//! v1: process-level invariants (no exec from target dir, read-only access enforced
//! by oaudit's own evidence reader). Future: OS-level sandboxing.

use crate::spec::{Mode, Spec};

pub fn requires_sandbox(specs: &[Spec]) -> bool {
    specs.iter().any(|s| s.meta.mode == Mode::Untrusted)
}

pub fn enforce(_specs: &[Spec]) -> anyhow::Result<()> {
    // v1 enforcement is by construction: oaudit only reads files, never executes
    // anything from the subject. No additional runtime enforcement needed yet.
    Ok(())
}
