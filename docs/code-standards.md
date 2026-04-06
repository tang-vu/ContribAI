# Code Standards & Development Guidelines

**Version:** 5.8.1 | **Language:** Rust 2021 | **Status:** Active

---

## Quick Reference

| Standard | Rule |
|----------|------|
| **Language** | Rust 2021 edition |
| **Style** | rustfmt (autoformat) + clippy (lint) |
| **Types** | serde structs for data, strong typing for functions |
| **Async** | All I/O operations use Tokio async |
| **Testing** | cargo test, 418 tests, co-located in source files |
| **Database** | SQLite via rusqlite (sync, wrapped with spawn_blocking) |
| **Errors** | thiserror enums from `core::error` |
| **Config** | serde_yaml + serde Deserialize structs |
| **File Size** | Code files ≤ 200 LOC, split if larger |
| **Function Size** | ≤ 50 lines |

---

## Rust Conventions

### Type Safety (MANDATORY)

**All public APIs require explicit types:**

```rust
// Good ✓
pub async fn analyze(
    &self,
    repo: &Repository,
    config: &ContribAIConfig,
    skip_cache: bool,
) -> Result<Vec<Finding>> {
    // ...
}

// Bad ✗
pub async fn analyze(&self, repo: &Repository) -> Result<Vec<Finding>> {
    let x = get_something(); // Type not obvious from context
}
```

### Async/Await (MANDATORY FOR I/O)

**All I/O operations are async via Tokio:**

```rust
// Good ✓ — async HTTP request
pub async fn search_repositories(&self, query: &str) -> Result<Vec<Repository>> {
    let response = self.client.get(&url)
        .header("Authorization", format!("Bearer {}", self.token))
        .send()
        .await?;
    let repos: Vec<Repository> = response.json().await?;
    Ok(repos)
}

// Good ✓ — sync DB wrapped in spawn_blocking
let stats = tokio::task::spawn_blocking(move || {
    let conn = Connection::open(&db_path)?;
    conn.query_row("SELECT COUNT(*) FROM submitted_prs", [], |row| row.get(0))
}).await??;

// Bad ✗ — blocking I/O in async context
pub async fn fetch_data(&self) -> Result<String> {
    std::fs::read_to_string("file.txt")? // Blocks the runtime!
}
```

### Documentation Comments

**Required for all public functions, structs, and modules:**

```rust
/// Generate a code fix for a finding.
///
/// Uses LLM to generate a targeted fix, validates syntax,
/// and scores quality before returning.
///
/// # Arguments
/// * `finding` - Issue detected by analyzer
/// * `context` - File content and surrounding context
///
/// # Errors
/// Returns `ContribAIError::Generation` if LLM fails or quality < threshold.
pub async fn generate_fix(
    &self,
    finding: &Finding,
    context: &FileContext,
) -> Result<Contribution> {
    // ...
}
```

### Module Documentation

**Every module file starts with `//!` doc comments:**

```rust
//! MCP server and client for Model Context Protocol integration.
//!
//! - `server`: Exposes ContribAI's GitHub tools via stdio (for Claude/Antigravity).
//! - `client`: Consumes external MCP servers via stdio subprocess.

pub mod client;
pub mod server;
```

### Import Organization

**Follow this import order:**

```rust
// 1. Standard library
use std::collections::HashMap;
use std::sync::Arc;

// 2. Third-party crates (alphabetical)
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::{debug, error, info};

// 3. Local crate modules
use crate::core::config::ContribAIConfig;
use crate::core::error::{ContribAIError, Result};
use crate::core::models::{Finding, Repository};
```

### Constants & Enums

```rust
// Good ✓ — use constants and enums
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_FINDINGS_PER_REPO: usize = 2;
const QUALITY_THRESHOLD: f64 = 0.6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

// Bad ✗ — magic numbers
if timeout > 30 { ... }
if severity == "critical" { ... }
```

### Logging (tracing, NOT println!)

```rust
// Good ✓
use tracing::{debug, error, info, warn};

info!(repo = %repo.full_name, "Analyzing repository");
debug!(findings = findings.len(), "Analysis complete");
warn!(remaining = rate_limit, "API rate limit approaching");
error!(error = %e, repo = %repo.full_name, "Failed to create PR");

// Bad ✗
println!("Analyzing repo: {}", repo.full_name);
```

---

## Design Patterns

