//! Interactive setup wizard — `contribai init`.
//!
//! Walks user through provider selection, API keys, GitHub auth,
//! and writes the result to config.yaml. Inspired by `claude login`
//! and `gemini` setup flows.

use console::style;
use dialoguer::{Confirm, Input, Password, Select};
use std::path::{Path, PathBuf};

// ── Provider definitions ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LlmChoice {
    GeminiApiKey,
    VertexAi,
    OpenAi,
    Anthropic,
    Ollama,
}

impl LlmChoice {
    pub fn all() -> &'static [&'static str] {
        &[
            "Gemini (API Key)        — gemini-3-flash-preview, fast + free tier",
            "Vertex AI (Google Cloud)— uses gcloud ADC, no key needed",
            "OpenAI                  — gpt-4o, gpt-4-turbo",
            "Anthropic               — claude-3-5-sonnet",
            "Ollama (local)          — llama3, mistral, codestral",
        ]
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::GeminiApiKey,
            1 => Self::VertexAi,
            2 => Self::OpenAi,
            3 => Self::Anthropic,
            _ => Self::Ollama,
        }
    }

    pub fn provider_name(&self) -> &'static str {
        match self {
            Self::GeminiApiKey => "gemini",
            Self::VertexAi => "vertex",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Ollama => "ollama",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Self::GeminiApiKey => "gemini-3-flash-preview",
            Self::VertexAi => "gemini-3-flash-preview",
            Self::OpenAi => "gpt-4o",
            Self::Anthropic => "claude-3-5-sonnet-20241022",
            Self::Ollama => "llama3",
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum GithubAuthChoice {
    GhCli,
    Manual,
}

// ── Wizard result ─────────────────────────────────────────────────────────────

pub struct WizardResult {
    pub provider: LlmChoice,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub vertex_project: Option<String>,
    pub github_token: Option<String>,
    pub max_prs_per_day: u32,
    pub max_repos_per_run: u32,
    pub output_path: PathBuf,
}

// ── Main wizard ───────────────────────────────────────────────────────────────

/// Run the interactive setup wizard.
/// Returns `None` if user aborted.
pub fn run_init_wizard(output_path: Option<&Path>) -> anyhow::Result<Option<WizardResult>> {
    let term = console::Term::stdout();
    let _ = term.clear_screen();

    println!();
    println!("{}", style("  🤖 ContribAI Setup Wizard").cyan().bold());
    println!(
        "  {}",
        style("Configure your AI provider, GitHub auth, and limits.").dim()
    );
    println!();

    // ── Step 1: LLM Provider ─────────────────────────────────────────────────
    println!("{}", style("Step 1/4 — LLM Provider").yellow().bold());
    let provider_idx = Select::new()
        .with_prompt("Select your LLM provider")
        .items(LlmChoice::all())
        .default(0)
        .interact()?;
    let provider = LlmChoice::from_index(provider_idx);

    println!();

    // ── Step 2: Provider credentials ─────────────────────────────────────────
    println!("{}", style("Step 2/4 — Credentials").yellow().bold());
    let (api_key, base_url, vertex_project) = match provider {
        LlmChoice::GeminiApiKey => {
            println!(
                "  {}",
                style("Get your key at: https://aistudio.google.com/apikey").dim()
            );
            let key: String = Password::new()
                .with_prompt("Gemini API Key (hidden)")
                .allow_empty_password(true)
                .interact()?;
            (if key.is_empty() { None } else { Some(key) }, None, None)
        }
        LlmChoice::VertexAi => {
            println!(
                "  {}",
                style("Uses gcloud ADC — run 'gcloud auth application-default login' first.").dim()
            );
            let proj: String = Input::new()
                .with_prompt("Google Cloud Project ID")
                .default(std::env::var("GOOGLE_CLOUD_PROJECT").unwrap_or_default())
                .interact_text()?;
            (None, None, if proj.is_empty() { None } else { Some(proj) })
        }
        LlmChoice::OpenAi => {
            println!(
                "  {}",
                style("Get your key at: https://platform.openai.com/api-keys").dim()
            );
            let base_url: String = Input::new()
                .with_prompt("OpenAI-compatible base URL (optional)")
                .default("https://api.openai.com/v1".into())
                .allow_empty(true)
                .interact_text()
                .unwrap_or_default();
            let key: String = Password::new()
                .with_prompt("OpenAI API Key (hidden)")
                .allow_empty_password(true)
                .interact()?;
            let api_key = if key.is_empty() { None } else { Some(key) };
            (
                api_key,
                if base_url.trim().is_empty() {
                    None
                } else {
                    Some(base_url)
                },
                None,
            )
        }
        LlmChoice::Anthropic => {
            println!(
                "  {}",
                style("Get your key at: https://console.anthropic.com/").dim()
            );
            let base_url: String = Input::new()
                .with_prompt("Anthropic-compatible base URL (optional)")
                .default("https://api.anthropic.com/v1".into())
                .allow_empty(true)
                .interact_text()
                .unwrap_or_default();
            let key: String = Password::new()
                .with_prompt("Anthropic API Key (hidden)")
                .allow_empty_password(true)
                .interact()?;
            let api_key = if key.is_empty() { None } else { Some(key) };
            (
                api_key,
                if base_url.trim().is_empty() {
                    None
                } else {
                    Some(base_url)
                },
                None,
            )
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
            let base_url = if url.trim().is_empty() {
                None
            } else {
                Some(url)
            };
            (None, base_url, None)
        }
    };

    println!();

    // ── Step 3: GitHub auth ──────────────────────────────────────────────────
    println!(
        "{}",
        style("Step 3/4 — GitHub Authentication").yellow().bold()
    );

    let gh_ok = which::which("gh").is_ok();
    let gh_choices = if gh_ok {
        vec![
            "Auto-detect via gh CLI  (recommended — already installed)",
            "Enter token manually",
        ]
    } else {
        vec![
            "Auto-detect via gh CLI  (not found — install from cli.github.com)",
            "Enter token manually",
        ]
    };

    let gh_idx = Select::new()
        .with_prompt("GitHub authentication")
        .items(&gh_choices)
        .default(if gh_ok { 0 } else { 1 })
        .interact()?;

    let github_token = if gh_idx == 1 {
        println!(
            "  {}",
            style("Create at: https://github.com/settings/tokens").dim()
        );
        let t: String = Password::new()
            .with_prompt("GitHub Personal Access Token (hidden)")
            .allow_empty_password(true)
            .interact()?;
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    } else {
        None // will use gh CLI auto-detect at runtime
    };

    println!();

    // ── Step 4: Limits ───────────────────────────────────────────────────────
    println!("{}", style("Step 4/4 — Safety Limits").yellow().bold());
    let max_prs: u32 = Input::new()
        .with_prompt("Max PRs per day")
        .default(15u32)
        .interact_text()?;

    let max_repos: u32 = Input::new()
        .with_prompt("Max repos per run")
        .default(20u32)
        .interact_text()?;

    println!();

    // ── Output path ──────────────────────────────────────────────────────────
    let default_path = output_path
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "./config.yaml".to_string());

    let path_str: String = Input::new()
        .with_prompt("Save config to")
        .default(default_path)
        .interact_text()?;

    println!();

    // ── Confirm ──────────────────────────────────────────────────────────────
    print_wizard_summary(
        &provider,
        &api_key,
        &base_url,
        &vertex_project,
        max_prs,
        max_repos,
        &path_str,
    );

    let confirmed = Confirm::new()
        .with_prompt("Save configuration?")
        .default(true)
        .interact()?;

    if !confirmed {
        println!("{}", style("  Aborted — no changes made.").yellow());
        return Ok(None);
    }

    Ok(Some(WizardResult {
        provider,
        api_key,
        base_url,
        vertex_project,
        github_token,
        max_prs_per_day: max_prs,
        max_repos_per_run: max_repos,
        output_path: PathBuf::from(&path_str),
    }))
}

