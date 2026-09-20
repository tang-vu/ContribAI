<div align="center">

# ContribAI

**A maintainer-governed admission and evidence layer for AI-assisted open source contributions.**

[![CI](https://github.com/tang-vu/ContribAI/actions/workflows/ci.yml/badge.svg)](https://github.com/tang-vu/ContribAI/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-stable-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-686%20passing-brightgreen)](#verification)

[Website](https://contribai-topaz.vercel.app/) · [Deploy website](#deploy-the-public-website) · [Why ContribAI](#why-contribai) · [Safety model](#safety-model) · [Quick start](#quick-start) · [Consent protocol](#consent-protocol) · [Architecture](#architecture)

</div>

ContribAI analyzes repositories and prepares small, reviewable changes. It does **not** treat a
public repository as permission to write. Submission is a separate capability that must be enabled
by the operator, authorized by the target maintainer, bounded by policy, reviewed by a human, and
published as a draft pull request with an evidence receipt.

The [project website](https://contribai-topaz.vercel.app/) is a dependency-free introduction and
read-only onboarding path. It is informational only and exposes no ContribAI runtime capability.

### Deploy the public website

[![Deploy with Vercel](https://vercel.com/button)](https://vercel.com/new/clone?repository-url=https%3A%2F%2Fgithub.com%2Ftang-vu%2FContribAI)

The Vercel deployment serves only the static files under `site/`. It needs no environment
variables, tokens, functions, or access to the credential-bearing Rust runtime. Project
maintainers should follow the [existing-repository deployment guide](docs/VERCEL_DEPLOYMENT.md)
instead of cloning the repository through the button.

```text
public code ≠ permission to submit
generation ≠ admission
passing checks ≠ maintainer approval
```

## Why ContribAI

AI can make code generation cheap while making review expensive. The failure mode is not merely
bad code; it is uninvited work that transfers verification cost to maintainers. ContribAI is built
around the opposite incentive:

- **Maintainer intent first.** A repository manifest or maintainer-controlled issue label is
  required before an external proposal can be submitted.
- **Read-only by default.** Discovery, analysis, scheduling, patrol, and MCP start without external
  write capability.
- **Evidence over confidence.** Every admitted proposal records consent source, base revision,
  changed paths, changed-line budget, deterministic change fingerprint, and validation checks.
- **Bounded proposals.** Protected governance files, unapproved paths, expired permits, oversized
  changes, failed checks, and missing base revisions fail closed.
- **Human accountability.** The CLI requires an explicit review decision. Pull requests are always
  drafts. ContribAI never signs a CLA or merges code.

This is a tool for maintainers and accountable contributors—not a contribution-farming bot.

## Safety model

An external draft can be created only when every gate passes:

```text
operator --submit
      │
      ▼
repository manifest OR maintainer issue label
      │
      ▼
24-hour ContributionPermit bound to repository + base SHA
      │
      ▼
scope, protected-path, size, risk, quality, and validation checks
      │
      ▼
interactive human review
      │
      ▼
draft PR + EvidenceCapsule
```

Important invariants:

| Invariant | Enforcement |
|---|---|
| No implicit write access | Pipeline write capability defaults to off |
| No unsolicited proposal | Repository consent or approved issue label required |
| No moving-base ambiguity | Contribution branch starts at the attested commit SHA |
| No review/evidence substitution | Full candidate fingerprint and scope are recomputed at the write boundary |
| Revocable until write | Current manifest or issue approval is re-read immediately before the first write |
| No governance takeover | License, policy, workflow, funding, CODEOWNERS, and agent instruction paths are protected |
| No silent publication | GitHub PR creation always sets `draft: true` |
| No delegated legal act | CLA signing is never exposed through CLI automation or MCP |
| No hidden MCP mutation | MCP advertises read-only tools unless explicitly started with `--allow-writes` |

See [the threat model](docs/THREAT_MODEL.md) and
[the consent protocol](docs/CONSENT_PROTOCOL.md) for precise boundaries.

## Quick start

### Try it in five minutes — no Rust toolchain

Run the published container image. Only a container runtime is needed: no
install, no configuration, no GitHub token, no LLM key, and no writes:

```bash
docker run --rm ghcr.io/tang-vu/contribai:latest demo
docker run --rm ghcr.io/tang-vu/contribai:latest demo --json
```

Each release publishes the image only after it passes the same offline safety
demo that gates the release binaries. To build it from a checkout instead — still
no Rust toolchain:

```bash
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI
docker build -t contribai:local .
docker run --rm contribai:local demo
```

### Install a release

```bash
curl -fsSL https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.sh | bash
```

On Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.ps1 | iex
```

The installers select the latest published stable release and verify its checksum.
Set `CONTRIBAI_VERSION` to an exact tag (for example, `v6.10.0`) to pin a release.
See [installation options](docs/QUICKSTART.md#installation-options) for platform commands.
The isolated install path is smoke-tested against the published binary on Linux, macOS, and Windows for every release.

### Build from source

Requirements: Rust stable and Git. Credentials are not needed for the offline proof.

```bash
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI
cargo install --path crates/contribai-rs --locked
```

### Prove the boundary offline

Run the production consent, admission, and evidence path without config, credentials, network
access, or external writes:

```bash
contribai demo
contribai demo --json
contribai demo --manifest examples/quickstart-repository/.github/contribai.yml --json
```

The demo also probes a protected workflow path and confirms that it is denied. Continue with the
[five-minute walkthrough](docs/QUICKSTART.md) and inspect the bundled
[quickstart repository](examples/quickstart-repository/README.md).

### Connect locally

Configure secrets through environment variables instead of committing them:

```bash
export GITHUB_TOKEN="..."
export GEMINI_API_KEY="..."       # or OPENAI_API_KEY / ANTHROPIC_API_KEY
contribai init
contribai doctor
```

### Start safely

These commands do not submit anything:

```bash
contribai analyze https://github.com/owner/repo
contribai target https://github.com/owner/repo
contribai run
contribai patrol
contribai mcp-server
```

To run the v7 contribution lifecycle against a maintainer-authorized issue:

```bash
contribai contribute owner/repo --issue 123 --dry-run
contribai contribute owner/repo --issue 123
contribai runs
contribai inspect-run <run-id>
contribai conformance
```

`--dry-run` executes through evidence packaging and stops: no review prompt, no external write.
`conformance` runs the offline safety-invariant suite. `runs` and `inspect-run` are read-only.

To request an admitted draft proposal:

```bash
contribai target https://github.com/owner/repo --submit
contribai solve https://github.com/owner/repo --submit
contribai contribute owner/repo --issue 123 --submit
```

`--submit` is necessary but not sufficient. The repository must opt in, every check must pass, and
the local operator must approve the exact evidence-bearing change interactively. The review renders
every regular and test change without truncation; current consent is checked again before writing.

## Consent protocol

Maintainers can opt in repository-wide with `.github/contribai.yml`:

```yaml
schema_version: 1
enabled: true
max_files: 3
max_changed_lines: 120
allowed_paths: src/**, tests/**
```

The uppercase marker paths remain supported for compatibility with the experimental v1 protocol.

Operators can inspect the gate without invoking an LLM or writing to GitHub:

```bash
contribai consent-check owner/repo
contribai consent-check owner/repo --json --require-consent
```

For narrower approval, apply one of these labels to an issue:

- `contribai-approved` (canonical)
- `agent-ready` or `ai-contribution-approved` (compatibility aliases)

Issue approval is scoped to that issue. Repository consent is still bounded by its path and size
budgets. Missing, malformed, or disabled consent is a denial.

## Evidence Capsule

Each admitted draft includes a compact receipt such as:

```text
Permit:               1d28…
Consent:              maintainer label `agent-ready` on #42
Base revision:         7a93…
Change fingerprint:    c441…
Scope:                 2 files / 37 changed lines
Evidence expires:      2026-08-11T08:00:00Z
Submission mode:       draft only
Checks:                admission, quality, risk, validation
```

The capsule makes the proposal reproducible and reviewable. The write boundary rejects expired,
failed, cross-repository, scope-mismatched, or fingerprint-mismatched capsules and revalidates live
maintainer consent. It remains a local audit receipt, not a substitute for CI, code review,
provenance attestation, or maintainer judgment.

ContribAI attempts to append every terminal admission decision to the local audit ledger. Records
contain the candidate fingerprint, permit/base SHA when available, scope, checks, decision stage,
and reason—never generated file contents. Each record's SHA-256 receipt binds the preceding receipt,
so accidental edits or broken ordering are detectable when the complete local chain is verified:

```bash
contribai admissions
contribai admissions --repository owner/repo --decision blocked
contribai admissions --json
```

The hash chain is a local integrity check, not a signature or remote attestation. An approved
submission fails closed if its audit decision cannot be persisted.
Each append verifies the retained history in the same write transaction; corrupted history blocks
new receipts. See [audit recovery](docs/AUDIT_RECOVERY.md) for preserving and restoring evidence.

## Capabilities

- Tree-sitter analysis for 13 languages, with additional fallback language mappings
- Security, correctness, performance, testing, documentation, and code-quality analysis
- Cross-file symbol and import context
- Multi-provider LLM support: Gemini, OpenAI, Anthropic, Ollama, Vertex AI, and Copilot routing
- SQLite outcome memory and repository-specific preferences
- Local/Docker validation, risk classification, quality scoring, and circuit breaking
- Ratatui interface, read-only-by-default MCP server, and authenticated web dashboard
- Draft PR lifecycle and explicit patrol response capability
- Offline admission/evidence demo with a protected-path fail-closed probe
- Integrity-linked local admission audit through CLI, loopback/API-key-governed web API, MCP, and
  Prometheus

The Python implementation under `python/` is legacy reference code. Rust under
`crates/contribai-rs/` is the maintained implementation.

## Command model

| Command | Default behavior | Explicit mutation capability |
|---|---|---|
| `demo [--json]` | Offline policy and evidence walkthrough | None |
| `conformance [--json]` | Offline safety-invariant checks | None |
| `contribute <repo> --issue <n>` | Run the contribution lifecycle for an authorized issue | `--submit` (`--dry-run` never writes) |
| `runs [--json]` | List persisted contribution runs | None |
| `inspect-run <id> [--json]` | Inspect one run's lifecycle and evidence | None |
| `analyze <url>` | Analyze only | None |
| `target <url>` | Analyze and prepare candidate | `--submit` |
| `run` | Discover and assess | `--submit` |
| `hunt` | Multi-round assessment | `--submit` |
| `solve <url>` | Analyze issues | `--submit` |
| `watchlist` | Assess configured repositories | `--submit` |
| `patrol` | Read review state | `--respond` |
| `admissions [--json]` | Verify and inspect local admission decisions | None |
| `mcp-server` | Advertise read-only tools | `--allow-writes` (never PR creation or CLA signing) |

Run `contribai <command> --help` for the complete interface.

## Architecture

```text
CLI / TUI / MCP
       │
       ▼
Discovery ──► Analysis ──► Generation ──► Validation
                                            │
                                            ▼
                                     AdmissionController
                                      │ consent
                                      │ permit + base SHA
                                      │ scope + risk
                                      │ evidence checks
                                      ▼
                                      Human review
                                            │
                                            ▼
                                      Draft PR only
```

The main Rust modules are:

- `core/admission.rs` — consent (schemas 1–2), permits, scope enforcement, evidence capsules
- `core/run.rs` + `core/task_spec.rs` + `core/validation_graph.rs` + `core/challenge.rs` +
  `core/review_surface.rs` — persisted run state machine, task understanding, deterministic check
  evidence, adversarial challenge, review-cost surface
- `core/command_safety.rs` — deterministic argv command classification
- `core/evidence_v3.rs` — run-bound evidence capsule
- `orchestrator/pipeline.rs` — read and write capability orchestration
- `orchestrator/run_executor.rs` — v7 contribution-run lifecycle driver
- `orchestrator/memory.rs` — outcomes, run records/events, and append-only admission audit receipts
- `exec/` — isolated workspace, bounded argv runner, ecosystem check adapters
- `analysis/` — AST intelligence, triage, repository context, progressive skills
- `generator/` — candidate generation, validation, risk, scoring, self-review
- `github/` — resilient GitHub REST and GraphQL client
- `pr/` — evidence-required draft lifecycle and patrol
- `mcp/` — capability-aware MCP surface
- `web/` — local dashboard and authenticated remote API surface
- `site/` — static public onboarding; no runtime API, analytics, cookies, or external dependencies

See [ARCHITECTURE.md](ARCHITECTURE.md) for implementation details and
[docs/CONTRIBUTION_RUNS.md](docs/CONTRIBUTION_RUNS.md) for the run lifecycle.

## Configuration

Copy [config.example.yaml](config.example.yaml) to `config.yaml`, or use `contribai init`.
Safe defaults include:

- `pipeline.dry_run: true`
- `pipeline.agent_mode: plan`
- `scheduler.enabled: false`
- validation required
- web bound to localhost unless authentication is configured

Secrets may be loaded from environment variables. Never commit `config.yaml`, tokens, webhook
secrets, local databases, or event logs.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
node scripts/check-site.mjs
node --check site/app.js
node scripts/check-installers.mjs
node --test scripts/check-release-assets.test.mjs
(cd examples/quickstart-repository && npm test)
```

The current workspace runs **700 passing tests** plus one intentionally ignored doctest.

See the [current phased delivery roadmap](docs/project-roadmap.md) for release gates and upcoming work.

## Project policy

- AI-assisted contributions are welcome when disclosed and owned by a human contributor.
- Generated output receives no special trust and must satisfy the same review bar as human-written
  code.
- Do not use this project for bulk unsolicited issues, pull requests, comments, follows, or stars.
- Respect repository policy, rate limits, maintainer attention, licenses, and contributor identity.
- Security reports belong in private GitHub Security Advisories, not public issues.

Read [CONTRIBUTING.md](CONTRIBUTING.md), [GOVERNANCE.md](GOVERNANCE.md),
[SECURITY.md](SECURITY.md), and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing.

## License

ContribAI is free and open source software licensed under
[AGPL-3.0-or-later](LICENSE). The former Commons Clause restriction has been removed.
