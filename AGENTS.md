# AI Agent Guide for ContribAI

> This document is designed for AI assistants (GitHub Copilot, Claude, Cursor, Coderabbit, etc.)
> scanning this repository. It provides structured context to help AI understand the codebase.

## What This Project Is

ContribAI is an **autonomous AI agent** that contributes to open source projects on GitHub.
It discovers repos, analyzes code, generates fixes, and submits pull requests — all without human intervention.

**It is NOT** a library/SDK, web app, or CLI tool intended for end-user consumption.
It is itself an AI agent that operates on other GitHub repositories.

> **v6.6.0 — Primary implementation is Rust** (`crates/contribai-rs/`).
> Python code is in `python/` (legacy v4.1.0, kept for reference).

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | **Rust 2021** (primary), Python 3.11+ (legacy `python/`) |
| Async | tokio (full), async/await throughout |
| HTTP | reqwest 0.12 (async, rustls) |
| Database | SQLite (rusqlite, bundled) |
| LLM | Google Gemini 3.x (primary), OpenAI, Anthropic, Ollama, Vertex AI |
| GitHub | REST API v3 + GraphQL (via reqwest) |
| Web | axum 0.7 + tower-http |
| TUI | ratatui + crossterm |
| CLI | clap v4 (derive) + dialoguer + colored |
| AST | tree-sitter (13 languages: Python, JS, TS, Go, Rust, Java, C, C++, Ruby, PHP, C#, HTML, CSS) |
| Tests | 602 tests (mockall, wiremock, tokio-test) |
| Lint | clippy + ruff (Python legacy) |

## Project Structure

```
ContribAI/
├── crates/contribai-rs/        ← PRIMARY: Rust v6.6.0
│   ├── src/
│   │   ├── main.rs             entry point
│   │   ├── lib.rs              library root
│   │   ├── cli/
│   │   │   ├── mod.rs          40+ commands + interactive menu
│   │   │   ├── tui.rs          ratatui TUI (interactive command)
│   │   │   ├── wizard.rs       setup wizard
│   │   │   └── config_editor.rs get/set/list config
│   │   ├── core/
│   │   │   ├── config.rs       ContribAIConfig (serde_yaml)
│   │   │   └── events.rs       18 typed events + JSONL log
│   │   ├── github/
│   │   │   ├── client.rs       REST + GraphQL client
│   │   │   └── discovery.rs    repo search
│   │   ├── analysis/
│   │   │   ├── analyzer.rs     7 analyzers (22 file extensions)
│   │   │   ├── ast_intel.rs    tree-sitter AST (13 languages)
│   │   │   ├── skills.rs       17 progressive skills
│   │   │   └── context_compressor.rs
│   │   ├── generator/
│   │   │   ├── engine.rs       code generation
│   │   │   └── scorer.rs       quality scoring
│   │   ├── llm/
│   │   │   ├── provider.rs     multi-provider LLM
│   │   │   └── agents.rs       sub-agent registry
│   │   ├── orchestrator/
│   │   │   ├── pipeline.rs     main pipeline
│   │   │   └── memory.rs       SQLite + working memory (72h TTL)
│   │   ├── pr/
│   │   │   ├── manager.rs      PR lifecycle
│   │   │   └── patrol.rs       review monitor
│   │   ├── issues/solver.rs    issue solving
│   │   ├── mcp/
│   │   │   ├── server.rs       21 MCP tools (stdio)
│   │   │   └── client.rs       MCP client
│   │   ├── web/mod.rs          axum dashboard API
│   │   ├── sandbox/sandbox.rs  Docker + ast fallback
│   │   └── tools/protocol.rs  tool interface
│   ├── Cargo.toml              v6.6.0
│   └── tests/                 418 Rust tests
│
├── python/                     LEGACY Python v4.1.0
│   ├── contribai/              Python package (importable as 'contribai')
│   └── tests/                 Python pytest tests
│
├── Cargo.toml                  workspace root (cargo build from here)
├── pyproject.toml              Python legacy package config
└── config.yaml.template        shared config template
```

## Architecture (v6.6.0)

### Core Pipeline
```
CLI → Pipeline → Middleware Chain → Analysis → Generation → PR → CI Monitor
```

### Key Patterns
1. **CLI (40+ commands)** — clap derive + dialoguer menu (`cli/mod.rs`)
2. **Interactive TUI** — ratatui 4-tab UI: Dashboard/PRs/Repos/Actions (`cli/tui.rs`)
3. **Middleware Chain** — 5 ordered middlewares (`orchestrator/pipeline.rs`)
4. **Progressive Skills** — 17 analysis skills loaded on-demand (`analysis/skills.rs`)
5. **Sub-Agent Registry** — 5 agents with parallel execution (`llm/agents.rs`)
6. **Tool Protocol** — MCP-inspired tool interface (`tools/protocol.rs`)
7. **Outcome Learning** — Tracks PR outcomes per-repo (`orchestrator/memory.rs`)
8. **Context Compression** — LLM-driven compression (`analysis/context_compressor.rs`)
9. **MCP Server** — 21 tools via stdio for Claude Desktop (`mcp/server.rs`)
10. **Event Bus** — 18 typed events + JSONL logging (`core/events.rs`)
11. **Working Memory** — Auto-load/save per repo, 72h TTL (`orchestrator/memory.rs`)
12. **Sandbox** — Docker validation + local fallback (`sandbox/sandbox.rs`)
13. **Web Dashboard** — axum REST API (`web/mod.rs`)
14. **GraphQL** — GitHub GraphQL alongside REST v3 (`github/client.rs`)
15. **Dream System** — Background memory consolidation into repo profiles (`orchestrator/memory.rs`)
16. **Risk Classification** — LOW/MEDIUM/HIGH change risk gating (`generator/risk.rs`)
17. **Cross-file Import Resolution** — 5-language 1-hop import resolution (`analysis/ast_intel.rs`)
18. **Outcome-Aware Scoring** — 8-check quality gate including repo outcome history (`generator/scorer.rs`)
19. **Closed-PR Analysis** — Patrol fetches review feedback for rejected PRs (`pr/patrol.rs`)

## Code Conventions (Rust)

| Convention | Standard |
|-----------|----|
| Naming | `snake_case` functions/vars, `PascalCase` structs/enums |
| Docs | `///` doc comments, module-level `//!` |
| Async | All I/O is `async fn` with tokio |
| Error handling | `anyhow::Result` for app code, `thiserror` for lib errors |
| Imports | `use` at top, group std/external/crate |
| Type hints | Full types, `Option<String>`, `Result<T, E>` |
| Line length | 100 chars (clippy) |
| Formatting | `cargo fmt` (rustfmt) |

## Common Patterns (Rust)

### LLM Calls
```rust
// All LLM calls go through LlmProvider::complete()
let response = self.llm.complete(&prompt, Some(&system)).await?;
```

### GitHub API Calls
```rust
// All GitHub API calls go through GitHubClient
let content = self.github.get_file_content(owner, repo, path).await?;
self.github.create_or_update_file(owner, repo, path, &content, &message).await?;
```

### Configuration
```rust
// All config loaded via ContribAIConfig::from_yaml()
let config = ContribAIConfig::from_yaml("config.yaml")?;
let token = &config.github.token;
let provider = &config.llm.provider;
```

### Memory / Persistence
```rust
// SQLite via rusqlite — sync, bundled
let memory = Memory::open(&db_path)?;
memory.record_outcome(repo, pr_num, &url, "security_fix", "merged")?;
let prefs = memory.get_repo_preferences(repo)?;

// Working memory — 72h TTL per repo
memory.store_context(repo, "analysis_summary", &summary, 72)?;
let cached = memory.get_context(repo, "analysis_summary")?;
```

### CLI Command Handler Pattern
```rust
// Add to Commands enum in cli/mod.rs
MyCommand { arg: String },

// Add handler in Cli::run()
Commands::MyCommand { arg } => run_my_command(&arg, self.config.as_deref()).await,

// Implement handler
async fn run_my_command(arg: &str, config_path: Option<&str>) -> anyhow::Result<()> {
    print_banner();
    let config = load_config(config_path)?;
    // ...
    Ok(())
}
```

## CLI Commands (40+ total)

| Command | Handler | Description |
|---------|---------|-------------|
| `run` | `run_run()` | Auto-discover repos, submit PRs |
| `hunt` | `run_hunt()` | Aggressive multi-round discovery |
| `patrol` | `run_patrol()` | Monitor open PRs |
| `target` | `run_target()` | Target specific repo |
| `analyze` | `run_analyze()` | Dry-run analysis |
| `solve` | `run_solve()` | Solve GitHub issues |
| `stats` | `run_stats()` | Contribution stats |
| `status` | `run_status()` | PR status |
| `leaderboard` | `run_leaderboard()` | Merge rates by repo |
| `models` | `run_models()` | Available LLM models |
| `templates` | `run_templates()` | Contribution templates |
| `profile` | `run_profile()` | Named config profiles |
| `cleanup` | `run_cleanup()` | Delete merged forks |
| `notify-test` | `run_notify_test()` | Real HTTP to Slack/Discord/Telegram |
| `system-status` | `run_system_status()` | DB, rate limits, scheduler |
| `interactive` | `tui::run_interactive_tui()` | ratatui TUI browser |
| `web-server` | `run_web_server()` | axum dashboard |
| `schedule` | `run_schedule()` | Cron scheduler |
| `mcp-server` | `run_mcp_server()` | MCP stdio server |
| `init` | `wizard::run_wizard()` | Setup wizard |
| `login` | `run_login_check()` | Interactive auth & provider config |
| `dream` | `run_dream()` | Memory consolidation into repo profiles |
| `config-get/set/list` | `config_editor::*` | YAML config editor |
| `doctor` | `run_doctor()` | System health diagnostics |

## Testing

```bash
# From project root (Rust workspace):
cargo test                          # 602 tests
cargo test -- --nocapture           # with stdout
cargo test cli::                    # CLI tests only
cargo build --release               # production binary
cargo install --path crates/contribai-rs  # install to PATH

# Legacy Python tests:
cd python && pytest tests/ -v       # 400+ pytest tests
```

## Environment Variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `GITHUB_TOKEN` | Yes | GitHub API authentication |
| `GEMINI_API_KEY` | Yes* | Google Gemini LLM |
| `OPENAI_API_KEY` | Alt | OpenAI alternative |
| `ANTHROPIC_API_KEY` | Alt | Anthropic alternative |
| `GOOGLE_CLOUD_PROJECT` | Opt | Vertex AI project |

## File Organization Rules

- **Code files only**: ContribAI modifies `.py`, `.js`, `.ts`, `.go`, `.rs`, `.java`, `.rb`, `.php`, `.cs`, `.swift`, `.kt` etc.
- **Never modify**: `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `.github/FUNDING.yml`
- **Skip extensions**: `.md`, `.yaml`, `.json`, `.toml`, `.cfg`, `.ini`
- **Protected meta files**: Any governance/meta files are off-limits

## Known Limitations

1. Sandbox execution is opt-in (`sandbox.enabled = true`) — defaults to `ast.parse` fallback
2. Single-repo PRs only — no cross-repo changes
3. Rate limited by GitHub API (5000 req/hour authenticated)
4. Context window managed by `ContextCompressor` (default 30k tokens)
5. Windows: Vertex AI uses `cmd /c gcloud` for token fetch
