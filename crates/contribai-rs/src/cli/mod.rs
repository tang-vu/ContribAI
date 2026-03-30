//! CLI interface for ContribAI.
//!
//! Provides hunt, patrol, stats, and mcp-server commands
//! with rich console output via `colored` and `indicatif`.

use clap::{Parser, Subcommand};
use colored::Colorize;


/// ContribAI — AI agent that autonomously contributes to open source.
#[derive(Parser)]
#[command(name = "contribai", version, about, long_about = None)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, global = true)]
    config: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Auto-discover repos, analyze code, and submit PRs
    Run {
        /// Target language filter
        #[arg(short, long)]
        language: Option<String>,

        /// Star range (e.g. "100-5000")
        #[arg(short, long)]
        stars: Option<String>,

        /// Dry run — analyze but don't create PRs
        #[arg(long)]
        dry_run: bool,
    },

    /// Hunt mode: aggressive multi-round discovery
    Hunt {
        /// Number of discovery rounds
        #[arg(short, long, default_value = "5")]
        rounds: u32,

        /// Delay between rounds (seconds)
        #[arg(short, long, default_value = "30")]
        delay: u32,

        /// Target language
        #[arg(short, long)]
        language: Option<String>,

        /// Dry run
        #[arg(long)]
        dry_run: bool,
    },

    /// Monitor open PRs for review comments and respond
    Patrol {
        /// Dry run — check but don't respond
        #[arg(long)]
        dry_run: bool,
    },

    /// Target a specific repository
    Target {
        /// Repository URL (e.g., https://github.com/owner/repo)
        url: String,

        /// Dry run
        #[arg(long)]
        dry_run: bool,
    },

    /// Start MCP server for Claude/Antigravity integration
    McpServer,

    /// Show contribution statistics
    Stats,

    /// Show version and build info
    Version,
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Commands::Run {
                language,
                stars,
                dry_run,
            } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                print_config_summary(&config, dry_run);

                if let Some(lang) = &language {
                    println!("   {}: {}", "Language".dimmed(), lang.cyan());
                }
                if let Some(s) = &stars {
                    println!("   {}: {}", "Stars".dimmed(), s.cyan());
                }
                println!();

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config, &github, llm.as_ref(), &memory, &event_bus,
                );

                let result = pipeline.run(None, dry_run).await?;
                print_result(&result, dry_run);
                Ok(())
            }

            Commands::Hunt {
                rounds,
                delay: _delay,
                language,
                dry_run,
            } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;
                print_config_summary(&config, dry_run);

                println!(
                    "   {}: {} rounds",
                    "Hunt mode".yellow().bold(),
                    rounds.to_string().cyan()
                );
                if let Some(lang) = &language {
                    println!("   {}: {}", "Language".dimmed(), lang.cyan());
                }
                println!();

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config, &github, llm.as_ref(), &memory, &event_bus,
                );

                // Run pipeline for each round
                let mut total = contribai::orchestrator::pipeline::PipelineResult::default();
                for rnd in 1..=rounds {
                    println!(
                        "\n{} Round {}/{} {}",
                        "🔥".bold(),
                        rnd.to_string().cyan(),
                        rounds,
                        "━".repeat(40).dimmed()
                    );

                    match pipeline.run(None, dry_run).await {
                        Ok(result) => {
                            total.repos_analyzed += result.repos_analyzed;
                            total.findings_total += result.findings_total;
                            total.contributions_generated += result.contributions_generated;
                            total.prs_created += result.prs_created;
                            total.errors.extend(result.errors);
                        }
                        Err(e) => {
                            println!("  {} {}", "Error:".red(), e);
                            total.errors.push(e.to_string());
                        }
                    }
                }

                print_result(&total, dry_run);
                Ok(())
            }

            Commands::Patrol { dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "👁  {} {}",
                    "Patrol mode".cyan().bold(),
                    if dry_run { "(DRY RUN)".yellow().to_string() } else { "(LIVE)".green().to_string() }
                );

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;

                // Get open PRs from memory
                let prs = memory.get_prs(Some("open"), 50)?;
                let pr_values: Vec<serde_json::Value> = prs
                    .iter()
                    .map(|pr| {
                        serde_json::json!({
                            "repo": pr.get("repo").unwrap_or(&String::new()),
                            "pr_number": pr.get("pr_number").unwrap_or(&String::new()).parse::<i64>().unwrap_or(0),
                            "status": pr.get("status").unwrap_or(&String::new()),
                        })
                    })
                    .collect();

                let mut patrol = contribai::pr::patrol::PrPatrol::new(&github, llm.as_ref());
                let result = patrol.patrol(&pr_values, dry_run).await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                println!("\n{}", "━".repeat(50).dimmed());
                println!("  {} PRs checked:  {}", "📊".bold(), result.prs_checked.to_string().cyan());
                println!("  {} Fixes pushed: {}", "🔧".bold(), result.fixes_pushed.to_string().green());
                println!("  {} Replies sent: {}", "💬".bold(), result.replies_sent.to_string().cyan());
                if result.prs_skipped > 0 {
                    println!("  {} Skipped:     {}", "⏭".bold(), result.prs_skipped.to_string().yellow());
                }
                Ok(())
            }

            Commands::Target { url, dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "🎯 Targeting: {} {}",
                    url.cyan().bold(),
                    if dry_run { "(DRY RUN)".yellow().to_string() } else { "(LIVE)".green().to_string() }
                );
                println!();

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config, &github, llm.as_ref(), &memory, &event_bus,
                );

                let result = pipeline.run(None, dry_run).await?;
                print_result(&result, dry_run);
                Ok(())
            }

            Commands::McpServer => {
                print_banner();
                println!("🔌 MCP server starting on stdio...");
                println!("   Waiting for Claude Desktop connection...\n");

                let config = load_config(self.config.as_deref())?;
                let github = create_github(&config)?;
                let memory = create_memory(&config)?;

                contribai::mcp::server::run_stdio_server(&github, &memory).await?;
                Ok(())
            }

            Commands::Stats => {
                print_banner();
                let config = load_config(self.config.as_deref())?;
                let memory = create_memory(&config)?;

                let stats = memory.get_stats()?;

                println!("{}", "📊 ContribAI Statistics".cyan().bold());
                println!("{}", "━".repeat(40).dimmed());
                println!(
                    "  Repos analyzed:  {}",
                    stats.get("total_repos_analyzed").unwrap_or(&0).to_string().cyan()
                );
                println!(
                    "  PRs submitted:   {}",
                    stats.get("total_prs_submitted").unwrap_or(&0).to_string().cyan()
                );
                println!(
                    "  PRs merged:      {}",
                    stats.get("prs_merged").unwrap_or(&0).to_string().green()
                );
                println!(
                    "  Total runs:      {}",
                    stats.get("total_runs").unwrap_or(&0).to_string().cyan()
                );

                // Recent PRs
                let prs = memory.get_prs(None, 5)?;
                if !prs.is_empty() {
                    println!("\n{}", "Recent PRs:".bold());
                    for pr in &prs {
                        let status_str = pr.get("status").map(|s| s.as_str()).unwrap_or("unknown");
                        let status = match status_str {
                            "merged" => status_str.green().to_string(),
                            "open" => status_str.cyan().to_string(),
                            "closed" => status_str.red().to_string(),
                            _ => status_str.dimmed().to_string(),
                        };
                        println!(
                            "  #{} {} [{}] {}",
                            pr.get("pr_number").unwrap_or(&String::new()),
                            pr.get("repo").unwrap_or(&String::new()).dimmed(),
                            status,
                            pr.get("title").unwrap_or(&String::new()),
                        );
                    }
                }

                Ok(())
            }

            Commands::Version => {
                println!(
                    "{} {} (Rust)",
                    "contribai".cyan().bold(),
                    contribai::VERSION
                );
                println!("  Build: release (static binary)");
                println!("  Arch:  {}", std::env::consts::ARCH);
                println!("  OS:    {}", std::env::consts::OS);
                Ok(())
            }
        }
    }
}

