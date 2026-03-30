//! Configuration system for ContribAI.
//!
//! Reads `config.yaml` and environment variables.
//! Compatible with the Python version's config format.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::error::{ContribError, Result};

/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContribAIConfig {
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub contribution: ContributionConfig,
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    #[serde(default)]
    pub pipeline: PipelineConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub multi_model: MultiModelConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub quotas: QuotaConfig,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

impl ContribAIConfig {
    /// Load configuration from YAML file.
    pub fn from_yaml(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ContribError::Config(format!("Cannot read {}: {}", path.display(), e)))?;
        let mut config: Self = serde_yaml::from_str(&content)?;
        config.resolve_secrets();
        Ok(config)
    }

    /// Load from default location (`config.yaml` in cwd).
    pub fn load() -> Result<Self> {
        let candidates = [
            PathBuf::from("config.yaml"),
            PathBuf::from("config.yml"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".contribai")
                .join("config.yaml"),
        ];

        for path in &candidates {
            if path.exists() {
                return Self::from_yaml(path);
            }
        }

        // No config file found — use defaults + env vars
        Ok(Self::default())
    }

    /// Resolve empty secrets from environment variables and CLI tools.
    ///
    /// Mirrors Python's `@model_validator(mode='after')`:
    /// - GitHub token: `GITHUB_TOKEN` env → `gh auth token` CLI
    /// - Gemini key:   `GEMINI_API_KEY` env
    /// - Vertex project: `GOOGLE_CLOUD_PROJECT` env → `gcloud config get-value project`
    fn resolve_secrets(&mut self) {
        // GitHub token
        if self.github.token.is_empty() {
            self.github.token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
        }
        if self.github.token.is_empty() {
            self.github.token = resolve_gh_token();
        }

        // LLM secrets
        if self.llm.api_key.is_empty() {
            let env_map = [
                ("gemini", "GEMINI_API_KEY"),
                ("openai", "OPENAI_API_KEY"),
                ("anthropic", "ANTHROPIC_API_KEY"),
            ];
            for (provider, env_var) in &env_map {
                if self.llm.provider == *provider {
                    self.llm.api_key = std::env::var(env_var).unwrap_or_default();
                    break;
                }
            }
        }

        // Vertex AI project
        if self.llm.vertex_project.is_empty() {
            self.llm.vertex_project = std::env::var("GOOGLE_CLOUD_PROJECT")
                .unwrap_or_else(|_| resolve_gcloud_project());
        }
    }
}

impl Default for ContribAIConfig {
    fn default() -> Self {
        Self {
            github: GitHubConfig::default(),
            llm: LlmConfig::default(),
            analysis: AnalysisConfig::default(),
            contribution: ContributionConfig::default(),
            discovery: DiscoveryConfig::default(),
            pipeline: PipelineConfig::default(),
            storage: StorageConfig::default(),
            multi_model: MultiModelConfig::default(),
            scheduler: SchedulerConfig::default(),
            quotas: QuotaConfig::default(),
            notifications: NotificationConfig::default(),
            sandbox: SandboxConfig::default(),
        }
    }
}

/// GitHub API configuration.
#[derive(Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    /// GitHub personal access token (from env `GITHUB_TOKEN`).
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_rate_limit_buffer")]
    pub rate_limit_buffer: u32,
    #[serde(default = "default_max_prs_per_day")]
    pub max_prs_per_day: u32,
}

impl std::fmt::Debug for GitHubConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubConfig")
            .field("token", &"[REDACTED]")
            .field("rate_limit_buffer", &self.rate_limit_buffer)
            .field("max_prs_per_day", &self.max_prs_per_day)
            .finish()
    }
}

fn default_rate_limit_buffer() -> u32 {
    100
}
fn default_max_prs_per_day() -> u32 {
    5
}

