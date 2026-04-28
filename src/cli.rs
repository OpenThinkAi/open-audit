use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::render;
use crate::resolve;
use crate::spec::SpecSource;

#[derive(Parser, Debug)]
#[command(name = "oaudit", version, about = "Audit codebases against composable spec docs.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Audit a git repository (URL to clone, or local git path).
    Repo {
        /// URL or local path to a git repository.
        target: String,

        /// Comma-separated specs (e.g. `untrusted/security,trusted/supply-chain` or `./my.md`).
        #[arg(long)]
        against: Option<String>,

        /// Glob to limit scope (overrides spec defaults).
        #[arg(long)]
        scope: Option<String>,

        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
    },

    /// Audit a single file or non-git directory.
    File {
        /// Path to file or directory.
        target: PathBuf,

        #[arg(long)]
        against: Option<String>,

        #[arg(long)]
        scope: Option<String>,

        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
    },

    /// List available specs (built-in + repo-local).
    List,

    /// Print a spec's full content (or open in browser with --open).
    Explain {
        /// Spec name (e.g. `untrusted/security`) or path to a .md file.
        spec: String,

        /// Open the spec in a browser instead of printing to stdout.
        /// Requires Node + a ui-leaf bridge: set OAUDIT_UI_BRIDGE to a
        /// bridge.js path, or run from a source checkout (bridges/ui-leaf/
        /// is auto-detected).
        #[arg(long)]
        open: bool,
    },

    /// Scaffold .oaudit/ in the current directory.
    Init,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Format {
    Json,
    Human,
}

pub async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Repo { target, against, scope, format } => {
            let _ = (target, against, scope, format);
            bail!("repo: not yet implemented");
        }
        Command::File { target, against, scope, format } => {
            let _ = (target, against, scope, format);
            bail!("file: not yet implemented");
        }
        Command::List => list_specs(),
        Command::Explain { spec, open } => explain(&spec, open).await,
        Command::Init => crate::init::scaffold(std::env::current_dir()?).await,
    }
}

async fn explain(spec: &str, open: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let resolved = resolve::resolve_one(spec, &cwd)?;
    let label = source_label(&resolved.source);

    if open {
        render::render_spec(&resolved.body, Some(&label)).await
    } else if resolved.body.ends_with('\n') {
        print!("{}", resolved.body);
        Ok(())
    } else {
        println!("{}", resolved.body);
        Ok(())
    }
}

fn list_specs() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let local = resolve::list_local(&cwd);
    let local_paths: std::collections::HashSet<&str> =
        local.iter().map(|(p, _)| p.as_str()).collect();

    println!("built-in specs (use `oaudit explain <name>` to view):");
    for b in crate::builtins::all() {
        let suffix = if local_paths.contains(b.catalog_path) {
            "  (overridden by local)"
        } else {
            ""
        };
        println!("  {}{}", b.catalog_path, suffix);
    }

    if !local.is_empty() {
        println!();
        println!("repo-local specs (.oaudit/auditors/):");
        for (catalog, path) in &local {
            let rel = path.strip_prefix(&cwd).unwrap_or(path);
            println!("  {}  ({})", catalog, rel.display());
        }
    }
    Ok(())
}

fn source_label(source: &SpecSource) -> String {
    match source {
        SpecSource::Builtin(catalog) => format!("builtin: {catalog}"),
        SpecSource::Local(p) | SpecSource::AdHoc(p) => p.display().to_string(),
    }
}

