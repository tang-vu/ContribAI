use clap::{Parser, Subcommand};
use tracing::info;

/// ContribAI — AI agent that autonomously contributes to open source.
#[derive(Parser)]
#[command(name = "contribai", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover repos, analyze code, and submit PRs
    Hunt {
        /// Number of discovery rounds
        #[arg(short, long, default_value = "1")]
        rounds: u32,

        /// Dry run — analyze but don't create PRs
        #[arg(long)]
        dry_run: bool,

        /// Target language
        #[arg(short, long)]
        language: Option<String>,
    },

    /// Monitor open PRs for review comments and respond
    Patrol {
        /// Dry run — check but don't respond
        #[arg(long)]
        dry_run: bool,
    },

    /// Start MCP server for Claude/Antigravity integration
    McpServer,

    /// Show contribution statistics
    Stats,

    /// Show version info
    Version,
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Commands::Hunt {
                rounds,
                dry_run,
                language,
            } => {
                info!(rounds, dry_run, ?language, "Starting hunt");
                // TODO: Phase 6 — pipeline.rs
                println!("🔍 Hunt mode: {} rounds (dry_run: {})", rounds, dry_run);
                Ok(())
            }
            Commands::Patrol { dry_run } => {
                info!(dry_run, "Starting patrol");
                // TODO: Phase 5 — patrol.rs
                println!("👁 Patrol mode (dry_run: {})", dry_run);
                Ok(())
            }
            Commands::McpServer => {
                info!("Starting MCP server");
                // TODO: Phase 7 — mcp/server.rs
                println!("🔌 MCP server starting on stdio...");
                Ok(())
            }
            Commands::Stats => {
                // TODO: Phase 6 — memory.rs
                println!("📊 ContribAI v{}", contribai::VERSION);
                println!("Stats coming in Phase 6...");
                Ok(())
            }
            Commands::Version => {
                println!("contribai {}", contribai::VERSION);
                Ok(())
            }
        }
    }
}
