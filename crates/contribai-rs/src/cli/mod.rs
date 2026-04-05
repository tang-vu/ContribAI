//! CLI interface for ContribAI.
//!
//! Interactive CLI like `claude` / `gemini` — wizard setup, config get/set,
//! arrow-key menus, and all operations accessible without editing YAML.

pub mod config_editor;
#[cfg(feature = "tui")]
pub mod tui;
pub mod wizard;

use clap::{Parser, Subcommand};
use colored::Colorize;

/// ContribAI — AI agent that autonomously contributes to open source.
///
/// Run without arguments for interactive menu mode.
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
    command: Option<Commands>,
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

        /// Approve HIGH risk changes for auto-submission
        #[arg(long)]
        approve: bool,
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

        /// Approve HIGH risk changes for auto-submission
        #[arg(long)]
        approve: bool,
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

    /// Sweep all repositories in the watchlist (config.discovery.watchlist)
    Watchlist {
        /// Dry run — analyze but don't submit PRs
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

    // ── Interactive / setup commands ──────────────────────────────────────────
    /// Interactive setup wizard — configure provider, API keys, GitHub auth
    Init {
        /// Output config file path
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Check authentication status for all providers
    Login,

    /// Get or set configuration values without editing config.yaml
    ///
    /// Examples:
    ///   contribai config-get llm.provider
    ///   contribai config-set llm.provider vertex
    ///   contribai config-set github.max_prs_per_day 20
    ///   contribai config-list
    ConfigGet {
        /// Dotted key (e.g. llm.provider, github.max_prs_per_day)
        key: String,
    },

    ConfigSet {
        /// Dotted key (e.g. llm.provider)
        key: String,
        /// New value
        value: String,
    },

    ConfigList,

    // ── Parity commands (matches Python CLI) ──────────────────────────────────
    /// Show contribution leaderboard and merge rate statistics
    Leaderboard {
        /// Max entries to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// List available LLM models and their capabilities
    Models {
        /// Filter by task type (analysis, code, review, docs)
        #[arg(short, long)]
        task: Option<String>,
    },

    /// Send a test notification to configured channels (Slack, Discord, Telegram)
    NotifyTest,

    /// Clean up forks created by ContribAI (delete merged/closed PR forks)
    Cleanup {
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// List available contribution templates
    Templates {
        /// Filter by contribution type (e.g. security_fix, docs_improve)
        #[arg(short, long)]
        r#type: Option<String>,
    },

    /// Run pipeline with a named profile (security-focused, docs-focused, full-scan, gentle)
    Profile {
        /// Profile name, or 'list' to show all profiles
        name: String,

        /// Dry run — analyze but don't create PRs
        #[arg(long)]
        dry_run: bool,
    },

    /// Show ContribAI system status — memory DB, PRs, GitHub rate limits
    SystemStatus,

    /// Interactive TUI mode — browse PRs, repos, and run operations
    Interactive,

    /// Dream — consolidate memory into durable repo profiles
    ///
    /// Aggregates PR outcomes, feedback, and working memory
    /// into repo personality profiles for smarter contributions.
    Dream {
        /// Force dream even if gates haven't been met
        #[arg(long)]
        force: bool,
    },

    /// Run environment diagnostics — check config, auth, LLM, and system health
    Doctor,
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        // No subcommand → interactive menu (like `claude` / `gemini`)
        let command = match self.command {
            Some(cmd) => cmd,
            None => run_interactive_menu()?,
        };

        match command {
            Commands::Run {
                language,
                stars,
                dry_run,
                approve,
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
                if approve {
                    println!(
                        "   {}: {}",
                        "Approve".dimmed(),
                        "HIGH risk enabled".yellow()
                    );
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

                let mut pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config,
                    &github,
                    llm.as_ref(),
                    &memory,
                    &event_bus,
                );
                pipeline.set_approve_high_risk(approve);

                let result = pipeline.run(None, dry_run).await?;
                print_result(&result, dry_run);
                Ok(())
            }

            Commands::Hunt {
                rounds,
                delay,
                language,
                dry_run,
                approve,
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
                if approve {
                    println!(
                        "   {}: {}",
                        "Approve".dimmed(),
                        "HIGH risk enabled".yellow()
                    );
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

                let mut pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config,
                    &github,
                    llm.as_ref(),
                    &memory,
                    &event_bus,
                );
                pipeline.set_approve_high_risk(approve);

                let total = pipeline.hunt(rounds, delay as u64, dry_run, "both").await?;

                print_result(&total, dry_run);
                Ok(())
            }

            Commands::Patrol { dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "👁  {} {}",
                    "Patrol mode".cyan().bold(),
                    if dry_run {
                        "(DRY RUN)".yellow().to_string()
                    } else {
                        "(LIVE)".green().to_string()
                    }
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

                let mut patrol = contribai::pr::patrol::PrPatrol::new(&github, llm.as_ref())
                    .with_memory(&memory);
                let result = patrol
                    .patrol(&pr_values, dry_run)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                // Auto-clean PRs that returned 404
                let mut cleaned = 0u32;
                for err in &result.errors {
                    if let Some(rest) = err.strip_prefix("NOT_FOUND:") {
                        let parts: Vec<&str> = rest.rsplitn(2, ':').collect();
                        if parts.len() == 2 {
                            let pr_num: i64 = parts[0].parse().unwrap_or(0);
                            let repo_name = parts[1];
                            if pr_num > 0 {
                                let _ = memory.update_pr_status(repo_name, pr_num, "closed");
                                cleaned += 1;
                            }
                        }
                    }
                }

                println!("\n{}", "━".repeat(50).dimmed());
                println!(
                    "  {} PRs checked:  {}",
                    "📊".bold(),
                    result.prs_checked.to_string().cyan()
                );
                println!(
                    "  {} Fixes pushed: {}",
                    "🔧".bold(),
                    result.fixes_pushed.to_string().green()
                );
                println!(
                    "  {} Replies sent: {}",
                    "💬".bold(),
                    result.replies_sent.to_string().cyan()
                );
                if result.prs_skipped > 0 {
                    println!(
                        "  {} Skipped:     {}",
                        "⏭".bold(),
                        result.prs_skipped.to_string().yellow()
                    );
                }
                if cleaned > 0 {
                    println!(
                        "  {} Cleaned:     {} stale PRs removed from memory",
                        "🗑️".bold(),
                        cleaned.to_string().red()
                    );
                }
                Ok(())
            }

            Commands::Watchlist { dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                let watchlist = &config.discovery.watchlist;
                if watchlist.is_empty() {
                    println!(
                        "{} No repositories in watchlist. Add repos to config.yaml under discovery.watchlist:",
                        "⚠️".yellow()
                    );
                    println!();
                    println!("  discovery:");
                    println!("    watchlist:");
                    println!("      - \"owner/repo\"");
                    println!("      - \"myorg/myproject\"");
                    return Ok(());
                }

                println!(
                    "📋 Watchlist sweep: {} repo(s) {}",
                    watchlist.len().to_string().cyan().bold(),
                    if dry_run {
                        "(DRY RUN)".yellow().to_string()
                    } else {
                        "(LIVE)".green().to_string()
                    }
                );
                println!();

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config,
                    &github,
                    llm.as_ref(),
                    &memory,
                    &event_bus,
                );

                let mut total_findings = 0usize;
                let mut total_prs = 0usize;

                for (i, repo_ref) in watchlist.iter().enumerate() {
                    let parts: Vec<&str> = repo_ref.splitn(2, '/').collect();
                    if parts.len() != 2 {
                        println!(
                            "  {} Skipping invalid entry: {} (expected \"owner/repo\")",
                            "⚠️".yellow(),
                            repo_ref.red()
                        );
                        continue;
                    }
                    let (owner, name) = (parts[0], parts[1]);

                    println!(
                        "  [{}/{}] 🎯 {}",
                        (i + 1).to_string().cyan(),
                        watchlist.len(),
                        repo_ref.bold()
                    );

                    match pipeline.run_targeted(owner, name, dry_run).await {
                        Ok(result) => {
                            total_findings += result.findings_total;
                            total_prs += result.prs_created;
                            println!(
                                "         {} findings, {} PRs",
                                result.findings_total, result.prs_created
                            );
                        }
                        Err(e) => {
                            println!("         {} {}", "Error:".red(), e);
                        }
                    }
                    println!();
                }

                println!(
                    "📊 Watchlist complete: {} total findings, {} PRs submitted",
                    total_findings.to_string().cyan().bold(),
                    total_prs.to_string().green().bold(),
                );
                Ok(())
            }

            Commands::Target { url, dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "🎯 Targeting: {} {}",
                    url.cyan().bold(),
                    if dry_run {
                        "(DRY RUN)".yellow().to_string()
                    } else {
                        "(LIVE)".green().to_string()
                    }
                );
                println!();

                let (owner, name) = parse_github_url(&url)?;

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config,
                    &github,
                    llm.as_ref(),
                    &memory,
                    &event_bus,
                );

                let result = pipeline.run_targeted(&owner, &name, dry_run).await?;
                print_result(&result, dry_run);
                Ok(())
            }

            Commands::McpServer => {
                // MCP uses stdout for JSON-RPC — all human output goes to stderr
                eprintln!("🔌 ContribAI MCP server starting on stdio...");
                eprintln!("   Waiting for client connection...\n");

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
                    stats
                        .get("total_repos_analyzed")
                        .unwrap_or(&0)
                        .to_string()
                        .cyan()
                );
                println!(
                    "  PRs submitted:   {}",
                    stats
                        .get("total_prs_submitted")
                        .unwrap_or(&0)
                        .to_string()
                        .cyan()
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

            #[cfg(feature = "web")]
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

            #[cfg(not(feature = "web"))]
            Commands::WebServer { .. } => {
                anyhow::bail!("Web dashboard not available. Build with --features web");
            }

            Commands::Analyze { url } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!("🔍 Analyzing (dry-run): {}", url.cyan().bold());
                println!();

                let (owner, name) = parse_github_url(&url)?;

                let github = create_github(&config)?;
                let llm = create_llm(&config)?;
                let memory = create_memory(&config)?;
                let event_bus = contribai::core::events::EventBus::default();

                let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                    &config,
                    &github,
                    llm.as_ref(),
                    &memory,
                    &event_bus,
                );

                // Always dry_run=true — analysis only, no PRs created
                let result = pipeline.run_targeted(&owner, &name, true).await?;
                print_result(&result, true);
                Ok(())
            }

            Commands::Solve { url, dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!(
                    "🧩 Solving issues in: {} {}",
                    url.cyan().bold(),
                    if dry_run {
                        "(DRY RUN)".yellow().to_string()
                    } else {
                        "(LIVE)".green().to_string()
                    }
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
                    println!(
                        "  {} No solvable issues found in {}",
                        "⚠️".bold(),
                        full_name.cyan()
                    );
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

                // v5.5: Actually solve issues and create PRs
                println!();

                let memory = create_memory(&config)?;
                let file_tree = github
                    .get_file_tree(&owner, &name, None)
                    .await
                    .unwrap_or_default();

                let repo_context = contribai::core::models::RepoContext {
                    repo: repo.clone(),
                    file_tree,
                    readme_content: None,
                    contributing_guide: None,
                    relevant_files: std::collections::HashMap::new(),
                    open_issues: Vec::new(),
                    coding_style: None,
                    symbol_map: std::collections::HashMap::new(),
                    resolved_imports: std::collections::HashMap::new(),
                    file_ranks: std::collections::HashMap::new(),
                };

                let generator = contribai::generator::engine::ContributionGenerator::new(
                    llm.as_ref(),
                    &config.contribution,
                );

                let mut prs_created = 0u32;
                for issue in &issues {
                    println!(
                        "  {} Solving issue #{}...",
                        "🔧".bold(),
                        issue.number.to_string().cyan()
                    );

                    // Solve: issue → findings
                    let findings = solver.solve_issue_deep(issue, &repo, &repo_context).await;
                    if findings.is_empty() {
                        println!("    {} No actionable findings", "⚠️".dimmed());
                        continue;
                    }

                    // Fetch file contents for identified files
                    let mut ctx = repo_context.clone();
                    for f in &findings {
                        if !f.file_path.is_empty() && !ctx.relevant_files.contains_key(&f.file_path)
                        {
                            if let Ok(content) = github
                                .get_file_content(&owner, &name, &f.file_path, None)
                                .await
                            {
                                ctx.relevant_files.insert(f.file_path.clone(), content);
                            }
                        }
                    }

                    // Generate code for each finding
                    let mut valid = Vec::new();
                    for finding in &findings {
                        if let Ok(Some(mut contrib)) = generator.generate(finding, &ctx).await {
                            contrib.description =
                                format!("Fixes #{}\n\n{}", issue.number, contrib.description);
                            valid.push(contrib);
                        }
                    }

                    if valid.is_empty() {
                        println!("    {} Generation failed", "❌".dimmed());
                        continue;
                    }

                    // Merge into single PR
                    let file_count = valid.iter().map(|c| c.changes.len()).sum::<usize>();
                    let mut merged =
                        contribai::orchestrator::pipeline::merge_contributions_pub(valid);
                    merged.title = format!("fix: resolve #{} — {}", issue.number, issue.title);
                    merged.commit_message = format!(
                        "fix: resolve #{} — {}\n\nFixes #{}",
                        issue.number, issue.title, issue.number
                    );

                    if dry_run {
                        println!(
                            "    {} Would create PR ({} files)",
                            "[DRY RUN]".yellow(),
                            file_count
                        );
                        continue;
                    }

                    let mut pr_mgr = contribai::pr::manager::PrManager::new(&github);
                    match pr_mgr.create_pr(&merged, &repo).await {
                        Ok(pr_result) => {
                            prs_created += 1;
                            let _ = memory.record_pr(
                                &full_name,
                                pr_result.pr_number,
                                &pr_result.pr_url,
                                &merged.title,
                                &merged.contribution_type.to_string(),
                                &pr_result.branch_name,
                                &pr_result.fork_full_name,
                            );
                            println!(
                                "    {} PR #{} created → {}",
                                "✅".bold(),
                                pr_result.pr_number.to_string().green(),
                                pr_result.pr_url.dimmed()
                            );
                        }
                        Err(e) => {
                            println!("    {} PR failed: {}", "❌".bold(), format!("{}", e).red());
                        }
                    }
                }

                println!();
                if prs_created > 0 {
                    println!(
                        "  {} {} PR(s) created from {} issues",
                        "🎉".bold(),
                        prs_created.to_string().green(),
                        issues.len()
                    );
                } else if dry_run {
                    println!("  {} Dry run — no PRs submitted.", "[DRY RUN]".yellow());
                } else {
                    println!("  {} No PRs could be generated.", "⚠️".bold());
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
                    let last4: String = config
                        .github
                        .token
                        .chars()
                        .rev()
                        .take(4)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
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
                    if config.scheduler.enabled {
                        "yes".green().to_string()
                    } else {
                        "no".red().to_string()
                    }
                );

                Ok(())
            }

            Commands::Schedule { cron } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;

                println!("⏰ Starting scheduler with cron: {}", cron.cyan().bold());
                println!("   Press Ctrl+C to stop.\n");

                // Use Arc so the closure can own config data and re-create clients each run
                let config = std::sync::Arc::new(config);
                let config_clone = config.clone();

                let scheduler = contribai::scheduler::ContribScheduler::new(&cron, true)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                scheduler
                    .start(move || {
                        let cfg = config_clone.clone();
                        async move {
                            let github = match contribai::github::client::GitHubClient::new(
                                &cfg.github.token,
                                cfg.github.rate_limit_buffer,
                            ) {
                                Ok(g) => g,
                                Err(e) => return Err(e.to_string()),
                            };
                            let llm = match contribai::llm::provider::create_llm_provider(&cfg.llm)
                            {
                                Ok(l) => l,
                                Err(e) => return Err(e.to_string()),
                            };
                            let db_path = cfg.storage.resolved_db_path();
                            let memory =
                                match contribai::orchestrator::memory::Memory::open(&db_path) {
                                    Ok(m) => m,
                                    Err(e) => return Err(e.to_string()),
                                };
                            let event_bus = contribai::core::events::EventBus::default();
                            let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
                                &cfg,
                                &github,
                                llm.as_ref(),
                                &memory,
                                &event_bus,
                            );

                            // KAIROS: Run → Patrol → Dream (full autonomous loop)
                            tracing::info!("🔄 KAIROS cycle: Run → Patrol → Dream");

                            // 1. Pipeline run (discover + analyze + PR)
                            if let Err(e) = pipeline.run(None, cfg.pipeline.dry_run).await {
                                tracing::warn!(error = %e, "Pipeline run had errors");
                            }

                            // 2. Patrol (respond to review comments on open PRs)
                            let mut patrol = contribai::pr::patrol::PrPatrol::new(
                                &github,
                                llm.as_ref(),
                            )
                            .with_memory(&memory);
                            match memory.get_prs(Some("open"), 50) {
                                Ok(prs) => {
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
                                    match patrol.patrol(&pr_values, false).await {
                                        Ok(r) => tracing::info!(
                                            checked = r.prs_checked,
                                            fixes = r.fixes_pushed,
                                            "Patrol complete"
                                        ),
                                        Err(e) => tracing::warn!(error = %e, "Patrol had errors"),
                                    }
                                }
                                Err(e) => tracing::warn!(error = %e, "Could not load PR records"),
                            }

                            // 3. Dream (consolidate memory if gates are met)
                            pipeline.maybe_dream();

                            Ok(())
                        }
                    })
                    .await;

                Ok(())
            }

            // ── Interactive / setup commands ───────────────────────────────────
            Commands::Init { output } => {
                let out_path = output.as_deref();
                if let Some(result) = wizard::run_init_wizard(out_path.map(std::path::Path::new))? {
                    wizard::write_wizard_config(&result)?;
                }
                Ok(())
            }

            Commands::Login => run_login_check(self.config.as_deref()).await,

            Commands::ConfigGet { key } => {
                let path = config_editor::resolve_config_path(self.config.as_deref());
                config_editor::get_config_value(&path, &key)
            }

            Commands::ConfigSet { key, value } => {
                let path = config_editor::resolve_config_path(self.config.as_deref());
                config_editor::set_config_value(&path, &key, &value)
            }

            Commands::ConfigList => {
                let path = config_editor::resolve_config_path(self.config.as_deref());
                config_editor::list_config(&path)
            }

            // ── Parity commands ───────────────────────────────────────────────
            Commands::Leaderboard { limit } => run_leaderboard(self.config.as_deref(), limit),

            Commands::Models { task } => run_models(task.as_deref()),

            Commands::NotifyTest => run_notify_test(self.config.as_deref()).await,

            Commands::Cleanup { yes } => run_cleanup(self.config.as_deref(), yes).await,

            Commands::Templates { r#type } => run_templates(r#type.as_deref()),

            Commands::Profile { name, dry_run } => {
                print_banner();
                let config = load_config(self.config.as_deref())?;
                run_profile(&name, dry_run, &config).await
            }

            Commands::SystemStatus => run_system_status(self.config.as_deref()).await,

            #[cfg(feature = "tui")]
            Commands::Interactive => {
                let config = load_config(self.config.as_deref())?;
                tui::run_interactive_tui(&config)
            }

            #[cfg(not(feature = "tui"))]
            Commands::Interactive => {
                anyhow::bail!("TUI not available. Build with --features tui");
            }

            Commands::Dream { force } => {
                print_banner();
                run_dream(self.config.as_deref(), force)
            }

            Commands::Doctor => {
                print_banner();
                run_doctor(self.config.as_deref()).await
            }
        }
    }
}

