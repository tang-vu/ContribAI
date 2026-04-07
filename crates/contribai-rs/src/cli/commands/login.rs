//! Handles `Commands::Login` — check authentication status for all providers and interactively update config.

use crate::cli::wizard::{mask_secret, LlmChoice};
use crate::cli::{load_config, print_banner};
use colored::Colorize;
use console::style;
use dialoguer::{Input, Password, Select};

pub async fn run_login_check(config_path: Option<&str>) -> anyhow::Result<()> {
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

        // ── LLM Provider status (enhanced — detects ALL available sources) ──
        println!("  {}:", style("LLM Providers").bold());

        // 1. GitHub Copilot
        let copilot_ok = contribai::llm::copilot::copilot_available();
        if copilot_ok {
            println!(
                "    {:<16} {} (via gh CLI — models: gpt-4o, claude-sonnet, gemini)",
                style("Copilot:").bold(),
                style("✅ Token detected").green()
            );
        } else {
            println!(
                "    {:<16} {} — run 'gh auth login' first",
                style("Copilot:").bold(),
                style("⚪ Not configured").dim()
            );
        }

        // 2. Vertex AI
        let vertex_token = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/c", "gcloud", "auth", "print-access-token"])
                .output()
        } else {
            std::process::Command::new("gcloud")
                .args(["auth", "print-access-token"])
                .output()
        };
        let vertex_project = if config.llm.vertex_project.is_empty() {
            std::env::var("GOOGLE_CLOUD_PROJECT").unwrap_or_else(|_| "(not set)".into())
        } else {
            config.llm.vertex_project.clone()
        };
        match &vertex_token {
            Ok(out) if out.status.success() => {
                println!(
                    "    {:<16} {} (project: {})",
                    style("Vertex AI:").bold(),
                    style("✅ gcloud token OK").green(),
                    style(&vertex_project).cyan()
                );
            }
            _ => {
                println!(
                    "    {:<16} {} — run 'gcloud auth application-default login'",
                    style("Vertex AI:").bold(),
                    style("⚪ Not configured").dim()
                );
            }
        }

        // 3. Current provider (API key)
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
                        "    {:<16} {} ({} / {} key: ****{})",
                        style(&config.llm.provider).bold(),
                        style("✅ API key set").green(),
                        config.llm.provider,
                        config.llm.model,
                        last4
                    );
                } else {
                    let env_var = match config.llm.provider.as_str() {
                        "openai" => "OPENAI_API_KEY",
                        "anthropic" => "ANTHROPIC_API_KEY",
                        _ => "GEMINI_API_KEY",
                    };
                    println!(
                        "    {:<16} {} — set {} env var",
                        style(&config.llm.provider).bold(),
                        style(format!("❌ {} key missing", config.llm.provider)).red(),
                        env_var
                    );
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
                        "    {:<16} {}",
                        style("Ollama:").bold(),
                        style("✅ Running on localhost:11434").green()
                    );
                } else {
                    println!(
                        "    {:<16} {} — start with 'ollama serve'",
                        style("Ollama:").bold(),
                        style("❌ Not running").red()
                    );
                }
            }
            p => {
                println!(
                    "    {:<16} {}",
                    style(p).bold(),
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
