use anyhow::Result;
use clap::Parser;

mod builtins;
mod claude_session;
mod cli;
mod config;
mod evidence;
mod finding;
mod init;
mod output;
mod resolve;
mod run;
mod sandbox;
mod spec;
mod subject;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,oaudit=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();
    cli::dispatch(cli).await
}
