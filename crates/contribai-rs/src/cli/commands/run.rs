//! Handles `Commands::Run` — auto-discover repos, analyze code, and submit PRs.

use colored::Colorize;

use crate::cli::{
    create_github, create_llm, create_memory, load_config, print_banner, print_config_summary,
    print_result,
};

pub async fn run_run(
    config_path: Option<&str>,
    language: Option<String>,
    stars: Option<String>,
    dry_run: bool,
    approve: bool,
    mode: String,
) -> anyhow::Result<()> {
    print_banner();
    let mut config = load_config(config_path)?;

    // Override agent mode from CLI
    if mode != "build" {
        config.pipeline.agent_mode = mode.clone();
    }

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
    println!(
        "   {}: {}",
        "Mode".dimmed(),
        if mode == "plan" {
            "plan (read-only analysis)".yellow().to_string()
        } else {
            "build (full PR flow)".green().to_string()
        }
    );
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
    let _log_handle =
        contribai::core::events::FileEventLogger::new(&log_path).spawn_logger(&event_bus);
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