// ── Interactive menu ──────────────────────────────────────────────────────────

/// Show arrow-key menu when no subcommand given.
fn run_interactive_menu() -> anyhow::Result<Commands> {
    use console::style;
    use dialoguer::Select;

    println!();
    println!(
        "  {} — {}",
        style("ContribAI").cyan().bold(),
        style("AI Agent for Open Source Contributions").dim()
    );
    println!();

    let items = vec![
        "🖥️   Interactive  — full TUI browser (PRs, repos, actions)",
        "🚀  Run          — discover repos and submit PRs",
        "🎯  Target       — analyze a specific repo",
        "🔍  Analyze      — dry-run analysis only",
        "🐛  Solve        — solve open issues",
        "👁   Patrol       — monitor open PRs",
        "🕵️  Hunt         — aggressive multi-round hunt",
        "📊  Stats        — contribution statistics",
        "📋  Leaderboard  — merge rate & repo rankings",
        "📋  Status       — show submitted PRs",
        "🤖  Models       — list available LLM models",
        "📝  Templates    — list contribution templates",
        "🎨  Profile      — run with a named profile",
        "🧹  Cleanup      — delete merged PR forks",
        "🌐  Web server   — start dashboard",
        "📡  System status — DB, rate limits, scheduler",
        "🔔  Notify test  — test notification channels",
        "💤  Dream        — consolidate memory into repo profiles",
        "🩺  Doctor       — check environment health",
        "⚙️   Config       — show current config",
        "🛠   Config set   — change a setting",
        "🔐  Login        — check auth status",
        "✨  Init         — setup wizard",
        "❌  Exit",
    ];

    let selection = Select::new()
        .with_prompt("What do you want to do?")
        .items(&items)
        .default(0)
        .interact()?;

    println!();

    Ok(match selection {
        0 => Commands::Interactive,
        1 => Commands::Run {
            language: None,
            stars: None,
            dry_run: false,
            approve: false,
        },
        2 => {
            let url: String = dialoguer::Input::new()
                .with_prompt("Repository URL")
                .interact_text()?;
            Commands::Target {
                url,
                dry_run: false,
            }
        }
        3 => {
            let url: String = dialoguer::Input::new()
                .with_prompt("Repository URL")
                .interact_text()?;
            Commands::Analyze { url }
        }
        4 => {
            let url: String = dialoguer::Input::new()
                .with_prompt("Repository URL")
                .interact_text()?;
            Commands::Solve {
                url,
                dry_run: false,
            }
        }
        5 => Commands::Patrol { dry_run: false },
        6 => Commands::Hunt {
            rounds: 5,
            delay: 30,
            language: None,
            dry_run: false,
            approve: false,
        },
        7 => Commands::Stats,
        8 => Commands::Leaderboard { limit: 20 },
        9 => Commands::Status {
            filter: None,
            limit: 20,
        },
        10 => Commands::Models { task: None },
        11 => Commands::Templates { r#type: None },
        12 => {
            let name: String = dialoguer::Input::new()
                .with_prompt("Profile name (or 'list' to see all)")
                .default("list".into())
                .interact_text()?;
            Commands::Profile {
                name,
                dry_run: false,
            }
        }
        13 => Commands::Cleanup { yes: false },
        14 => Commands::WebServer {
            host: "127.0.0.1".into(),
            port: 8787,
        },
        15 => Commands::SystemStatus,
        16 => Commands::NotifyTest,
        17 => Commands::Dream { force: false },
        18 => Commands::Doctor,
        19 => Commands::Config,
        20 => {
            let key: String = dialoguer::Input::new()
                .with_prompt("Config key (e.g. llm.provider)")
                .interact_text()?;
            let value: String = dialoguer::Input::new()
                .with_prompt(format!("New value for {}", key))
                .interact_text()?;
            Commands::ConfigSet { key, value }
        }
        21 => Commands::Login,
        22 => Commands::Init { output: None },
        _ => std::process::exit(0),
    })
}

// ── Login check ───────────────────────────────────────────────────────────────

/// Dream — consolidate memory into durable repo profiles.
fn run_dream(config_path: Option<&str>, force: bool) -> anyhow::Result<()> {
    use console::style;

    println!("{}", style("💤 Dream — Memory Consolidation").cyan().bold());
    println!("{}", "━".repeat(50).dimmed());
    println!();

    let config = load_config(config_path)?;
    let memory = create_memory(&config)?;

    // Show pre-dream stats
    let stats = memory
        .get_dream_stats()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(
        "  {:<18} {}",
        style("Last dream:").bold(),
        stats.get("last_dream").unwrap_or(&"never".into())
    );
    println!(
        "  {:<18} {}",
        style("Sessions since:").bold(),
        stats.get("sessions_since_dream").unwrap_or(&"0".into())
    );
    println!(
        "  {:<18} {}",
        style("Repo profiles:").bold(),
        stats.get("repo_profiles").unwrap_or(&"0".into())
    );
    println!();

    // Check gates
    let should = memory
        .should_dream()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if !should && !force {
        println!(
            "  {} Dream gates not met (need 24h + 5 sessions).",
            style("💭").bold()
        );
        println!("  Use {} to override.", style("--force").yellow());
        return Ok(());
    }

    println!("  {} Running dream consolidation...", style("🌙").bold());
    println!();

    let result = memory.run_dream().map_err(|e| anyhow::anyhow!("{}", e))?;

    if result.success {
        println!("  {} Dream complete!", style("✅").bold());
        println!(
            "  {:<18} {}",
            style("Repos profiled:").bold(),
            style(result.repos_profiled.to_string()).green()
        );
        println!(
            "  {:<18} {}",
            style("Entries pruned:").bold(),
            style(result.entries_pruned.to_string()).yellow()
        );

        // Show updated profiles
        let board = memory
            .get_leaderboard(10)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        if !board.is_empty() {
            println!();
            println!("  {}", style("Repo Profiles:").cyan().bold());
            for entry in &board {
                let repo = entry.get("repo").map(|s| s.as_str()).unwrap_or("?");
                let rate = entry.get("merge_rate").map(|s| s.as_str()).unwrap_or("?");
                let preferred = entry.get("preferred").map(|s| s.as_str()).unwrap_or("[]");
                println!(
                    "    {} {:<30} merge rate: {} preferred: {}",
                    style("•").dim(),
                    style(repo).white(),
                    style(rate).green(),
                    style(preferred).dim()
                );
            }
        }
    } else {
        println!("  {} Dream consolidation failed.", style("❌").bold());
    }

    println!();
    Ok(())
}

// ── Doctor ────────────────────────────────────────────────────────────────────

/// Run environment diagnostics — check config, auth, LLM, memory, rate limits.
async fn run_doctor(config_path: Option<&str>) -> anyhow::Result<()> {
    use console::style;

    println!(
        "{}",
        style("🩺 Doctor — Environment Diagnostics").cyan().bold()
    );
    println!("{}", style("━".repeat(50)).dim());
    println!();

    let mut pass = 0u32;
    let mut fail = 0u32;

    // ── 1. Config file ────────────────────────────────
    print!("  {:.<40} ", "Config file parseable");
    match load_config(config_path) {
        Ok(config) => {
            println!("{}", style("✅ OK").green());
            pass += 1;

            // ── 2. GitHub token ────────────────────────────────
            print!("  {:.<40} ", "GitHub token valid");
            match contribai::github::client::GitHubClient::new(
                &config.github.token,
                config.github.rate_limit_buffer,
            ) {
                Ok(github) => {
                    // Try an API call
                    match github.get_repo_details("octocat", "hello-world").await {
                        Ok(_) => {
                            println!("{}", style("✅ OK").green());
                            pass += 1;
                        }
                        Err(e) => {
                            println!("{} ({})", style("❌ FAIL").red(), e);
                            fail += 1;
                        }
                    }

                    // ── 3. Rate limit status ────────────────────
                    print!("  {:.<40} ", "GitHub rate limit");
                    let rate = github.get_rate_status();
                    if rate.is_low {
                        println!(
                            "{} ({} remaining)",
                            style("⚠️  LOW").yellow(),
                            rate.remaining
                        );
                        fail += 1;
                    } else {
                        println!("{} ({} remaining)", style("✅ OK").green(), rate.remaining);
                        pass += 1;
                    }
                }
                Err(e) => {
                    println!("{} ({})", style("❌ FAIL").red(), e);
                    fail += 1;
                    // Skip rate limit check
                    print!("  {:.<40} ", "GitHub rate limit");
                    println!("{}", style("⏭️  SKIP").dim());
                }
            }

            // ── 4. Memory DB ────────────────────────────────────
            print!("  {:.<40} ", "Memory database");
            match create_memory(&config) {
                Ok(memory) => {
                    // Test a read operation
                    match memory.get_prs(None, 1) {
                        Ok(_) => {
                            println!("{}", style("✅ OK").green());
                            pass += 1;
                        }
                        Err(e) => {
                            println!("{} ({})", style("❌ FAIL").red(), e);
                            fail += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("{} ({})", style("❌ FAIL").red(), e);
                    fail += 1;
                }
            }

            // ── 5. LLM provider ────────────────────────────────
            print!(
                "  {:.<40} ",
                format!("LLM provider ({})", config.llm.provider)
            );
            match contribai::llm::provider::create_llm_provider(&config.llm) {
                Ok(llm) => {
                    match llm
                        .complete("Reply with exactly: OK", None, Some(0.0), Some(10))
                        .await
                    {
                        Ok(resp) => {
                            if resp.trim().is_empty() {
                                println!("{} (empty response)", style("⚠️  WARN").yellow());
                                fail += 1;
                            } else {
                                println!("{}", style("✅ OK").green());
                                pass += 1;
                            }
                        }
                        Err(e) => {
                            let msg = format!("{}", e);
                            let short = if msg.len() > 60 { &msg[..60] } else { &msg };
                            println!("{} ({})", style("❌ FAIL").red(), short);
                            fail += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("{} ({})", style("❌ FAIL").red(), e);
                    fail += 1;
                }
            }

            // ── 6. gcloud (if Vertex AI) ──────────────────────
            if config.llm.use_vertex() {
                print!("  {:.<40} ", "gcloud CLI available");
                #[cfg(target_os = "windows")]
                let result = std::process::Command::new("cmd")
                    .args(["/c", "gcloud", "version"])
                    .output();
                #[cfg(not(target_os = "windows"))]
                let result = std::process::Command::new("gcloud").arg("version").output();
                match result {
                    Ok(out) if out.status.success() => {
                        println!("{}", style("✅ OK").green());
                        pass += 1;
                    }
                    _ => {
                        println!("{} (gcloud not found)", style("❌ FAIL").red());
                        fail += 1;
                    }
                }
            }

            // ── 7. Version ──────────────────────────────────────
            print!("  {:.<40} ", "ContribAI version");
            println!("{}", style(format!("v{}", contribai::VERSION)).cyan());
            pass += 1;
        }
        Err(e) => {
            println!("{} ({})", style("❌ FAIL").red(), e);
            fail += 1;
            println!();
            println!(
                "  {} Config is broken — fix it first (run `contribai init`)",
                style("💡").bold()
            );
        }
    }

    // ── Summary ────────────────────────────────────────
    println!();
    println!("{}", style("━".repeat(50)).dim());
    if fail == 0 {
        println!(
            "  {} All {} checks passed — ready to contribute!",
            style("🎉").bold(),
            pass
        );
    } else {
        println!(
            "  {} {}/{} checks passed, {} failed",
            style("⚠️").bold(),
            pass,
            pass + fail,
            style(fail).red().bold()
        );
    }
    println!();
    Ok(())
}

async fn run_login_check(config_path: Option<&str>) -> anyhow::Result<()> {
    use crate::cli::wizard::{mask_secret, LlmChoice};
    use console::style;
    use dialoguer::{Input, Password, Select};

    print_banner();

    loop {
        println!("{}", style("🔐 Authentication Status").cyan().bold());
        println!("{}", "━".repeat(50).dimmed());
        println!();

        let config = load_config(config_path).unwrap_or_default();

        // ── GitHub status ────────────────────────────────────────────────────
        let _gh_configured = if !config.github.token.is_empty() {
            let last4: String = config
                .github
                .token
                .chars()
                .rev()
                .take(4)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            println!(
                "  {:<18} {} (token: ****{})",
                style("GitHub:").bold(),
                style("✅ Token set").green(),
                last4
            );
            true
        } else {
            let gh_result = if cfg!(target_os = "windows") {
                std::process::Command::new("cmd")
                    .args(["/c", "gh", "auth", "token"])
                    .output()
            } else {
                std::process::Command::new("gh")
                    .args(["auth", "token"])
                    .output()
            };
            match gh_result {
                Ok(out) if out.status.success() => {
                    println!(
                        "  {:<18} {}",
                        style("GitHub:").bold(),
                        style("✅ Connected via gh CLI").green()
                    );
                    true
                }
                _ => {
                    println!(
                        "  {:<18} {} — set GITHUB_TOKEN or run 'gh auth login'",
                        style("GitHub:").bold(),
                        style("❌ Not configured").red()
                    );
                    false
                }
            }
        };

        // ── LLM Provider status ──────────────────────────────────────────────
        match config.llm.provider.as_str() {
            "gemini" | "openai" | "anthropic" => {
                if !config.llm.api_key.is_empty() {
                    let last4: String = config
                        .llm
                        .api_key
                        .chars()
                        .rev()
                        .take(4)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    println!(
                        "  {:<18} {} ({} / {} key: ****{})",
                        style("LLM:").bold(),
                        style("✅ API key set").green(),
                        config.llm.provider,
                        config.llm.model,
                        last4
                    );
                } else {
                    println!(
                        "  {:<18} {} — set {} env var",
                        style("LLM:").bold(),
                        style(format!("❌ {} key missing", config.llm.provider)).red(),
                        match config.llm.provider.as_str() {
                            "openai" => "OPENAI_API_KEY",
                            "anthropic" => "ANTHROPIC_API_KEY",
                            _ => "GEMINI_API_KEY",
                        }
                    );
                }
            }
            "vertex" => {
                let token_result = if cfg!(target_os = "windows") {
                    std::process::Command::new("cmd")
                        .args(["/c", "gcloud", "auth", "print-access-token"])
                        .output()
                } else {
                    std::process::Command::new("gcloud")
                        .args(["auth", "print-access-token"])
                        .output()
                };
                let project = if config.llm.vertex_project.is_empty() {
                    std::env::var("GOOGLE_CLOUD_PROJECT").unwrap_or_else(|_| "(not set)".into())
                } else {
                    config.llm.vertex_project.clone()
                };
                match token_result {
                    Ok(out) if out.status.success() => {
                        println!(
                            "  {:<18} {} (project: {})",
                            style("Vertex AI:").bold(),
                            style("✅ gcloud token OK").green(),
                            style(&project).cyan()
                        );
                    }
                    _ => {
                        println!(
                            "  {:<18} {} — run 'gcloud auth application-default login'",
                            style("Vertex AI:").bold(),
                            style("❌ gcloud token failed").red()
                        );
                    }
                }
            }
            "ollama" => {
                let ok = std::process::Command::new("curl")
                    .args(["-s", "http://localhost:11434/api/tags"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if ok {
                    println!(
                        "  {:<18} {}",
                        style("Ollama:").bold(),
                        style("✅ Running on localhost:11434").green()
                    );
                } else {
                    println!(
                        "  {:<18} {} — start with 'ollama serve'",
                        style("Ollama:").bold(),
                        style("❌ Not running").red()
                    );
                }
            }
            p => {
                println!(
                    "  {:<18} {}",
                    style("LLM:").bold(),
                    style(format!("⚪ Provider: {}", p)).dim()
                );
            }
        }

        // ── MCP ──────────────────────────────────────────────────────────────
        println!(
            "  {:<18} {} — start with 'contribai mcp-server'",
            style("MCP Server:").bold(),
            style("⚪ Not running (stdio mode)").dim()
        );

        println!();

        // ── Interactive action menu ──────────────────────────────────────────
        let actions = vec![
            "✅ Done — exit",
            "🔄 Switch LLM provider",
            "🔑 Set GitHub token",
            "🧙 Run full setup wizard (contribai init)",
        ];

        let action = Select::new()
            .with_prompt("What would you like to do?")
            .items(&actions)
            .default(0)
            .interact()?;

        match action {
            0 => {
                // Done
                println!("{}", style("  ✅ Done!").green());
                break;
            }
            1 => {
                // Switch LLM provider
                println!();
                println!("{}", style("  Switch LLM Provider").yellow().bold());

                let provider_idx = Select::new()
                    .with_prompt("Select LLM provider")
                    .items(LlmChoice::all())
                    .default(0)
                    .interact()?;
                let choice = LlmChoice::from_index(provider_idx);

                let (api_key, base_url, vertex_project) = match choice {
                    LlmChoice::VertexAi => {
                        println!(
                            "  {}",
                            style("Uses gcloud ADC — run 'gcloud auth application-default login' first.").dim()
                        );
                        let proj: String = Input::new()
                            .with_prompt("Google Cloud Project ID")
                            .default(std::env::var("GOOGLE_CLOUD_PROJECT").unwrap_or_default())
                            .interact_text()?;
                        (String::new(), String::new(), proj)
                    }
                    LlmChoice::Ollama => {
                        println!(
                            "  {}",
                            style("Make sure Ollama is running: https://ollama.ai").dim()
                        );
                        let default_url = "http://localhost:11434";
                        let url: String = Input::new()
                            .with_prompt("Ollama base URL")
                            .default(default_url.into())
                            .interact_text()
                            .unwrap_or_else(|_| default_url.into());
                        (String::new(), url, String::new())
                    }
                    _ => {
                        let env_hint = match choice {
                            LlmChoice::GeminiApiKey => "https://aistudio.google.com/apikey",
                            LlmChoice::OpenAi => "https://platform.openai.com/api-keys",
                            LlmChoice::Anthropic => "https://console.anthropic.com/",
                            _ => "",
                        };
                        println!(
                            "  {}",
                            style(format!("Get your key at: {}", env_hint)).dim()
                        );
                        let default_url = match choice {
                            LlmChoice::OpenAi => "https://api.openai.com/v1",
                            LlmChoice::Anthropic => "https://api.anthropic.com/v1",
                            _ => "",
                        };
                        let base_url: String = if default_url.is_empty() {
                            String::new()
                        } else {
                            Input::new()
                                .with_prompt(format!(
                                    "{} base URL (optional)",
                                    choice.provider_name()
                                ))
                                .default(default_url.into())
                                .allow_empty(true)
                                .interact_text()
                                .unwrap_or_default()
                        };
                        let key: String = Password::new()
                            .with_prompt(format!("{} API Key (hidden)", choice.provider_name()))
                            .allow_empty_password(true)
                            .interact()?;
                        (key, base_url, String::new())
                    }
                };

                // Write to config
                let config_file = config_path.unwrap_or("config.yaml");
                let yaml = if std::path::Path::new(config_file).exists() {
                    std::fs::read_to_string(config_file)?
                } else {
                    String::new()
                };

                let mut lines: Vec<String> = yaml.lines().map(String::from).collect();
                let mut changed = false;

                for line in lines.iter_mut() {
                    let trimmed = line.trim_start().to_string();
                    if trimmed.starts_with("provider:") && yaml.contains("llm:") {
                        *line = format!("  provider: \"{}\"", choice.provider_name());
                        changed = true;
                    } else if trimmed.starts_with("model:") {
                        *line = format!("  model: \"{}\"", choice.default_model());
                    } else if trimmed.starts_with("api_key:") {
                        *line = format!("  api_key: \"{}\"", api_key);
                    } else if trimmed.starts_with("base_url:") {
                        *line = format!("  base_url: \"{}\"", base_url);
                    } else if trimmed.starts_with("vertex_project:") {
                        *line = format!("  vertex_project: \"{}\"", vertex_project);
                    }
                }

                if changed {
                    let updated = lines.join("\n") + "\n";
                    std::fs::write(config_file, updated)?;
                    println!(
                        "  {} Switched to {} (model: {})",
                        style("✅").green(),
                        style(choice.provider_name()).cyan().bold(),
                        style(choice.default_model()).cyan()
                    );
                    if !api_key.is_empty() {
                        println!("  {} API key: {}", style("🔑").dim(), mask_secret(&api_key));
                    }
                    if !vertex_project.is_empty() {
                        println!(
                            "  {} Project: {}",
                            style("☁️").dim(),
                            style(&vertex_project).cyan()
                        );
                    }
                } else {
                    println!(
                        "  {} Could not find llm section in config — run 'contribai init' first",
                        style("⚠️").yellow()
                    );
                }
                println!();
            }
            2 => {
                // Set GitHub token
                println!();
                println!("{}", style("  Set GitHub Token").yellow().bold());
                println!(
                    "  {}",
                    style("Create at: https://github.com/settings/tokens").dim()
                );
                println!("  {}", style("Scopes needed: repo, workflow").dim());

                let token: String = Password::new()
                    .with_prompt("GitHub PAT (hidden)")
                    .allow_empty_password(true)
                    .interact()?;

                if !token.is_empty() {
                    let config_file = config_path.unwrap_or("config.yaml");
                    if std::path::Path::new(config_file).exists() {
                        let yaml = std::fs::read_to_string(config_file)?;
                        let mut lines: Vec<String> = yaml.lines().map(String::from).collect();
                        for line in lines.iter_mut() {
                            if line.trim_start().starts_with("token:") {
                                *line = format!("  token: \"{}\"", token);
                            }
                        }
                        std::fs::write(config_file, lines.join("\n") + "\n")?;
                        println!(
                            "  {} GitHub token saved to {}",
                            style("✅").green(),
                            style(config_file).cyan()
                        );
                    } else {
                        println!(
                            "  {} No config.yaml found — run 'contribai init' first",
                            style("⚠️").yellow()
                        );
                    }
                } else {
                    println!("  {} Skipped (empty token)", style("⚪").dim());
                }
                println!();
            }
            3 => {
                // Run init wizard
                println!();
                let result = crate::cli::wizard::run_init_wizard(None)?;
                if let Some(r) = result {
                    crate::cli::wizard::write_wizard_config(&r)?;
                }
                println!();
            }
            _ => {}
        }
    }

    Ok(())
}

// ── Leaderboard ───────────────────────────────────────────────────────────────

fn run_leaderboard(config_path: Option<&str>, limit: usize) -> anyhow::Result<()> {
    use colored::Colorize;

    print_banner();
    let config = load_config(config_path)?;
    let memory = create_memory(&config)?;

    println!("{}", "🏆 Contribution Leaderboard".cyan().bold());
    println!("{}", "━".repeat(60).dimmed());
    println!();

    let stats = memory.get_stats()?;
    let total = stats.get("total_prs_submitted").copied().unwrap_or(0);
    let merged = stats.get("prs_merged").copied().unwrap_or(0);
    let closed = stats.get("prs_closed").copied().unwrap_or(0);
    let open = total.saturating_sub(merged + closed);
    let merge_rate = if total > 0 { merged * 100 / total } else { 0 };

    println!(
        "  {:<18} {}",
        "Total PRs:".dimmed(),
        total.to_string().cyan().bold()
    );
    println!(
        "  {:<18} {}  {}  {}",
        "Status:".dimmed(),
        format!("✅ Merged: {}", merged).green(),
        format!("❌ Closed: {}", closed).red(),
        format!("🟡 Open: {}", open).yellow()
    );
    println!(
        "  {:<18} {}",
        "Merge rate:".dimmed(),
        format!("{}%", merge_rate).cyan().bold()
    );
    println!();

    // Per-repo breakdown from memory
    let prs = memory.get_prs(None, limit * 5)?;
    if !prs.is_empty() {
        // Aggregate by repo
        let mut repo_map: std::collections::HashMap<String, (u32, u32, u32)> =
            std::collections::HashMap::new();
        for pr in &prs {
            let repo = pr
                .get("repo")
                .map(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            let status = pr.get("status").map(|s| s.as_str()).unwrap_or("unknown");
            let entry = repo_map.entry(repo).or_insert((0, 0, 0));
            entry.0 += 1;
            if status == "merged" {
                entry.1 += 1;
            }
            if status == "closed" {
                entry.2 += 1;
            }
        }

        let mut repos: Vec<(String, u32, u32, u32)> = repo_map
            .into_iter()
            .map(|(r, (t, m, c))| (r, t, m, c))
            .collect();
        repos.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));
        repos.truncate(limit);

        println!(
            "{:<32} {:>6} {:>8} {:>8} {:>6}",
            "Repo".bold(),
            "Total".bold(),
            "Merged".bold(),
            "Closed".bold(),
            "Rate".bold()
        );
        println!("{}", "─".repeat(64).dimmed());

        for (repo, total, merged, closed) in &repos {
            let rate = if *total > 0 { merged * 100 / total } else { 0 };
            let rate_str = format!("{}%", rate);
            let rate_colored = if rate >= 70 {
                rate_str.green().to_string()
            } else if rate >= 40 {
                rate_str.yellow().to_string()
            } else {
                rate_str.red().to_string()
            };
            let repo_short: String = repo.chars().take(30).collect();
            println!(
                "  {:<30} {:>6} {:>8} {:>8} {:>6}",
                repo_short,
                total.to_string().cyan(),
                merged.to_string().green(),
                closed.to_string().red(),
                rate_colored
            );
        }
    } else {
        println!("  {}", "No PR history yet.".dimmed());
    }

    println!();
    Ok(())
}

// ── Models ────────────────────────────────────────────────────────────────────

fn run_models(task_filter: Option<&str>) -> anyhow::Result<()> {
    use colored::Colorize;

    struct ModelDef {
        name: &'static str,
        provider: &'static str,
        tier: &'static str,
        coding: u8,
        analysis: u8,
        speed: u8,
        cost: &'static str,
        best_for: &'static str,
    }

    const MODELS: &[ModelDef] = &[
        // ── Google Gemini 3.x (latest) ────────────────────────────────
        ModelDef {
            name: "gemini-3.1-pro-preview",
            provider: "google",
            tier: "PRO",
            coding: 10,
            analysis: 10,
            speed: 7,
            cost: "$1.25/$10.0",
            best_for: "analysis, code",
        },
        ModelDef {
            name: "gemini-3-pro-preview",
            provider: "google",
            tier: "PRO",
            coding: 10,
            analysis: 10,
            speed: 7,
            cost: "$1.25/$10.0",
            best_for: "analysis, code",
        },
        ModelDef {
            name: "gemini-3-flash-preview",
            provider: "google",
            tier: "FLASH",
            coding: 9,
            analysis: 9,
            speed: 9,
            cost: "$0.15/$0.60",
            best_for: "code, review",
        },
        ModelDef {
            name: "gemini-3.1-flash-lite-preview",
            provider: "google",
            tier: "LITE",
            coding: 8,
            analysis: 7,
            speed: 10,
            cost: "$0.02/$0.10",
            best_for: "docs, review",
        },
        // ── Google Gemini 2.5 (stable) ────────────────────────────────
        ModelDef {
            name: "gemini-2.5-pro",
            provider: "google",
            tier: "PRO",
            coding: 9,
            analysis: 9,
            speed: 7,
            cost: "$1.25/$10.0",
            best_for: "analysis, code",
        },
        ModelDef {
            name: "gemini-2.5-flash",
            provider: "google",
            tier: "FLASH",
            coding: 8,
            analysis: 8,
            speed: 9,
            cost: "$0.30/$2.50",
            best_for: "analysis, review, docs",
        },
        ModelDef {
            name: "gemini-2.5-flash-lite",
            provider: "google",
            tier: "LITE",
            coding: 7,
            analysis: 7,
            speed: 10,
            cost: "$0.10/$0.40",
            best_for: "docs, review",
        },
        // ── OpenAI ─────────────────────────────────────────────────────
        ModelDef {
            name: "gpt-5.4",
            provider: "openai",
            tier: "PRO",
            coding: 9,
            analysis: 9,
            speed: 7,
            cost: "$2.50/$15.0",
            best_for: "code, analysis",
        },
        ModelDef {
            name: "gpt-5.4-mini",
            provider: "openai",
            tier: "FLASH",
            coding: 8,
            analysis: 8,
            speed: 9,
            cost: "$0.75/$4.50",
            best_for: "code, review",
        },
        ModelDef {
            name: "gpt-5.4-nano",
            provider: "openai",
            tier: "LITE",
            coding: 7,
            analysis: 7,
            speed: 10,
            cost: "$0.20/$1.25",
            best_for: "docs, review",
        },
        // ── Anthropic ──────────────────────────────────────────────────
        ModelDef {
            name: "claude-opus-4.6",
            provider: "anthropic",
            tier: "PRO",
            coding: 10,
            analysis: 10,
            speed: 6,
            cost: "$5.00/$25.0",
            best_for: "code, analysis",
        },
        ModelDef {
            name: "claude-sonnet-4.6",
            provider: "anthropic",
            tier: "FLASH",
            coding: 9,
            analysis: 9,
            speed: 7,
            cost: "$3.00/$15.0",
            best_for: "code, analysis",
        },
        ModelDef {
            name: "claude-haiku-4.5",
            provider: "anthropic",
            tier: "LITE",
            coding: 7,
            analysis: 7,
            speed: 9,
            cost: "$1.00/$5.00",
            best_for: "docs, review",
        },
        // ── Ollama (local) ─────────────────────────────────────────────
        ModelDef {
            name: "llama3.3",
            provider: "ollama",
            tier: "LOCAL",
            coding: 8,
            analysis: 7,
            speed: 8,
            cost: "free",
            best_for: "all (offline)",
        },
        ModelDef {
            name: "qwen2.5-coder",
            provider: "ollama",
            tier: "LOCAL",
            coding: 9,
            analysis: 7,
            speed: 8,
            cost: "free",
            best_for: "code (offline)",
        },
        ModelDef {
            name: "deepseek-coder-v2",
            provider: "ollama",
            tier: "LOCAL",
            coding: 9,
            analysis: 7,
            speed: 7,
            cost: "free",
            best_for: "code (offline)",
        },
    ];

    let filter_lower = task_filter.map(|s| s.to_lowercase());
    let models: Vec<&ModelDef> = MODELS
        .iter()
        .filter(|m| {
            filter_lower
                .as_ref()
                .map(|f| m.best_for.contains(f.as_str()))
                .unwrap_or(true)
        })
        .collect();

    print_banner();

    if let Some(f) = task_filter {
        println!("{} {}", "🤖 Models for task:".cyan().bold(), f.yellow());
    } else {
        println!("{}", "🤖 Available LLM Models".cyan().bold());
    }
    println!("{}", "━".repeat(95).dimmed());
    println!(
        "  {:<30} {:<10} {:<8} {:>5} {:>6} {:>6}  {:<14} {}",
        "Model".bold(),
        "Provider".bold(),
        "Tier".bold(),
        "Code".bold(),
        "Analy".bold(),
        "Speed".bold(),
        "Cost (in/out)".bold(),
        "Best For".bold()
    );
    println!("{}", "─".repeat(95).dimmed());

    for m in &models {
        let tier_colored = match m.tier {
            "PRO" => m.tier.red().to_string(),
            "FLASH" => m.tier.yellow().to_string(),
            "LOCAL" => m.tier.green().to_string(),
            _ => m.tier.dimmed().to_string(),
        };
        println!(
            "  {:<30} {:<10} {:<16} {:>5} {:>6} {:>6}  {:<14} {}",
            m.name.cyan(),
            m.provider.dimmed(),
            tier_colored,
            m.coding,
            m.analysis,
            m.speed,
            m.cost,
            m.best_for.dimmed()
        );
    }

    println!();
    println!("{}", "Default Task Assignments:".bold());
    println!(
        "  {:<20} {}",
        "analysis:".dimmed(),
        "gemini-3-flash-preview".cyan()
    );
    println!(
        "  {:<20} {}",
        "code:".dimmed(),
        "gemini-3.1-pro-preview".cyan()
    );
    println!(
        "  {:<20} {}",
        "review:".dimmed(),
        "gemini-3-flash-preview".cyan()
    );
    println!(
        "  {:<20} {}",
        "docs:".dimmed(),
        "gemini-3.1-flash-lite-preview".cyan()
    );
    println!();
    Ok(())
}

// ── Notify test ───────────────────────────────────────────────────────────────

async fn run_notify_test(config_path: Option<&str>) -> anyhow::Result<()> {
    use colored::Colorize;

    let config = load_config(config_path)?;
    let n = &config.notifications;

    let slack = n.slack_webhook.as_deref().unwrap_or("");
    let discord = n.discord_webhook.as_deref().unwrap_or("");
    let tg_token = n.telegram_token.as_deref().unwrap_or("");
    let tg_chat = n.telegram_chat_id.as_deref().unwrap_or("");

    let channels_configured = !slack.is_empty() || !discord.is_empty() || !tg_token.is_empty();

    if !channels_configured {
        println!(
            "  {} No notification channels configured in config.yaml",
            "⚠️".yellow()
        );
        println!("  Set one of these first:");
        println!(
            "    {}",
            "contribai config-set notifications.slack_webhook https://hooks.slack.com/services/..."
                .cyan()
        );
        println!("    {}", "contribai config-set notifications.discord_webhook https://discord.com/api/webhooks/...".cyan());
        println!(
            "    {}",
            "contribai config-set notifications.telegram_token <bot-token>".cyan()
        );
        return Ok(());
    }

    println!("{}", "📣 Sending test notifications...".cyan().bold());
    println!("{}", "━".repeat(50).dimmed());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let title = "🤖 ContribAI Test Notification";
    let body  = "✅ Your ContribAI notifications are working! This is a test message from `contribai notify-test`.";

    // ── Slack ──────────────────────────────────────────────────────────────
    if !slack.is_empty() {
        print!(
            "  🔔 Slack:   {} ... ",
            slack.chars().take(40).collect::<String>().dimmed()
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let payload = serde_json::json!({
            "text": format!("*{}*\n{}", title, body),
        });

        match client.post(slack).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                println!("{}", "✅ sent".green());
            }
            Ok(resp) => {
                println!("{} (HTTP {})", "❌ failed".red(), resp.status());
            }
            Err(e) => {
                println!("{}: {}", "❌ error".red(), e);
            }
        }
    }

    // ── Discord ────────────────────────────────────────────────────────────
    if !discord.is_empty() {
        print!("  🎮 Discord: configured ... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let payload = serde_json::json!({
            "content": format!("**{}**\n{}", title, body),
        });

        match client.post(discord).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 204 => {
                println!("{}", "✅ sent".green());
            }
            Ok(resp) => {
                println!("{} (HTTP {})", "❌ failed".red(), resp.status());
            }
            Err(e) => {
                println!("{}: {}", "❌ error".red(), e);
            }
        }
    }

    // ── Telegram ───────────────────────────────────────────────────────────
    if !tg_token.is_empty() {
        print!("  📱 Telegram: chat {} ... ", tg_chat.dimmed());
        std::io::Write::flush(&mut std::io::stdout()).ok();

        if tg_chat.is_empty() {
            println!("{}", "⚠️  telegram_chat_id not set".yellow());
        } else {
            let url = format!("https://api.telegram.org/bot{}/sendMessage", tg_token);
            let payload = serde_json::json!({
                "chat_id": tg_chat,
                "text": format!("<b>{}</b>\n{}", title, body),
                "parse_mode": "HTML",
            });

            match client.post(&url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    println!("{}", "✅ sent".green());
                }
                Ok(resp) => {
                    let txt = resp.text().await.unwrap_or_default();
                    println!(
                        "{}: {}",
                        "❌ failed".red(),
                        txt.chars().take(80).collect::<String>()
                    );
                }
                Err(e) => {
                    println!("{}: {}", "❌ error".red(), e);
                }
            }
        }
    }

    println!();
    println!(
        "  {} All channels tested. Check your apps!",
        "🎉".green().bold()
    );
    Ok(())
}

// ── Cleanup ───────────────────────────────────────────────────────────────────

async fn run_cleanup(config_path: Option<&str>, yes: bool) -> anyhow::Result<()> {
    use colored::Colorize;
    use dialoguer::Confirm;

    print_banner();
    let config = load_config(config_path)?;
    let memory = create_memory(&config)?;

    println!(
        "{}",
        "🧹 Cleanup — Forks created by ContribAI".cyan().bold()
    );
    println!("{}", "━".repeat(60).dimmed());
    println!();

    let all_prs = memory.get_prs(None, 1000)?;
    if all_prs.is_empty() {
        println!(
            "  {} No PRs in database. Nothing to clean up.",
            "💡".dimmed()
        );
        return Ok(());
    }

    // Group by fork
    let mut forks: std::collections::HashMap<
        String,
        Vec<std::collections::HashMap<String, String>>,
    > = std::collections::HashMap::new();

    for pr in &all_prs {
        let fork = pr.get("fork").map(|s| s.as_str()).unwrap_or("");
        if !fork.is_empty() {
            forks.entry(fork.to_string()).or_default().push(pr.clone());
        }
    }

    if forks.is_empty() {
        println!(
            "  {} No forks recorded in database (PRs may be direct branch contributions).",
            "💡".dimmed()
        );
        return Ok(());
    }

    println!(
        "  Found {} fork(s) in database\n",
        forks.len().to_string().cyan()
    );

    let mut safe_to_delete: Vec<String> = vec![];
    let mut has_open: Vec<String> = vec![];

    for (fork_name, prs) in &forks {
        println!("  📁 {}", fork_name.bold());
        let all_resolved = prs.iter().all(|pr| {
            let status = pr.get("status").map(|s| s.as_str()).unwrap_or("unknown");
            status == "merged" || status == "closed"
        });

        for pr in prs {
            let num = pr.get("pr_number").map(|s| s.as_str()).unwrap_or("?");
            let title: String = pr
                .get("title")
                .map(|s| s.as_str())
                .unwrap_or("")
                .chars()
                .take(50)
                .collect();
            let status = pr.get("status").map(|s| s.as_str()).unwrap_or("unknown");
            let icon = match status {
                "merged" => "🟢",
                "open" => "🟡",
                _ => "🔴",
            };
            println!("     PR #{}: {} [{} {}]", num.cyan(), title, icon, status);
        }

        if all_resolved {
            println!("     {} All PRs resolved — safe to delete\n", "✅".green());
            safe_to_delete.push(fork_name.clone());
        } else {
            println!("     {} Has open PRs — keeping\n", "⚠️".yellow());
            has_open.push(fork_name.clone());
        }
    }

    println!("{}", "━".repeat(60).dimmed());
    if !has_open.is_empty() {
        println!(
            "  {} {} fork(s) with open PRs (kept)",
            "⚠️".yellow(),
            has_open.len()
        );
    }

    if safe_to_delete.is_empty() {
        println!("  {} No forks to clean up.", "💡".dimmed());
        return Ok(());
    }

    println!(
        "  {} {} fork(s) safe to delete:",
        "✅".green(),
        safe_to_delete.len()
    );
    for f in &safe_to_delete {
        println!("    - {}", f.cyan());
    }

    let confirmed = yes
        || Confirm::new()
            .with_prompt(format!("\n  🗑️  Delete {} fork(s)?", safe_to_delete.len()))
            .default(false)
            .interact()?;

    if !confirmed {
        println!("  {}", "Cancelled.".dimmed());
        return Ok(());
    }

    for f in &safe_to_delete {
        let result = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/c", "gh", "repo", "delete", f, "--yes"])
                .output()
        } else {
            std::process::Command::new("gh")
                .args(["repo", "delete", f, "--yes"])
                .output()
        };

        match result {
            Ok(out) if out.status.success() => println!("  {} Deleted {}", "✅".green(), f.cyan()),
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                println!("  {} Failed to delete {}: {}", "❌".red(), f, err.trim());
            }
            Err(e) => println!("  {} Failed: {}", "❌".red(), e),
        }
    }

    println!("\n  🎉 Cleanup done!");
    Ok(())
}

