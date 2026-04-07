# ContribAI Architecture

> v5.15.0 — For developers and maintainers.

## System Overview

ContribAI is an autonomous AI agent that discovers GitHub repositories, analyzes code, generates fixes, and submits pull requests — all without human intervention.

```
┌─────────────────────────────────────────────────────────────┐
│                        CLI (clap)                           │
│   run | hunt | patrol | target | analyze | solve | ...      │
└──────────────────────┬──────────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────────┐
│                  ContribPipeline                              │
│  ┌──────────┐  ┌────────────┐  ┌──────────┐  ┌───────────┐ │
│  │Discovery │→ │Analysis    │→ │Generation│→ │PR Manager │ │
│  │          │  │(AST+LLM)   │  │          │  │           │ │
│  └──────────┘  └────────────┘  └──────────┘  └───────────┘ │
│       │              │               │              │       │
│  ┌────▼────┐   ┌─────▼─────┐   ┌────▼────┐   ┌────▼────┐  │
│  │Middleware│   │Skills     │   │Risk     │   │Patrol   │  │
│  │Chain     │   │Context    │   │Scoring  │   │Monitor  │  │
│  └─────────┘   └───────────┘   └─────────┘   └─────────┘  │
└───────┬───────────┬───────────┬──────────────────┬─────────┘
        │           │           │                  │
   ┌────▼──┐  ┌─────▼───┐ ┌────▼─────┐    ┌───────▼───────┐
   │GitHub │  │tree-    │ │Quality   │    │Memory (SQLite)│
   │REST/  │  │sitter   │ │Scorer    │    │+ Dream System │
   │GraphQL│  │(13 langs│ │+Circuit  │    │(72h TTL)      │
   │       │  │ )       │ │Breaker   │    │               │
   └───────┘  └─────────┘ └──────────┘    └───────────────┘
```

## Core Pipeline

The main pipeline flow is:

1. **Discovery** — Search GitHub for repos matching criteria (stars, language, activity)
2. **Analysis** — Fetch file tree, extract AST symbols (tree-sitter), run LLM analyzers in parallel
3. **Generation** — For each finding, generate code changes via LLM with search/replace format
4. **PR Creation** — Fork repo, create branch, commit changes, submit PR with CLA auto-sign
5. **Patrol** — Monitor open PRs for review feedback, auto-fix if possible

### Middleware Chain

Every repo passes through 5 middlewares (ordered):

| # | Middleware | Purpose |
|---|-----------|---------|
| 1 | `RateLimit` | Check daily PR limit (`max_prs_per_day`) |
| 2 | `Validation` | Verify repo data is valid |
| 3 | `Retry` | Wrap processing with retry logic |
| 4 | `DCO` | Developer Certificate of Origin signoff |
| 5 | `QualityGate` | Block if quality score < threshold |

## Key Components

### Code Analysis (`analysis/`)