fn print_banner() {
    let banner = format!(
        r#"
   ____            _        _ _      _    ___
  / ___|___  _ __ | |_ _ __(_) |__  / \  |_ _|
 | |   / _ \| '_ \| __| '__| | '_ \/ _ \  | |
 | |__| (_) | | | | |_| |  | | |_) / ___ \ | |
  \____\___/|_| |_|\__|_|  |_|_.__/_/   \_\___|

  AI Agent for Open Source Contributions v{}
"#,
        contribai::VERSION
    );
    println!("{}", banner.cyan());
}

fn print_config_summary(config: &contribai::core::config::ContribAIConfig, dry_run: bool) {
    let mode = if dry_run {
        "DRY RUN".yellow().bold().to_string()
    } else {
        "LIVE".green().bold().to_string()
    };

    println!("🚀 Starting ContribAI pipeline ({})", mode);
    println!(
        "   {}: {} ({})",
        "LLM".dimmed(),
        config.llm.provider.cyan(),
        config.llm.model.dimmed()
    );
    println!(
        "   {}: {}",
        "Max PRs/day".dimmed(),
        config.github.max_prs_per_day.to_string().cyan()
    );
}

fn print_result(result: &contribai::orchestrator::pipeline::PipelineResult, dry_run: bool) {
    println!("\n{}", "━".repeat(50).dimmed());

    if dry_run {
        println!("{}", "  [DRY RUN] No PRs were actually created".yellow());
    }

    println!(
        "  {} Repos analyzed:         {}",
        "📦",
        result.repos_analyzed.to_string().cyan()
    );
    println!(
        "  {} Findings:               {}",
        "🔍",
        result.findings_total.to_string().cyan()
    );
    println!(
        "  {} Contributions generated: {}",
        "⚙️",
        result.contributions_generated.to_string().cyan()
    );
    println!(
        "  {} PRs created:            {}",
        "🎉",
        result.prs_created.to_string().green().bold()
    );

    if !result.errors.is_empty() {
        println!(
            "  {} Errors:                 {}",
            "⚠️",
            result.errors.len().to_string().red()
        );
    }
}