impl Default for GitHubConfig {
    fn default() -> Self {
        // Priority: GITHUB_TOKEN env → `gh auth token` CLI
        let token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
        let token = if token.is_empty() {
            resolve_gh_token()
        } else {
            token
        };
        Self {
            token,
            rate_limit_buffer: default_rate_limit_buffer(),
            max_prs_per_day: default_max_prs_per_day(),
        }
    }
}

/// Run `gh auth token` to get the GitHub token from gh CLI.
fn resolve_gh_token() -> String {
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// LLM provider configuration.
#[derive(Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// For OpenAI-compatible endpoints.
    pub base_url: Option<String>,
    /// Google Cloud project for Vertex AI (replaces api_key auth).
    #[serde(default)]
    pub vertex_project: String,
    /// Vertex AI endpoint location (default: "global").
    #[serde(default = "default_vertex_location")]
    pub vertex_location: String,
}

impl LlmConfig {
    /// Whether to use Vertex AI instead of API key auth.
    pub fn use_vertex(&self) -> bool {
        !self.vertex_project.is_empty()
    }
}

impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("provider", &self.provider)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("base_url", &self.base_url)
            .field("vertex_project", &self.vertex_project)
            .field("vertex_location", &self.vertex_location)
            .finish()
    }
}

fn default_provider() -> String {
    "gemini".to_string()
}
fn default_model() -> String {
    "gemini-2.5-flash".to_string()
}
fn default_temperature() -> f64 {
    0.3
}
fn default_max_tokens() -> u32 {
    4096
}
fn default_vertex_location() -> String {
    "global".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        // Priority for Gemini: GEMINI_API_KEY env → Vertex AI via GOOGLE_CLOUD_PROJECT env → gcloud CLI
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        let vertex_project = std::env::var("GOOGLE_CLOUD_PROJECT")
            .unwrap_or_else(|_| resolve_gcloud_project());
        Self {
            provider: default_provider(),
            api_key,
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            base_url: None,
            vertex_project,
            vertex_location: default_vertex_location(),
        }
    }
}

/// Run `gcloud config get-value project` to get the active GCP project.
fn resolve_gcloud_project() -> String {
    std::process::Command::new("gcloud")
        .args(["config", "get-value", "project"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "(unset)")
        .unwrap_or_default()
}

/// Analysis engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    #[serde(default = "default_analyzers")]
    pub enabled_analyzers: Vec<String>,
    #[serde(default = "default_max_file_size_kb")]
    pub max_file_size_kb: u64,
    #[serde(default)]
    pub skip_patterns: Vec<String>,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
}

fn default_analyzers() -> Vec<String> {
    vec![
        "security".into(),
        "code_quality".into(),
        "performance".into(),
    ]
}
fn default_max_file_size_kb() -> u64 {
    100
}
fn default_max_context_tokens() -> usize {
    30_000
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            enabled_analyzers: default_analyzers(),
            max_file_size_kb: default_max_file_size_kb(),
            skip_patterns: vec![],
            max_context_tokens: default_max_context_tokens(),
        }
    }
}

/// Contribution generation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionConfig {
    #[serde(default = "default_max_changes_per_pr")]
    pub max_changes_per_pr: usize,
    #[serde(default)]
    pub sign_off: bool,
    /// Commit message convention: "conventional", "angular", "none".
    #[serde(default = "default_commit_convention")]
    pub commit_convention: String,
    /// PR description style: "descriptive", "minimal".
    #[serde(default = "default_pr_style")]
    pub pr_style: String,
    /// Whether to GPG-sign commits.
    #[serde(default = "default_sign_commits")]
    pub sign_commits: bool,
    /// Maximum character length for PR body.
    #[serde(default = "default_max_pr_body_length")]
    pub max_pr_body_length: usize,
    /// Whether to include test changes in PRs.
    #[serde(default = "default_include_tests")]
    pub include_tests: bool,
}

