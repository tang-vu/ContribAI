<div align="center">

# ContribAI

**Autonomous AI agent that discovers, analyzes, and submits<br>Pull Requests to open source projects on GitHub.**

[![Rust](https://img.shields.io/badge/Rust-1.75+-f74c00?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/v5.8.1-blue?style=for-the-badge&logo=github&logoColor=white)](https://github.com/tang-vu/ContribAI/releases)
[![License](https://img.shields.io/badge/AGPL--3.0-green?style=for-the-badge&logo=opensourceinitiative&logoColor=white)](LICENSE)
[![Tests](https://img.shields.io/badge/418_tests-passing-brightgreen?style=for-the-badge&logo=checkmarx&logoColor=white)](#testing)
[![PRs Merged](https://img.shields.io/badge/10_PRs-merged-blueviolet?style=for-the-badge&logo=git&logoColor=white)](HALL_OF_FAME.md)

<br>

[**Getting Started**](#-getting-started) · [**Features**](#-features) · [**Commands**](#-commands) · [**Architecture**](#-architecture) · [**Hall of Fame**](HALL_OF_FAME.md)

<br>

```
Set it up once. Wake up to merged PRs.
```

</div>

---

## 🏆 Track Record

<table>
<tr>
<td width="50%">

| Metric | |
|:-------|------:|
| **PRs Submitted** | `44+` |
| **PRs Merged** | `10` |
| **Repos Contributed** | `21+` |
| **Languages Analyzed** | `13` |

</td>
<td width="50%">

**Notable Contributions:**

🌍 `Worldmonitor` — 45k ⭐ · 3 merged<br>
🕵️ `Maigret` — 19k ⭐ · 3 merged<br>
🤖 `AI-Research-SKILLs` — 6k ⭐ · 1 merged<br>
📊 `s-tui` — 5k ⭐ · 1 merged<br>
🔍 `HolmesGPT` — 2k ⭐ · 1 merged

</td>
</tr>
</table>

> See the full **[Hall of Fame →](HALL_OF_FAME.md)** for every PR with links.

---

## ⚡ How It Works

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Discovery  │────▶│  Analysis   │────▶│  Generator  │────▶│  PR + CI    │────▶│   Patrol    │
│             │     │             │     │             │     │             │     │             │
│ Search repos│     │ 13-lang AST │     │ LLM-powered │     │ Fork, commit│     │ Auto-fix    │
│ by language │     │ 17 skills   │     │ code gen +  │     │ create PR   │     │ review      │
│ and stars   │     │ security,   │     │ self-review │     │ sign CLA    │     │ feedback    │
│             │     │ quality,    │     │ + scoring   │     │ monitor CI  │     │ & reply     │
│             │     │ performance │     │             │     │             │     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
```

---

## 🚀 Getting Started

### Install

```bash
# Build from source (recommended)
git clone https://github.com/tang-vu/ContribAI.git && cd ContribAI
cargo install --path crates/contribai-rs

# Or one-line install
curl -fsSL https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.sh | bash
# Windows:
irm https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.ps1 | iex
```

### Configure

```bash
contribai init     # Interactive setup wizard
contribai login    # Verify auth + switch LLM providers
```

### Run

```bash
contribai hunt                # Autonomous: discover → analyze → PR
contribai target <repo_url>   # Target a specific repo
contribai analyze <repo_url>  # Dry-run analysis (no PRs)
contribai interactive         # Browse in ratatui TUI
```

<details>
<summary>📝 <strong>Example config.yaml</strong></summary>

```yaml
github:
  token: "ghp_your_token"       # or set GITHUB_TOKEN env var

llm:
  provider: "gemini"            # gemini | openai | anthropic | ollama | vertex
  model: "gemini-3-flash-preview"
  api_key: "your_api_key"       # or set GEMINI_API_KEY env var
  # base_url: "https://api.openai.com/v1"  # Optional: override default endpoint for OpenAI-compatible providers

discovery:
  languages:                    # default: all 15 languages
    - python
    - javascript
    - typescript
    - go
    - rust
  stars_range: [100, 5000]
```

See [`config.yaml.template`](config.yaml.template) for all options.

</details>

---

## ✨ Features

<table>
<tr>
<td width="50%" valign="top">

### 🔍 Code Analysis
- **13-language AST** via tree-sitter
- Security: SQLi, XSS, resource leaks
- Code quality, complexity, dead code
- Performance bottlenecks
- Documentation gaps
- **17 progressive skills** loaded on-demand

### 🤖 Multi-LLM Support
- **Gemini 3.x** (default) — Flash, Pro, Lite tiers
- OpenAI, Anthropic, Ollama, Vertex AI
- Smart task routing across model tiers
- 5 specialized sub-agents

### 🎯 Hunt Mode
- Multi-round autonomous hunting
- Issue-first strategy
- Cross-file fixes
- Outcome learning per repo

</td>
<td width="50%" valign="top">

### 👁 PR Patrol
- Monitors PRs for review feedback
- LLM-classifies maintainer comments
- Auto-pushes code fixes
- Auto-replies to questions
- Auto-cleans stale PRs from memory

### 🔌 Integrations
- **MCP Server** — 21 tools for Claude Desktop
- **Web Dashboard** — axum REST API at `:8787`
- **Cron Scheduler** — automated runs
- **Docker** — compose-ready deployment
- **Webhooks** — Slack, Discord, Telegram

### 🛡 Safety
- AI policy detection
- CLA auto-signing
- Quality gate scoring
- Duplicate PR prevention
- Protected file guardrails

</td>
</tr>
</table>

### Supported Languages

| Deep AST (tree-sitter) | Fallback Parser |
|:----------------------:|:---------------:|
| Python · JavaScript · TypeScript · Go · Rust · Java | Kotlin → Java |
| C · C++ · Ruby · PHP · C# · HTML · CSS | Swift → Java · Vue/Svelte → HTML |

---

## 📖 Commands

ContribAI ships with **40+ commands** accessible via CLI or interactive menu.

<details>
<summary>🔥 <strong>Hunt & Contribute</strong></summary>

```bash
contribai hunt                        # Autonomous discovery + PRs
contribai hunt --dry-run              # Analyze only, no PRs
contribai run                         # Single pipeline run
contribai target <url>                # Target specific repo
contribai analyze <url>               # Dry-run analysis
contribai solve <url>                 # Solve open issues
```

</details>

<details>
<summary>📊 <strong>Monitor & Stats</strong></summary>

```bash
contribai patrol                      # Respond to PR reviews
contribai status                      # PR status table
contribai stats                       # Contribution statistics
contribai leaderboard                 # Merge rate by repo
contribai system-status               # DB, rate limits, scheduler
```

</details>

<details>
<summary>🖥️ <strong>Interactive & Config</strong></summary>

```bash
contribai                             # Interactive menu (22 items)
contribai interactive                 # ratatui TUI browser
contribai init                        # Setup wizard
contribai login                       # Interactive auth + provider config
contribai config-list                 # Show all config
contribai config-get llm.provider     # Get config value
contribai config-set llm.provider openai  # Set config value
contribai profile security-focused    # Named profile
```

</details>

<details>
<summary>🌐 <strong>Servers & Tools</strong></summary>

```bash
contribai web-server                  # Dashboard at :8787
contribai schedule                    # Cron scheduler
contribai mcp-server                  # MCP stdio server
contribai cleanup                     # Remove stale forks
contribai notify-test                 # Test Slack/Discord/Telegram
```

</details>

---

## 🏗 Architecture

```
ContribAI/
├── crates/contribai-rs/src/        ← Rust v5.8.1 (primary)
│   ├── cli/                        40+ commands + ratatui TUI
│   ├── core/                       Config, events, error types
│   ├── github/                     REST v3 + GraphQL client
│   ├── analysis/                   13-lang AST + 17 skills
│   ├── generator/                  LLM code generation + scoring
│   ├── orchestrator/               Pipeline + SQLite memory (72h TTL)
│   ├── llm/                        Multi-provider + 5 sub-agents
│   ├── pr/                         PR lifecycle + patrol + CI
│   ├── mcp/                        21-tool MCP server (stdio)
│   ├── web/                        axum dashboard + webhooks
│   ├── sandbox/                    Docker + local fallback
│   └── tools/                      Tool protocol interface
│
└── python/                         Legacy v4.1.0 (reference only)
```

<details>
<summary>🔧 <strong>Tech Stack</strong></summary>

| Layer | Technology |
|:------|:-----------|
| Language | **Rust 2021** (primary), Python 3.11+ (legacy) |
| Async | Tokio (full), async/await throughout |
| HTTP | reqwest 0.12 (async, rustls-tls) |
| Database | SQLite (rusqlite, bundled) |
| LLM | Gemini 3.x, OpenAI, Anthropic, Ollama, Vertex AI |
| GitHub | REST API v3 + GraphQL |
| AST | tree-sitter (13 languages) |
| Web | axum 0.7 + tower-http |
| TUI | ratatui + crossterm |
| CLI | clap v4 + dialoguer + colored |
| Tests | 418 tests (mockall, wiremock, tokio-test) |

</details>

See [`docs/system-architecture.md`](docs/system-architecture.md) for the full design.

---

## 🧪 Testing

```bash
cargo test                  # Run all 418 tests
cargo test -- --nocapture   # With stdout output
cargo test ast_intel        # AST module tests only
cargo clippy                # Lint check
```

---

## 🔌 MCP Server

Use ContribAI as a tool provider for **Claude Desktop** or **Antigravity IDE**:

```json
{
  "mcpServers": {
    "contribai": {
      "command": "contribai",
      "args": ["mcp-server"]
    }
  }
}
```

> 21 tools available: repo analysis, PR management, GitHub search, issue solving, memory queries, and more.

---

## 🐳 Docker

```bash
docker compose up -d dashboard            # Dashboard at :8787
docker compose run --rm runner run        # One-shot pipeline run
docker compose up -d dashboard scheduler  # Dashboard + cron scheduler
```

---

## 📚 Documentation

| Document | Description |
|:---------|:------------|
| [**Hall of Fame**](HALL_OF_FAME.md) | 10 merged · 14 closed across 21+ repos |
| [**AGENTS.md**](AGENTS.md) | AI agent guide — architecture, patterns, CLI reference |
| [**Deployment Guide**](docs/deployment-guide.md) | Install, Docker, config, all 22 CLI commands |
| [**System Architecture**](docs/system-architecture.md) | Pipeline, middleware, events, LLM routing |
| [**Codebase Summary**](docs/codebase-summary.md) | Module map, tech stack, data structures |
| [**Project Roadmap**](docs/project-roadmap.md) | Version history and future plans |

---

## 📄 License

**AGPL-3.0 + Commons Clause** — see [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with Rust 🦀 and LLMs 🤖**

[Releases](https://github.com/tang-vu/ContribAI/releases) · [Issues](https://github.com/tang-vu/ContribAI/issues) · [Hall of Fame](HALL_OF_FAME.md)

</div>
