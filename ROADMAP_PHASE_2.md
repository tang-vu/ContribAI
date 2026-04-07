# ContribAI — Sprint Roadmap Phase 2

> Learnings from OpenCode, Claude Code, and other coding agents.
> Estimated: 8 sprints × 2 weeks = 16 weeks total.

---

## Sprint 8: Agent Modes + Permission System
**Duration:** 2 weeks
**Priority:** 🔴 CRITICAL

### 8.1: Agent Modes (Plan vs Build)
- Add `--mode` flag: `plan` (read-only analysis) | `build` (full PR flow)
- `plan` mode: phân tích + báo cáo, KHÔNG generate code hoặc tạo PR
- `build` mode: full pipeline như hiện tại
- Config: `pipeline.default_mode: "build"`
- TUI: `Tab` để switch giữa plan/build

### 8.2: Rule-Based Permission System
- Replace `sandbox.enabled` với granular `permissions` config:
  ```yaml
  pipeline:
    permissions:
      file_read: "allow"                        # always allow reading files
      file_edit: { "src/**": "allow", "**/config.*": "ask", "**/*.lock": "deny" }
      file_create: { "tests/**": "allow", "**": "ask" }
      file_delete: "deny"
      shell_command: { "cargo test": "allow", "cargo build": "allow", "*": "ask" }
      pr_create: "ask"                           # ask before creating PR
      pr_push: "allow"                           # auto-push after ask
  ```
- Permission actions: `"allow"` | `"ask"` | `"deny"`
- Pattern matching: glob patterns (`**`, `*`, `?`)
- Wildcard: `"*"` matches everything
- Interactive prompt: khi `ask` → hỏi user yes/no/skip/all
- Non-interactive: khi không có terminal → `ask` → `deny`

### 8.3: Filesystem Snapshot Tracking
- Track file diffs trước/sau khi generate code
- Store in `file_snapshots` table: `repo`, `path`, `before`, `after`, `timestamp`
- Support `contribai undo` — rollback last generated changes
- Config: `pipeline.snapshot: true` (default)

**Acceptance Criteria:**
- ✅ `contribai analyze --mode plan` runs analysis only
- ✅ `contribai run --mode build` runs full pipeline
- ✅ Permission rules enforced — denied actions blocked
- ✅ `contribai undo` reverts last code changes
- ✅ 15+ tests for permission matching

---

## Sprint 9: Small Model Routing + Auto Compaction
**Duration:** 2 weeks
**Priority:** 🔴 CRITICAL

### 9.1: Small Model Routing
- Add `llm.small_model` config — cheap model cho lightweight tasks:
  ```yaml
  llm:
    model: "gemini/gemini-3-pro"         # primary model
    small_model: "gemini/gemini-3-flash" # cheap model for light tasks
  ```
- Route automatically:
  - PR title generation → small model
  - Commit message generation → small model
  - Context compaction/summarization → small model
  - Code analysis → primary model
  - Code generation → primary model
- Fallback: nếu `small_model` không set → dùng `model`

### 9.2: Auto Context Compaction
- Khi context window vượt threshold (default: 80%), tự động compress:
  1. Summarize old messages thành 1-2 paragraph
  2. Keep recent messages + system prompt intact
  3. Use `small_model` for summarization
- Config:
  ```yaml
  pipeline:
    compaction:
      auto: true
      threshold: 0.8       # 80% of max context
      reserved_tokens: 4096 # reserve for response
      prune: true           # prune old tool results
  ```
- Compaction strategy:
  - Summarize tool outputs → 1 line each
  - Merge consecutive assistant messages
  - Keep file contents only if still referenced

### 9.3: Context Budget Manager
- Real-time context tracking: `used_tokens / max_tokens`
- Display budget trong TUI: `[████████░░░░░░] 62%`
- Auto-compaction trigger khi vượt threshold
- Manual compaction: `contribai compact-context`

**Acceptance Criteria:**
- ✅ Small model used for PR titles, commit messages
- ✅ Context auto-compacts at 80% threshold
- ✅ TUI shows context budget bar
- ✅ 10+ tests for compaction logic
- ✅ Cost savings: ~30% fewer tokens on average run

---

## Sprint 10: Custom Commands + Multi-Session
**Duration:** 2 weeks
**Priority:** 🟡 HIGH

### 10.1: Custom Commands from Config
- User định nghĩa custom workflows trong config:
  ```yaml
  commands:
    fix-security:
      description: "Fix security issues only"
      analyzers: [security]
      contribution_types: [security_fix]
      risk_tolerance: high
      approve: true
    docs-only:
      description: "Improve documentation"
      analyzers: [docs]
      contribution_types: [docs_improve]
      skip_non_docs: true
    test-gen:
      description: "Generate tests for uncovered files"
      analyzers: [testing]
      contribution_types: [testing]
      max_files_per_pr: 5
  ```
