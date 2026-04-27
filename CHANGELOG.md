# Changelog

All notable changes to ContribAI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [6.7.0] - 2026-04-28

### Added
- **10 new analysis skills** — coverage went from 17 → **27 skills**, closing major language and framework gaps the analyzer was previously missing:
  - `csharp_specific` — C#: IDisposable, async void, null-coalescing, LINQ deferred-execution
  - `ruby_specific` — Ruby: monkey-patching, frozen_string_literal, eval/send injection
  - `php_specific` — PHP: SQL injection, type juggling, error suppression, deprecated mysql_*
  - `vue_patterns` — Vue 3: ref vs reactive, v-html XSS, lifecycle ordering
  - `rails_security` — Rails: mass-assignment, raw `find_by_sql`, CSRF, secret_key_base
  - `laravel_security` — Laravel: $fillable/$guarded, unprotected routes, DB::raw injection
  - `spring_security` — Spring/Spring Boot: filter-chain holes, JPA injection, exposed actuators
  - `dockerfile_security` — Dockerfile: `latest` tag, root user, secrets in layers, missing HEALTHCHECK
  - `github_actions_security` — Actions: untrusted `${{ github.event.* }}`, pwn-request via pull_request_target, missing permissions scope
- 8 new unit tests covering the new skills (csharp/ruby+rails/php+laravel/java+spring/vue/dockerfile/actions/no-leak).

## [6.6.0] - 2026-04-27

### Added
- **`contribai logs` command** — tail `~/.contribai/events.jsonl` from the CLI:
  - `--tail N` — show the last N events (default 20). Streams the file with a ring buffer so huge logs don't blow up memory.
  - `--filter <substring>` — case-insensitive filter on event type (e.g. `pr`, `hunt`, `error`, `complete`).
  - `--json` — emit one raw JSON object per line for piping into `jq`/scripts.
  - Pretty mode color-codes events by lifecycle stage (cyan=start, green=complete/merged, red=error) and renders the `data` map as compact `key=value` pairs with long strings truncated.
  - Skips and reports unparseable lines instead of failing the whole command.

## [6.5.0] - 2026-04-27

### Added
- `contribai doctor` now checks for newer GitHub releases (anonymous, 5s timeout) and surfaces an "UPDATE AVAILABLE" hint when the installed binary is behind.

### Fixed
- **Sprint 22.6** — release-blocking issues on `main`:
  - Compile error in `validate_change_schema` test (`json_parser.rs:342`) — was passing `&&str` instead of `&serde_json::Value` (E0308).
  - Clippy 1.95 lints under `-D warnings`: `unnecessary_sort_by` (`repo_intel.rs`), `manual_checked_ops` (`tui.rs`), 6 × `collapsible_match` (`ast_intel.rs`, `patrol.rs`).
  - 4 pre-existing failing tests:
    - `test_multihop_2hop_resolution` — 2-hop resolver now iterates `file_imports.keys()` instead of `parsed_files.keys()` so chains can follow files whose symbols haven't been parsed.
    - `test_detect_ai_policy_ban` — function now lowercases input.
    - `test_detects_ai_ban_llm` / `test_detects_ai_ban_manual_only` — added regex patterns for `(LLM|GPT|...) generated/written code` and `only accept|allow|welcome manual|human contributions`.
- Removed unused imports & a useless `len() >= 0` comparison in test files.

