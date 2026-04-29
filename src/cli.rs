use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::render;
use crate::resolve;

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

        /// Comma-separated specs to audit against
        /// (e.g. `untrusted/security,trusted/supply-chain` or `./my.md`).
        /// The default treats the subject as untrusted third-party code.
        #[arg(long, default_value = "untrusted/security")]
        against: String,

        /// Single glob to limit which files are audited. Replaces the
        /// spec's include list, but the spec's exclude list is preserved
        /// (so safety excludes like `target/**` still apply). Multi-glob
        /// limiting is not supported — use a custom spec file for that.
        #[arg(long)]
        scope: Option<String>,

        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
    },

    /// Audit a single file or non-git directory.
    File {
        /// Path to a single file or a non-git directory. (For git
        /// repositories use `oaudit repo` to enable git-history evidence.)
        target: PathBuf,

        /// Comma-separated specs to audit against
        /// (e.g. `untrusted/security,trusted/supply-chain` or `./my.md`).
        /// The default treats the subject as untrusted third-party code.
        #[arg(long, default_value = "untrusted/security")]
        against: String,

        /// Single glob to limit which files are audited. Replaces the
        /// spec's include list, but the spec's exclude list is preserved
        /// (so safety excludes like `target/**` still apply). Multi-glob
        /// limiting is not supported — use a custom spec file for that.
        #[arg(long)]
        scope: Option<String>,

        /// Output format.
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

pub async fn dispatch(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Repo { target, against, scope, format } => {
            audit_repo(&target, &against, scope.as_deref(), format).await
        }
        Command::File { target, against, scope, format } => {
            audit_file(&target, &against, scope.as_deref(), format).await
        }
        Command::List => list_specs().map(|_| 0),
        Command::Explain { spec, open } => explain(&spec, open).await.map(|_| 0),
        Command::Init => crate::init::scaffold(std::env::current_dir()?).await.map(|_| 0),
    }
}

async fn audit_repo(
    target: &str,
    against: &str,
    scope: Option<&str>,
    format: Format,
) -> Result<u8> {
    let cwd = std::env::current_dir()?;
    let specs = resolve::resolve(against, &cwd)?;
    let repo = crate::subject::repo::open(target).await?;
    let subject = crate::subject::Subject::Repo(repo);
    audit(&subject, &specs, scope, format).await
}

async fn audit_file(
    target: &std::path::Path,
    against: &str,
    scope: Option<&str>,
    format: Format,
) -> Result<u8> {
    let cwd = std::env::current_dir()?;
    let specs = resolve::resolve(against, &cwd)?;
    let file = crate::subject::file::open(target).await?;
    let subject = crate::subject::Subject::File(file);
    audit(&subject, &specs, scope, format).await
}

async fn audit(
    subject: &crate::subject::Subject,
    specs: &[crate::spec::Spec],
    scope: Option<&str>,
    format: Format,
) -> Result<u8> {
    let outcome = crate::run::run(subject, specs, scope).await?;
    crate::output::emit(&outcome.report, &outcome.stats, format)?;
    Ok(crate::output::exit_code(&outcome.report))
}

async fn explain(spec: &str, open: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (full, label) = resolve::lookup_raw(spec, &cwd)?;

    if open {
        render::render_spec(&full, Some(&label)).await
    } else if full.ends_with('\n') {
        print!("{full}");
        Ok(())
    } else {
        println!("{full}");
        Ok(())
    }
}

fn list_specs() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let local = resolve::list_local(&cwd);
    let local_paths: HashSet<&str> = local.iter().map(|(p, _)| p.as_str()).collect();

    let mut builtins: Vec<&crate::builtins::Builtin> = crate::builtins::all().iter().collect();
    builtins.sort_by_key(|b| b.catalog_path);

    println!("built-in specs (use `oaudit explain <mode>/<name>` to view):");
    for b in builtins {
        let suffix = if local_paths.contains(b.catalog_path) {
            "  (overridden by local)"
        } else {
            ""
        };
        println!("  {}{}", b.catalog_path, suffix);
    }

    if local.is_empty() {
        println!();
        println!("(no repo-local specs at .oaudit/auditors/ — `oaudit init` scaffolds it)");
    } else {
        let builtin_paths: HashSet<&str> =
            crate::builtins::all().iter().map(|b| b.catalog_path).collect();
        println!();
        println!("repo-local specs (use `oaudit explain <mode>/<name>` to view):");
        for (catalog, path) in &local {
            let rel = path.strip_prefix(&cwd).unwrap_or(path);
            let suffix = if builtin_paths.contains(catalog.as_str()) {
                "  (overrides built-in)"
            } else {
                ""
            };
            println!("  {}  ({}){}", catalog, rel.display(), suffix);
        }
    }
    Ok(())
}