fn print_wizard_summary(
    provider: &LlmChoice,
    api_key: &Option<String>,
    base_url: &Option<String>,
    vertex_project: &Option<String>,
    max_prs: u32,
    max_repos: u32,
    path: &str,
) {
    println!("{}", style("  Summary").bold());
    println!(
        "  {:<20} {}",
        style("Provider:").dim(),
        style(provider.provider_name()).cyan()
    );
    println!(
        "  {:<20} {}",
        style("Model:").dim(),
        style(provider.default_model()).cyan()
    );

    if let Some(k) = api_key {
        let masked = mask_secret(k);
        println!("  {:<20} {}", style("API Key:").dim(), style(masked).cyan());
    }
    if let Some(u) = base_url {
        println!("  {:<20} {}", style("Base URL:").dim(), style(u).cyan());
    }
    if let Some(p) = vertex_project {
        println!(
            "  {:<20} {}",
            style("Vertex Project:").dim(),
            style(p).cyan()
        );
    }
    println!(
        "  {:<20} {}",
        style("Max PRs/day:").dim(),
        style(max_prs).cyan()
    );
    println!(
        "  {:<20} {}",
        style("Max repos/run:").dim(),
        style(max_repos).cyan()
    );
    println!(
        "  {:<20} {}",
        style("Config path:").dim(),
        style(path).cyan()
    );
    println!();
}

