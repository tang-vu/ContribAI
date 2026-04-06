# System Architecture

**Version:** 5.8.1 | **Language:** Rust | **Last Updated:** 2026-04-05

---

## High-Level Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│                    ContribAI Pipeline (v5.8.1 Rust)             │
└─────────────────────────────────────────────────────────────────┘

Input: GitHub Repository (URL or discovery)
   ▼
┌─────────────────────────────────────────────────────────────────┐
│ 1. DISCOVERY                                                    │
│ ├─ GitHub Search API (language, stars, activity)               │
│ ├─ GraphQL search for advanced queries                         │
│ ├─ Hunt Mode: Multi-round discovery with watchlist + rotation   │
│ ├─ Issue-driven: Fetch open issues from repo                   │
│ ├─ 12-signal triage scoring (Rust-only)                        │
│ └─ Duplicate check: Skip if already analyzed                   │
└────────────────────────┬────────────────────────────────────────┘
   ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. MIDDLEWARE CHAIN (5 middlewares)                              │
│ ├─ RateLimitMiddleware: Check daily PR limit + API rate        │
│ ├─ ValidationMiddleware: Validate repo data exists             │
│ ├─ RetryMiddleware: 2 retries with exponential backoff         │
│ ├─ DCOMiddleware: Compute Signed-off-by signature              │
│ └─ QualityGateMiddleware: Score check (min 0.6/1.0)           │
└────────────────────────┬────────────────────────────────────────┘
   ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. ANALYSIS                                                     │
│ ├─ Language/Framework detection                                │
│ ├─ Progressive skill loading (17 skills, on-demand)            │
│ ├─ Tree-sitter AST parsing (13 languages, Rust-only)          │
│ ├─ PageRank file importance ranking (Rust-only)                │
│ ├─ 3-tier context compression with signature extraction        │
│ ├─ 7 Multi-strategy analyzers (parallel via tokio):            │
│ │  ├─ SecurityStrategy (hardcoded secrets, SQL injection, XSS) │
│ │  ├─ CodeQualityStrategy (dead code, error handling)          │
│ │  ├─ PerformanceStrategy (N+1 queries, blocking calls)        │
│ │  ├─ DocumentationStrategy (missing docstrings, READMEs)      │
│ │  ├─ UIUXStrategy (accessibility, responsive design)          │
│ │  ├─ RefactoringStrategy (unused imports, complexity)         │
│ │  └─ FrameworkStrategy (Django/Flask/FastAPI/React/Express)   │
│ ├─ Deep validation: LLM validates findings against file context│
│ └─ Result: Vec<Finding> with severity + description            │
└────────────────────────┬────────────────────────────────────────┘
   ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. GENERATION                                                   │
│ ├─ For each finding:                                           │
│ │  ├─ LLM generates code fix (with retry on failure)           │
│ │  ├─ Self-review: LLM validates own fix                       │
│ │  ├─ Quality scoring: 8-check gate (7 code + 1 outcome history)│
│ │  ├─ Risk classification: Low/Medium/High for auto-submit     │
│ │  ├─ Syntax validation (balanced brackets, no-op detection)   │
│ │  ├─ Fuzzy matching for duplicate detection                   │
│ │  └─ Result: Contribution with confidence score               │
│ ├─ Cross-file detection: Find same pattern across files        │
│ └─ Filter: Keep only score >= 0.6                              │
└────────────────────────┬────────────────────────────────────────┘
   ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. PR CREATION (Unless dry-run)                                │
│ ├─ Fork repository (or use existing fork)                      │
│ ├─ Create feature branch (naming: contribai/finding-type-repo) │
│ ├─ Commit changes with DCO signoff                             │
│ ├─ Create PR with detailed description                         │
│ ├─ Auto-sign CLA if required (CLA-Assistant, EasyCLA)          │
│ ├─ Record PR in memory (submitted_prs table)                   │
│ └─ Result: PR URL + number                                     │
└────────────────────────┬────────────────────────────────────────┘
   ▼
┌─────────────────────────────────────────────────────────────────┐
│ 6. POST-PROCESSING                                              │
│ ├─ Event emission (PRCreated, PipelineCompleted)               │
│ ├─ JSONL event logging (~/.contribai/events.jsonl)             │
│ ├─ Notification dispatch (Slack, Discord, Telegram)            │
│ ├─ Memory update (record outcomes, dream consolidation)        │
│ ├─ PR Patrol monitoring (async, background, conversation-aware)│
│ ├─ Closed-PR failure analysis (review + CI feedback → memory)  │
│ └─ CI status tracking (auto-close 404 PRs on failure)          │
└────────────────────────┬────────────────────────────────────────┘
   ▼