- **`analyzer.rs`** — Main orchestrator: fetches files, runs AST + LLM analysis in parallel
- **`ast_intel.rs`** — tree-sitter AST parsing for 13 languages (Python, JS, TS, Go, Rust, Java, C, C++, Ruby, PHP, C#, HTML, CSS)
- **`skills.rs`** — 17 progressive skills loaded on-demand by language/framework
- **`context_compressor.rs`** — Token budget management for LLM context window
- **`repo_map.rs`** — PageRank-based file importance scoring from import graphs
- **`triage.rs`** — Weighted triage scoring for findings

### Code Generation (`generator/`)

- **`engine.rs`** — LLM-powered code generation with search/replace format
- **`scorer.rs`** — Quality scoring with 8 checks (debug code, no-changes, etc.)
- **`risk.rs`** — Change risk classification (LOW/MEDIUM/HIGH)

### LLM Integration (`llm/`)

- **`provider.rs`** — Multi-provider LLM: Gemini (default), OpenAI, Anthropic, Ollama, Vertex AI
- **`cache.rs`** — Content-addressable SHA-256 response cache (7-day TTL)
- **`router.rs`** — Task routing: Analysis/CodeGen/Review → optimal model
- **`agents.rs`** — Sub-agent registry (Analyzer, CodeGen, Reviewer, DocsWriter, Planner)

### GitHub Integration (`github/`)

- **`client.rs`** — Full REST API v3 client with rate limiting + retry
- **`discovery.rs`** — Repository search and filtering
- **`guidelines.rs`** — Fetch and parse CONTRIBUTING.md, PR templates

### Persistence (`orchestrator/`)

- **`memory.rs`** — SQLite persistence (9 tables):
  - `analyzed_repos`, `submitted_prs`, `findings_cache`, `run_log`
  - `pr_outcomes`, `repo_preferences`, `working_memory` (72h TTL)
  - `dream_meta`, `pr_conversations`
- **`circuit_breaker.rs`** — Circuit breaker for LLM failures (Closed → Open → HalfOpen)
- **`dream_lock.rs`** — Mutex-based lock for dream consolidation (TOCTOU-safe)
- **`pipeline.rs`** — Main pipeline orchestrator

### PR Management (`pr/`)

- **`manager.rs`** — Fork, branch, commit, PR creation, CLA auto-sign
- **`patrol.rs`** — Review feedback monitor, auto-fix, auto-reply

### CLI (`cli/`)

- **`mod.rs`** — Command definitions (clap derive, 40+ commands)
- **`commands/`** — 27 command handler files (one per command)
- **`common.rs`** — Shared helpers (load_config, create_github, etc.)
- **`tui.rs`** — ratatui interactive TUI (4 tabs: Dashboard/PRs/Repos/Actions)
- **`wizard.rs`** — Interactive setup wizard
- **`config_editor.rs`** — YAML config get/set/list

### Supporting Systems

| System | File | Purpose |
|--------|------|---------|
| **MCP Server** | `mcp/server.rs` | 21 JSON-RPC tools over stdio for Claude Desktop |
| **Web Dashboard** | `web/mod.rs` | axum REST API + HTML dashboard on configurable port |
| **Sandbox** | `sandbox/mod.rs` | Docker syntax validation + local/AST fallback |
| **Event Bus** | `core/events.rs` | 18 typed events + JSONL file logging |

## Data Model

### Core Entities (`core/models.rs`)

```rust
Repository        // GitHub repo metadata
Finding           // Analysis result (severity, file_path, suggestion)
Contribution      // Generated code changes (FileChange[], commit msg)
Symbol            // AST-extracted code symbol (function, class, etc.)
PrResult          // PR outcome (merged, closed, rejected)
```

### Events (`core/events.rs`)

18 typed events logged to JSONL:
`PipelineStart`, `PipelineComplete`, `AnalysisStart`, `AnalysisComplete`,
`GenerationStart`, `GenerationComplete`, `PrCreated`, `PrMerged`, `PrClosed`,
`HuntRoundStart`, `HuntRoundComplete`, `PatrolStart`, `PatrolComplete`,
`CircuitBreakerOpen`, `CircuitBreakerClosed`, `DreamStart`, `DreamComplete`,
`Error`

## Configuration

Config loaded from `config.yaml` with env var fallback. Key sections:

```yaml
github:
  token: ""                    # or GITHUB_TOKEN env
  max_prs_per_day: 5
llm:
  provider: "gemini"           # gemini | openai | anthropic | ollama
  model: "gemini-3-flash-preview"
  cache_enabled: true          # LLM response cache
  cache_ttl_days: 7
analysis:
  enabled_analyzers: [security, code_quality, performance]
  max_context_tokens: 30000
pipeline:
  min_quality_score: 0.6
  risk_tolerance: "medium"     # low | medium | high
  circuit_breaker_failure_threshold: 5
  circuit_breaker_cooldown_secs: 300
sandbox:
  enabled: true                # ON by default
  mode: "local"                # docker | local | ast | off
  require_validation: true
web:
  tls_enabled: false
  tls_cert_path: ""
  tls_key_path: ""
```

## Adding a New CLI Command

1. Define the command variant in `cli/mod.rs`:
   ```rust
   #[derive(Subcommand)]
   enum Commands {
       /// Description
       MyCommand {
           #[arg(long)]
           my_arg: Option<String>,
       },
   }
   ```
2. Create `cli/commands/my_command.rs`:
   ```rust
   pub async fn run_my_command(
       config_path: Option<&str>,
       my_arg: Option<String>,
   ) -> anyhow::Result<()> {
       // implementation
   }
   ```
3. Add handler in `Cli::run()` match:
   ```rust
   Commands::MyCommand { my_arg } => {
       commands::my_command::run_my_command(self.config.as_deref(), my_arg).await
   }
   ```
4. Register in `cli/commands/mod.rs`:
   ```rust
   pub mod my_command;
   ```

## Adding a New LLM Provider

1. Implement `LlmProvider` trait in `llm/provider.rs`:
   ```rust
   pub struct MyProvider { /* fields */ }
   
   #[async_trait]
   impl LlmProvider for MyProvider {
       async fn complete(&self, ...) -> Result<String> { /* ... */ }
       async fn chat(&self, ...) -> Result<String> { /* ... */ }
   }
   ```
2. Add to `create_llm_provider()` factory:
   ```rust
   "myprovider" => Ok(Box::new(MyProvider::new(config)?)),
   ```
3. The cache layer wraps automatically if `llm.cache_enabled: true`.

## Adding a New Analysis Skill

1. Add skill to `analysis/skills.rs`:
   ```rust
   "my_skill" => vec!["keyword1", "keyword2"],
   ```
2. Skills are matched against repo language + detected frameworks.
3. The skill's keywords are injected into the LLM analysis prompt.

## Database Schema

See `orchestrator/memory.rs` for the full schema. 9 tables:

| Table | Purpose | Key Columns |
|-------|---------|-------------|
| `analyzed_repos` | Track analyzed repos | `full_name`, `language`, `stars`, `findings` |
| `submitted_prs` | Track submitted PRs | `repo`, `pr_number`, `status`, `title` |
| `findings_cache` | Deduplicate findings | `id`, `repo`, `type`, `severity` |
| `run_log` | Pipeline run history | `started_at`, `repos_analyzed`, `prs_created` |
| `pr_outcomes` | PR merge/rejection tracking | `repo`, `pr_number`, `outcome`, `pr_type` |
| `repo_preferences` | Auto-computed per-repo prefs | `repo`, `preferred_types`, `merge_rate` |
| `working_memory` | Hot context with TTL | `repo`, `key`, `value`, `expires_at` |
| `dream_meta` | Dream system state | `key`, `value`, `updated_at` |
| `pr_conversations` | PR conversation threads | `repo`, `pr_number`, `role`, `body` |

## Security Model

- **Prompt Injection**: Repository content is sanitized before LLM calls — control characters stripped, XML-wrapped, 10 known injection patterns detected
- **Token Encryption**: GitHub tokens can be encrypted with `contribai encrypt-token` (HMAC-SHA256 PBKDF2, 1000 iterations)
- **Sandbox Validation**: Code is validated via AST parsing before PR submission
- **Circuit Breaker**: Pipeline stops after N consecutive LLM failures to save API quota
- **API Key Auth**: Web dashboard requires `X-API-Key` header (optional)
- **Webhook Verification**: GitHub webhooks verified via HMAC-SHA256