/// Mask a secret: show last 4 chars, rest as `*`.
pub fn mask_secret(s: &str) -> String {
    if s.len() <= 4 {
        return "*".repeat(s.len());
    }
    let visible = &s[s.len() - 4..];
    format!("{}{}{}{}****{}", "*", "*", "*", "*", visible)
}

// ── YAML writer ───────────────────────────────────────────────────────────────

/// Write wizard result to a config.yaml file.
pub fn write_wizard_config(result: &WizardResult) -> anyhow::Result<()> {
    // Load existing config if present, else use default template
    let existing = if result.output_path.exists() {
        std::fs::read_to_string(&result.output_path)?
    } else {
        default_config_template()
    };

    let updated = apply_wizard_to_yaml(&existing, result);

    if let Some(parent) = result.output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    std::fs::write(&result.output_path, updated)?;
    println!(
        "  {} Config saved to {}",
        style("✅").green(),
        style(result.output_path.display()).cyan().bold()
    );
    println!(
        "  {} Run {} to verify setup.",
        style("→").dim(),
        style("contribai login").cyan()
    );
    Ok(())
}

/// Apply wizard choices onto existing YAML content.
fn apply_wizard_to_yaml(yaml: &str, result: &WizardResult) -> String {
    let mut lines: Vec<String> = yaml.lines().map(String::from).collect();

    let mut has_base_url = false;

    for line in &mut lines {
        let trimmed = line.trim_start().to_string();

        // LLM provider
        if trimmed.starts_with("provider:") && (yaml.contains("\nllm:") || yaml.starts_with("llm:"))
        {
            *line = format!("  provider: \"{}\"", result.provider.provider_name());
        }
        // Model
        else if trimmed.starts_with("model:") {
            *line = format!("  model: \"{}\"", result.provider.default_model());
        }
        // API key
        else if trimmed.starts_with("api_key:") {
            let val = result.api_key.as_deref().unwrap_or("");
            *line = format!("  api_key: \"{}\"", val);
        }
        // Base URL
        else if trimmed.starts_with("base_url:") {
            has_base_url = true;
            let val = result.base_url.as_deref().unwrap_or("");
            *line = format!("  base_url: \"{}\"", val);
        }
        // Vertex project
        else if trimmed.starts_with("vertex_project:") {
            let val = result.vertex_project.as_deref().unwrap_or("");
            *line = format!("  vertex_project: \"{}\"", val);
        }
        // GitHub token
        else if trimmed.starts_with("token:") {
            let val = result.github_token.as_deref().unwrap_or("");
            *line = format!("  token: \"{}\"", val);
        }
        // Max PRs
        else if trimmed.starts_with("max_prs_per_day:") {
            *line = format!("  max_prs_per_day: {}", result.max_prs_per_day);
        }
        // Max repos
        else if trimmed.starts_with("max_repos_per_run:") {
            *line = format!("  max_repos_per_run: {}", result.max_repos_per_run);
        }
    }

    // If base_url: is missing from the file (legacy config), insert it after api_key:
    if !has_base_url {
        let val = result.base_url.as_deref().unwrap_or("");
        let insert = format!("  base_url: \"{}\"", val);
        if let Some(i) = lines
            .iter()
            .position(|l| l.trim_start().starts_with("api_key:"))
        {
            lines.insert(i + 1, insert);
        }
    }

    lines.join("\n") + "\n"
}