Output: PipelineResult { repos_analyzed, prs_created, findings_count }
```

---

## Middleware Chain

5 middlewares wrap the core processing loop in order:

| Order | Middleware | Purpose | Example Decision |
|-------|-----------|---------|------------------|
| 1 | `RateLimitMiddleware` | Check daily limits + API rate | Skip if PR count >= 15/day |
| 2 | `ValidationMiddleware` | Validate repo structure exists | Skip if no src dir found |
| 3 | `RetryMiddleware` | Auto-retry on transient failure | Retry on 502/503/504 (2x) |
| 4 | `DCOMiddleware` | Compute Signed-off-by | Add to every commit |
| 5 | `QualityGateMiddleware` | Min quality score threshold | Skip if avg score < 0.6 |

```rust
// Middleware trait
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn process(
        &self,
        repo: &Repository,
        next: &dyn Fn(&Repository) -> BoxFuture<Result<PipelineResult>>,
    ) -> Result<PipelineResult>;
}
```

---

## Sub-Agent Registry

5 specialized agents with parallel execution via Tokio:

| Agent | Role | Wraps | Max Concurrent |
|-------|------|-------|----------------|
| `AnalyzerAgent` | Code analysis | `CodeAnalyzer` | 3 |
| `GeneratorAgent` | Fix generation | `ContributionGenerator` | 3 |
| `PatrolAgent` | PR monitoring | `PRPatrol` | 1 |
| `ComplianceAgent` | CLA/DCO/CI | `PRManager` | 3 |
| `IssueAgent` | Issue solving | `IssueSolver` | 2 |

```rust
// Parallel execution with tokio
let (analysis, generation) = tokio::join!(
    analyzer_agent.analyze(&repo),
    generator_agent.generate(&findings),
);

// Concurrency control with semaphore
let semaphore = Arc::new(Semaphore::new(3));
let tasks: Vec<_> = repos.iter().map(|repo| {
    let permit = semaphore.clone().acquire_owned().await?;
    tokio::spawn(async move {
        let result = pipeline.process_repo(repo).await;
        drop(permit);
        result
    })
}).collect();
```

---

## Event Bus System

18 typed events with async subscribers and JSONL file logging.

### Event Types

```rust
pub enum Event {
    // Discovery
    RepositoryDiscovered { repo: String, timestamp: DateTime<Utc> },
    // Analysis
    RepositoryAnalyzed { repo: String, findings_count: usize, timestamp: DateTime<Utc> },
    FindingDetected { repo: String, finding_type: String, severity: String },
    // Generation
    ContributionGenerated { finding_type: String, confidence: f64 },
    CodeChangeGenerated { repo: String, file: String },
    // PR lifecycle
    PRCreated { repo: String, pr_number: u64, url: String, timestamp: DateTime<Utc> },
    PRMerged { repo: String, pr_number: u64, time_to_merge_hours: f64 },
    PRClosed { repo: String, pr_number: u64, reason: String },
    // Patrol
    PRPatrolStarted { repo: String, open_pr_count: usize },
    ReviewFound { repo: String, pr_number: u64, review_state: String },
    // System
    ConfigLoaded { config_file: String },
    PipelineStarted { mode: String, repo_count: usize },
    PipelineCompleted { status: String, repos_processed: usize, prs_created: usize },
    ErrorOccurred { error: String, module: String },
    RateLimitExceeded { service: String, reset_time: u64 },
    IssueFound { repo: String, issue_number: u64 },
    SchedulerStarted { cron: String },
    WebhookReceived { event_type: String, repo: String },
}
```

### JSONL Logging

Events automatically append to `~/.contribai/events.jsonl`:

```json
{"event":"PRCreated","repo":"owner/name","pr_number":42,"url":"...","timestamp":"2026-03-31T10:00:00Z"}
```

---

## LLM Routing & Multi-Model Support

### Provider Architecture

```
┌─────────────────┐
│  LlmConfig      │
│ (provider, key, │
│  model, temp)   │
└────────┬────────┘
         ▼
┌─────────────────────────────┐
│ LlmProvider trait (dyn)     │
└────────┬────────────────────┘
         │
    ┌────┴────┬────────┬──────────┐
    ▼         ▼        ▼          ▼
