use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

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

    /// Print a spec's full content.
    Explain {
        /// Spec name (e.g. `untrusted/security`) or path to a .md file.
        spec: String,
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
        Command::Explain { spec } => {
            let _ = spec;
            bail!("explain: not yet implemented");
        }
        Command::Init => {
            crate::init::scaffold(std::env::current_dir()?).await
        }
    }
}