- CLI: `contribai cmd fix-security` → chạy custom workflow
- TUI: `/fix-security` shortcut

### 10.2: Multi-Session Support
- Chạy nhiều pipeline sessions song song:
  ```bash
  contribai session new --name hunt-python   # Session 1
  contribai session new --name hunt-go         # Session 2
  contribai session list                       # Xem tất cả sessions
  contribai session attach hunt-python         # Attach vào session
  contribai session kill hunt-go              # Kill session
  ```
- Mỗi session có: riêng memory context, riêng circuit breaker state
- Session storage: `~/.contribai/sessions/<id>.json`
- Share session: `contribai session export <id>` → JSON file

### 10.3: Session Forking
- Fork session từ checkpoint:
  ```bash
  contribai session fork <id> --from-step analysis
  ```
- Useful để thử nghiệm different strategies trên cùng repo

**Acceptance Criteria:**
- ✅ Custom commands chạy đúng config
- ✅ Multiple sessions chạy song song
- ✅ Session list/attach/kill/for/export/import
- ✅ 15+ tests for session management

---

## Sprint 11: Authentication Ecosystem
**Duration:** 2 weeks
**Priority:** 🟡 HIGH

### 11.1: GitHub Copilot Auth Integration
- Support GitHub Copilot token as LLM provider:
  ```yaml
  llm:
    provider: "copilot"
    # Auto-detects from gh copilot token
  ```
- Flow:
  1. `gh auth login` → GitHub token
  2. Exchange for Copilot token via `https://api.github.com/copilot_internal/v2/token`
  3. Use Copilot token để call `gpt-4o`, `claude-sonnet-4`, `gemini-2.5-pro` qua Copilot API
- Benefits: User đã có Copilot subscription → không cần API key riêng

### 11.2: OpenAI ChatGPT Plus/Pro Auth
- Support ChatGPT Plus/Pro account sharing:
  ```yaml
  llm:
    provider: "chatgpt"
    access_token: ""  # From chat.openai.com/api/auth/session
  ```
- Flow: Extract `access_token` từ browser localStorage hoặc `chatgpt` CLI
- Model mapping: `gpt-4o`, `gpt-4.1`, `o3`, `o4-mini`

### 11.3: Provider Auto-Detection + Login Flow
- `contribai login` cải tiến: detect tất cả available auth sources:
  ```
  🔐 Authentication Status
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    GitHub:            ✅ Token set (via gh CLI)
    LLM Providers:
      Gemini:          ❌ GEMINI_API_KEY not set
      OpenAI:          ❌ OPENAI_API_KEY not set
      Copilot:         ✅ Token detected (via gh copilot)
      ChatGPT:         ❌ Not configured
      Vertex AI:       ✅ gcloud configured (project-xxx)
      Ollama:          ⚪ Running locally (localhost:11434)
  ```
- Auto-prioritize: Copilot > Vertex AI > API keys > Ollama
- Interactive switch: `contribai login` → chọn provider → auto-config

### 11.4: Token Refresh + Expiry Handling
- Auto-refresh expiring tokens:
  - Copilot token: refresh trước khi expire (5-min TTL)
  - Vertex AI token: gcloud token (55-min TTL, đã có)
  - ChatGPT token: manual refresh (no auto-refresh)
- Warning khi token sắp expire: `⚠️ Copilot token expires in 2 minutes`
- Graceful fallback: khi token expire → try next available provider

### 11.5: Provider Fallback Chain
- Configure fallback chain:
  ```yaml
  llm:
    primary: "copilot/gpt-4o"
    fallback:
      - "vertex/gemini-3-pro"
      - "openai/gpt-4o"
      - "ollama/llama-3.3-70b"
  ```
- Auto-fallback khi: rate limit, API error, token expired
- Log: `⚠️ Copilot rate limited → falling back to Vertex AI`

**Acceptance Criteria:**
- ✅ `contribai login` detects all available auth sources
- ✅ Copilot auth works (token exchange + API calls)
- ✅ Token auto-refresh for Copilot + Vertex AI
- ✅ Provider fallback chain works
- ✅ 10+ tests for auth flows

---

## Sprint 12: MCP Remote + LSP Validation
**Duration:** 2 weeks
**Priority:** 🟡 HIGH