┌────────┐┌────────┐┌────────┐┌────────┐
│Gemini  ││OpenAI  ││Anthropic│Ollama  │
│Provider││Provider││Provider ││Provider│
└────────┘└────────┘└────────┘└────────┘
    │         │        │          │
    └─────────┴────────┴──────────┘
              │
         ┌────▼────────┐
         │ TaskRouter  │
         │ (Route by   │
         │  strategy)  │
         └─────┬───────┘
               │
   ┌───────────┼───────────┐
   ▼           ▼           ▼
Economy    Balanced    Performance
(fast)     (mid-tier)  (powerful)
```

### Task Routing Strategies

| Strategy | Model Selection | Use Case |
|----------|-----------------|----------|
| **Economy** | Cheapest + fastest (Gemini Flash) | Triage, classification |
| **Balanced** | Mid-tier model (Gemini Pro) | Code generation, analysis |
| **Performance** | Most capable (GPT-4, Claude) | Complex generation, review |

### Token-Aware Context Management

- **Budget per analysis:** 30,000 tokens
- **3-tier compression:** Full → Signatures → Summary
- **5-language signature extraction:** Rust, Python, JS/TS, Go, Java

---

## Memory & Persistence Layer

### SQLite Schema (7 Tables)

```sql
CREATE TABLE analyzed_repos (
    id INTEGER PRIMARY KEY, repo_id TEXT UNIQUE,
    owner TEXT, name TEXT, url TEXT, language TEXT,
    last_analyzed TEXT, findings_count INTEGER, status TEXT
);
CREATE TABLE submitted_prs (
    id INTEGER PRIMARY KEY, repo_id TEXT, pr_number INTEGER,
    url TEXT, title TEXT, status TEXT, created_at TEXT, merged_at TEXT
);
CREATE TABLE findings_cache (
    id INTEGER PRIMARY KEY, repo_id TEXT,
    findings_json TEXT, timestamp TEXT, ttl_expires TEXT
);
CREATE TABLE run_log (
    id INTEGER PRIMARY KEY, timestamp TEXT, status TEXT,
    repos_analyzed INTEGER, prs_created INTEGER, errors_count INTEGER
);
CREATE TABLE pr_outcomes (
    id INTEGER PRIMARY KEY, repo_id TEXT, pr_number INTEGER,
    outcome TEXT, feedback TEXT, time_to_close_hours REAL
);
CREATE TABLE repo_preferences (
    id INTEGER PRIMARY KEY, repo_id TEXT UNIQUE,
    preferred_types TEXT, rejected_types TEXT, merge_rate REAL, avg_review_hours REAL
);
CREATE TABLE ci_monitor (
    id INTEGER PRIMARY KEY, repo_id TEXT, pr_number INTEGER,
    ci_status TEXT, last_checked TEXT
);
```

### Async Database Access

```rust
// rusqlite is sync; wrapped with spawn_blocking
let stats = tokio::task::spawn_blocking(move || {
    let conn = Connection::open(&db_path)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM submitted_prs WHERE status = 'merged'",
        [], |row| row.get(0),
    )?;
    Ok(count)
}).await??;
```

---

## MCP Server (Model Context Protocol)

Claude Desktop integration via stdio JSON-RPC. 21 exposed tools.

### Tool Categories

**GitHub Read (7 tools):**
- `search_repos` — Search GitHub by language/stars
- `get_repo_info` — Fetch repo metadata
- `get_file_tree` — List repo structure
- `get_file_content` — Read file contents
- `get_open_issues` — List open issues
- `get_pr_reviews` — Get PR review list
- `get_pr_comments` — Get PR comment thread

**GitHub Write (4 tools):**
- `fork_repo` — Fork a repository
- `create_branch` — Create feature branch
- `push_file_change` — Commit changes
- `create_pr` — Create pull request

**PR Management (3 tools):**
- `add_pr_review_comment` — Reply to review comment
- `dismiss_review` — Dismiss a PR review
- `sign_cla` — CLA signing (handled by patrol)

**Safety (2 tools):**
- `check_duplicate_pr` — Detect if PR already exists
- `check_ai_policy` — Check if repo bans AI contributions

**Maintenance (3 tools):**
- `patrol_prs` — Monitor open PRs for feedback
- `cleanup_forks` — Remove stale forks
- `get_stats` — Return overall statistics

**Identity (2 tools):**
- `get_authenticated_user` — Current GitHub user info
- `get_branch_info` — Branch details

---

## Web Server Architecture

### Axum-Based Dashboard

```rust
let app = Router::new()
    .route("/", get(dashboard))
    .route("/api/stats", get(api_stats))
    .route("/api/repos", get(api_repos))
    .route("/api/run", post(api_run))           // API key required
    .route("/api/run/target", post(api_target))  // API key required
    .route("/api/webhooks/github", post(github_webhook))  // HMAC-SHA256
    .route("/api/health", get(health))
    .with_state(app_state);
