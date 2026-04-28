use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use crate::builtins;
use crate::render;

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

    /// List available specs (built-in + repo-local + ad-hoc).
    List,

    /// Print a spec's full content (or open in browser with --open).
    Explain {
        /// Spec name (e.g. `untrusted/security`) or path to a .md file.
        spec: String,

        /// Open the spec in a browser instead of printing to stdout.
        /// (Dev builds only in v1 — needs Node + bridges/ui-leaf/. Release
        /// builds error with a hint pointing at OAUDIT_UI_BRIDGE.)
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
        Command::List => {
            let _ = crate::builtins::all();
            bail!("list: not yet implemented");
        }
        Command::Explain { spec, open } => explain(&spec, open).await,
        Command::Init => {
            crate::init::scaffold(std::env::current_dir()?).await
        }
    }
}

/// Look up `spec` as either a filesystem path or a builtin catalog path
/// (`<mode>/<name>`), and either print its body or render it in a browser.
async fn explain(spec: &str, open: bool) -> Result<()> {
    let (markdown, label) = lookup_spec_body(spec)?;
    if open {
        render::render_spec(&markdown, Some(&label)).await
    } else {
        if markdown.ends_with('\n') {
            print!("{markdown}");
        } else {
            println!("{markdown}");
        }
        Ok(())
    }
}

fn lookup_spec_body(spec: &str) -> Result<(String, String)> {
    let path_shaped = spec.contains('/') || spec.contains('.');

    if path_shaped {
        let path = Path::new(spec);
        if path.is_file() {
            let body = std::fs::read_to_string(path)
                .with_context(|| format!("reading spec file {}", path.display()))?;
            return Ok((body, path.display().to_string()));
        }
        // Catalog-shaped strings (`trusted/security`) take this branch but the
        // file doesn't exist on disk — fall through to the builtin lookup.
        if let Some(b) = builtins::all().iter().find(|b| b.catalog_path == spec) {
            return Ok((b.body.to_string(), format!("builtin: {}", b.catalog_path)));
        }
        bail!(
            "spec `{spec}` not found as a file path nor a builtin catalog path.\n  Available builtins:\n{}",
            builtins_index(),
        );
    }

    bail!(
        "spec `{spec}` is ambiguous: bare names need to be qualified.\n  Use `<mode>/<name>` (e.g. `trusted/security`) or a path to a .md file.\n  Available builtins:\n{}",
        builtins_index(),
    )
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
