# Changelog

All notable changes to ContribAI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-03-18

### Added
- **Plugin System**: Entry-point based `AnalyzerPlugin` / `GeneratorPlugin` with auto-discovery
- **Webhooks**: GitHub webhook receiver (issues.opened, issues.labeled, push) with HMAC-SHA256
- **Usage Quotas**: Daily tracking for GitHub API calls, LLM calls, and token usage
- **API Key Auth**: `X-API-Key` header auth for dashboard mutation endpoints
- **Docker Compose**: 3-service setup (dashboard, scheduler, runner) with shared volumes
- Dockerfile EXPOSE 8787 + healthcheck
- Updated README with Phase 4+5 features, Docker docs, plugin guide
- Updated config.example.yaml with all new sections
- 24 new tests (total: 221 tests)



## [0.4.0] - 2026-03-18

### Added
- **Web Dashboard**: FastAPI REST API + static HTML dashboard with stats, PRs, repos, run history
- **Scheduler**: APScheduler-based cron scheduling for automated pipeline runs
- **Parallel Processing**: `asyncio.gather` + Semaphore for concurrent repo processing (default 3)
- **Contribution Templates**: 5 built-in YAML templates (gitignore, license, badges, type-hints, security-headers)
- **Community Profiles**: 4 named presets (security-focused, docs-focused, full-scan, gentle)
- **CLI Commands**: `serve`, `schedule`, `templates`, `profile <name>`
- **Config Sections**: `SchedulerConfig`, `WebConfig`, `PipelineConfig`
- **Memory**: `get_run_history()` endpoint for dashboard
- **28 new tests** covering all Phase 4 modules (total: 197 tests)

### Dependencies
- `fastapi>=0.115,<1.0`
- `uvicorn>=0.32,<1.0`
- `apscheduler>=3.10,<4.0`


## [0.3.0] - 2026-03-18

### Added
- **Issue Solver**: Classify GitHub issues by labels/keywords, filter by solvability, LLM-powered solving
- **Framework Strategies**: Auto-detect Django, Flask, FastAPI, React/Next.js, Express with tailored analysis
- **Quality Scorer**: 7-check quality gate before PR submission (change size, commit format, debug code, placeholders)
- **CLI**: New `solve` command for issue-driven contributions
- Tests: 169 total tests covering all modules

## [0.2.0] - 2026-03-18

### Added
- **Retry Utilities**: `async_retry` decorator with exponential backoff + jitter
- **LRU Cache**: Response caching for GitHub API and LLM calls
- **Test Suite**: 128 tests across all modules (config, models, memory, LLM, GitHub, discovery, analyzer, generator, PR manager, CLI)
- Integration tests for pipeline dry run and analyze-only mode

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
- **Team Infrastructure**: 8 agent roles, 10 workflows, CI/CD pipelines
- **GitHub Templates**: PR template, bug/feature/security issue templates
- **DevOps**: Dockerfile, Makefile, GitHub Actions CI/CD
- **Documentation**: README, CONTRIBUTING, CHANGELOG, SECURITY
