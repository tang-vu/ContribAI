//! Run and inspect the bounded read-only agent harness.

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use clap::{Args, Subcommand};
use contribai::core::config::LlmConfig;
use contribai::core::error::Result;
use contribai::harness::snapshot::HarnessSnapshot;
use contribai::harness::store::HarnessStore;
use contribai::harness::{Harness, HarnessCancellation, HarnessLimits, HarnessStatus};
use contribai::llm::provider::{create_llm_provider_raw, ChatMessage, LlmProvider};

#[derive(Args)]
pub struct HarnessArgs {
    /// Local metadata journal (default: ~/.contribai/harness.db)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: HarnessCommand,
}

#[derive(Subcommand)]
enum HarnessCommand {
    /// Run a real provider on explicitly selected files. Requires --config.
    Run {
        task: String,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        /// Exact relative text file to allow; repeat for each file
        #[arg(long = "file", required = true)]
        files: Vec<String>,
        #[arg(long, default_value_t = 12)]
        max_turns: u32,
        #[arg(long, default_value_t = 24)]
        max_tools: u32,
        /// Total reserved output tokens, not actual usage or a dollar budget
        #[arg(long, default_value_t = 16_384)]
        token_budget: u32,
        #[arg(long, default_value_t = 120)]
        timeout_seconds: u64,
        #[arg(long)]
        json: bool,
    },
    /// Exercise the real loop with a deterministic fixture, no model/network
    Demo {
        #[arg(long)]
        json: bool,
    },
    /// List local run metadata without contacting a model
    Runs {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Inspect ordered run events without contacting a model
    Inspect {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run_harness(config_path: Option<&str>, args: HarnessArgs) -> anyhow::Result<()> {
    let db = match args.db {
        Some(path) => path,
        None => dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("home directory unavailable; supply --db"))?
            .join(".contribai/harness.db"),
    };
    // Inspection never initializes a missing journal or falls back to live work.
    if matches!(
        args.command,
        HarnessCommand::Runs { .. } | HarnessCommand::Inspect { .. }
    ) && !db.is_file()
    {
        anyhow::bail!("harness journal does not exist; run harness demo or harness run first");
    }
    match args.command {
        HarnessCommand::Runs { limit, json } => {
            let runs = HarnessStore::open(&db)?.list(limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&runs)?);
            } else {
                for run in runs {
                    println!(
                        "{}  {:?}  {} model calls / {} tools",
                        run.id, run.status, run.model_calls, run.tool_calls
                    );
                }
            }
            Ok(())
        }
        HarnessCommand::Inspect { run_id, json } => {
            let (run, events) = HarnessStore::open(&db)?
                .inspect(&run_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown harness run"))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"run":run,"events":events}))?
                );
            } else {
                println!(
                    "{}  {:?}\nReason: {}\nExternal writes: disabled",
                    run.id,
                    run.status,
                    run.reason.as_deref().unwrap_or("completion unknown")
                );
                for event in events {
                    println!("{}", serde_json::to_string(&event)?);
                }
            }
            Ok(())
        }
        HarnessCommand::Demo { json } => {
            let snapshot = HarnessSnapshot::from_files(BTreeMap::from([(
                "README.md".into(),
                "A tiny read-only harness fixture.".into(),
            )]))?;
            execute(
                &DemoProvider,
                "demo_fixture",
                "deterministic",
                &snapshot,
                &db,
                "Describe the selected fixture.",
                HarnessLimits::default(),
                json,
            )
            .await
        }
        HarnessCommand::Run {
            task,
            workspace,
            files,
            max_turns,
            max_tools,
            token_budget,
            timeout_seconds,
            json,
        } => {
            let path = config_path.ok_or_else(|| anyhow::anyhow!("harness run requires an explicit --config; selected files are sent to that provider"))?;
            // Do not load ContribAIConfig: its legacy secret resolution may run
            // synchronous gh/gcloud credential helpers even for local inference.
            // A required llm section makes provider selection explicit.
            #[derive(serde::Deserialize)]
            struct ProviderConfig {
                llm: serde_yaml::Value,
            }
            let config: ProviderConfig =
                serde_yaml::from_str(&tokio::fs::read_to_string(path).await?)?;
            for field in ["provider", "model"] {
                if config
                    .llm
                    .get(field)
                    .and_then(serde_yaml::Value::as_str)
                    .is_none_or(|s| s.trim().is_empty())
                {
                    anyhow::bail!("harness config requires an explicit llm.{field}");
                }
            }
            let llm: LlmConfig = serde_yaml::from_value(config.llm)?;
            // These adapters use asynchronous HTTP. Synchronous external credential
            // helpers and implicit provider/account fallback are deliberately absent.
            if !matches!(
                llm.provider.as_str(),
                "ollama" | "openai" | "anthropic" | "gemini"
            ) || llm.use_vertex()
                || llm.copilot
            {
                anyhow::bail!("harness supports explicit ollama, openai, anthropic, or API-key gemini configuration only; external credential helpers are not supported");
            }
            let limits = HarnessLimits {
                max_model_calls: max_turns,
                max_tool_calls: max_tools,
                max_reserved_output_tokens: token_budget,
                max_duration_ms: timeout_seconds
                    .checked_mul(1000)
                    .ok_or_else(|| anyhow::anyhow!("timeout too large"))?,
                ..HarnessLimits::default()
            };
            limits.validate()?;
            let snapshot = HarnessSnapshot::load(&workspace, &files).await?;
            let provider = create_llm_provider_raw(&llm)?;
            execute(
                provider.as_ref(),
                &llm.provider,
                &llm.model,
                &snapshot,
                &db,
                &task,
                limits,
                json,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    provider: &dyn LlmProvider,
    provider_name: &str,
    model_name: &str,
    snapshot: &HarnessSnapshot,
    db: &std::path::Path,
    task: &str,
    limits: HarnessLimits,
    json: bool,
) -> anyhow::Result<()> {
    let journal = HarnessStore::open(db)?;
    let cancel = HarnessCancellation::default();
    let signal_cancel = cancel.clone();
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal_cancel.cancel();
        }
    });
    let result = Harness {
        provider,
        provider_name,
        model_name,
        snapshot,
        journal: &journal,
        limits,
    }
    .run(task, &cancel)
    .await;
    signal.abort();
    let outcome = result?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    } else {
        println!(
            "Run {}: {:?}\nProvider: {}/{}\nExternal writes: disabled",
            outcome.run.id,
            outcome.run.status,
            terminal_text(provider_name),
            terminal_text(model_name)
        );
        if let Some(answer) = &outcome.answer {
            println!("\n{}", terminal_text(answer));
        }
        println!(
            "\nInspect: contribai harness --db {} inspect {} --json",
            db.display(),
            outcome.run.id
        );
    }
    if outcome.run.status != HarnessStatus::Completed {
        anyhow::bail!(
            "harness stopped: {}",
            outcome.run.reason.as_deref().unwrap_or("unknown")
        );
    }
    Ok(())
}

fn terminal_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

/// Explicit offline fixture, never selected by live configuration or fallback.
struct DemoProvider;

#[async_trait]
impl LlmProvider for DemoProvider {
    async fn complete(
        &self,
        _prompt: &str,
        _system: Option<&str>,
        _temperature: Option<f64>,
        _max_tokens: Option<u32>,
    ) -> Result<String> {
        unreachable!("the harness uses multi-turn chat")
    }
    async fn chat(
        &self,
        messages: &[ChatMessage],
        _system: Option<&str>,
        _temperature: Option<f64>,
        _max_tokens: Option<u32>,
    ) -> Result<String> {
        Ok(match messages.len() {
            1 => r#"{"action":"list_files"}"#,
            3 => r#"{"action":"read_file","path":"README.md"}"#,
            _ => r#"{"action":"finish","answer":"Offline fixture completed: listed and read README.md. No real model, network, shell commands, edits, or submissions were used."}"#,
        }.into())
    }
}