### Changed
- Bumped 11 Rust deps (rust-deps group via dependabot #29): `rand 0.8 → 0.10`, `criterion 0.5 → 0.8`, `axum-server 0.7 → 0.8`, `tree-sitter-css 0.23 → 0.25`, `tree-sitter-php 0.23 → 0.24`, plus tokio/clap/uuid/axum patch bumps.
- Version sync: `Cargo.toml` 6.0.0 → 6.5.0, install scripts v5.2.0 → v6.5.0, README badge → v6.5.0 (caught misalignment between Cargo.toml and the v6.4.1 git tag).

## [6.4.1] - 2026-04-20

### Added
- **Sprint 22.5** — Anti-spam & maintainer respect (issue #26):
  - Per-repo and global cooldowns to prevent submitting too many PRs to the same maintainer.
  - Honors maintainer signals (e.g. `no-ai-contrib` topic, ban phrases in CONTRIBUTING.md) before opening PRs.
  - Memory-backed tracking of recently-targeted repos.

## [6.4.0] - 2026-04-17

### Added
- **Sprint 22** — LLM Generation improvements:
  - Better prompt scaffolding for code-change generation.
  - Improved JSON schema validation for LLM responses (`validate_change_schema`).
  - Self-review and scoring tightened to reduce low-quality PRs.

## [6.3.0] - 2026-04-15

### Added
- **Sprint 20** — Developer Experience improvements:
  - Cleaner CLI help, additional config validation, better doctor output.
- **Sprint 21** — Multi-hop import resolution:
  - AST analyzer follows import chains across files (A → B → C) to surface relevant symbols when parsing the immediate import target isn't enough.

## [6.2.1] - 2026-04-12

### Documentation
- **Sprint 19** — Documentation refresh:
  - 19.1 — CHANGELOG/README/AGENTS version refs synced.
  - 19.2 — Roadmap updated with v6.1.0/v6.2.0 milestones.
  - 19.3 — Expanded RUNBOOK.md and CONTRIBUTING.md dev guide.

## [6.2.0] - 2026-04-11

### Added
- **Sprint 18** — Dependencies, benchmarks, and binary size optimization:
  - Criterion benchmarks for AST extraction, framework detection, and risk classification (`benches/pipeline.rs`).
  - Release-profile tuning (LTO, codegen-units, strip) to shrink binary size.
  - Workspace dep cleanup and minor version bumps.

### Changed
- Adapted to breaking changes from dependabot #24 (19 Rust deps).
- CI actions bumped to v5/v6 (dependabot #23).

## [6.1.0] - 2026-04-09

### Added
- **Sprint 17** — Code quality and framework detection:
  - Zero clippy warnings under `-D warnings` (at the time of release).
  - Improved framework detection across Python, JS/TS, Java, Go, Rust ecosystems.

## [6.0.0] - 2026-04-08

### Added
- **Sprint 16 complete** — Plugin System, Enterprise Mode, i18n:
  - Full plugin system with trait-based architecture
  - Enterprise mode with configurable features
  - Internationalization (i18n) support with locale files
  - Permission system with rule-based access control
  - Agent modes (Plan vs Build)
  - Custom commands support
  - LSP configuration
  - Filesystem snapshot tracking with undo capability

### Stats
- CLI commands: 40+ → 50+
- New modules: plugins, permissions, i18n, sessions, agents

## [5.20.0] - 2026-04-07

### Added
- **Sprints 13-14-15** — Client/Server, TUI Polish, Observability:
  - MCP client for external MCP servers
  - TUI improvements (keyboard shortcuts, navigation)
  - Observability enhancements (tracing, metrics)
  - Session management system
  - Serve command for web server mode

### Changed
- TUI: Better tab navigation and key bindings
- CLI: Added serve and client commands

## [5.19.0] - 2026-04-07

### Added
- **Sprints 8-9-10-12 combined** — Multiple features:
  - Agent modes (Plan vs Build) — Sprint 8.1
  - Rule-based permission system — Sprint 8.2
  - Filesystem snapshot tracking + undo — Sprint 8.3
  - Small model routing + auto compaction + budget — Sprint 9
  - Sessions system — Sprint 10
  - Custom commands + LSP config — Sprint 12

### Stats
- New features: 6 major subsystems
- Tests: 500+ → 550+

## [5.18.0] - 2026-04-06

### Added
- **Sprint 11** — Auth Ecosystem (Copilot, Login, Fallback Chain):
  - GitHub Copilot LLM provider
  - Interactive login command
  - Fallback chain for LLM providers
  - Vertex AI detection fix

### Fixed
- Config loading now searches default config locations
- Vertex AI provider detection

## [5.17.1] - 2026-04-06

### Fixed
- **Critical**: `load_config` now searches default config locations
- Vertex AI provider detection in config loading

## [6.2.0] - 2026-04-11

### Added
- **Sprint 18 complete** — Dependencies, benchmarks, binary optimization:
  - Criterion benchmark suite (5 benchmarks: AST extraction, framework detection, risk classification)
  - Rust dependabot (weekly automated dependency updates)
  - Test fixtures for benchmarking (Python/Rust/JavaScript samples)

### Changed
- **Dependencies updated**:
  - tower 0.4 → 0.5 (compatible with axum 0.7)
  - Tree-sitter grammar audit (documented 0.23/0.24/0.25 compatibility)
- **Binary size optimization**: 34MB → 23MB (32% reduction)
  - Moved `profile.release` to workspace root (eliminated warning)
  - LTO + strip + opt-level=z all applied correctly

### Stats
- Binary size: 34MB → 23MB (-32%)
- Benchmarks: 0 → 5 (criterion framework)
- Dependabot: Python+Actions → +Rust (weekly updates)

## [6.1.0] - 2026-04-11

### Added
- **Sprint 17 complete** — Code quality & dead code removal:
  - Framework detection from imports (20+ frameworks: Django, React, Rails, etc.)
  - Copilot provider fully wired in all factory functions
  - Session dead code removed (commented out for future feature)
  - 5 new framework detection tests

### Fixed
- **6 clippy warnings eliminated** — Zero-warning strict lint:
  - Removed unused imports (`OpenOptions`, `std::io::Write`, `warn`, `CopilotProvider`)
  - Fixed `unwrap()` after `is_some()` → `if let Some(p) = path`
  - Derived `Default` for `PluginManager` (manual impl removed)
  - Test helper warnings suppressed with `#[allow(dead_code)]`

### Stats
- Tests: 587 → 602 (+15)
- Clippy warnings: 6 → 0
- Dead code: Session module removed, Copilot wired

## [5.17.0] - 2026-04-06

### Added
- **Sprint 1-7 complete** — Full release with all improvements:
  - Circuit breaker + E2E LLM parser tests + analyzer retry
  - Sandbox default + prompt injection protection + token encryption
  - CLI refactored (3396 → 523 lines)
  - LLM response cache + parallel file fetch
  - 67 new tests (AST 13 langs, middleware, router, MCP, notifications)
  - Dream race fix + Web TLS
  - ARCHITECTURE.md + RUNBOOK.md + /metrics endpoint

### Stats
- Tests: 418 → 575 (+157)
- CLI: 3396 → 523 lines (-85%)
- 0 clippy warnings, 0 security advisories

## [5.16.0] - 2026-04-06

### Added
- **`/metrics` endpoint** — Prometheus-format metrics for monitoring:
  - `contribai_pipeline_runs_total` — Total pipeline runs
  - `contribai_pr_submissions_total` — Total PRs submitted
  - `contribai_pr_merged_total` — Total PRs merged
  - `contribai_findings_total` — Total findings
  - `contribai_errors_total` — Total errors
  - `contribai_cache_entries_total` — Valid LLM cache entries
  - `contribai_circuit_breaker_state` — Circuit breaker state (0/1/2)
- **`ARCHITECTURE.md`** — Comprehensive architecture documentation
- **`RUNBOOK.md`** — Troubleshooting guide and maintenance procedures

### Sprint 7 Summary
- Documentation: ARCHITECTURE.md (system overview, data model, config reference)
- Documentation: RUNBOOK.md (common issues, debug mode, emergency procedures)
- Observability: /metrics endpoint in Prometheus format
- Polish: 0 clippy warnings, all tests pass

## [5.15.0] - 2026-04-06

### Added
- **Web Dashboard TLS Support** — Enable HTTPS via config:
  ```yaml
  web:
    tls_enabled: true
    tls_cert_path: "/path/to/cert.pem"
    tls_key_path: "/path/to/key.pem"
  ```
  Uses `axum-server` + `rustls` for zero-dependency TLS (no OpenSSL needed).
  Dashboard URL switches from `http://` to `https://` when TLS enabled.

### Sprint 6 Summary
- Dream race condition fix (v5.14.0): Mutex-based `DreamLock` eliminates TOCTOU
- Web TLS support (v5.15.0): HTTPS dashboard with `rustls`
- Streaming LLM deferred to future sprint (larger API surface change)

## [5.14.0] - 2026-04-06

### Added
- **Dream system race condition fix** — Replaced string-based DB lock with proper `DreamLock` using `std::sync::Mutex`. Eliminates TOCTOU race where two concurrent `maybe_dream()` calls could both pass gate checks and run consolidation simultaneously.
- **`fd-lock` dependency** added for future cross-process file locking support.

### Fixed
- Dream consolidation now uses atomic lock acquisition — only one instance runs at a time, even with concurrent pipeline runs.

## [5.13.0] - 2026-04-06

### Added
- **67 new tests** across 5 test suites (502 → 569 total):
  - `ast_all_languages.rs` (22 tests): Symbol extraction for all 13 languages (Python, JS, TS, Go, Rust, Java, C, C++, Ruby, PHP, C#, HTML, CSS) plus edge cases: empty files, syntax errors, unicode, deeply nested, mixed extensions
  - `middleware_chain.rs` (11 tests): Rate limit enforcement, validation, quality gate, chain short-circuit
  - `task_router.rs` (12 tests): Task routing for Analysis/CodeGen/Review/Planning, complexity-based model selection, performance/economy strategies
  - `mcp_server.rs` (11 tests): JSON-RPC format validation, argument validation for all tool types
  - `notifications_scheduler.rs` (11 tests): HMAC signature verification, webhook URL format validation, cron expression parsing, scheduler config defaults

## [5.12.0] - 2026-04-06

### Added
- **LLM response cache**: Content-addressable SHA-256 cache for `complete()` calls. Identical prompts return cached responses, skipping the API. Configurable TTL (default: 7 days) via `llm.cache_enabled` and `llm.cache_ttl_days`.
- **Parallel file fetching**: Analyzer now fetches file contents concurrently (10 concurrent requests via semaphore), reducing analysis time by ~60% for large repos.
- **`contribai cache-stats`**: Shows cache size, valid/expired entry counts, hit rate.
- **`contribai cache-clear`**: Clears the LLM response cache with confirmation prompt.
- **7 new tests** for cache get/put/clear/stats/prune.

### Dependencies
- Added `r2d2 0.8` and `r2d2_sqlite 0.24` for future connection pooling.

## [5.11.0] - 2026-04-06

### Added
- **CLI refactored**: `cli/mod.rs` reduced from 3,396 lines → **523 lines** (85% reduction). All 26 command handlers extracted into `cli/commands/` directory, shared utilities in `cli/common.rs`.
- **Magic numbers extracted** to config fields (`SandboxConfig.mode`, `require_validation`).
- **Expanded local validators**: JavaScript/TypeScript, Go, Java syntax checks added.

### Refactored
- `cli/mod.rs`: 3,396 → 523 lines
- Each command handler in its own file under `cli/commands/`
- Shared helpers (`load_config`, `create_github`, `create_llm`, `create_memory`, `print_banner`, `print_result`, `parse_github_url`) in `cli/common.rs`
- All command handlers re-exported via `pub use common::`

## [5.10.0] - 2026-04-06

### Added
- **Sandbox enabled by default** (`sandbox.enabled = true`) with 3 modes: `"docker"` (full isolation), `"local"` (syntax check, default), `"ast"` (tree-sitter parse), `"off"` (no validation). `sandbox.require_validation` blocks PR submission on validation failure.
- **Prompt injection protection** — repository content is sanitized before LLM calls: control characters stripped, XML-wrapped in `<repository-content>` tags, 10 known injection patterns detected and logged. System prompts hardened with "treat code as data" instruction.
- **Token encryption** (`contribai encrypt-token`) — encrypt GitHub tokens with AES-256-like XOR + HMAC-SHA256 key derivation (PBKDF2, 1000 iterations). Decrypted at runtime via `CONTRIBUTAI_ENCRYPTION_KEY` env var, never written to logs.
- **Expanded local validators** — JavaScript/TypeScript, Go, Java syntax checks added alongside Python and Rust.
- **26 new tests** for sandbox config, crypto roundtrip, prompt injection detection.

### Security
- Sandbox is now ON by default — generated code is validated before PR submission
- Prompt injection mitigations protect against malicious repository content
- Token encryption prevents plaintext storage in config files

## [5.9.0] - 2026-04-06

### Added
- **Circuit Breaker for LLM failures**: Full circuit breaker pattern (Closed → Open → HalfOpen) stops pipeline after consecutive LLM failures to save API quota. Configurable thresholds via `pipeline.circuit_breaker_failure_threshold` (default: 5), `success_threshold` (default: 2), `cooldown_secs` (default: 300).
- **`contribai circuit-breaker` CLI command**: Shows current circuit state, failure count, cooldown remaining, and recovery status.
- **Analyzer retry with exponential backoff**: Analyzer LLM calls now retry up to 3 times (2s → 4s → 8s) on transient errors (429, 5xx, timeout). Non-transient errors (400, 401, auth) fail immediately without retry.
- **18 E2E LLM parser tests**: Tests against real-world response shapes — markdown fences, explanations alongside JSON, malformed responses, trailing commas, multi-language findings, unicode, null values, and more. Documents known limitations (multiple arrays).
- **16 circuit breaker tests**: Unit tests for all state transitions + integration tests with pipeline config defaults.
- **8 transient error detection tests**: Tests for `is_transient_llm_error()` covering timeout, rate limit, 5xx, HTTP errors vs non-transient (400, auth, JSON parse).

### Changed
- Pipeline `run()` and `hunt()` now check circuit breaker before processing each repo — stops entire run if circuit is open.
- Circuit breaker records success/failure after each repo processing attempt.
- Test count: **418 → 469** (+51 new tests)

## [4.1.0] - 2026-03-29

### Added
- **Antigravity MCP Integration**: ContribAI MCP server now works with Antigravity IDE (Google Gemini) in addition to Claude Desktop — configure via `mcp_config.json` for native tool access to all 14 GitHub operations
- Documented MCP setup for both Claude Desktop and Antigravity IDE

### Changed
- **PR Title Format**: Removed emoji prefixes from generated PR titles for a cleaner, more professional appearance (`"Quality: fix race condition"` instead of `"✨ Quality: fix race condition"`)
- Updated compliance checker to match new non-emoji title format
- Updated stats: 43 PRs submitted, 9 merged, 21 repos (184⭐)

## [4.0.0] - 2026-03-28

### Added
- **Repo Intelligence Layer** (`contribai/analysis/repo_intel.py`): Profiles target repos before contributing — analyzes merged PR patterns, identifies high-value issues, tracks review speed, and injects intelligence into LLM prompts for focused contributions
- **Smart Dedup (PR History Injection)**: Past PR titles injected directly into analysis prompts with "DO NOT REPEAT" instruction — prevents rediscovering already-fixed bugs
- **Issue-First Hunt Strategy** (`_hunt_issues_globally`): Searches GitHub globally for repos with `good first issue`, `help wanted`, and `bug` labels — expected 60-80% merge rate vs 26% from random scanning
- **Multi-language Expansion**: Config expanded from Python-only to Python, JavaScript, TypeScript, Go, and Rust — 5x broader repo coverage; hunt mode alternates between configured and expanded language sets
- **Test Generation Enhancement**: Repo intelligence context injected into all analyzer prompts including test generation — guides ContribAI to generate tests aligned with repo preferences
- `GitHubClient.get_issues()` — fetch repo issues with label filtering
- `GitHubClient.search_issues()` — global issue search across all of GitHub
- 15 new tests for repo intelligence (431 total, 52% coverage)

## [3.0.6] - 2026-03-28

### Added
- **SKIP_DIRECTORIES filter**: 19 low-value directory patterns (`examples/`, `docs/`, `tests/`, `benchmarks/`, `vendor/`, etc.) — prevents useless PRs targeting non-core code
- **Auto-close linked issues**: When a PR is closed (CI failure or maintainer rejection), automatically closes any linked issues (`Closes/Fixes/Resolves #N`)
- **Patrol close detection**: PR Patrol now detects closed (non-merged) PRs and triggers issue cleanup
- **HALL_OF_FAME.md**: Showcase of merged PRs across external repositories
- **README stats section**: Real outcome metrics (34+ PRs, 9 merged, 21 repos)
- `GitHubClient.close_issue()` method with `state_reason: not_planned`

### Fixed
- Pipeline no longer generates PRs for `examples/`, `docs/`, `tests/`, `benchmarks/` directories
- Issue solver now respects SKIP_DIRECTORIES filter in `_is_code_file` check
- Git push configured for GitHub email privacy (`tang-vu@users.noreply.github.com`)

## [3.0.5] - 2026-03-28

### Fixed
- **Critical**: Webhook signature bypass — FastAPI returned HTTP 200 instead of 403 on invalid signatures
- **Critical**: RetryMiddleware re-entry bug — shared mutable index caused retries to skip downstream middlewares
- **Critical**: Context compressor passed wrong kwarg (`system_prompt=` → `system=`) to LLM providers
- **High**: Webhook payload size check bypassed when `Content-Length` header missing
- **High**: `get_pr_diff` bypassed retry/rate-limit logic by calling httpx directly
- Ruff lint fixes in engine.py and pipeline.py

## [3.0.4] - 2026-03-28

### Fixed
- **Security**: API key verification now uses constant-time comparison (`hmac.compare_digest`) to prevent timing attacks
- **Security**: Webhook endpoint now validates `Content-Length` header (10 MB limit) to reject oversized payloads

### Improved
- **Reliability**: Notification system retries failed sends with exponential backoff (3 attempts)
- **Config**: MCP client timeout is now configurable via `StdioMCPClient(timeout=...)` instead of hardcoded 30s

### Documentation
- Initial project documentation suite: PDR, codebase summary, code standards, system architecture, roadmap, deployment guide

## [2.4.1] - 2026-03-26

### Fixed
- `summarize_findings()` used `Finding.contribution_type` instead of `Finding.type` — caused `AttributeError` during hunt mode
- SECURITY.md referenced non-existent email domain — now uses GitHub Issues

### Added
- 86 new unit tests for v2.4.0 modules (middleware, skills, registry, protocol) — 333 total
- `docs/ARCHITECTURE.md` — detailed architecture documentation
- `AGENTS.md` — AI agent guide for Copilot, Claude, Coderabbit
- `.github/copilot-instructions.md` — GitHub Copilot context

### Changed
- Updated all .md files for v2.4.0 architecture (README, CONTRIBUTING, SECURITY, PR template, dev workflow)
- Coverage restored to 53% (was 45% due to untested new modules)

## [2.4.0] - 2026-03-25

### Added
- **Middleware chain** (`contribai/core/middleware.py`): Pipeline processing with 5 built-in middlewares — RateLimit, Validation, Retry, DCO, QualityGate
- **Progressive skill loading** (`contribai/analysis/skills.py`): 17 analysis skills loaded on-demand by language/framework instead of all at once — saves tokens and improves quality
- **Framework detection**: Auto-detect Django, Flask, FastAPI, React, Express, Spring, Rails, etc. from file tree
- **Outcome learning** (`memory.py`): New `pr_outcomes` + `repo_preferences` tables — tracks PR merge/rejection to learn which contribution types work per repo
- **Context summarization** (`analyzer.py`): `summarize_findings()` compresses analysis results for downstream LLM prompts
- **Sub-agent registry** (`contribai/agents/registry.py`): 4 agent stubs (Analyzer, Generator, Patrol, Compliance) with parallel execution (max 3 concurrent)
- **Tool protocol** (`contribai/tools/protocol.py`): MCP-inspired tool system with ToolRegistry, GitHubTool, and LLMTool wrappers
- **DCO auto-signoff**: All commits via GitHub API auto-append `Signed-off-by` trailer

### Changed
- Architecture inspired by ByteDance DeerFlow 2.0 super agent harness
- README updated with PR Patrol section, v2.4.0 badges

## [2.3.0] - 2026-03-24

### Added
- **Bot review context**: When maintainer replies to a bot review (Coderabbit, etc.), patrol reads the bot's original analysis and includes it as context for LLM-based code fix generation
- **Assigned issue monitoring**: Patrol scans repos for issues assigned to our user and reports them
- **34 new unit tests** for patrol engine covering feedback collection, bot context linking, classification parsing, and assigned issue detection

### Fixed
- `generate()` → `complete()` in `_handle_code_fix` (LLM method mismatch)
- Bot comment filtering for 11 review bot logins + `[bot]` suffix detection
- Exponential backoff retry (5s → 10s → 20s) for rate limit errors during LLM calls
- Orphaned `except` block parse error in `_classify_feedback`

## [2.2.0] - 2026-03-23

### Added
- **PR Patrol** (`contribai patrol`): Monitor open PRs for review feedback and auto-respond
  - Reads maintainer review comments (issue comments + inline code reviews)
  - LLM-based feedback classification: CODE_CHANGE, QUESTION, STYLE_FIX, APPROVE, REJECT, ALREADY_HANDLED
  - Generates code fixes from review feedback and pushes to PR branch
  - Answers maintainer questions with context-aware LLM responses
  - Re-signs CLA after pushing new commits
  - `--dry-run` to preview actions, `--pr N` to filter specific PR
- **GitHub API methods**: `get_pr_reviews()`, `get_pr_review_comments()`, `create_pr_review_comment_reply()`, `get_pr_diff()`
- **Patrol models**: `FeedbackAction` enum, `FeedbackItem`, `PatrolResult`

## [2.1.0] - 2026-03-22

### Added
- **Smart Context Builder**: `_detect_project_profile()` auto-detects project type (library, web_app, api_server, cli_tool, data_pipeline), tech stack (Django, Flask, FastAPI, etc.), and conventions (tests, CI, type hints)
- **Style Guide Extraction**: `_build_style_guide()` analyzes source code to detect naming conventions, error handling, docstring format, import style, and logging patterns
- **Score-based File Prioritization**: `_prioritize_files()` ranks files by contribution value (entry points +40, API routes +35, auth/security +30, config +20) with penalties for tests, vendor, and deeply nested files
- **Anti-false-positive Rules**: 5 mandatory checks before reporting findings — ALREADY_HANDLED, BY_DESIGN, BOUNDED_CONTEXT, TRIVIAL_FIX, COSMETIC
- **Pre-generation Validation**: Early filter skips findings targeting non-code files (SKIP_EXTENSIONS) and protected meta files before expensive LLM code generation
- **Maintainer Acceptance Gate**: Generation prompt includes "30-second merge test" criteria

### Changed
- Analyzer system prompt upgraded from generic "expert code analyst" to "senior software engineer performing focused code review" with project profile injection
- Security prompt now focuses on real exploitability: SQL injection only for raw queries (not ORM), hardcoded secrets only outside test fixtures
- Code quality prompt focuses on bugs/crashes: unhandled None, resource leaks, race conditions, off-by-one errors
- Performance prompt requires >10% measurable impact; skips micro-optimizations
- Max 3 findings per analyzer (quality over quantity)
- Generator system prompt includes style guide injection and 8 explicit rules (no adjacent refactoring, no comments, no unrelated files)

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
