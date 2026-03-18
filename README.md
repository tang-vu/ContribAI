# ContribAI 🤖

> **AI Agent that automatically contributes to open source projects on GitHub**

ContribAI discovers open source repositories, analyzes them for improvement opportunities, generates high-quality fixes, and submits them as Pull Requests — all autonomously.

[![Python 3.11+](https://img.shields.io/badge/python-3.11+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-197%20passed-brightgreen)](#)
[![Version](https://img.shields.io/badge/version-0.5.0-blue)](#)

---

## ✨ Features

### Core Pipeline
- 🔍 **Smart Discovery** – Finds contribution-friendly repos by language, stars, activity
- 🔒 **Security Analysis** – Detects hardcoded secrets, SQL injection, XSS
- ✨ **Code Quality** – Finds dead code, missing error handling, complexity issues
- 📝 **Documentation** – Catches missing docstrings, incomplete READMEs
- 🎨 **UI/UX** – Identifies accessibility issues, responsive design gaps
- 🤖 **Multi-LLM** – Gemini (primary), OpenAI, Anthropic, Ollama
- 📤 **Auto-PR** – Forks, branches, commits, and creates PRs automatically

### Scale (v0.4.0+)
- 🌐 **Web Dashboard** – FastAPI REST API + static dashboard at `:8787`
- ⏰ **Scheduler** – APScheduler cron-based automated runs
- ⚡ **Parallel Processing** – `asyncio.gather` + Semaphore (3 concurrent repos)
- 📋 **Templates** – 5 built-in contribution templates (gitignore, license, badges, etc.)
- 🎭 **Profiles** – Named presets: `security-focused`, `docs-focused`, `full-scan`, `gentle`

### Production Ready (v0.5.0)
- 🔌 **Plugin System** – Entry-point based plugins for custom analyzers/generators
- 🪝 **Webhooks** – GitHub webhook receiver for auto-triggering on issues/push
- 📊 **Usage Quotas** – Track GitHub + LLM API calls with daily limits
- 🔑 **API Auth** – API key authentication for dashboard mutation endpoints
- 🐳 **Docker** – Dockerfile + docker-compose (dashboard, scheduler, runner)

## 🏗️ Architecture

```
Discovery → Analysis → Generation → Quality Gate → PR
    │           │           │            │            │
    ▼           ▼           ▼            ▼            ▼
 GitHub    4 Analyzers   LLM-based    7-check      Fork+Branch
 Search    + Framework   code gen     scorer       +Commit+PR
 + Webhooks + Plugins   + self-review + Quotas     + Templates
```

## 📦 Installation

```bash
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI
pip install -e ".[dev]"
```

### Docker

```bash
# Build and run dashboard
docker compose up -d dashboard

# One-shot run
docker compose run --rm runner run --dry-run

# Dashboard + scheduler
docker compose up -d dashboard scheduler
```

## ⚙️ Configuration

```bash
cp config.example.yaml config.yaml
```

Edit `config.yaml`:

```yaml
github:
  token: "ghp_your_token_here"

llm:
  provider: "gemini"
  model: "gemini-2.5-flash"
  api_key: "your_api_key"

discovery:
  languages: [python, javascript]
  stars_range: [100, 5000]
```

## 🚀 Usage

### Auto-discover and contribute

```bash
contribai run                          # Full autonomous run
contribai run --dry-run                # Preview without creating PRs
contribai run --language python        # Filter by language
```

### Target a specific repo

```bash
contribai target https://github.com/owner/repo
contribai target https://github.com/owner/repo --dry-run
```

### Solve open issues

```bash
contribai solve https://github.com/owner/repo
```

### Web Dashboard & Scheduler

```bash
contribai serve                        # Dashboard at :8787
contribai serve --port 9000            # Custom port
contribai schedule --cron "0 */6 * * *"  # Auto-run every 6h
```

### Templates & Profiles

```bash
contribai templates                    # List contribution templates
contribai profile list                 # List profiles
contribai profile security-focused     # Run with profile
contribai profile gentle --dry-run     # Gentle mode
```

### Status & stats

```bash
contribai status        # Check submitted PRs
contribai stats         # Overall statistics
contribai config        # View current config
```

## 🔌 Plugin System

Create custom analyzers as Python packages:

```python
from contribai.plugins.base import AnalyzerPlugin

class MyAnalyzer(AnalyzerPlugin):
    @property
    def name(self): return "my-analyzer"

    async def analyze(self, context):
        # Your analysis logic
        return findings
```

Register via entry points in `pyproject.toml`:

```toml
[project.entry-points."contribai.analyzers"]
my_analyzer = "my_package:MyAnalyzer"
```

## 📁 Project Structure

```
contribai/
├── core/              # Config, models, exceptions, quotas, profiles
├── llm/               # Multi-provider LLM (Gemini, OpenAI, Anthropic, Ollama)
├── github/            # GitHub API client & repo discovery
├── analysis/          # LLM-powered code analysis + framework strategies
├── generator/         # Contribution generator + self-review + quality scorer
├── issues/            # Issue-driven contribution solver
├── pr/                # PR lifecycle manager
├── orchestrator/      # Pipeline orchestrator & persistent memory
├── plugins/           # Plugin system (analyzer/generator extensions)
├── templates/         # Contribution templates (5 built-in YAML)
├── scheduler/         # APScheduler cron-based automation
├── web/               # FastAPI dashboard, auth, webhooks
└── cli/               # Rich CLI (11 commands)
```

## 🧪 Testing

```bash
pytest tests/ -v                  # Run all 197 tests
pytest tests/ -v --cov=contribai  # With coverage
ruff check contribai/             # Lint
ruff format contribai/            # Format
```

## 🛡️ Safety

- **Daily PR limit** – Configurable max PRs per day (default: 10)
- **Quality scorer** – 7-check gate prevents low-quality PRs
- **API quotas** – Track and limit GitHub + LLM usage daily
- **API key auth** – Protect dashboard mutation endpoints
- **Webhook verification** – HMAC-SHA256 signature checking
- **Dry run mode** – Preview everything without creating PRs
- **Rate limit awareness** – Exponential backoff with jitter

## 📄 License

MIT License – see [LICENSE](LICENSE) for details.

---

**Made with ❤️ for the open source community**
