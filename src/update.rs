//! `oaudit update` — re-run the matching installer for this binary.
//!
//! Detects how `oaudit` was installed by inspecting the current
//! executable path, then shells out to the matching install command:
//!
//! - npm wrapper (`…/node_modules/@openthink/audit/…`) → `npm install -g @openthink/audit@latest`
//! - everything else (the cargo-dist shell installer is the canonical
//!   path) → re-run the installer from GitHub Releases
//!
//! Detection is intentionally not finer-grained: cargo-dist's default
//! install-path is `$CARGO_HOME/bin`, which is the same directory as
//! `cargo install`, so a `.cargo/bin/` substring can't distinguish the
//! two. Users who installed via a package manager (Homebrew, apt, etc.)
//! get a warning before the shell installer runs so they can hit Ctrl+C
//! and use their package manager instead — see `run_shell_installer()`.
//! `--yes` skips the warning pause for CI / scripted updates.
//!
//! Windows hits an explicit bail: the cargo-dist `.sh` installer can't
//! run there, and Windows binaries aren't shipped yet.
//!
//! Both auto-install commands are idempotent and self-report their final
//! version, so this command does no version comparison of its own.

use anyhow::{Context, Result, bail};
use std::process::Stdio;
use tokio::process::Command;

const INSTALLER_URL: &str =
    "https://github.com/OpenThinkAi/open-audit/releases/latest/download/open-audit-installer.sh";

const RELEASES_URL: &str = "https://github.com/OpenThinkAi/open-audit/releases/latest";

pub(crate) async fn run(yes: bool) -> Result<()> {
    let exe = std::env::current_exe().context("locate current executable")?;
    let exe_str = exe.to_string_lossy();

    // npm check first, with both separators, so a Windows npm install
    // routes correctly instead of falling through to the Windows bail.
    if exe_str.contains("node_modules/@openthink/audit")
        || exe_str.contains("node_modules\\@openthink\\audit")
    {
        run_npm().await
    } else if cfg!(windows) {
        bail!(
            "automatic update is not supported on Windows yet.\n\
             Reinstall manually from {RELEASES_URL}"
        )
    } else {
        run_shell_installer(&exe_str, yes).await
    }
}

async fn run_npm() -> Result<()> {
    eprintln!("oaudit: running `npm install -g @openthink/audit@latest`");
    let status = Command::new("npm")
        .args(["install", "-g", "@openthink/audit@latest"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("spawn `npm install -g @openthink/audit@latest`")?;

    if !status.success() {
        bail!(
            "npm install failed (exit {:?}).\n\
             Try `npm install -g @openthink/audit@latest` manually, or reinstall from {RELEASES_URL}",
            status.code()
        );
    }
    Ok(())
}

async fn run_shell_installer(current_exe: &str, yes: bool) -> Result<()> {
    // Heuristic: if the running binary lives somewhere a system or
    // user package manager typically owns (Homebrew, apt, etc.), the
    // shell installer would drop a second oaudit in `$CARGO_HOME/bin`
    // and shadow it on PATH. Warn before doing it so the user can hit
    // Ctrl+C and reinstall via the channel they actually use. `--yes`
    // skips the pause (CI, scripted updates) but still emits the warning
    // so the action stays auditable in logs.
    if looks_package_manager_owned(current_exe) {
        eprintln!(
            "oaudit: warning — this binary lives at {current_exe},\n\
             which looks package-manager-owned (Homebrew, apt, …).\n\
             The shell installer will write to $CARGO_HOME/bin and may shadow\n\
             your existing copy on PATH. If you installed via a package manager,\n\
             hit Ctrl+C now and use that channel to update instead."
        );
        if !yes {
            eprintln!("(continuing in 5s — pass --yes to skip)");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

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
        bail!(
            "installer failed (exit {:?}).\n\
             Reinstall manually from {RELEASES_URL}",
            status.code()
        );
    }
    Ok(())
}

fn looks_package_manager_owned(path: &str) -> bool {
    // Common package-manager bin dirs. Not exhaustive — best-effort
    // hint to a user about to silently shadow a managed install.
    const MARKERS: &[&str] = &[
        "/opt/homebrew/",   // Apple Silicon Homebrew
        "/usr/local/Cellar/", "/usr/local/opt/", // Intel Homebrew
        "/home/linuxbrew/", // Linuxbrew
        "/usr/bin/",        // apt / pacman / dnf
        "/usr/sbin/",
        "/nix/store/",      // Nix
    ];
    MARKERS.iter().any(|m| path.contains(m))
}
