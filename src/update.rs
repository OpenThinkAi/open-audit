//! `oaudit update` — re-run the matching installer for this binary.
//!
//! Detects how `oaudit` was installed by inspecting the current
//! executable path, then shells out to the matching install command:
//!
//! - npm wrapper (`…/node_modules/open-audit/…`) → `npm install -g open-audit@latest`
//! - shell installer (cargo-dist) → re-run the installer from GitHub Releases
//!
//! Both install commands are idempotent and self-report their final
//! version, so this command does no version comparison of its own.

use anyhow::{Context, Result, bail};
use std::process::Stdio;
use tokio::process::Command;

const INSTALLER_URL: &str =
    "https://github.com/OpenThinkAi/open-audit/releases/latest/download/open-audit-installer.sh";

pub async fn run() -> Result<()> {
    let exe = std::env::current_exe().context("locate current executable")?;
    let exe_str = exe.to_string_lossy();

    if exe_str.contains("node_modules/open-audit") {
        run_npm().await
    } else if cfg!(windows) {
        bail!(
            "automatic update is not supported on Windows yet.\n\
             Reinstall manually from https://github.com/OpenThinkAi/open-audit/releases/latest"
        )
    } else {
        run_shell_installer().await
    }
}

async fn run_npm() -> Result<()> {
    eprintln!("oaudit: updating via npm…");
    let status = Command::new("npm")
        .args(["install", "-g", "open-audit@latest"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("spawn `npm install -g open-audit@latest`")?;

    if !status.success() {
        bail!("npm install failed (exit {:?})", status.code());
    }
    Ok(())
}

async fn run_shell_installer() -> Result<()> {
    eprintln!("oaudit: updating via {INSTALLER_URL}");
    // curl -LsSf <url> | sh
    // Piped via a single shell invocation so the curl exit status
    // propagates through `set -o pipefail`-style handling in `sh`.
    let script = format!("set -e; curl -LsSf {INSTALLER_URL} | sh");
    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("spawn installer pipeline")?;

    if !status.success() {
        bail!("installer failed (exit {:?})", status.code());
    }
    Ok(())
}
