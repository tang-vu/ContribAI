# Changelog

All notable changes to ContribAI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0] - 2026-03-22

### Added
- **Parallel Hunt Mode**: `asyncio.gather` + semaphore for concurrent repo processing in hunt
  - New `_hunt_process_repo()` method extracted as class method
  - Honors `max_concurrent_repos` config (default: 3)
- **GitHub API retry with backoff**: `_request()` retries 3× on 502/503/504 errors (2s/4s/8s)
- **Fork cleanup command**: `contribai cleanup` — syncs PR statuses, removes stale forks via `gh repo delete`
- **Code-only file filter**: `SKIP_EXTENSIONS` (.md, .yaml, .json, .toml, .rst, .txt, .cfg, .ini, .lock) and `PROTECTED_META_FILES` (LICENSE, CONTRIBUTING.md, etc.) prevent non-code modifications
- **Hunt mode flags**: `--mode analysis|issues|both` for fine-grained control
- **EXE standalone behavior**: Defaults to `info` command when run without arguments, pauses before exit

### Changed
- `max_repos_per_run` from config is now respected in hunt mode (was hardcoded to 3)
- `star_tiers` in hunt mode now prioritizes configured `stars_range` first
- Daily PR limit default changed from 10 to 15
- Test count: 213 tests (refactored from 287)

### Fixed
- Hunt mode ignored `max_repos_per_run` config, used hardcoded `targets[:3]`
- 504 Gateway Timeout crashes when pushing files to GitHub API
- Unwanted PRs modifying non-code files (CONTRIBUTING.md, LICENSE, .yaml, .json)


## [1.0.0] - 2026-03-20

### Added
- **Stealth Mode**: PRs appear human-written — no ContribAI branding in body, branch names, or comments
- **CLA Auto-signing**: Detects CLAAssistant/EasyCLA bots and auto-signs CLA agreements
- **AI Policy Detection**: Checks `AI_POLICY.md` and `CONTRIBUTING.md` for anti-AI contribution policies, skips banned repos
- **Max 2 findings per repo**: Prevents spamming repos with too many PRs
- `create_pr_comment()` method in GitHubClient

### Changed
- Branch names: `fix/xxx` instead of `contribai/fix-xxx` (stealth)
- PR body: clean `## Problem / ## Solution / ## Changes` format
- CI auto-close message: no branding or emoji
- License: AGPL-3.0 + Commons Clause (from MIT)

### Fixed
- Updated all test assertions to v1.0.0

## [0.11.0] - 2026-03-20

### Added
- **Hunt Mode**: Autonomous multi-round repo discovery and PR creation
- `contribai hunt --rounds N --delay M` CLI command
- Configurable delay between hunt rounds
- 5 new tests (total: 287 tests)

## [0.10.0] - 2026-03-20

### Added
- **GitHub API dedup**: Prevents searching same repos twice across rounds
- **Cross-file pattern matching**: Detects same issue across multiple files and fixes all in one PR
- **Duplicate PR prevention**: Title similarity matching prevents creating duplicate PRs

## [0.9.0] - 2026-03-19

### Added
- **Deep finding validation**: LLM re-validates findings against full file context to filter false positives
- **Post-PR CI monitoring**: Polls CI check runs and auto-closes PRs that fail
- **Fuzzy search/replace matching**: Fallback matching when exact search strings don't match

## [0.8.0] - 2026-03-19

### Added
- **Performance analyzer**: Detects blocking calls, string allocation, N+1 queries
- **Refactor analyzer**: Finds unused imports, non-null assertions, encoding issues
- **Testing analyzer**: Identifies missing test coverage opportunities

### Fixed
- CI test failures and lint formatting errors

## [0.7.1] - 2026-03-19

### Fixed
- Auto-check PR template checkboxes for repos with required checklists
- Use search/replace blocks instead of full-file replacement to preserve existing code

## [0.7.0] - 2026-03-19

### Added
- **Multi-Model Agent System**: Task-based routing to different LLM models
- **Model Tiers**: Fast models for triage, powerful models for code generation
- **Vertex AI**: Google Cloud Vertex AI provider support
- **Env var fallback**: Token/API key resolution from environment variables
- **Auto-create issue**: Creates GitHub issue alongside PR for traceability
- **Post-PR compliance loop**: Monitors PR feedback and auto-fixes
- **Repo guidelines compliance**: Reads CONTRIBUTING.md and adapts PR format
- 287 tests total

## [0.6.0] - 2026-03-18

### Added
- **Interactive TUI**: Rich-based CLI interactive mode for browsing, selecting, and approving contributions
- **Contribution Leaderboard**: PR merge/close rate tracking with repo rankings and type-based stats
- **Multi-language Analyzers**: 19 analysis rules for JavaScript/TypeScript (7), Go (6), Rust (6)
- **Notification System**: Slack webhook, Discord embeds, Telegram Bot API integration
- 3 new CLI commands: `interactive`, `leaderboard`, `notify-test`
- `NotificationConfig` in config with per-channel and event-type toggles
- `httpx` dependency for notification HTTP clients

## [0.5.0] - 2026-03-18

### Added
- **Plugin System**: Entry-point based `AnalyzerPlugin` / `GeneratorPlugin` with auto-discovery
- **Webhooks**: GitHub webhook receiver (issues.opened, issues.labeled, push) with HMAC-SHA256
- **Usage Quotas**: Daily tracking for GitHub API calls, LLM calls, and token usage
- **API Key Auth**: `X-API-Key` header auth for dashboard mutation endpoints
- **Docker Compose**: 3-service setup (dashboard, scheduler, runner) with shared volumes

## [0.4.0] - 2026-03-18

### Added
- **Web Dashboard**: FastAPI REST API + static HTML dashboard with stats, PRs, repos, run history
- **Scheduler**: APScheduler-based cron scheduling for automated pipeline runs
- **Parallel Processing**: `asyncio.gather` + Semaphore for concurrent repo processing (default 3)
- **Contribution Templates**: 5 built-in YAML templates
- **Community Profiles**: 4 named presets (security-focused, docs-focused, full-scan, gentle)

## [0.3.0] - 2026-03-18

### Added
- **Issue Solver**: Classify GitHub issues by labels/keywords, filter by solvability, LLM-powered solving
- **Framework Strategies**: Auto-detect Django, Flask, FastAPI, React/Next.js, Express
- **Quality Scorer**: 7-check quality gate before PR submission

## [0.2.0] - 2026-03-18

### Added
- **Retry Utilities**: `async_retry` decorator with exponential backoff + jitter
- **LRU Cache**: Response caching for GitHub API and LLM calls
- **Test Suite**: 128 tests across all modules

## [0.1.0] - 2026-03-17

### Added
- **Core Pipeline**: Full discover → analyze → generate → PR workflow
- **Multi-LLM Support**: Gemini (primary), OpenAI, Anthropic, Ollama providers
- **GitHub Integration**: Async API client with rate limiting, repo discovery
- **Code Analysis**: Security, code quality, documentation, and UI/UX analyzers
- **Contribution Generator**: LLM-powered code generation with self-review
- **PR Manager**: Automated fork → branch → commit → PR workflow
- **Memory System**: SQLite-backed persistent tracking of repos and PRs
- **Rich CLI**: Commands: `run`, `target`, `analyze`, `status`, `stats`, `config`
