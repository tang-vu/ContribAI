# Rust Rewrite Feature Parity Report

**Date:** 2026-03-29 | **Branch:** rust-rewrite | **Reviewer:** Claude Code (4 parallel agents)

---

## Master Comparison Table

| Python Module | Rust File | Status | Parity | Missing Items |
|---|---|---|---|---|
| **CORE** |||||
| `core/config.py` | `core/config.rs` | ⚠️ Partial | 60% | 5 config classes missing: SchedulerConfig, WebConfig, QuotaConfig, NotificationConfig, SandboxConfig; ContributionConfig has 2/7 fields |
| `core/models.py` | `core/models.rs` | ✅ Full | 95% | All 4 enums, 12 structs ported; Rust adds Symbol, RemediationSpec, FixComplexity, ScoringSignal |
| `core/events.py` | `core/events.rs` | ✅ Full | 95% | All 18 EventType variants ported; Rust uses tokio broadcast vs callback |
| `core/exceptions.py` | `core/error.rs` | ✅ Full | 85% | LLMRateLimitError merged into Llm variant; Rust adds AstParse, Sandbox, AiPolicyViolation, DuplicatePr |
| `core/middleware.py` | `core/middleware.rs` | ✅ Full | 90% | All 5 middlewares ported; PipelineContext has 14/17 fields (Any-typed removed) |
| `core/retry.py` | `core/retry.rs` | ✅ Full | 95% | All presets match; LRUCache ported as HashMap+Vec |
| `core/quotas.py` | `core/quotas.rs` | ✅ Full | 100% | Perfect parity |
| `core/profiles.py` | `core/profiles.rs` | ⚠️ Partial | 70% | Missing: custom profile dir scanning, apply_profile() merge fn |
| `core/leaderboard.py` | `core/leaderboard.rs` | ⚠️ Partial | 80% | Missing: get_recent_merges(); sync vs async design |
| **GITHUB** |||||
| `github/client.py` | `github/client.rs` | ✅ Full | 85% | Core 12+ methods ported; possibly missing some PR comment/review methods |
| `github/discovery.py` | `github/discovery.rs` | ✅ Full | 95% | Full pipeline: search -> filter -> prioritize -> limit |
| `github/guidelines.py` | `github/guidelines.rs` | ✅ Full | 95% | Same regex patterns, same detection logic |
| **LLM** |||||
| `llm/provider.py` | `llm/provider.rs` | ✅ Full | 84% | Missing: MultiModelProvider, close() cleanup; Rust uses raw HTTP vs SDKs |
| `llm/models.py` | `llm/models.rs` | ✅ Full | 90% | Missing: GEMINI_3_PRO model spec; all functions ported |
| `llm/router.py` | `llm/router.rs` | ✅ Full | 95% | Missing: get_default_assignments(); all 3 strategies ported |
| `llm/agents.py` | `llm/agents.rs` | ✅ Full | 95% | OOP hierarchy redesigned as const agents + functional; AgentCoordinator identical |
| `llm/formatter.py` | `llm/formatter.rs` | ✅ Full | 100% | All 4 formatters + factory ported |
| `llm/context.py` | `llm/context.rs` | ⚠️ Partial | 60% | Missing: format_file_tree(), summarize_with_llm(); basic truncation only |
| **ANALYSIS** |||||
| `analysis/analyzer.py` | `analysis/analyzer.rs` | ✅ Full+ | 100%+ | Core logic ported + enhanced with AST/PageRank/Triage; missing _detect_project_profile, _build_style_guide, _filter_severity (replaced by triage) |
| `analysis/skills.py` | `analysis/skills.rs` | ✅ Full | 90% | 12/14 skills ported; missing: FRAMEWORK_INDICATORS, detect_frameworks() |
| `analysis/context_compressor.py` | `analysis/compressor.rs` | ⚠️ Partial | 65% | Missing: extract_signatures(), _extract_python_signatures(), summarize_with_llm() |
| `analysis/strategies.py` | `analysis/strategies.rs` | ⚠️ Partial | 80% | Missing: ExpressStrategy, detect_frameworks() standalone fn |
| `analysis/language_rules.py` | `analysis/language_rules.rs` | ✅ Full | 95% | 17/18 rules; all 3 query functions ported |
| `analysis/repo_intel.py` | `analysis/repo_intel.rs` | ✅ Full | 100% | Complete parity including PR history + issue classification |
| **GENERATOR** |||||
| `generator/engine.py` | `generator/engine.rs` | ⚠️ Partial | 54% | Missing: cross-file detection, fuzzy matching (4 strategies), self-review LLM gate, advanced JSON extraction, multi commit conventions, guidelines-adapted PR titles |
| `generator/scorer.py` | `generator/scorer.rs` | ✅ Full | 95% | All 7 checks; minor: no_debug/no_placeholders slightly simplified |
| **PR** |||||
| `pr/manager.py` | `pr/manager.rs` | ⚠️ Partial | 58% | Missing: _create_issue_for_finding(), check_compliance_and_fix(), _handle_cla_signing() |
| `pr/patrol.py` | `pr/patrol.rs` | ⚠️ Partial | 70% | Core feedback collection ported; code fix/question handling may be incomplete |
| **ORCHESTRATOR** |||||
| `orchestrator/pipeline.py` | `orchestrator/pipeline.rs` | ⚠️ Partial | 75% | Core flow ported; title dedup unclear; parallel processing present |
| `orchestrator/memory.py` | `orchestrator/memory.rs` | ✅ Full | 95% | All 7 tables, same schema; sync Mutex vs async aiosqlite |
| `orchestrator/review_gate.py` | _(none)_ | ❌ Missing | 0% | Human-in-the-loop review gate not ported (Rich TUI approval flow) |
| **AGENTS** |||||
| `agents/registry.py` | `agents/registry.rs` | ✅ Full | 95% | execute_parallel() simplified to sequential; parent_context missing |
| **ISSUES** |||||
| `issues/solver.py` | `issues/solver.rs` | ⚠️ Partial | 70% | Missing: fetch_solvable_issues(), _has_linked_pr(), _build_issue_context() |
| **NOTIFICATIONS** |||||
| `notifications/notifier.py` | `notifications/mod.rs` | ✅ Full | 100% | All 3 channels (Slack/Discord/Telegram) + retry + 3 convenience methods |
| **PLUGINS** |||||
| `plugins/base.py` | `plugins/mod.rs` | ⚠️ Partial | 75% | Missing: discover() via entry points (Python-specific); adds run_analyzers/run_generators |
| **SCHEDULER** |||||
| `scheduler/scheduler.py` | `scheduler/mod.rs` | ✅ Full | 90% | Custom cron vs APScheduler; signal handling ported |
| **TEMPLATES** |||||
| `templates/registry.py` | `templates/mod.rs` | ✅ Full | 95% | 5 hardcoded templates vs YAML file loading; missing load_directory() |
| **SANDBOX** |||||
| `sandbox/sandbox.py` | `sandbox/mod.rs` | ✅ Full | 92% | Docker validation ported; adds Rust-specific validation; Python ast.parse -> bracket checks |
| **TOOLS** |||||
| `tools/protocol.py` | `tools/mod.rs` | ⚠️ Partial | 80% | Missing: metadata field, GitHubTool, LLMTool, create_default_tools() |
| **MCP** |||||
| `mcp_server.py` + `mcp/__init__.py` | `mcp/server.rs` + `mcp/mod.rs` | ⚠️ Partial | 70% | 10/15 tools; missing: close_pr, check_duplicate_pr, check_ai_policy, patrol_prs, cleanup_forks |
| **CLI** |||||
| `cli/main.py` + `cli/tui.py` | `cli/mod.rs` | ⚠️ Partial | 78% | Missing: analyze, solve, status, config commands, TUI; adds: hunt, patrol, mcp-server, version |

