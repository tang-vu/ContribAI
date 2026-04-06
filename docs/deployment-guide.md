# Deployment Guide

**Version:** 5.8.1 | **Language:** Rust | **Last Updated:** 2026-04-05

---

## Quick Start

### Option 1: One-Line Install (Recommended)

```bash
# Linux / macOS (auto-detects OS and architecture)
curl -fsSL https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.ps1 | iex
```

### Option 2: Build from Source (Development)

```bash
# Clone repository
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI

# Build (release, optimized) — from project root
cargo build --release

# Or install to PATH:
cargo install --path crates/contribai-rs

# Configure
contribai init            # Interactive setup wizard
contribai login           # Verify auth + switch LLM providers

# Run
contribai hunt
contribai web-server  # Web dashboard at :8787
```

### Option 2: Docker (Production-Ready)

```bash
cp config.example.yaml config.yaml
# Edit config.yaml

# Start web dashboard
docker compose up -d dashboard

# Run single analysis
docker compose run --rm runner run

# Start scheduler
docker compose up -d dashboard scheduler
```

### Option 3: Static Binary (Simplest)

```bash
# Install to PATH (recommended)
cargo install --path crates/contribai-rs
contribai --version  # 5.5.0
contribai --help

# Or run directly from build output:
./crates/contribai-rs/target/release/contribai --help
```

---

## Installation Methods

### Method 1: Cargo Build (Recommended for Development)

```bash
# Clone & build
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI/crates/contribai-rs

# Development build (fast compile, debug symbols)
cargo build

# Release build (optimized, ~5x faster runtime)
cargo build --release

# Run tests to verify
cargo test
```

**Verification:**

```bash
./target/release/contribai --help
# Shows all 40+ commands
```