fn load_config(
    path: Option<&str>,
) -> anyhow::Result<contribai::core::config::ContribAIConfig> {
    use contribai::core::config::ContribAIConfig;

    if let Some(p) = path {
        ContribAIConfig::from_yaml(std::path::Path::new(p))
            .map_err(|e| anyhow::anyhow!("{}", e))
    } else {
        ContribAIConfig::load().map_err(|e| anyhow::anyhow!("{}", e))
    }
}

fn create_github(
    config: &contribai::core::config::ContribAIConfig,
) -> anyhow::Result<contribai::github::client::GitHubClient> {
    if config.github.token.is_empty() {
        anyhow::bail!("GitHub token not configured! Set GITHUB_TOKEN env or config.yaml");
    }
    contribai::github::client::GitHubClient::new(
        &config.github.token,
        config.github.rate_limit_buffer,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))
}

fn create_llm(
    config: &contribai::core::config::ContribAIConfig,
) -> anyhow::Result<Box<dyn contribai::llm::provider::LlmProvider>> {
    contribai::llm::provider::create_llm_provider(&config.llm)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

fn create_memory(
    config: &contribai::core::config::ContribAIConfig,
) -> anyhow::Result<contribai::orchestrator::memory::Memory> {
    let db_path = config.storage.resolved_db_path();
    contribai::orchestrator::memory::Memory::open(&db_path)
        .map_err(|e| anyhow::anyhow!("{}", e))
}
