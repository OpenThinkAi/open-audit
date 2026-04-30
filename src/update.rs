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
    eprintln!("oaudit: running `npm install -g open-audit@latest`");
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
    eprintln!("oaudit: running shell installer from {INSTALLER_URL}");
    // `set -euo pipefail` so a curl failure (404, DNS, network) aborts
    // the pipeline instead of being masked by `sh` exiting 0 on empty
    // input. POSIX `sh` doesn't reliably support pipefail, so we
    // require bash explicitly — the cargo-dist installer being piped
    // in also requires bash, so we're not adding a dependency.
    let script = format!("set -euo pipefail; curl -LsSf {INSTALLER_URL} | sh");
    let status = Command::new("bash")
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
