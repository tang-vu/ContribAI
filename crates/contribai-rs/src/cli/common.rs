//! Shared utilities for CLI command handlers.

use contribai::core::config::ContribAIConfig;
use contribai::core::events::EventBus;
use contribai::github::client::GitHubClient;
use contribai::orchestrator::memory::Memory;

/// Load config from path or use defaults.
/// If no path given, searches default locations:
/// `./config.yaml` → `./config.yml` → `~/.contribai/config.yaml` → defaults.
pub fn load_config(path: Option<&str>) -> anyhow::Result<ContribAIConfig> {
    match path {
        Some(p) => {
            let cfg = ContribAIConfig::from_yaml(std::path::Path::new(p))
                .map_err(|e| anyhow::anyhow!("Failed to load {}: {}", p, e))?;
            Ok(cfg)
        }
        None => {
            // Try default locations
            match ContribAIConfig::load() {
                Ok(cfg) => Ok(cfg),
                Err(e) => {
                    // If loading fails, fall back to defaults (with env var resolution)
                    tracing::warn!("Could not load default config: {}", e);
                    Ok(ContribAIConfig::default())
                }
            }
        }
    }
}

/// Print the ContribAI banner.
pub fn print_banner() {
    use colored::Colorize;
    println!();
    println!(
        "{} {}",
        "ContribAI".bold().cyan(),
        format!("v{}", contribai::VERSION).dimmed()
    );
    println!("{}", "━".repeat(50).dimmed());
    println!();
}

/// Create a GitHub client from config.
pub fn create_github(config: &ContribAIConfig) -> anyhow::Result<GitHubClient> {
    if config.github.token.is_empty() {
        anyhow::bail!("GitHub token not configured! Set GITHUB_TOKEN env or config.yaml");
    }
    GitHubClient::new(&config.github.token, config.github.rate_limit_buffer)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Create an LLM provider from config.
pub fn create_llm(
    config: &ContribAIConfig,
) -> anyhow::Result<Box<dyn contribai::llm::provider::LlmProvider>> {
    contribai::llm::provider::create_llm_provider(&config.llm).map_err(|e| anyhow::anyhow!("{}", e))
}

/// Open or create the memory database.
pub fn create_memory(config: &ContribAIConfig) -> anyhow::Result<Memory> {
    let db_path = config.storage.resolved_db_path();
    Memory::open(&db_path).map_err(|e| anyhow::anyhow!("{}", e))
}

// Re-exports needed by command handlers
#[allow(dead_code)]
pub fn create_event_bus() -> EventBus {
    EventBus::new(1000)
}

/// Parse a GitHub URL into (owner, repo) tuple.
pub fn parse_github_url(url: &str) -> anyhow::Result<(String, String)> {
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

/// Print config summary.
pub fn print_config_summary(config: &ContribAIConfig, dry_run: bool) {
    use colored::Colorize;
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

/// Print pipeline result.
pub fn print_result(result: &contribai::orchestrator::pipeline::PipelineResult, dry_run: bool) {
    use colored::Colorize;
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
