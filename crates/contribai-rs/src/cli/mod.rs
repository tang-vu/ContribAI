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

    /// Start the web dashboard API server
    WebServer {
        /// Host to bind (default: 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to listen on (default: 5000)
        #[arg(short, long, default_value = "5000")]
        port: u16,
    },

    /// Show contribution statistics
    Stats,

    /// Show version and build info
    Version,

    /// Analyze a repository without creating PRs (analysis-only, always dry run)
    Analyze {
        /// Repository URL (e.g., https://github.com/owner/repo)
        url: String,
    },

    /// Solve open issues in a repository
    Solve {
        /// Repository URL (e.g., https://github.com/owner/repo)
        url: String,

        /// Dry run — classify but don't create PRs
        #[arg(long)]
        dry_run: bool,
    },

    /// Show submitted PRs and their statuses
    Status {
        /// Filter by status (open, merged, closed)
        #[arg(short, long)]
        filter: Option<String>,

        /// Max number of PRs to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Show current configuration
    Config,

    /// Start the scheduler for automated runs
    Schedule {
        /// Cron expression (e.g., "0 */6 * * *")
        #[arg(short, long, default_value = "0 */6 * * *")]
        cron: String,
    },
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

                // ── v5.4: JSONL event logger ─────────────────────────────────
                let log_path = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".contribai")
                    .join("events.jsonl");
                let _log_handle = contribai::core::events::FileEventLogger::new(&log_path)
                    .spawn_logger(&event_bus);
                println!("   {}: {}", "Event log".dimmed(), log_path.display());

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

                // ── v5.4: JSONL event logger ─────────────────────────────────
                let log_path = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".contribai")
                    .join("events.jsonl");
                let _log_handle = contribai::core::events::FileEventLogger::new(&log_path)
                    .spawn_logger(&event_bus);

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

            Commands::WebServer { host, port } => {
                print_banner();
                println!(
                    "  🌐 Starting web dashboard on {}:{}",
                    host.cyan(),
                    port.to_string().cyan()
                );
                println!("  Open http://{}:{} in your browser\n", host, port);
                let config = load_config(self.config.as_deref())?;
                let memory = create_memory(&config)?;
                contribai::web::run_server(memory, &config, &host, port)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            }

            Commands::Analyze { url } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "🔍 Analyzing (dry-run): {}",
                    url.cyan().bold()
                );
                println!();

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config, &github, llm.as_ref(), &memory, &event_bus,
                );

                // Always dry_run=true — analysis only, no PRs created
                let result = pipeline.run(None, true).await?;
                print_result(&result, true);
                println!("\n   Targeted repo: {}", url.dimmed());
                Ok(())
            }

            Commands::Solve { url, dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "🧩 Solving issues in: {} {}",
                    url.cyan().bold(),
                    if dry_run { "(DRY RUN)".yellow().to_string() } else { "(LIVE)".green().to_string() }
                );
                println!();

                let (owner, name) = parse_github_url(&url)?;
                let full_name = format!("{}/{}", owner, name);

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;

                let repo = contribai::core::models::Repository {
                    owner: owner.clone(),
                    name: name.clone(),
                    full_name: full_name.clone(),
                    description: None,
                    language: None,
                    languages: std::collections::HashMap::new(),
                    stars: 0,
                    forks: 0,
                    open_issues: 0,
                    default_branch: "main".to_string(),
                    topics: vec![],
                    html_url: url.clone(),
                    clone_url: format!("https://github.com/{}.git", full_name),
                    has_contributing: false,
                    has_license: false,
                    last_push_at: None,
                    created_at: None,
                };

                let solver = contribai::issues::solver::IssueSolver::new(llm.as_ref(), &github);
                let issues = solver.fetch_solvable_issues(&repo, 10, 3).await;

                if issues.is_empty() {
                    println!("  {} No solvable issues found in {}", "⚠️".bold(), full_name.cyan());
                    return Ok(());
                }

                println!(
                    "  {} Found {} solvable issue(s):\n",
                    "📋".bold(),
                    issues.len().to_string().cyan()
                );
                println!(
                    "  {:>6}  {:<45}  {:<12}  {}",
                    "Issue#".dimmed(),
                    "Title".dimmed(),
                    "Category".dimmed(),
                    "URL".dimmed()
                );
                println!("  {}", "─".repeat(80).dimmed());

                for issue in &issues {
                    let category = solver.classify_issue(issue);
                    let cat_str = format!("{:?}", category);
                    let title: String = issue.title.chars().take(43).collect();
                    println!(
                        "  {:>6}  {:<45}  {:<12}  {}",
                        format!("#{}", issue.number).cyan(),
                        title,
                        cat_str.yellow(),
                        issue.html_url.dimmed(),
                    );
                }

                if dry_run {
                    println!("\n  {} Dry run — no PRs submitted.", "[DRY RUN]".yellow());
                }
                Ok(())
            }

            Commands::Status { filter, limit } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;
                let memory = create_memory(&config)?;

                let prs = memory.get_prs(filter.as_deref(), limit)?;

                println!("{}", "📋 Submitted PRs".cyan().bold());
                println!("{}", "━".repeat(80).dimmed());

                if prs.is_empty() {
                    println!("  No PRs found.");
                    return Ok(());
                }

                println!(
                    "  {:>4}  {:<30}  {:<8}  {}",
                    "PR#".dimmed(),
                    "Repo".dimmed(),
                    "Status".dimmed(),
                    "URL".dimmed()
                );
                println!("  {}", "─".repeat(76).dimmed());

                for pr in &prs {
                    let pr_number = pr.get("pr_number").map(|s| s.as_str()).unwrap_or("?");
                    let repo = pr.get("repo").map(|s| s.as_str()).unwrap_or("unknown");
                    let status_str = pr.get("status").map(|s| s.as_str()).unwrap_or("unknown");
                    let url = pr.get("url").map(|s| s.as_str()).unwrap_or("");

                    let status_colored = match status_str {
                        "merged" => status_str.green().to_string(),
                        "open" => status_str.cyan().to_string(),
                        "closed" => status_str.red().to_string(),
                        _ => status_str.dimmed().to_string(),
                    };

                    let repo_short: String = repo.chars().take(28).collect();
                    println!(
                        "  {:>4}  {:<30}  {:<8}  {}",
                        format!("#{}", pr_number).cyan(),
                        repo_short,
                        status_colored,
                        url.dimmed(),
                    );
                }

                println!("\n  Showing {} PR(s).", prs.len().to_string().cyan());
                Ok(())
            }

            Commands::Config => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!("{}", "⚙️  Current Configuration".cyan().bold());
                println!("{}", "━".repeat(50).dimmed());

                // GitHub token — show last 4 chars masked
                let token_display = if config.github.token.is_empty() {
                    "(not set)".red().to_string()
                } else {
                    let last4: String = config.github.token.chars().rev().take(4).collect::<String>()
                        .chars().rev().collect();
                    format!("****{}", last4).yellow().to_string()
                };
                println!("  {:<18} {}", "GitHub token:".dimmed(), token_display);
                println!(
                    "  {:<18} {}",
                    "Max PRs/day:".dimmed(),
                    config.github.max_prs_per_day.to_string().cyan()
                );

                println!(
                    "  {:<18} {} / {}",
                    "LLM:".dimmed(),
                    config.llm.provider.cyan(),
                    config.llm.model.dimmed()
                );

                let langs = config.discovery.languages.join(", ");
                println!(
                    "  {:<18} {} | stars: {}-{}",
                    "Discovery:".dimmed(),
                    langs.cyan(),
                    config.discovery.stars_min.to_string().dimmed(),
                    config.discovery.stars_max.to_string().dimmed()
                );

                println!(
                    "  {:<18} {} concurrent | quality: {}",
                    "Pipeline:".dimmed(),
                    config.pipeline.max_concurrent_repos.to_string().cyan(),
                    config.pipeline.min_quality_score.to_string().dimmed()
                );

                let db_path = config.storage.resolved_db_path();
                println!(
                    "  {:<18} {}",
                    "Storage:".dimmed(),
                    db_path.display().to_string().dimmed()
                );

                println!(
                    "  {:<18} {} (enabled: {})",
                    "Scheduler:".dimmed(),
                    config.scheduler.cron.cyan(),
                    if config.scheduler.enabled { "yes".green().to_string() } else { "no".red().to_string() }
                );

                Ok(())
            }

            Commands::Schedule { cron } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "⏰ Starting scheduler with cron: {}",
                    cron.cyan().bold()
                );
                println!("   Press Ctrl+C to stop.\n");

                // Use Arc so the closure can own config data and re-create clients each run
                let config = std::sync::Arc::new(config);
                let config_clone = config.clone();

                let scheduler = contribai::scheduler::ContribScheduler::new(&cron, true)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                scheduler.start(move || {
                    let cfg = config_clone.clone();
                    async move {
                        let github = match contribai::github::client::GitHubClient::new(
                            &cfg.github.token,
                            cfg.github.rate_limit_buffer,
                        ) {
                            Ok(g) => g,
                            Err(e) => return Err(e.to_string()),
                        };
                        let llm = match contribai::llm::provider::create_llm_provider(&cfg.llm) {
                            Ok(l) => l,
                            Err(e) => return Err(e.to_string()),
                        };
                        let db_path = cfg.storage.resolved_db_path();
                        let memory = match contribai::orchestrator::memory::Memory::open(&db_path) {
                            Ok(m) => m,
                            Err(e) => return Err(e.to_string()),
                        };
                        let event_bus = contribai::core::events::EventBus::default();
                        let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                            &cfg, &github, llm.as_ref(), &memory, &event_bus,
                        );
                        pipeline
                            .run(None, cfg.pipeline.dry_run)
                            .await
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    }
                })
                .await;

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

/// Parse a GitHub URL into (owner, repo) tuple.
fn parse_github_url(url: &str) -> anyhow::Result<(String, String)> {
    // Handle both https://github.com/owner/repo and owner/repo formats
    let path = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .unwrap_or(url);

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 2 {
        Ok((parts[0].to_string(), parts[1].to_string()))
    } else {
        Err(anyhow::anyhow!(
            "Invalid GitHub URL: {}. Expected format: https://github.com/owner/repo",
            url
        ))
    }
}