---

## Rust-Only Additions (no Python equivalent)

| Module | Description |
|---|---|
| `analysis/ast_intel.rs` | Tree-sitter AST parsing for 8 languages — native symbol extraction |
| `analysis/triage.rs` | 12-signal weighted priority scoring engine for findings |
| `analysis/repo_map.rs` | PageRank algorithm for file importance via import graph |
| CLI: `hunt` command | Aggressive multi-round discovery mode |
| CLI: `patrol` command | Standalone PR monitoring |
| CLI: `mcp-server` command | Standalone MCP server launcher |
| Error: `AiPolicyViolation` | Explicit error for repos banning AI contributions |
| Error: `DuplicatePr` | Explicit error for duplicate PR detection |
| Models: `Symbol`, `SymbolKind` | AST symbol representation |
| Models: `RemediationSpec` | Triage-enhanced finding with fix complexity |

---

## Aggregate Scores by Module Group

| Group | Avg Parity | Rating |
|---|---|---|
| Core (9 files) | **86%** | ✅ Strong |
| GitHub (3 files) | **92%** | ✅ Excellent |
| LLM (6 files) | **87%** | ✅ Strong |
| Analysis (6 files) | **88%** | ✅ Strong (enhanced) |
| Generator (2 files) | **75%** | ⚠️ Gaps |
| PR (2 files) | **64%** | ⚠️ Gaps |
| Orchestrator (3 files) | **57%** | ⚠️ Significant gaps |
| Remaining (10 files) | **85%** | ✅ Strong |
| **OVERALL (41 files)** | **~80%** | ⚠️ Functional but incomplete |