// ── Templates ─────────────────────────────────────────────────────────────────

fn run_templates(type_filter: Option<&str>) -> anyhow::Result<()> {
    use colored::Colorize;

    struct TemplateDef {
        name: &'static str,
        r#type: &'static str,
        severity: &'static str,
        description: &'static str,
        languages: &'static str,
    }

    const TEMPLATES: &[TemplateDef] = &[
        TemplateDef {
            name: "sql-injection-fix",
            r#type: "security_fix",
            severity: "critical",
            description: "Fix SQL injection vulnerabilities",
            languages: "python, js, ts, go",
        },
        TemplateDef {
            name: "xss-fix",
            r#type: "security_fix",
            severity: "high",
            description: "Fix XSS vulnerabilities",
            languages: "js, ts",
        },
        TemplateDef {
            name: "path-traversal-fix",
            r#type: "security_fix",
            severity: "high",
            description: "Fix path traversal issues",
            languages: "python, go, rust",
        },
        TemplateDef {
            name: "missing-docstrings",
            r#type: "docs_improve",
            severity: "low",
            description: "Add missing docstrings to functions",
            languages: "python",
        },
        TemplateDef {
            name: "readme-badges",
            r#type: "docs_improve",
            severity: "low",
            description: "Add CI/coverage badges to README",
            languages: "all",
        },
        TemplateDef {
            name: "error-handling",
            r#type: "code_quality",
            severity: "medium",
            description: "Improve error handling patterns",
            languages: "python, go, rust",
        },
        TemplateDef {
            name: "add-type-hints",
            r#type: "code_quality",
            severity: "low",
            description: "Add Python type hints",
            languages: "python",
        },
        TemplateDef {
            name: "add-tests",
            r#type: "code_quality",
            severity: "medium",
            description: "Add missing unit tests",
            languages: "python, js, ts, go",
        },
        TemplateDef {
            name: "performance-cache",
            r#type: "performance_opt",
            severity: "medium",
            description: "Add caching to expensive operations",
            languages: "python, go",
        },
        TemplateDef {
            name: "refactor-long-fn",
            r#type: "refactor",
            severity: "low",
            description: "Break up overly long functions",
            languages: "python, js, ts",
        },
        TemplateDef {
            name: "dependency-update",
            r#type: "security_fix",
            severity: "medium",
            description: "Update vulnerable dependencies",
            languages: "all",
        },
        TemplateDef {
            name: "add-logging",
            r#type: "code_quality",
            severity: "low",
            description: "Add structured logging",
            languages: "python, go, rust",
        },
        TemplateDef {
            name: "issue-fix",
            r#type: "feature_add",
            severity: "medium",
            description: "Fix a GitHub issue based on repro steps",
            languages: "all",
        },
        TemplateDef {
            name: "ui-accessibility",
            r#type: "ui_ux_fix",
            severity: "medium",
            description: "Fix accessibility issues (aria, contrast, focus)",
            languages: "js, ts",
        },
    ];

    let templates: Vec<&TemplateDef> = TEMPLATES
        .iter()
        .filter(|t| {
            type_filter
                .map(|f| t.r#type == f || t.r#type.contains(f))
                .unwrap_or(true)
        })
        .collect();

    print_banner();
    println!("{}", "📋 Contribution Templates".cyan().bold());
    println!("{}", "━".repeat(100).dimmed());

    if templates.is_empty() {
        println!(
            "  {} No templates match filter '{}'",
            "⚠️".yellow(),
            type_filter.unwrap_or("")
        );
    } else {
        println!(
            "  {:<25} {:<18} {:<10} {:<38} {}",
            "Name".bold(),
            "Type".bold(),
            "Severity".bold(),
            "Description".bold(),
            "Languages".bold()
        );
        println!("{}", "─".repeat(100).dimmed());

        for t in &templates {
            let sev_colored = match t.severity {
                "critical" => t.severity.red().bold().to_string(),
                "high" => t.severity.red().to_string(),
                "medium" => t.severity.yellow().to_string(),
                _ => t.severity.dimmed().to_string(),
            };
            println!(
                "  {:<25} {:<18} {:<18} {:<38} {}",
                t.name.cyan(),
                t.r#type.dimmed(),
                sev_colored,
                t.description.chars().take(38).collect::<String>(),
                t.languages.dimmed()
            );
        }
    }
    println!();
    Ok(())
}

// ── Profile ───────────────────────────────────────────────────────────────────

#[allow(dead_code)]
struct Profile {
    name: &'static str,
    description: &'static str,
    analyzers: &'static [&'static str],
    contribution_types: &'static [&'static str],
    severity_threshold: &'static str,
    max_prs_per_day: u32,
    max_repos: u32,
    dry_run: bool,
}