### Pattern 1: Trait-Based Providers (Strategy)

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str, max_tokens: usize) -> Result<String>;
    fn name(&self) -> &str;
}

pub struct GeminiProvider { /* ... */ }
pub struct OpenAIProvider { /* ... */ }

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn complete(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        // Gemini-specific implementation
    }
    fn name(&self) -> &str { "gemini" }
}

// Factory function
pub fn create_llm_provider(config: &LlmConfig) -> Box<dyn LlmProvider> {
    match config.provider.as_str() {
        "gemini" => Box::new(GeminiProvider::new(config)),
        "openai" => Box::new(OpenAIProvider::new(config)),
        _ => panic!("Unknown provider: {}", config.provider),
    }
}
```

### Pattern 2: Middleware Chain

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn process(
        &self,
        repo: &Repository,
        next: &dyn Fn(&Repository) -> BoxFuture<Result<PipelineResult>>,
    ) -> Result<PipelineResult>;
}

pub struct RateLimitMiddleware { max_prs_per_day: usize }

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn process(&self, repo: &Repository, next: /* ... */) -> Result<PipelineResult> {
        if self.daily_count() >= self.max_prs_per_day {
            return Err(ContribAIError::RateLimit("Daily PR limit".into()));
        }
        next(repo).await
    }
}
```

### Pattern 3: Event Bus (Observer)

```rust
pub struct EventBus {
    handlers: Vec<Box<dyn Fn(&Event) + Send + Sync>>,
    jsonl_path: Option<PathBuf>,
}

impl EventBus {
    pub fn emit(&self, event: Event) {
        // Notify all handlers
        for handler in &self.handlers {
            handler(&event);
        }
        // Append to JSONL log
        if let Some(ref path) = self.jsonl_path {
            let line = serde_json::to_string(&event).unwrap();
            // append to file...
        }
    }
}
```

### Pattern 4: Serde Models for All Data

```rust
// Good ✓ — serde struct with validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub finding_type: String,
    pub file_path: String,
    pub line: usize,
    pub description: String,
    pub severity: Severity,
    #[serde(default)]
    pub context: String,
}

// Bad ✗ — using HashMap for structured data
let finding: HashMap<String, String> = HashMap::new();
```

### Pattern 5: Dependency Injection via Constructors

```rust
// Good ✓
pub struct CodeAnalyzer {
    llm: Box<dyn LlmProvider>,
    github: GitHubClient,
    config: ContribAIConfig,
}

impl CodeAnalyzer {
    pub fn new(
        llm: Box<dyn LlmProvider>,
        github: GitHubClient,
        config: ContribAIConfig,
    ) -> Self {
        Self { llm, github, config }
    }
}

// Bad ✗ — creating dependencies internally
impl CodeAnalyzer {
    pub fn new() -> Self {
        let llm = create_llm_provider(); // Hard to test!
        Self { llm }
    }
}
```

---

## Error Handling

### Error Types (thiserror)

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContribAIError {
    #[error("Analysis error: {0}")]
    Analysis(String),
    #[error("GitHub API error: {0}")]
    GitHub(String),
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ContribAIError>;
```

### Error Handling Best Practices

```rust
// Good ✓ — use ? operator and map_err
pub async fn process_repo(&self, repo: &Repository) -> Result<PipelineResult> {
    let findings = self.analyzer.analyze(repo).await
        .map_err(|e| ContribAIError::Analysis(format!("Failed for {}: {e}", repo.full_name)))?;

    let contributions = self.generator.generate_fixes(&findings).await?;
    Ok(PipelineResult { prs: contributions })
}

// Bad ✗ — silently swallowing errors
match self.analyzer.analyze(repo).await {
    Ok(f) => f,
    Err(_) => vec![], // Lost error context!
}
```

---

## Testing Strategy

### Co-located Tests

Tests live in the same file as the code they test:

```rust
// At bottom of each .rs file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_threshold() {
        assert!(QUALITY_THRESHOLD > 0.0);
        assert!(QUALITY_THRESHOLD <= 1.0);
    }

    #[tokio::test]
    async fn test_async_operation() {
        let result = some_async_fn().await;
        assert!(result.is_ok());
    }
}
```

### Test Patterns

```rust
// Parametric testing
#[test]
fn test_severity_ordering() {
    let cases = vec![
        (Severity::Critical, true),
        (Severity::Low, false),
    ];
    for (severity, expected_high) in cases {
        assert_eq!(severity.is_high(), expected_high);
    }
}

