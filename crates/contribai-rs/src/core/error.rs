//! Error types for ContribAI.

use thiserror::Error;

/// Top-level error type for ContribAI operations.
#[derive(Error, Debug)]
pub enum ContribError {
    #[error("GitHub API error: {0}")]
    GitHub(String),

    #[error("GitHub rate limit exceeded, resets at {reset_at}")]
    RateLimit { reset_at: String },

    #[error("LLM provider error: {0}")]
    Llm(String),

    #[error("Analysis error: {0}")]
    Analysis(String),

    #[error("Generation error: {0}")]
    Generation(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("AST parsing error: {0}")]
    AstParse(String),

    #[error("Sandbox error: {0}")]
    Sandbox(String),

    #[error("PR creation error: {0}")]
    PrCreation(String),

    #[error("AI policy violation: repo {repo} bans AI contributions")]
    AiPolicyViolation { repo: String },

    #[error("Duplicate PR: already submitted to {repo}")]
    DuplicatePr { repo: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type alias for ContribAI operations.
pub type Result<T> = std::result::Result<T, ContribError>;