const PROFILES: &[Profile] = &[
    Profile {
        name: "security-focused",
        description: "Focus on security vulnerabilities and fixes",
        analyzers: &["security"],
        contribution_types: &["security_fix", "code_quality"],
        severity_threshold: "high",
        max_prs_per_day: 5,
        max_repos: 10,
        dry_run: false,
    },
    Profile {
        name: "docs-focused",
        description: "Focus on documentation improvements",
        analyzers: &["docs"],
        contribution_types: &["docs_improve"],
        severity_threshold: "low",
        max_prs_per_day: 10,
        max_repos: 15,
        dry_run: false,
    },
    Profile {
        name: "full-scan",
        description: "Run all analyzers with low threshold",
        analyzers: &[
            "security",
            "code_quality",
            "docs",
            "performance",
            "refactor",
        ],
        contribution_types: &[
            "security_fix",
            "docs_improve",
            "code_quality",
            "performance_opt",
            "refactor",
        ],
        severity_threshold: "low",
        max_prs_per_day: 20,
        max_repos: 20,
        dry_run: false,
    },
    Profile {
        name: "gentle",
        description: "Low-impact: small fixes, dry run by default",
        analyzers: &["docs", "code_quality"],
        contribution_types: &["docs_improve", "code_quality"],
        severity_threshold: "high",
        max_prs_per_day: 3,
        max_repos: 2,
        dry_run: true,
    },
];

