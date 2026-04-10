//! CLI interface for ContribAI.
//!
//! Interactive CLI like `claude` / `gemini` — wizard setup, config get/set,
//! arrow-key menus, and all operations accessible without editing YAML.

pub mod commands;
pub mod common;
pub mod config_editor;
#[cfg(feature = "tui")]
pub mod tui;
pub mod wizard;

// Re-export common helpers for command handlers
pub use common::{
    create_github, create_llm, create_memory, load_config, parse_github_url, print_banner,
    print_config_summary, print_result,
};

use clap::{Parser, Subcommand};

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

        /// Agent mode: "build" (full PR flow) or "plan" (read-only analysis)
        #[arg(long, default_value = "build")]
        mode: String,
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

    /// Start the pipeline server (remote API mode)
    Serve {
        /// Host to bind (default: 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to listen on (default: 9876)
        #[arg(long, default_value = "9876")]
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

    /// Check circuit breaker status — shows LLM failure state and cooldown
    CircuitBreaker,

    /// Encrypt a GitHub token for secure storage
    EncryptToken {
        /// Plain text token to encrypt
        #[arg(long)]
        token: Option<String>,
        /// Passphrase for encryption
        #[arg(long)]
        passphrase: Option<String>,
    },

    /// Show LLM response cache statistics
    CacheStats,

    /// Clear the LLM response cache
    CacheClear {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Rollback last generated file changes
    Undo {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
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
                mode,
            } => {
                commands::run::run_run(
                    self.config.as_deref(),
                    language,
                    stars,
                    dry_run,
                    approve,
                    mode,
                )
                .await
            }
            Commands::Hunt {
                rounds,
                delay,
                language,
                dry_run,
                approve,
            } => {
                commands::hunt::run_hunt(
                    self.config.as_deref(),
                    rounds,
                    delay,
                    language,
                    dry_run,
                    approve,
                )
                .await
            }
            Commands::Patrol { dry_run } => {
                commands::patrol::run_patrol(self.config.as_deref(), dry_run).await
            }
            Commands::Watchlist { dry_run } => {
                commands::watchlist::run_watchlist(self.config.as_deref(), dry_run).await
            }
            Commands::Target { url, dry_run } => {
                commands::target::run_target(self.config.as_deref(), url, dry_run).await
            }
            Commands::Analyze { url } => {
                commands::analyze::run_analyze(self.config.as_deref(), url).await
            }
            Commands::Solve { url, dry_run } => {
                commands::solve::run_solve(self.config.as_deref(), url, dry_run).await
            }
            Commands::McpServer => {
                commands::mcp_server::run_mcp_server(self.config.as_deref()).await
            }
            Commands::Stats => commands::stats::run_stats(self.config.as_deref()).await,
            Commands::Status { filter, limit } => {
                commands::status::run_status(self.config.as_deref(), filter, limit).await
            }
            Commands::Version => {
                print_banner();
                println!("contribai {} (Rust)", contribai::VERSION);
                Ok(())
            }
            #[cfg(feature = "web")]
            Commands::WebServer { host, port } => {
                commands::web_server::run_web_server(self.config.as_deref(), host, port).await
            }
            Commands::Serve { host, port } => {
                commands::serve::run_serve(self.config.as_deref(), host, port).await
            }
            #[cfg(not(feature = "web"))]
            Commands::WebServer { .. } => {
                anyhow::bail!("Web dashboard not available. Build with --features web");
            }
            #[cfg(not(feature = "web"))]
            Commands::Serve { .. } => {
                anyhow::bail!("Pipeline server not available. Build with --features web");
            }
            Commands::Schedule { cron } => {
                commands::schedule::run_schedule(self.config.as_deref(), cron).await
            }
            Commands::Interactive => {
                #[cfg(feature = "tui")]
                {
                    let config = load_config(self.config.as_deref())?;
                    tui::run_interactive_tui(&config)
                }
                #[cfg(not(feature = "tui"))]
                {
                    anyhow::bail!("TUI not available. Build with --features tui");
                }
            }
            Commands::Leaderboard { limit } => {
                commands::leaderboard::run_leaderboard(self.config.as_deref(), limit)
            }
            Commands::Models { task } => commands::models::run_models(task.as_deref()),
            Commands::Templates { r#type } => commands::templates::run_templates(r#type.as_deref()),
            Commands::Profile { name, dry_run } => {
                let config = load_config(self.config.as_deref())?;
                commands::profile::run_profile(&name, dry_run, &config).await
            }
            Commands::Cleanup { yes } => {
                commands::cleanup::run_cleanup(self.config.as_deref(), yes).await
            }
            Commands::NotifyTest => {
                commands::notify_test::run_notify_test(self.config.as_deref()).await
            }
            Commands::SystemStatus => {
                commands::system_status::run_system_status(self.config.as_deref()).await
            }
            Commands::Dream { force } => commands::dream::run_dream(self.config.as_deref(), force),
            Commands::Doctor => commands::doctor::run_doctor(self.config.as_deref()).await,
            Commands::CircuitBreaker => {
                commands::circuit_breaker::run_circuit_breaker_status(self.config.as_deref()).await
            }
            Commands::EncryptToken { token, passphrase } => {
                commands::encrypt_token::run_encrypt_token(token.as_deref(), passphrase.as_deref())
            }
            Commands::CacheStats => commands::cache_stats::run_cache_stats(self.config.as_deref()),
            Commands::CacheClear { yes } => {
                commands::cache_clear::run_cache_clear(self.config.as_deref(), yes)
            }
            Commands::Init { output } => {
                commands::init::run_init(output.as_deref(), output.clone())
            }
            Commands::Login => commands::login::run_login_check(self.config.as_deref()).await,
            Commands::Config => commands::config::run_config(self.config.as_deref()),
            Commands::ConfigGet { key } => {
                commands::config::run_config_get(self.config.as_deref(), key)
            }
            Commands::ConfigSet { key, value } => {
                commands::config::run_config_set(self.config.as_deref(), key, value)
            }
            Commands::ConfigList => commands::config::run_config_list(self.config.as_deref()),
            Commands::Undo { yes } => commands::undo::run_undo(self.config.as_deref(), yes),
        }
    }
}

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
        "⚡  Circuit      — check LLM circuit breaker status",
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
            mode: "build".to_string(),
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
        19 => Commands::CircuitBreaker,
        20 => Commands::Config,
        21 => {
            let key: String = dialoguer::Input::new()
                .with_prompt("Config key (e.g. llm.provider)")
                .interact_text()?;
            let value: String = dialoguer::Input::new()
                .with_prompt(format!("New value for {}", key))
                .interact_text()?;
            Commands::ConfigSet { key, value }
        }
        22 => Commands::Login,
        23 => Commands::Init { output: None },
        _ => std::process::exit(0),
    })
}
