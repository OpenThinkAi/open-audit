use anyhow::Result;
use clap::Parser;
use std::io::Write;
use std::process::ExitCode;

mod builtins;
mod claude_session;
mod cli;
mod config;
mod evidence;
mod finding;
mod init;
mod output;
mod render;
mod resolve;
mod run;
mod sandbox;
mod spec;
mod subject;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,oaudit=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();
    let result: Result<u8> = cli::dispatch(cli).await;

    // Flush stdout BEFORE exiting — std::process::exit doesn't flush, and
    // ExitCode does the same on the way out. Without this, JSON written
    // by output::emit can be truncated on a piped consumer when we exit
    // non-zero.
    let _ = std::io::stdout().flush();

    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}