**Prerequisites:**
- Rust 1.75+ (install via [rustup](https://rustup.rs))
- C compiler (for tree-sitter grammars)
- OpenSSL dev headers (Linux: `libssl-dev`, macOS: via homebrew)

### Method 2: Docker (Recommended for Production)

```dockerfile
# Multi-stage build — small final image
FROM rust:1.75 AS builder
WORKDIR /app
COPY crates/contribai-rs/ .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/contribai /usr/local/bin/contribai
EXPOSE 8787
ENTRYPOINT ["contribai"]
```

```bash
# Build image
docker build -t contribai:5.5.0 .

# Run
docker run --rm contribai:5.5.0 --help
```

### Method 3: Kubernetes (Enterprise)

```bash
kubectl create namespace contribai
kubectl apply -k kubernetes/overlays/production/
kubectl get pods -n contribai
```

---

## Configuration

### Configuration File

Create `config.yaml` from `config.example.yaml`:

```yaml
# GitHub Configuration
github:
  token: "ghp_..."                    # Personal Access Token (required)
  max_prs_per_day: 15                 # Safety limit (1-100)
  rate_limit_margin: 100              # Buffer before hitting API limit
  fork_timeout_seconds: 30            # Timeout for fork operation

# LLM Configuration
llm:
  provider: "gemini"                  # gemini | openai | anthropic | ollama
  model: "gemini-3-flash-preview"     # Model ID (v5.4.0+)
  api_key: "your_api_key"             # API key for provider
  # base_url: "https://api.openai.com/v1"  # Optional: override default API endpoint for OpenAI/Anthropic-compatible providers (e.g. proxies, local gateways)
  temperature: 0.5                    # Creativity (0.0-2.0)
  max_tokens: 2000                    # Max response length
  timeout_seconds: 60                 # Request timeout

# Discovery Configuration
discovery:
  languages: ["python", "javascript"]
  stars_range: [100, 5000]
  min_activity_days: 180
  exclude_repos: []

# Analysis Configuration
analysis:
  enabled_analyzers:
    - security
    - code_quality
    - performance
    - documentation
    - ui_ux
    - refactoring
  max_file_size_kb: 50
  skip_patterns: ["*.md", "*.yaml", "*.json"]

# Pipeline Configuration
pipeline:
  concurrent_repos: 3
  retry_attempts: 2
  retry_backoff_seconds: 2
  timeout_seconds: 300

# Multi-Model Task Routing
multi_model:
  task_routing:
    analysis: "economy"
    generation: "performance"
    review: "balanced"

# Web Dashboard
web:
  host: "0.0.0.0"
  port: 8787
  api_keys: ["your-secret-key"]       # API key auth for mutations
  webhook_secret: "github-secret"     # HMAC-SHA256 webhook verification

# Notifications
notifications:
  slack: "https://hooks.slack.com/services/..."
  discord: "https://discord.com/api/webhooks/..."
  telegram: "https://api.telegram.org/bot..."

# Scheduler
scheduler:
  enabled: true
  timezone: "UTC"
```

### Environment Variables (Override YAML)

```bash
export CONTRIBAI_GITHUB_TOKEN="ghp_..."
export CONTRIBAI_LLM_PROVIDER="gemini"
export CONTRIBAI_LLM_API_KEY="your_api_key"
export CONTRIBAI_LLM_MODEL="gemini-3-flash-preview"
export CONTRIBAI_LLM_BASE_URL="https://api.openai.com/v1"  # Optional: override default endpoint
export GITHUB_WEBHOOK_SECRET="your-webhook-secret"
export CONTRIBAI_WEB_PORT="8787"
```

**Precedence:** CLI flags > Env vars > config.yaml > Defaults

### Profile Presets

Defined in `crates/contribai-rs/src/core/profiles.rs`:

```bash
contribai profile security-focused   # Security analyzers only, conservative
contribai profile docs-focused       # Documentation analyzers only
contribai profile full-scan          # All analyzers, higher PR limit
contribai profile gentle             # Code quality only, low PR limit
```

---

## Environment Variables

### Required

| Variable | Description | Example |
|----------|-------------|---------|
| `CONTRIBAI_GITHUB_TOKEN` | GitHub PAT (required) | `ghp_abc123...` |
| `CONTRIBAI_LLM_API_KEY` | LLM provider API key | `AIzaSy...` (Gemini) |

### Optional

| Variable | Description | Default |
|----------|-------------|---------|
| `CONTRIBAI_LLM_PROVIDER` | LLM provider | `gemini` |
| `CONTRIBAI_LLM_MODEL` | Model ID | `gemini-2.5-flash` |
| `CONTRIBAI_LLM_BASE_URL` | Override default API endpoint | (provider default) |
| `GITHUB_WEBHOOK_SECRET` | Webhook HMAC secret | (none) |
| `CONTRIBAI_HOME` | Data directory | `~/.contribai` |
| `CONTRIBAI_LOG_LEVEL` | Log level | `INFO` |
| `RUST_LOG` | Rust tracing filter | `contribai_rs=info` |

---

## Docker Deployment

### Docker Compose

```yaml
version: '3.8'
services:
  dashboard:
    build: .
    command: serve --host 0.0.0.0 --port 8787
    ports: ["8787:8787"]
    environment:
      CONTRIBAI_GITHUB_TOKEN: ${GITHUB_TOKEN}
      CONTRIBAI_LLM_API_KEY: ${LLM_API_KEY}
    volumes:
      - ./config.yaml:/app/config.yaml
      - contribai-data:/root/.contribai
    restart: unless-stopped

  runner:
    build: .
    command: run
    environment:
      CONTRIBAI_GITHUB_TOKEN: ${GITHUB_TOKEN}
      CONTRIBAI_LLM_API_KEY: ${LLM_API_KEY}
    volumes:
      - ./config.yaml:/app/config.yaml
      - contribai-data:/root/.contribai
    depends_on: [dashboard]

  scheduler:
    build: .
    command: schedule --cron "0 */6 * * *"
    environment:
      CONTRIBAI_GITHUB_TOKEN: ${GITHUB_TOKEN}
      CONTRIBAI_LLM_API_KEY: ${LLM_API_KEY}
    volumes:
      - ./config.yaml:/app/config.yaml
      - contribai-data:/root/.contribai
    depends_on: [dashboard]
    restart: unless-stopped

volumes:
  contribai-data:
```

```bash
export GITHUB_TOKEN="ghp_..."
export LLM_API_KEY="AIzaSy..."
docker compose up -d dashboard
docker compose logs -f dashboard
```

---

## CLI Commands Reference (22 Commands)

### Core Operations

```bash
contribai run                         # Single full pipeline
contribai hunt                        # Autonomous multi-round hunting
contribai hunt --dry-run              # Preview without PRs
contribai patrol                      # Monitor PRs for review feedback
contribai target <url>                # Analyze specific repo
contribai analyze <url>               # Dry-run analysis
contribai solve <url>                 # Solve open issues
```

### Discovery & Stats

```bash
contribai stats                       # Contribution statistics
contribai status                      # PR status table (colored)
contribai leaderboard                 # Merge rate by repo
contribai models                      # Available LLM models
contribai templates                   # Contribution templates
contribai system-status               # DB, rate limits, scheduler
```

### Interactive & Setup

```bash
contribai                             # Interactive menu (22 items)
contribai interactive                 # ratatui TUI browser
contribai init                        # Setup wizard
contribai login                       # Auth status for all providers
contribai notify-test                 # Test Slack/Discord/Telegram (real HTTP)
```

### Config

```bash
contribai config-list                 # Show all config
contribai config-get llm.provider     # Get single value
contribai config-set llm.provider openai # Set value
contribai config-set llm.base_url "https://your-proxy.example.com/v1"  # Set custom endpoint
contribai profile security-focused    # Named profile
```

### Servers & Automation

```bash
contribai web-server                  # Dashboard at :8787
contribai web-server --port 9000      # Custom port
contribai schedule                    # Cron scheduler
contribai mcp-server                  # MCP stdio (Claude Desktop)
contribai cleanup                     # Remove stale forks
```

---

## Web Dashboard

### Routes

| Route | Method | Auth | Purpose |
|-------|--------|------|---------|
| `/` | GET | No | Dashboard UI |
| `/api/stats` | GET | No | Statistics |
| `/api/repos` | GET | No | Analyzed repos |
| `/api/health` | GET | No | Health check |
| `/api/run` | POST | API Key | Trigger pipeline |
| `/api/run/target` | POST | API Key | Target specific repo |
| `/api/webhooks/github` | POST | HMAC-SHA256 | GitHub webhook |

### API Key Authentication

```bash
# Pass API key as query parameter
curl -X POST "http://localhost:8787/api/run?api_key=your-secret-key"

# Constant-time comparison prevents timing attacks
```

### Webhook Setup (GitHub)

1. Go to repo settings → Webhooks
2. Payload URL: `http://your-server:8787/api/webhooks/github`
3. Content type: `application/json`
4. Secret: set to match `GITHUB_WEBHOOK_SECRET` env var
5. Events: `push`, `pull_request`, `issues`

Webhook verification uses HMAC-SHA256 via `X-Hub-Signature-256` header.

---

## Scheduler Setup

### Cron Syntax

```bash
contribai schedule --cron "0 */6 * * *"    # Every 6 hours
contribai schedule --cron "0 9 * * *"      # Daily at 9 AM UTC
contribai schedule --cron "0 18 * * 1"     # Every Monday at 6 PM
contribai schedule --cron "0 8 * * 1-5"    # Every weekday at 8 AM
```

### Systemd Integration (Linux)

```ini
[Unit]
Description=ContribAI Scheduler
After=network.target

[Service]
Type=simple
User=contribai
ExecStart=/usr/local/bin/contribai schedule --cron "0 */6 * * *"
Restart=always
RestartSec=10
Environment="CONTRIBAI_GITHUB_TOKEN=ghp_..."
Environment="CONTRIBAI_LLM_API_KEY=AIzaSy..."

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable contribai-scheduler
sudo systemctl start contribai-scheduler
```

---

## Troubleshooting

| Issue | Cause | Fix |
|-------|-------|-----|
| `401 Unauthorized (GitHub)` | Invalid token | Check token format and scopes |
| `429 Too Many Requests` | API rate limit | Reduce concurrent_repos, add delays |
| `RESOURCE_EXHAUSTED (LLM)` | LLM rate limit | Use economy model, reduce max_tokens |
| `Connection refused` | Service not running | `contribai serve` |
| `Database locked` | Concurrent access | Wait and retry |
| Link errors on build | Missing C compiler/OpenSSL | Install build deps (gcc, libssl-dev) |

### Debug Mode

```bash
# Enable debug logging via Rust tracing
RUST_LOG=contribai_rs=debug contribai run

# Or set in config
export CONTRIBAI_LOG_LEVEL=DEBUG

# Check event log
tail -f ~/.contribai/events.jsonl | jq
```

### Health Checks

```bash
curl http://localhost:8787/api/health
sqlite3 ~/.contribai/memory.db "PRAGMA integrity_check;"
```

---

## Security Checklist

- [ ] GitHub token stored in env vars (not in code)
- [ ] LLM API key stored in env vars (not in code)
- [ ] API keys configured for web dashboard mutations
- [ ] GitHub webhook secret configured (HMAC-SHA256)
- [ ] HTTPS enabled (reverse proxy: nginx/caddy)
- [ ] Firewall restricts access to :8787
- [ ] Database backups configured
- [ ] Dependencies audited (`cargo audit`)
- [ ] No secrets in config.yaml template
- [ ] Binary built with `--release` for production

---

## Document Metadata

- **Created:** 2026-03-28
- **Last Updated:** 2026-04-01
- **Version:** 5.2.0 (Interactive TUI + full CLI parity)