fn default_max_changes_per_pr() -> usize {
    5
}
fn default_commit_convention() -> String {
    "conventional".to_string()
}
fn default_pr_style() -> String {
    "descriptive".to_string()
}
fn default_sign_commits() -> bool {
    true
}
fn default_max_pr_body_length() -> usize {
    4000
}
fn default_include_tests() -> bool {
    true
}

impl Default for ContributionConfig {
    fn default() -> Self {
        Self {
            max_changes_per_pr: default_max_changes_per_pr(),
            sign_off: false,
            commit_convention: default_commit_convention(),
            pr_style: default_pr_style(),
            sign_commits: default_sign_commits(),
            max_pr_body_length: default_max_pr_body_length(),
            include_tests: default_include_tests(),
        }
    }
}

/// Discovery configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default = "default_disc_languages")]
    pub languages: Vec<String>,
    #[serde(default = "default_disc_stars_min")]
    pub stars_min: i64,
    #[serde(default = "default_disc_stars_max")]
    pub stars_max: i64,
    #[serde(default = "default_disc_max_results")]
    pub max_results: usize,
}

fn default_disc_languages() -> Vec<String> {
    vec!["python".into()]
}
fn default_disc_stars_min() -> i64 {
    50
}
fn default_disc_stars_max() -> i64 {
    10000
}
fn default_disc_max_results() -> usize {
    10
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            languages: default_disc_languages(),
            stars_min: default_disc_stars_min(),
            stars_max: default_disc_stars_max(),
            max_results: default_disc_max_results(),
        }
    }
}

/// Pipeline orchestrator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_min_quality_score")]
    pub min_quality_score: f64,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_max_repos_per_run")]
    pub max_repos_per_run: usize,
    #[serde(default = "default_max_concurrent_repos")]
    pub max_concurrent_repos: usize,
}

fn default_max_retries() -> u32 {
    2
}
fn default_min_quality_score() -> f64 {
    0.6
}
fn default_max_repos_per_run() -> usize {
    10
}
fn default_max_concurrent_repos() -> usize {
    3
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            min_quality_score: default_min_quality_score(),
            dry_run: false,
            max_repos_per_run: default_max_repos_per_run(),
            max_concurrent_repos: default_max_concurrent_repos(),
        }
    }
}

/// Storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_db_path")]
    pub db_path: String,
}

fn default_db_path() -> String {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".contribai")
        .join("memory.db")
        .to_string_lossy()
        .to_string()
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
        }
    }
}

impl StorageConfig {
    /// Resolve the database path, creating parent directories if needed.
    pub fn resolved_db_path(&self) -> PathBuf {
        let path = PathBuf::from(&self.db_path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        path
    }
}

/// Multi-model routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_strategy")]
    pub strategy: String,
}

fn default_strategy() -> String {
    "cost_optimized".to_string()
}

impl Default for MultiModelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: default_strategy(),
        }
    }
}

/// Scheduler configuration for cron-based runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Cron expression controlling run frequency.
    #[serde(default = "default_scheduler_cron")]
    pub cron: String,
    /// Whether scheduled runs are active.
    #[serde(default = "default_scheduler_enabled")]
    pub enabled: bool,
}

fn default_scheduler_cron() -> String {
    "0 */6 * * *".to_string()
}
fn default_scheduler_enabled() -> bool {
    true
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            cron: default_scheduler_cron(),
            enabled: default_scheduler_enabled(),
        }
    }
}

/// API usage quota configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// Maximum GitHub API calls per day.
    #[serde(default = "default_github_daily")]
    pub github_daily: u32,
    /// Maximum LLM API calls per day.
    #[serde(default = "default_llm_daily")]
    pub llm_daily: u32,
    /// Maximum LLM tokens consumed per day.
    #[serde(default = "default_llm_tokens_daily")]
    pub llm_tokens_daily: u64,
}

fn default_github_daily() -> u32 {
    1000
}
fn default_llm_daily() -> u32 {
    500
}
fn default_llm_tokens_daily() -> u64 {
    1_000_000
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            github_daily: default_github_daily(),
            llm_daily: default_llm_daily(),
            llm_tokens_daily: default_llm_tokens_daily(),
        }
    }
}