### 12.1: MCP Remote Client
- ContribAI kết nối external MCP servers như tools:
  ```yaml
  mcp:
    github-copilot:
      type: remote
      url: https://api.githubcopilot.com/mcp
      headers:
        Authorization: "Bearer $COPILOT_TOKEN"
    local-tools:
      type: local
      command: ["npx", "-y", "@modelcontextprotocol/server-tools"]
    custom-api:
      type: remote
      url: https://my-mcp-server.com/mcp
      oauth:
        clientId: "contribai"
        clientSecret: "$MCP_SECRET"
        scope: "tools:execute"
  ```
- MCP tools available trong pipeline như built-in tools
- Auto-discover tools từ MCP server capabilities

### 12.2: LSP-Based Code Validation
- Replace sandbox AST parse với LSP diagnostics:
  1. Start LSP server cho repo language (TypeScript → tsserver, Python → pyright)
  2. Send generated code qua LSP
  3. Parse diagnostics: errors → reject, warnings → warn, none → accept
- Config:
  ```yaml
  sandbox:
    mode: "lsp"                 # lsp | ast | docker | local | off
    lsp_timeout: 10             # seconds to wait for diagnostics
    accept_warnings: false      # reject if warnings present
  ```
- Fallback: nếu LSP không available → AST parse

### 12.3: LSP Auto-Discovery
- Tự động phát hiện LSP server từ repo:
  - `package.json` có `typescript` → dùng `typescript-language-server`
  - `requirements.txt` có `pyright` → dùng `pyright`
  - `Cargo.toml` → dùng `rust-analyzer`
- Download + install LSP nếu chưa có (cache trong `~/.contribai/lsp/`)

**Acceptance Criteria:**
- ✅ MCP remote client connects + discovers tools
- ✅ LSP validation catches errors in generated code
- ✅ LSP auto-discovers server from repo structure
- ✅ Fallback to AST when LSP unavailable
- ✅ 10+ tests for MCP + LSP

---

## Sprint 13: Client/Server Architecture
**Duration:** 2 weeks
**Priority:** 🟡 HIGH

### 13.1: Pipeline Server Mode
- Chạy pipeline như standalone server:
  ```bash
  contribai serve --port 9876
  ```
- Server exposes JSON-RPC API:
  - `run` — trigger pipeline run
  - `analyze` — analyze specific repo
  - `status` — get pipeline status
  - `sessions/list` — list active sessions
  - `sessions/create` — create new session
  - `cache/stats` — LLM cache stats

### 13.2: Remote Client
- CLI kết nối remote server:
  ```bash
  contribai run --server http://remote-host:9876
  contribai analyze --server http://remote-host:9876 <url>
  ```
- Auth: API key hoặc mTLS cho remote connections
- TUI: `contribai tui --server http://remote-host:9876`

### 13.3: Mobile/Desktop Client Foundation
- REST API compatible với external clients
- WebSocket streaming cho real-time progress
- Session sync giữa clients (start on CLI, monitor on mobile)

### 13.4: Health + Metrics API
- `GET /health` — server health check
- `GET /metrics` — Prometheus metrics (đã có từ Sprint 7)
- `GET /api/sessions` — active sessions
- `GET /api/stats` — pipeline statistics

**Acceptance Criteria:**
- ✅ `contribai serve` starts pipeline server
- ✅ Remote client can trigger runs
- ✅ TUI connects to remote server
- ✅ Health + metrics endpoints work
- ✅ 10+ tests for server/client

---

## Sprint 14: TUI Enhancement + UX Polish
**Duration:** 2 weeks
**Priority:** 🟢 MEDIUM

### 14.1: Configurable Keybinds
- Full keybind configuration:
  ```yaml
  tui:
    keybinds:
      agent_cycle: "tab"
      model_list: "m"
      session_new: "n"
      input_submit: "return"
      input_newline: "shift+return"
      leader: "ctrl+x"
  ```
- Vim-like keybinds: `hjkl`, `ctrl+d/u`, `gg/G`
- Leader key system: `ctrl+x` → `r` = run, `a` = analyze

### 14.2: Context Budget Display
- Real-time context usage bar trong TUI
- Show: tokens used, tokens remaining, cost estimate
- Color-coded: green (<60%), yellow (60-80%), red (>80%)

### 14.3: Agent Switching in TUI
- `Tab` để switch giữa `plan` và `build` mode
- Visual indicator: `[PLAN]` vs `[BUILD]`
- Different prompt templates cho mỗi mode

### 14.4: Tool Details Panel
- Expandable tool execution details trong TUI
- Show: tool name, input, output, duration, status
- Filter: show only failed tools, or all

### 14.5: Session Export/Share
- `contribai session export <id>` → JSON file
- Shareable format cho debugging/review
- Import: `contribai session import session.json`

**Acceptance Criteria:**
- ✅ Keybinds configurable
- ✅ Context budget bar trong TUI
- ✅ Agent switching via Tab
- ✅ Tool details panel
- ✅ Session export/import

