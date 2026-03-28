# ContribAI

> **AI Agent that automatically contributes to open source projects on GitHub**

[![Python 3.11+](https://img.shields.io/badge/python-3.11+-blue.svg)](https://www.python.org/downloads/)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-416%20passed-brightgreen)](#testing)
[![Version](https://img.shields.io/badge/version-3.0.6-blue)](https://github.com/tang-vu/ContribAI/releases)

### 🏆 Results

| Metric | Count |
|--------|-------|
| **PRs Submitted** | 34+ |
| **PRs Merged** | 9 |
| **Repos Contributed** | 21 |
| **Notable Repos** | Maigret (10k⭐), Worldmonitor (45k⭐), s-tui (4k⭐) |

> Set it up once, wake up to merged PRs. See the [**Hall of Fame →**](HALL_OF_FAME.md)

ContribAI discovers open source repositories, analyzes code for improvements, generates fixes, and submits Pull Requests — all autonomously.

```
  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
  │ Discovery│───▶│ Analysis │───▶│Generator │───▶│ PR + CI  │───▶│ Patrol   │
  │          │    │ 20 skills│    │ LLM +    │    │ Fork,    │    │ Auto-fix │
  │ Find repos│    │ Security │    │ self-    │    │ commit,  │    │ review   │
  │ by lang, │    │ quality, │    │ review,  │    │ create   │    │ feedback │
  │ stars    │    │ perf     │    │ scoring  │    │ PR + CLA │    │ & reply  │
  └──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
```

**Safety:** Quality gate (7-check scorer), duplicate detection, AI policy respect, CI monitoring, rate limiting, dry-run mode

## Quick Start

```bash
# Install
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI
pip install -e ".[dev]"

# Configure
cp config.example.yaml config.yaml
# Edit config.yaml with your GitHub token + LLM API key

# Run
contribai hunt              # Autonomous: discover repos → analyze → PR
contribai target <repo_url> # Target a specific repo
contribai run --dry-run     # Preview without creating PRs
```

## Features

| Category | Highlights |
|----------|-----------|
| **Analysis** | Security (secrets, SQLi, XSS), code quality, performance, docs, UI/UX, refactoring |
| **LLM** | Gemini, OpenAI, Anthropic, Ollama, Vertex AI — smart task routing across model tiers |
| **Hunt Mode** | Multi-round autonomous hunting, cross-file fixes, inter-repo delay |
| **PR Patrol** | Monitors PRs for review feedback, auto-responds and pushes code fixes |
| **MCP Server** | 14 tools for Claude Desktop via stdio protocol |
| **Safety** | AI policy detection, CLA auto-signing, quality gate, duplicate prevention |
| **Platform** | Web dashboard, scheduler, webhooks, Docker, profiles, plugins |
| **Notifications** | Slack, Discord, Telegram with retry |

## Usage

```bash
# Hunt mode (autonomous)
contribai hunt                         # Discover and contribute
contribai hunt --rounds 5 --delay 15   # 5 rounds, 15min delay
contribai hunt --mode issues           # Issue solving only

# Target specific repos
contribai target <repo_url>            # Analyze and contribute
contribai solve <repo_url>             # Solve open issues

# Monitor & maintain
contribai patrol                       # Respond to PR reviews
contribai status                       # Check submitted PRs
contribai stats                        # Overall statistics
contribai cleanup                      # Remove stale forks

# Platform
contribai serve                        # Dashboard at :8787
contribai schedule --cron "0 */6 * * *"  # Auto-run every 6h

# Profiles
contribai profile security-focused     # Run with preset profile
```

## Configuration

```yaml
# config.yaml
github:
  token: "ghp_your_token"       # or set GITHUB_TOKEN env var

llm:
  provider: "gemini"            # gemini | openai | anthropic | ollama
  model: "gemini-2.5-flash"
  api_key: "your_api_key"

discovery:
  languages: [python, javascript]
  stars_range: [100, 5000]
```

See [`config.example.yaml`](config.example.yaml) for all options.

## Architecture

```
contribai/
├── core/           # Config, models, middleware, events, retry, quotas
├── llm/            # Multi-provider LLM + task routing + context management
├── github/         # GitHub API client, discovery, guidelines
├── analysis/       # 20+ analysis skills + framework detection + compression
├── generator/      # Fix generation + self-review + quality scoring
├── orchestrator/   # Pipeline, SQLite memory (7 tables), review gate
├── pr/             # PR lifecycle + patrol + CLA/DCO compliance
├── issues/         # Issue classification + multi-file solving
├── agents/         # Sub-agent registry (DeerFlow-inspired)
├── tools/          # Extensible tool protocol
├── mcp/            # MCP client for external tools
├── mcp_server.py   # MCP server (14 tools for Claude Desktop)
├── sandbox/        # Docker-based code validation
├── web/            # FastAPI dashboard + webhooks + auth
├── scheduler/      # APScheduler cron automation
├── notifications/  # Slack, Discord, Telegram
├── plugins/        # Entry-point plugin system
├── templates/      # YAML contribution templates
└── cli/            # Rich CLI + TUI
```

See [`docs/system-architecture.md`](docs/system-architecture.md) for detailed architecture.

## Docker

```bash
docker compose up -d dashboard            # Dashboard at :8787
docker compose run --rm runner run        # One-shot run
docker compose up -d dashboard scheduler  # Dashboard + scheduler
```

## Testing

```bash
pytest tests/ -v                    # Run all 416 tests
pytest tests/ -v --cov=contribai    # With coverage
ruff check contribai/               # Lint
ruff format contribai/              # Format
```

## Extending

**Plugins** — Create custom analyzers/generators as Python packages:

```python
from contribai.plugins.base import AnalyzerPlugin

class MyAnalyzer(AnalyzerPlugin):
    @property
    def name(self): return "my-analyzer"

    async def analyze(self, context):
        return findings
```

```toml
# pyproject.toml
[project.entry-points."contribai.analyzers"]
my_analyzer = "my_package:MyAnalyzer"
```

**MCP** — Use ContribAI from Claude Desktop:

```json
{
  "mcpServers": {
    "contribai": {
      "command": "python",
      "args": ["-m", "contribai.mcp_server"]
    }
  }
}
```

## Documentation

| Doc | Description |
|-----|-------------|
| [`HALL_OF_FAME.md`](HALL_OF_FAME.md) | **9 merged PRs** across 21 repos — real results |
| [`system-architecture.md`](docs/system-architecture.md) | Pipeline, middleware, events, LLM routing |
| [`code-standards.md`](docs/code-standards.md) | Conventions, patterns, testing |
| [`deployment-guide.md`](docs/deployment-guide.md) | Install, Docker, config, CLI reference |
| [`project-roadmap.md`](docs/project-roadmap.md) | Version history and future plans |
| [`codebase-summary.md`](docs/codebase-summary.md) | Module map and tech stack |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Contribution guidelines |

## License

AGPL-3.0 + Commons Clause — see [LICENSE](LICENSE) for details.