```

### Security

- **API Key Auth:** Constant-time comparison (`verify_api_key`)
- **Webhook Verification:** HMAC-SHA256 signature (`X-Hub-Signature-256`)
- **State:** `AppState { memory, config, api_keys, webhook_secret }`

---

## Configuration Structure

```yaml
github:
  token: "ghp_..."
  max_prs_per_day: 15
  rate_limit_margin: 100

llm:
  provider: "gemini"           # gemini | openai | anthropic | ollama
  model: "gemini-3-flash-preview"
  api_key: "..."
  # base_url: "https://..."    # Optional: override default API endpoint for compatible providers
  temperature: 0.5
  max_tokens: 2000

discovery:
  languages: ["python", "javascript"]
  stars_range: [100, 5000]
  min_activity_days: 180

analysis:
  enabled_analyzers: [security, code_quality, performance, documentation, ui_ux, refactoring]
  max_file_size_kb: 50

pipeline:
  concurrent_repos: 3
  retry_attempts: 2
  timeout_seconds: 300

web:
  api_keys: ["key1", "key2"]
  webhook_secret: "github-secret"

notifications:
  slack: "https://hooks.slack.com/..."
  discord: "https://discord.com/api/webhooks/..."
  telegram: "https://api.telegram.org/bot..."
```

---

## Error Handling Strategy

### Error Types (thiserror)

```rust
#[derive(Debug, thiserror::Error)]
pub enum ContribAIError {
    #[error("Analysis error: {0}")]
    Analysis(String),
    #[error("Generation error: {0}")]
    Generation(String),
    #[error("GitHub API error: {0}")]
    GitHub(String),
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Failure Recovery

| Error Type | Handling | Recovery |
|-----------|----------|----------|
| **GitHub 5xx** | Log warning | Retry up to 2x with backoff |
| **LLM timeout** | Log error | Retry with shorter context |
| **Rate limit** | Log warning | Skip repo, continue to next |
| **Invalid config** | Log error | Fail fast with descriptive message |
| **Database error** | Log error | Crash & restart (systemd) |

---

## Dependency Flow

```
┌──────────────────────────────────────────────────┐
│              CLI / Web / Scheduler               │
│          (clap, axum, tokio-cron)                │
└──────────────────┬───────────────────────────────┘
                   │
        ┌──────────┴──────────┐
        ▼                     ▼
   ┌──────────┐         ┌──────────────┐
   │Orchestrator│       │ Agents       │
   │(Pipeline,  │       │(Registry with│
   │Hunt,Memory)│       │ 4 sub-agents)│
   └──────┬─────┘       └──────┬───────┘
          │                    │
    ┌─────┴────┬────┬──────────┤
    ▼          ▼    ▼          ▼
┌────────┐┌─────────┐┌──┐┌─────────┐
│Analysis││Generator ││PR││ Issues  │
│+Triage ││+Scorer   ││Mgr│ Solver │
└───┬────┘└────┬────┘└─┬┘└────┬────┘
    │          │       │      │
    └──────────┼───────┴──────┘
               │
        ┌──────┴──────┐
        ▼             ▼
    ┌────────┐   ┌──────────┐
    │  LLM   │   │  GitHub  │
    │Provider│   │  Client  │
    └────┬───┘   └────┬─────┘
         │             │
         └─────┬───────┘
               ▼
         ┌──────────────┐
         │   CORE       │
         │ (Config,     │
         │  Models,     │
         │  Events,     │
         │  Middleware,  │
         │  Errors)     │
         └──────────────┘
```

**All arrows point downward (acyclic dependency graph).**

---

## Document Metadata

- **Created:** 2026-03-28
- **Last Updated:** 2026-04-04
- **Version:** 5.8.1 (Closed-PR analysis, outcome-aware scoring, cross-file imports, 418 tests)
