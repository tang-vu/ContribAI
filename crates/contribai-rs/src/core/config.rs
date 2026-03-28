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
}

impl ContribAIConfig {
    /// Load configuration from YAML file.
    pub fn from_yaml(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ContribError::Config(format!("Cannot read {}: {}", path.display(), e)))?;
        let config: Self = serde_yaml::from_str(&content)?;
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
        }
    }
}

/// GitHub API configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    /// GitHub personal access token (from env `GITHUB_TOKEN`).
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_rate_limit_buffer")]
    pub rate_limit_buffer: u32,
    #[serde(default = "default_max_prs_per_day")]
    pub max_prs_per_day: u32,
}

fn default_rate_limit_buffer() -> u32 {
    100
}
fn default_max_prs_per_day() -> u32 {
    5
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            token: std::env::var("GITHUB_TOKEN").unwrap_or_default(),
            rate_limit_buffer: default_rate_limit_buffer(),
            max_prs_per_day: default_max_prs_per_day(),
        }
    }
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key: std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            base_url: None,
        }
    }
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
}

fn default_max_changes_per_pr() -> usize {
    5
}

impl Default for ContributionConfig {
    fn default() -> Self {
        Self {
            max_changes_per_pr: default_max_changes_per_pr(),
            sign_off: false,
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
}

fn default_max_retries() -> u32 {
    2
}
fn default_min_quality_score() -> f64 {
    0.6
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            min_quality_score: default_min_quality_score(),
            dry_run: false,
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
}