---

## Sprint 15: Observability + Monitoring
**Duration:** 2 weeks
**Priority:** 🟢 MEDIUM

### 15.1: OpenTelemetry Integration
- Export traces to Jaeger, Zipkin, OTLP:
  ```yaml
  experimental:
    openTelemetry:
      enabled: true
      endpoint: "http://localhost:4318"
      serviceName: "contribai"
  ```
- Trace: pipeline run, LLM calls, file fetches, PR creation
- Span attributes: repo, model, tokens, duration, cost

### 15.2: Structured JSON Logging
- Log format: JSON lines với correlation IDs
- Fields: `timestamp`, `level`, `message`, `trace_id`, `span_id`, `repo`, `model`
- Config:
  ```yaml
  log:
    level: "info"
    format: "json"
    output: "stderr"
    file: "~/.contribai/logs/pipeline.log"
  ```

### 15.3: Grafana Dashboard
- Pre-built Grafana dashboard JSON:
  - Pipeline runs over time
  - LLM token usage + cost
  - PR merge rate by repo
  - Cache hit rate
  - Circuit breaker state timeline
  - Error rate by type

### 15.4: Alert Rules
- Configurable alerts:
  ```yaml
  alerts:
    circuit_breaker_open: true    # notify when circuit opens
    pr_rejected_rate: 0.5         # alert if >50% PRs rejected
    daily_cost_usd: 10            # alert if daily cost > $10
    token_budget_exceeded: true   # alert if context budget exceeded
  ```

**Acceptance Criteria:**
- ✅ OpenTelemetry traces exported
- ✅ Structured JSON logging
- ✅ Grafana dashboard JSON included
- ✅ Alert rules configurable
- ✅ 5+ tests for observability

---

## Sprint 16: Polish + Release
**Duration:** 2 weeks
**Priority:** 🟢 MEDIUM

### 16.1: Plugin System
- Extensible plugin architecture:
  ```yaml
  plugin:
    - name: "custom-analyzer"
      path: "./plugins/analyzer.py"
    - name: "slack-notifier"
      npm: "@contribai/slack-notifier"
  ```
- Plugin hooks: `on_analysis_complete`, `on_pr_created`, `on_pr_merged`, `on_error`
- Plugin SDK: Python + TypeScript

### 16.2: Enterprise Mode
- Enterprise config:
  ```yaml
  enterprise:
    url: "https://contribai.mycompany.com"
    sso: "saml"
    allowed_providers: ["copilot", "vertex"]
    max_daily_cost_usd: 50
  ```
- SSO integration: SAML, OIDC
- Audit logging: all actions logged to enterprise server

### 16.3: Localization
- i18n for CLI messages:
  - English (default)
  - Vietnamese
  - Japanese
  - Chinese (Simplified)
- TUI labels translated

### 16.4: Final Polish
- `cargo clippy -- -D warnings` — 0 warnings
- `cargo audit` — 0 advisories
- All tests pass: target 600+ tests
- Release binaries for all 4 platforms
- Update all docs: README, ARCHITECTURE, RUNBOOK

**Acceptance Criteria:**
- ✅ Plugin system works (load + execute hooks)
- ✅ Enterprise mode with SSO
- ✅ CLI messages in 4 languages
- ✅ 600+ tests, 0 clippy warnings, 0 audit advisories
- ✅ Release v6.0.0 with all platforms

---

## Summary

| Sprint | Focus | Key Deliverables |
|--------|-------|-----------------|
| **8** | Agent Modes + Permissions | Plan/Build modes, Rule-based permissions, File snapshots |
| **9** | Small Model + Compaction | Small model routing, Auto context compaction, Budget display |
| **10** | Custom Commands + Sessions | User-defined commands, Multi-session, Session forking |
| **11** | Auth Ecosystem | Copilot auth, ChatGPT auth, Auto-detection, Fallback chain |
| **12** | MCP Remote + LSP | MCP remote client, LSP validation, Auto LSP discovery |
| **13** | Client/Server | Pipeline server, Remote client, Mobile-ready API |
| **14** | TUI Polish | Configurable keybinds, Context budget, Agent switching |
| **15** | Observability | OpenTelemetry, JSON logging, Grafana dashboard, Alerts |
| **16** | Polish + Release | Plugin system, Enterprise mode, i18n, v6.0.0 release |

### Target State sau Phase 2:
- **Tests:** 575 → 600+
- **Version:** 5.17.1 → 6.0.0
- **LLM Providers:** 5 → 9+ (thêm Copilot, ChatGPT, fallback chain)
- **CLI Commands:** 40 → 55+
- **Features từ OpenCode:** 11/11 implemented
