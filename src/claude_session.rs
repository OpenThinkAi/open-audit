//! Long-lived `claude` CLI subprocess in headless stream-json mode.
//!
//! One session reused across all auditor runs in a single oaudit invocation.
//! Auth fallback (env API key → claude.ai OAuth) handled by the CLI itself.
//!
//! Runtime dep: `claude` (Claude Code CLI) on $PATH.

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

pub struct ClaudeSession {
    _child: Child,
    _stdin: ChildStdin,
    _stdout: BufReader<ChildStdout>,
}

impl ClaudeSession {
    /// Spawn `claude --input-format=stream-json --output-format=stream-json --print`
    /// and prepare for query/response cycles.
    pub async fn spawn() -> Result<Self> {
        anyhow::bail!("claude_session::spawn not yet implemented")
    }

    /// Send a system+user prompt pair, await the JSON response.
    pub async fn query(&mut self, _system: &str, _user: &str) -> Result<String> {
        anyhow::bail!("claude_session::query not yet implemented")
    }
}

/// Verify `claude --version` resolves on $PATH.
pub async fn preflight() -> Result<String> {
    let output = tokio::process::Command::new("claude")
        .arg("--version")
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("`claude` CLI not found on PATH: {e}\n\nInstall: https://claude.com/claude-code"))?;
    if !output.status.success() {
        anyhow::bail!("`claude --version` failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[allow(dead_code)]
async fn _shape_check(s: &mut BufReader<ChildStdout>, w: &mut ChildStdin) {
    // placeholder so the imports above stay live; real impl uses these.
    let mut buf = String::new();
    let _ = s.read_line(&mut buf).await;
    let _ = w.write_all(b"").await;
}