async fn run_profile(
    name: &str,
    dry_run: bool,
    config: &contribai::core::config::ContribAIConfig,
) -> anyhow::Result<()> {
    use colored::Colorize;

    // "list" keyword → show all profiles
    if name == "list" || name == "--list" {
        println!("{}", "📋 Available Profiles".cyan().bold());
        println!("{}", "━".repeat(70).dimmed());
        println!(
            "  {:<22} {:<35} {:<10} {}",
            "Name".bold(),
            "Description".bold(),
            "Threshold".bold(),
            "Dry Run".bold()
        );
        println!("{}", "─".repeat(70).dimmed());
        for p in PROFILES {
            println!(
                "  {:<22} {:<35} {:<10} {}",
                p.name.cyan(),
                p.description.chars().take(35).collect::<String>(),
                p.severity_threshold.yellow(),
                if p.dry_run {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                }
            );
        }
        println!();
        println!(
            "  {} Use: {}",
            "→".dimmed(),
            "contribai profile <name>".cyan()
        );
        return Ok(());
    }

    let profile = PROFILES.iter().find(|p| p.name == name);
    let profile = match profile {
        Some(p) => p,
        None => {
            anyhow::bail!(
                "Profile '{}' not found. Available: {}",
                name,
                PROFILES
                    .iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    };

    let effective_dry_run = dry_run || profile.dry_run;

    println!(
        "  {} Running with profile: {}",
        "🎯".cyan(),
        profile.name.cyan().bold()
    );
    println!("  {}", profile.description.dimmed());
    println!("  Analyzers: {}", profile.analyzers.join(", ").yellow());
    println!("  Severity:  {}", profile.severity_threshold.yellow());
    println!(
        "  Max PRs/day: {}",
        profile.max_prs_per_day.to_string().cyan()
    );
    if effective_dry_run {
        println!("  {} DRY RUN mode", "[DRY RUN]".yellow().bold());
    }
    println!();

    let github = create_github(config)?;
    let llm = create_llm(config)?;
    let memory = create_memory(config)?;
    let event_bus = contribai::core::events::EventBus::default();

    let pipeline = contribai::orchestrator::pipeline::ContribPipeline::new(
        config,
        &github,
        llm.as_ref(),
        &memory,
        &event_bus,
    );

    let result = pipeline.run(None, effective_dry_run).await?;
    print_result(&result, effective_dry_run);
    Ok(())
}

// ── System status ─────────────────────────────────────────────────────────────

async fn run_system_status(config_path: Option<&str>) -> anyhow::Result<()> {
    use colored::Colorize;

    print_banner();
    println!("{}", "📊 ContribAI System Status".cyan().bold());
    println!("{}", "━".repeat(60).dimmed());
    println!();

    let config = load_config(config_path)?;
    let memory = create_memory(&config)?;
    let stats = memory.get_stats()?;

    // Memory DB
    let db_path = config.storage.resolved_db_path();
    let db_size = std::fs::metadata(&db_path)
        .map(|m| format!("{:.1} KB", m.len() as f64 / 1024.0))
        .unwrap_or_else(|_| "not found".to_string());

    println!("{}", "  💾 Memory Database".bold());
    println!(
        "  {:<25} {}",
        "Path:".dimmed(),
        db_path.display().to_string().cyan()
    );
    println!("  {:<25} {}", "Size:".dimmed(), db_size.cyan());
    println!(
        "  {:<25} {}",
        "Repos analyzed:".dimmed(),
        stats
            .get("total_repos_analyzed")
            .copied()
            .unwrap_or(0)
            .to_string()
            .cyan()
    );
    println!(
        "  {:<25} {}",
        "PRs submitted:".dimmed(),
        stats
            .get("total_prs_submitted")
            .copied()
            .unwrap_or(0)
            .to_string()
            .cyan()
    );
    println!(
        "  {:<25} {}  {}  {}",
        "PR status:".dimmed(),
        format!("✅ {}", stats.get("prs_merged").copied().unwrap_or(0)).green(),
        format!("❌ {}", stats.get("prs_closed").copied().unwrap_or(0)).red(),
        format!("🟡 open:{}", {
            let t = stats.get("total_prs_submitted").copied().unwrap_or(0);
            let m = stats.get("prs_merged").copied().unwrap_or(0);
            let c = stats.get("prs_closed").copied().unwrap_or(0);
            t.saturating_sub(m + c)
        })
        .yellow()
    );
    println!();

    // Events log
    let events_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".contribai")
        .join("events.jsonl");
    if events_path.exists() {
        let lines = std::fs::read_to_string(&events_path)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        println!("{}", "  📋 Event Log".bold());
        println!(
            "  {:<25} {}",
            "Path:".dimmed(),
            events_path.display().to_string().cyan()
        );
        println!("  {:<25} {}", "Events:".dimmed(), lines.to_string().cyan());
        println!();
    }

    // GitHub rate limit
    println!("{}", "  🔑 GitHub API".bold());
    let github = create_github(&config);
    match github {
        Ok(gh) => match gh.check_rate_limit().await {
            Ok(info) => {
                let remaining = info.remaining;
                let color = if remaining > 1000 {
                    "green"
                } else if remaining > 100 {
                    "yellow"
                } else {
                    "red"
                };
                let remaining_str = remaining.to_string();
                let displayed = match color {
                    "green" => remaining_str.green().to_string(),
                    "yellow" => remaining_str.yellow().to_string(),
                    _ => remaining_str.red().to_string(),
                };
                println!(
                    "  {:<25} {} / {} requests remaining",
                    "Rate limit:".dimmed(),
                    displayed,
                    info.limit
                );
            }
            Err(_) => println!(
                "  {:<25} {}",
                "Rate limit:".dimmed(),
                "could not check".dimmed()
            ),
        },
        Err(_) => println!(
            "  {:<25} {}",
            "Rate limit:".dimmed(),
            "token not configured".red()
        ),
    }
    println!();

    // LLM provider
    println!("{}", "  🤖 LLM Provider".bold());
    println!(
        "  {:<25} {}",
        "Provider:".dimmed(),
        config.llm.provider.cyan()
    );
    println!("  {:<25} {}", "Model:".dimmed(), config.llm.model.cyan());
    if !config.llm.vertex_project.is_empty() {
        println!(
            "  {:<25} {}",
            "Vertex project:".dimmed(),
            config.llm.vertex_project.cyan()
        );
    }
    println!();

    // Scheduler
    println!("{}", "  ⏰ Scheduler".bold());
    println!(
        "  {:<25} {}",
        "Status:".dimmed(),
        if config.scheduler.enabled {
            "enabled".green().to_string()
        } else {
            "disabled".dimmed().to_string()
        }
    );
    println!(
        "  {:<25} {}",
        "Cron:".dimmed(),
        config.scheduler.cron.cyan()
    );
    println!();

    Ok(())
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
        "  📦 Repos analyzed:         {}",
        result.repos_analyzed.to_string().cyan()
    );
    println!(
        "  🔍 Findings:               {}",
        result.findings_total.to_string().cyan()
    );
    println!(
        "  ⚙️ Contributions generated: {}",
        result.contributions_generated.to_string().cyan()
    );
    println!(
        "  🎉 PRs created:            {}",
        result.prs_created.to_string().green().bold()
    );

    if !result.errors.is_empty() {
        println!(
            "  ⚠️ Errors:                 {}",
            result.errors.len().to_string().red()
        );
    }
}

fn load_config(path: Option<&str>) -> anyhow::Result<contribai::core::config::ContribAIConfig> {
    use contribai::core::config::ContribAIConfig;

    if let Some(p) = path {
        ContribAIConfig::from_yaml(std::path::Path::new(p)).map_err(|e| anyhow::anyhow!("{}", e))
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
    contribai::llm::provider::create_llm_provider(&config.llm).map_err(|e| anyhow::anyhow!("{}", e))
}

fn create_memory(
    config: &contribai::core::config::ContribAIConfig,
) -> anyhow::Result<contribai::orchestrator::memory::Memory> {
    let db_path = config.storage.resolved_db_path();
    contribai::orchestrator::memory::Memory::open(&db_path).map_err(|e| anyhow::anyhow!("{}", e))
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
