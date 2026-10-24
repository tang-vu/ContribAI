use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing — write to stderr so MCP stdio channel stays clean
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();
    // The dispatcher embeds futures for every command, including large legacy
    // pipelines. Keep it off the main future's stack on platforms with a small
    // default stack (notably Windows), without changing any command's limits.
    Box::pin(cli.run()).await
}
