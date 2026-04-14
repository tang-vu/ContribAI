# Contributing to ContribAI

Thank you for your interest in contributing to ContribAI! 🎉

## 🚀 Quick Start

```bash
# Clone & build (Rust — primary)
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI
cargo build --release
cargo install --path crates/contribai-rs

# Verify
cargo test              # 602 tests must pass
contribai --help        # shows 50+ commands
```

> **Legacy Python** is in `python/` (v4.1.0, reference only).  
> If working on Python legacy: `cd python && pip install -e ".[dev]" && pytest tests/ -v`

## 📋 Development Workflow

1. **Create a branch** from `main`:
   ```bash
   git checkout -b feat/your-feature
   ```

2. **Write Rust code** following our standards:
   - All I/O is `async fn` with tokio
   - `anyhow::Result` for app code, `thiserror` for lib errors
   - `snake_case` functions/vars, `PascalCase` structs/enums
   - `///` doc comments on all public items
   - Lines max 100 chars (clippy enforced)

3. **Write tests** co-located with source (`#[cfg(test)] mod tests`)

4. **Lint & format**:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```

5. **Run tests**:
   ```bash
   cargo test              # 602 tests
   cargo test -- --nocapture   # with stdout
   ```

6. **Commit** with conventional messages:
   ```bash
   git commit -m "feat: add Django security analyzer"
   ```
   Valid prefixes: `feat`, `fix`, `refactor`, `docs`, `test`, `perf`, `chore`

7. **Push & create PR** using the PR template

## 🏗️ Project Structure (v5.1.0)

| Directory | Purpose |
|-----------|---------|
| `crates/contribai-rs/src/cli/` | 22 commands + ratatui TUI |
| `crates/contribai-rs/src/core/` | Config, models, middleware, events |
| `crates/contribai-rs/src/analysis/` | 7 analyzers + 17 progressive skills |
| `crates/contribai-rs/src/llm/` | Multi-provider LLM + 5 sub-agents |
| `crates/contribai-rs/src/github/` | GitHub REST + GraphQL client |
| `crates/contribai-rs/src/generator/` | Code generation + self-review + scorer |
| `crates/contribai-rs/src/pr/` | PR lifecycle + patrol |
| `crates/contribai-rs/src/orchestrator/` | Pipeline + SQLite memory (72h TTL) |
| `crates/contribai-rs/src/mcp/` | 21-tool MCP server (stdio JSON-RPC) |
| `crates/contribai-rs/src/web/` | axum dashboard + webhooks |
| `crates/contribai-rs/tests/` | 335 unit tests |
| `python/` | Legacy v4.1.0 (reference only) |
| `docs/` | Architecture documentation |
| `.agents/workflows/` | Development workflows |

## 🔑 Key Architecture Patterns

1. **Middleware Chain** — 5 ordered middlewares: RateLimit → Validation → Retry → DCO → QualityGate
2. **Progressive Skills** — 17 analysis skills loaded on-demand by language/framework
3. **Sub-Agent Registry** — 5 agents with parallel execution
4. **Tool Protocol** — MCP-inspired interface for GitHub/LLM tools
5. **Outcome Learning** — Tracks PR merge/rejection per repo (SQLite)
6. **Context Compression** — LLM-driven compression with 30k token budget
7. **Interactive TUI** — ratatui 4-tab browser (Dashboard/PRs/Repos/Actions)

See [AGENTS.md](AGENTS.md) for full architecture and code patterns.

## 🛠 Developer Guide

### Adding a New CLI Command

1. **Add to `Commands` enum** in `src/cli/mod.rs`:
```rust
MyCommand {
    /// Optional argument
    #[arg(long)]
    my_arg: Option<String>,
},
```

2. **Add handler** in `run()` method:
```rust
Commands::MyCommand { my_arg } => {
    commands::my_command::run_my_command(self.config.as_deref(), my_arg).await
}
```

3. **Create handler file** in `src/cli/commands/my_command.rs`:
```rust
use crate::cli::{load_config, print_banner};
use console::style;

pub fn run_my_command(config_path: Option<&str>, my_arg: Option<String>) -> anyhow::Result<()> {
    print_banner();
    let config = load_config(config_path)?;
    
    println!("{}", style("My Command").cyan().bold());
    // ... implementation
    Ok(())
}
```

4. **Export from** `src/cli/commands/mod.rs`:
```rust
pub mod my_command;
```

### Adding Tests

1. **Unit tests** — co-locate with source:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        assert_eq!(my_function(), expected);
    }
}
```

2. **Integration tests** — add file in `tests/`:
```rust
// tests/my_feature.rs
use contribai::my_module;

#[test]
fn test_integration() {
    // Uses mock GitHub or mock LLM
}
```

3. **Run your test**:
```bash
cargo test my_feature       # Run specific test file
cargo test                  # Run all 602 tests
```

### Adding an Analyzer

1. **Add analysis rule** in `src/analysis/language_rules.rs`
2. **Extend triage engine** in `src/analysis/triage.rs`
3. **Add test** in `tests/` with mock LLM responses

### Code Style

- **Async all the way**: Use `async fn` for all I/O
- **Error handling**: `anyhow::Result` in CLI/handlers, `thiserror` in library code
- **Naming**: `snake_case` functions, `PascalCase` types
- **Docs**: `///` on all public items
- **Line length**: Max 100 chars
- **Format**: Always `cargo fmt` before committing
- **Clippy**: Zero warnings allowed (`-D warnings`)

## 🤖 AI Agent Guide

If you're an AI assistant scanning this repo, see [AGENTS.md](AGENTS.md) for structured context.

## ✅ PR Requirements

- All CI checks pass (clippy, tests)
- Tests for new/changed code
- Documentation updated (if user-facing)
- Follows conventional commits
- CHANGELOG.md updated (if applicable)
