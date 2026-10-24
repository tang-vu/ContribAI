use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;

fn main() -> anyhow::Result<()> {
    // Initialize tracing — write to stderr so MCP stdio channel stays clean
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Parse before entering Tokio: clap's debug-build command constructors need
    // headroom on Windows' small default main-thread stack. Heap-pin the command
    // future so runtime entry-point frames do not embed/copy the dispatcher too.
    // This does not increase the stack size or change any command's limits.
    let cli = cli::Cli::parse();
    tokio::runtime::Runtime::new()?.block_on(Box::pin(cli.run()))
}