fn default_config_template() -> String {
    // Embed a minimal default config template
    r#"# ContribAI Configuration
# Generated by 'contribai init'
# Edit manually or use: contribai config set <key> <value>

github:
  token: ""
  max_repos_per_run: 20
  max_prs_per_day: 15
  rate_limit_buffer: 100

llm:
  provider: "gemini"
  model: "gemini-3-flash-preview"
  api_key: ""
  base_url: ""
  temperature: 0.3
  max_tokens: 8192
  vertex_project: ""
  vertex_location: "global"

analysis:
  enabled_analyzers:
    - security
    - code_quality
    - docs
    - performance
    - refactor
  severity_threshold: "medium"
  max_file_size_kb: 500
  skip_patterns:
    - "*.min.js"
    - "*.min.css"
    - "vendor/*"
    - "node_modules/*"
    - "*.lock"

contribution:
  enabled_types:
    - security_fix
    - docs_improve
    - code_quality
  max_files_per_pr: 10
  run_tests_before_pr: true
  style:
    commit_convention: "conventional"
    pr_description_style: "detailed"

discovery:
  languages:
    - python
    - javascript
    - typescript
    - go
    - rust
  stars_range: [50, 5000]
  min_last_activity_days: 30
  require_contributing_guide: false
  topics: []

storage:
  db_path: "~/.contribai/memory.db"
  cache_ttl_hours: 24

pipeline:
  max_concurrent_repos: 3
  timeout_per_repo_sec: 300

scheduler:
  enabled: false
  cron: "0 */6 * * *"
  timezone: "UTC"
  max_concurrent: 3

web:
  host: "127.0.0.1"
  port: 8787
  enabled: true
  api_keys: []
  webhook_secret: ""

quota:
  github_daily_limit: 5000
  llm_daily_limit: 1000
  llm_daily_tokens: 1000000

notifications:
  slack_webhook: ""
  discord_webhook: ""
  telegram_token: ""
  telegram_chat_id: ""
  on_merge: true
  on_close: true
  on_run_complete: true

multi_model:
  enabled: true
  strategy: "balanced"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret() {
        assert_eq!(mask_secret("abcdefgh"), "********efgh");
        assert_eq!(mask_secret("ab"), "**");
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn test_llm_choice_provider_names() {
        assert_eq!(LlmChoice::GeminiApiKey.provider_name(), "gemini");
        assert_eq!(LlmChoice::VertexAi.provider_name(), "vertex");
        assert_eq!(LlmChoice::OpenAi.provider_name(), "openai");
        assert_eq!(LlmChoice::Anthropic.provider_name(), "anthropic");
        assert_eq!(LlmChoice::Ollama.provider_name(), "ollama");
    }

    #[test]
    fn test_llm_choice_default_models() {
        assert!(LlmChoice::GeminiApiKey.default_model().contains("gemini"));
        assert!(LlmChoice::OpenAi.default_model().contains("gpt"));
        assert!(LlmChoice::Anthropic.default_model().contains("claude"));
    }

    #[test]
    fn test_apply_wizard_to_yaml() {
        let yaml = "llm:\n  provider: \"gemini\"\n  model: \"old-model\"\n  api_key: \"\"\n  base_url: \"\"\n  vertex_project: \"\"\n";
        let result = WizardResult {
            provider: LlmChoice::OpenAi,
            api_key: Some("sk-test123".into()),
            base_url: Some("https://api.openai.com/v1".into()),
            vertex_project: None,
            github_token: None,
            max_prs_per_day: 10,
            max_repos_per_run: 5,
            output_path: std::path::PathBuf::from("test.yaml"),
        };
        let updated = apply_wizard_to_yaml(yaml, &result);
        assert!(updated.contains("openai"));
        assert!(updated.contains("gpt-4o"));
        assert!(updated.contains("sk-test123"));
        assert!(updated.contains("base_url: \"https://api.openai.com/v1\""));
    }

    /// Verifies base_url is inserted when missing from legacy config.yaml
    #[test]
    fn test_apply_wizard_to_yaml_inserts_base_url_when_missing() {
        // Legacy config without base_url: key
        let yaml = "llm:\n  provider: \"anthropic\"\n  model: \"claude-3-5-sonnet-20241022\"\n  api_key: \"sk-old\"\n  temperature: 0.3\n  vertex_project: \"\"\n";
        let result = WizardResult {
            provider: LlmChoice::Anthropic,
            api_key: Some("sk-new".into()),
            base_url: Some("https://api.anthropic.com/v1".into()),
            vertex_project: None,
            github_token: None,
            max_prs_per_day: 15,
            max_repos_per_run: 20,
            output_path: std::path::PathBuf::from("test.yaml"),
        };
        let updated = apply_wizard_to_yaml(yaml, &result);
        assert!(updated.contains("base_url: \"https://api.anthropic.com/v1\""));
        // base_url inserted after api_key
        let api_key_pos = updated.find("api_key: \"sk-new\"").unwrap();
        let base_url_pos = updated
            .find("base_url: \"https://api.anthropic.com/v1\"")
            .unwrap();
        assert!(base_url_pos > api_key_pos);
    }

    #[test]
    fn test_llm_from_index_all() {
        for i in 0..5 {
            let _ = LlmChoice::from_index(i);
        }
    }
}
