use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::{IsTerminal, Read};
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

    /// Audit a single file or non-git directory. Pass `-` to read text
    /// from stdin (sugar for `oaudit text`).
    File {
        /// Path to a single file or a non-git directory, or `-` to read
        /// from stdin. (For git repositories use `oaudit repo` to enable
        /// git-history evidence.)
        target: PathBuf,

        /// Comma-separated specs to audit against
        /// (e.g. `untrusted/security,trusted/supply-chain` or `./my.md`).
        /// Default depends on the subject: a file path defaults to
        /// `untrusted/security`; `-` (stdin) defaults to
        /// `untrusted/llm-security` so the sugar matches `oaudit text`.
        #[arg(long)]
        against: Option<String>,

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

    /// Audit a string read from stdin. Built for callers that have an
    /// untrusted text input in hand (issue body, RAG snippet, support
    /// ticket) and don't want to round-trip through a tempfile.
    Text {
        /// How to refer to the input in findings (`location.file`, titles,
        /// evidence). Defaults to `stdin`.
        #[arg(long, default_value = "stdin")]
        label: String,

        /// Comma-separated specs to audit against
        /// (e.g. `untrusted/llm-security` or `./my.md`).
        /// Defaults to `untrusted/llm-security` — the text-shaped subject
        /// calls for the LLM/agent surface auditor.
        #[arg(long)]
        against: Option<String>,

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

    /// Update oaudit to the latest release.
    ///
    /// Detects how oaudit was installed and re-runs the matching
    /// installer:
    ///
    ///   - npm wrapper → `npm install -g open-audit@latest`
    ///   - everything else → cargo-dist shell installer from GitHub Releases
    ///
    /// If the binary lives in a path that looks package-manager-owned
    /// (Homebrew, apt, etc.), the shell installer warns and pauses 5s
    /// before running so you can hit Ctrl+C and use that channel
    /// instead. Pass `--yes` to skip the pause (for CI / scripted use).
    Update {
        /// Skip the package-manager-shadowing warning pause.
        #[arg(long, alias = "force")]
        yes: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Format {
    Json,
    Human,
}

/// Canonical message for `--scope` rejection on stdin/text. Single source
/// of truth so the CLI sugar path (`oaudit file -`) and `evidence::gather`
/// (defense-in-depth on the same condition) can't drift to two different
/// wordings.
pub(crate) const STDIN_SCOPE_REJECT_MSG: &str =
    "--scope has no effect when reading from stdin. Remove --scope.";

/// Default spec when `oaudit file <path>` is invoked without `--against`.
const DEFAULT_FILE_AGAINST: &str = "untrusted/security";

/// Default spec when `oaudit file -` (or `oaudit text`) is invoked without
/// `--against`. The text-shaped subject calls for the LLM/agent surface
/// auditor, not the code-malice one.
const DEFAULT_TEXT_AGAINST: &str = "untrusted/llm-security";

pub async fn dispatch(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Repo { target, against, scope, format } => {
            audit_repo(&target, &against, scope.as_deref(), format).await
        }
        Command::File { target, against, scope, format } => {
            // `oaudit file -` is sugar for `oaudit text` with default label
            // `stdin`. Sniff before any path canonicalization so `-` doesn't
            // round-trip through a "path does not exist" error. The default
            // spec also forks here: stdin → `untrusted/llm-security` (matches
            // `oaudit text`); a file path → `untrusted/security`. Resolving
            // the default after this branch keeps the two sugar forms
            // congruent when the user omits `--against`.
            if target.as_os_str() == OsStr::new("-") {
                if scope.is_some() {
                    bail!(STDIN_SCOPE_REJECT_MSG);
                }
                let against = against.as_deref().unwrap_or(DEFAULT_TEXT_AGAINST);
                return audit_text("stdin", against, format).await;
            }
            let against = against.as_deref().unwrap_or(DEFAULT_FILE_AGAINST);
            audit_file(&target, against, scope.as_deref(), format).await
        }
        Command::Text { label, against, format } => {
            let against = against.as_deref().unwrap_or(DEFAULT_TEXT_AGAINST);
            audit_text(&label, against, format).await
        }
        Command::List => list_specs().map(|_| 0),
        Command::Explain { spec, open } => explain(&spec, open).await.map(|_| 0),
        Command::Init => crate::init::scaffold(std::env::current_dir()?).await.map(|_| 0),
        Command::Update { yes } => crate::update::run(yes).await.map(|_| 0),
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

async fn audit_text(label: &str, against: &str, format: Format) -> Result<u8> {
    let cwd = std::env::current_dir()?;
    let specs = resolve::resolve(against, &cwd)?;
    let content = read_stdin().context("reading stdin")?;
    let text = crate::subject::text::new(label, content)?;
    let subject = crate::subject::Subject::Text(text);
    audit(&subject, &specs, None, format).await
}

/// Read all of stdin into a String. Caps at `MAX_TEXT_BYTES + 1` so an
/// errant pipe can't OOM us before we've validated the size; the +1
/// guarantees `subject::text::new` sees a length that exceeds the cap
/// when oversized and produces the proper error.
///
/// If stdin is a TTY, a one-line hint is written to stderr first so
/// users who invoked `oaudit text` interactively don't stare at a
/// silently-blocking process.
fn read_stdin() -> Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("reading from stdin… pipe input or press Ctrl-D to send EOF");
    }
    let mut buf = String::new();
    let cap = (crate::subject::text::MAX_TEXT_BYTES + 1) as u64;
    stdin.lock().take(cap).read_to_string(&mut buf)?;
    Ok(buf)
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