/// Notification channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Slack incoming webhook URL.
    pub slack_webhook: Option<String>,
    /// Discord webhook URL.
    pub discord_webhook: Option<String>,
    /// Telegram bot token.
    pub telegram_token: Option<String>,
    /// Telegram chat ID to send messages to.
    pub telegram_chat_id: Option<String>,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            slack_webhook: None,
            discord_webhook: None,
            telegram_token: None,
            telegram_chat_id: None,
        }
    }
}

/// Sandbox execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Whether sandboxed code execution is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Override Docker image for the sandbox container.
    pub docker_image: Option<String>,
    /// Sandbox execution timeout in seconds.
    #[serde(default = "default_sandbox_timeout")]
    pub timeout_seconds: u64,
}

fn default_sandbox_timeout() -> u64 {
    30
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            docker_image: None,
            timeout_seconds: default_sandbox_timeout(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ContribAIConfig::default();
        assert_eq!(config.llm.provider, "gemini");
        assert_eq!(config.llm.model, "gemini-2.5-flash");
        assert_eq!(config.analysis.max_context_tokens, 30_000);
        assert_eq!(config.pipeline.min_quality_score, 0.6);
    }

    #[test]
    fn test_config_from_yaml() {
        let yaml = r#"
github:
  rate_limit_buffer: 200
llm:
  provider: openai
  model: gpt-4o
analysis:
  enabled_analyzers:
    - security
    - performance
"#;
        let config: ContribAIConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.github.rate_limit_buffer, 200);
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o");
        assert_eq!(config.analysis.enabled_analyzers.len(), 2);
    }

    #[test]
    fn test_storage_resolved_path() {
        let storage = StorageConfig {
            db_path: "/tmp/test/memory.db".to_string(),
        };
        let path = storage.resolved_db_path();
        assert_eq!(path, PathBuf::from("/tmp/test/memory.db"));
    }

    // -------------------------------------------------------------------------
    // SchedulerConfig
    // -------------------------------------------------------------------------

    #[test]
    fn test_scheduler_config_defaults() {
        let s = SchedulerConfig::default();
        assert_eq!(s.cron, "0 */6 * * *");
        assert!(s.enabled);
    }

    #[test]
    fn test_scheduler_config_deser_empty() {
        let s: SchedulerConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(s.cron, "0 */6 * * *");
        assert!(s.enabled);
    }

    #[test]
    fn test_scheduler_config_deser_partial() {
        let yaml = "cron: \"0 0 * * *\"";
        let s: SchedulerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(s.cron, "0 0 * * *");
        assert!(s.enabled); // default preserved
    }

    // -------------------------------------------------------------------------
    // QuotaConfig
    // -------------------------------------------------------------------------

    #[test]
    fn test_quota_config_defaults() {
        let q = QuotaConfig::default();
        assert_eq!(q.github_daily, 1000);
        assert_eq!(q.llm_daily, 500);
        assert_eq!(q.llm_tokens_daily, 1_000_000);
    }

    #[test]
    fn test_quota_config_deser_empty() {
        let q: QuotaConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(q.github_daily, 1000);
        assert_eq!(q.llm_daily, 500);
        assert_eq!(q.llm_tokens_daily, 1_000_000);
    }

    #[test]
    fn test_quota_config_deser_partial() {
        let yaml = "github_daily: 200";
        let q: QuotaConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(q.github_daily, 200);
        assert_eq!(q.llm_daily, 500); // default preserved
    }

    // -------------------------------------------------------------------------
    // NotificationConfig
    // -------------------------------------------------------------------------

    #[test]
    fn test_notification_config_defaults() {
        let n = NotificationConfig::default();
        assert!(n.slack_webhook.is_none());
        assert!(n.discord_webhook.is_none());
        assert!(n.telegram_token.is_none());
        assert!(n.telegram_chat_id.is_none());
    }

    #[test]
    fn test_notification_config_deser_empty() {
        let n: NotificationConfig = serde_json::from_str("{}").unwrap();
        assert!(n.slack_webhook.is_none());
        assert!(n.discord_webhook.is_none());
    }

    #[test]
    fn test_notification_config_deser_with_values() {
        let yaml = "slack_webhook: \"https://hooks.slack.com/test\"";
        let n: NotificationConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            n.slack_webhook.as_deref(),
            Some("https://hooks.slack.com/test")
        );
        assert!(n.discord_webhook.is_none());
    }

    // -------------------------------------------------------------------------
    // SandboxConfig
    // -------------------------------------------------------------------------

    #[test]
    fn test_sandbox_config_defaults() {
        let s = SandboxConfig::default();
        assert!(!s.enabled);
        assert!(s.docker_image.is_none());
        assert_eq!(s.timeout_seconds, 30);
    }

    #[test]
    fn test_sandbox_config_deser_empty() {
        let s: SandboxConfig = serde_json::from_str("{}").unwrap();
        assert!(!s.enabled);
        assert_eq!(s.timeout_seconds, 30);
    }

    #[test]
    fn test_sandbox_config_deser_partial() {
        let yaml = "enabled: true\ndocker_image: \"rust:1.78\"";
        let s: SandboxConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(s.enabled);
        assert_eq!(s.docker_image.as_deref(), Some("rust:1.78"));
        assert_eq!(s.timeout_seconds, 30); // default preserved
    }

    // -------------------------------------------------------------------------
    // ContributionConfig new fields
    // -------------------------------------------------------------------------

    #[test]
    fn test_contribution_config_new_field_defaults() {
        let c = ContributionConfig::default();
        assert_eq!(c.commit_convention, "conventional");
        assert_eq!(c.pr_style, "descriptive");
        assert!(c.sign_commits);
        assert_eq!(c.max_pr_body_length, 4000);
        assert!(c.include_tests);
    }

    #[test]
    fn test_contribution_config_deser_empty() {
        let c: ContributionConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c.commit_convention, "conventional");
        assert_eq!(c.pr_style, "descriptive");
        assert!(c.sign_commits);
        assert_eq!(c.max_pr_body_length, 4000);
        assert!(c.include_tests);
    }

    // -------------------------------------------------------------------------
    // ContribAIConfig: new top-level fields present
    // -------------------------------------------------------------------------

    #[test]
    fn test_root_config_has_new_fields() {
        let cfg = ContribAIConfig::default();
        // scheduler
        assert!(cfg.scheduler.enabled);
        assert_eq!(cfg.scheduler.cron, "0 */6 * * *");
        // quotas
        assert_eq!(cfg.quotas.github_daily, 1000);
        // notifications
        assert!(cfg.notifications.slack_webhook.is_none());
        // sandbox
        assert!(!cfg.sandbox.enabled);
        assert_eq!(cfg.sandbox.timeout_seconds, 30);
    }

    #[test]
    fn test_root_config_new_fields_deser_from_yaml() {
        let yaml = r#"
scheduler:
  enabled: false
  cron: "0 0 * * *"
quotas:
  github_daily: 50
notifications:
  slack_webhook: "https://hooks.slack.com/x"
sandbox:
  enabled: true
  timeout_seconds: 60
"#;
        let cfg: ContribAIConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!cfg.scheduler.enabled);
        assert_eq!(cfg.scheduler.cron, "0 0 * * *");
        assert_eq!(cfg.quotas.github_daily, 50);
        assert_eq!(
            cfg.notifications.slack_webhook.as_deref(),
            Some("https://hooks.slack.com/x")
        );
        assert!(cfg.sandbox.enabled);
        assert_eq!(cfg.sandbox.timeout_seconds, 60);
    }
}