// Testing error conditions
#[test]
fn test_empty_command_rejected() {
    let client = StdioMcpClient::new(&[]);
    // Client creation succeeds but connect will fail
    assert!(client.cmd.is_empty());
}
```

### Test Commands

```bash
# Run all tests
cargo test

# Run specific module tests
cargo test mcp::server

# Run with output
cargo test -- --nocapture

# Run single test
cargo test test_quality_threshold
```

**Test Count:** 418 tests across 67 source files

---

## Code Quality Tools

### Clippy (Linting)

```bash
# Run clippy
cargo clippy -- -W clippy::all

# Fix auto-fixable issues
cargo clippy --fix
```

### Rustfmt (Formatting)

```bash
# Check formatting
cargo fmt -- --check

# Auto-format
cargo fmt
```

### Pre-Commit Checks

```bash
# Before every commit:
cargo fmt -- --check && cargo clippy -- -W clippy::all && cargo test
```

---

## File Organization

### Module Layout

```
crates/contribai-rs/src/module/
├── mod.rs              # Public API re-exports + module docs
├── main_component.rs   # Primary struct/logic
├── sub_component.rs    # Supporting types
└── (tests co-located in each file)
```

### Public API (mod.rs)

```rust
//! Module description.
pub mod main_component;
pub mod sub_component;

// Re-export key types
pub use main_component::PrimaryStruct;
```

### File Size Limits

| Type | Max LOC | Action |
|------|---------|--------|
| Rust module | 200 | Split into sub-modules |
| Function | 50 | Extract sub-functions |
| Impl block | 300 | Split by responsibility |
| Test module | 500 | Still co-located but split logic |

---

## Configuration Management

### Serde Config Structs

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ContribAIConfig {
    pub github: GitHubConfig,
    pub llm: LlmConfig,
    pub discovery: DiscoveryConfig,
    pub analysis: AnalysisConfig,
    pub pipeline: PipelineConfig,
    pub web: WebConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubConfig {
    pub token: String,
    #[serde(default = "default_max_prs")]
    pub max_prs_per_day: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_max_prs() -> usize { 15 }
```

### Loading Config

```rust
// From YAML file
let config: ContribAIConfig = serde_yaml::from_str(&yaml_content)?;

// Environment variable overrides (CONTRIBAI_* prefix)
let token = std::env::var("CONTRIBAI_GITHUB_TOKEN")
    .unwrap_or_else(|_| config.github.token.clone());
```

---

## Performance Guidelines

### Async Concurrency

- **Max concurrent repos:** 3 (via `tokio::sync::Semaphore`)
- **Max concurrent API calls:** 5 per provider
- **Timeout defaults:** 30s (GitHub), 60s (LLM)

### Token Budgeting

- **Per-analysis budget:** 30,000 tokens
- **3-tier compression:** Full code → Signatures → Summary
- **Language-aware extraction** for 5 languages

### Database

- **Batch inserts:** Use transactions for multiple writes
- **Indexes:** On `repo_id`, `pr_number`, `timestamp`
- **spawn_blocking:** All rusqlite calls wrapped

---

## Security Standards

### Secrets

- **Never log** API keys, tokens, or credentials
- **Use env vars** `CONTRIBAI_GITHUB_TOKEN`, `CONTRIBAI_LLM_API_KEY`
- **Validate inputs** from external sources
- **Sanitize LLM output** before code execution

### Crypto

- **HMAC-SHA256** for webhook signature verification
- **Constant-time comparison** for API key auth (timing attack mitigation)
- Dependencies: `hmac`, `sha2`, `hex` crates

### Dependencies

- **Audit:** `cargo audit` in CI
- **Auto-update:** Dependabot in GitHub
- **Lock versions:** `Cargo.lock` committed

### Access Control

- **GitHub:** Use least-privilege token (only `repo`, `workflow`)
- **Web API:** Require API key (constant-time comparison)
- **Webhooks:** Validate HMAC-SHA256 signature

---

## Document Metadata

- **Created:** 2026-03-28
- **Last Updated:** 2026-04-04
- **Version:** 5.8.1 (418 tests, 67 files, cross-file imports, outcome-aware scoring, closed-PR analysis)