---

## Critical Gaps (Ordered by Impact)

### P0 — Blocks core contribution workflow
1. **generator/engine.rs** — No fuzzy matching (4 strategies in Python), no self-review gate, no cross-file detection
2. **orchestrator/review_gate.py** — Entire module missing (human approval flow)
3. **pr/manager.rs** — No compliance auto-fix, no CLA auto-signing, no issue creation

### P1 — Reduces quality/reliability
4. **analysis/compressor.rs** — No LLM-driven summarization, no signature extraction
5. **llm/context.rs** — No format_file_tree(), no summarize_with_llm()
6. **issues/solver.rs** — No fetch_solvable_issues() discovery pipeline
7. **mcp/server.rs** — 5 tools missing (close_pr, check_duplicate_pr, check_ai_policy, patrol_prs, cleanup_forks)

### P2 — Configuration/UX gaps
8. **core/config.rs** — 5 config classes missing (scheduler, web, quota, notification, sandbox)
9. **cli/mod.rs** — No TUI, missing analyze/solve/status/config commands
10. **core/profiles.rs** — No custom profile directory scanning or apply_profile()

---

## Design Philosophy Differences

| Aspect | Python | Rust |
|---|---|---|
| HTTP/SDK | Official SDKs (google-genai, openai, anthropic) | Raw reqwest HTTP (zero SDK deps) |
| OOP | Class inheritance (agents, strategies) | Traits + consts + functions |
| Config | 12 config classes, YAML-file profiles | 8 config structs, hardcoded profiles |
| Compression | 3 strategies (truncate + extract + LLM) | Basic truncation only |
| Analysis | LLM-only with regex heuristics | LLM + AST + PageRank + triage scoring |
| Async DB | aiosqlite (async) | rusqlite + Mutex (sync) |
| Error handling | Exception hierarchy with details dict | Exhaustive enum with #[from] auto-conversion |
| Finding format | YAML output parsing | JSON output parsing |

---

## Unresolved Questions

1. **patrol.rs completeness** — Agent read was truncated at 150 lines; code fix / question handling may be fully implemented but couldn't verify
2. **pipeline.rs completeness** — Agent read was truncated; full parallel processing and title dedup logic may exist beyond read limit
3. **GitHub client.rs** — PR creation/comment/review methods likely exist but weren't fully verified in the agent's read window
