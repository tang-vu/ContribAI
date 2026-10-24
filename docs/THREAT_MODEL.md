# Threat Model

## Scope

ContribAI reads untrusted GitHub content, sends selected context to configured LLM providers,
generates candidate code, may validate it locally or in Docker, and can create artifacts in a user's
fork and draft proposals in a consenting upstream repository.

Assets include GitHub and LLM credentials, private repository content, maintainer attention,
contributor identity, local files and databases, release integrity, and the reputation of users and
target projects.

## Trust boundaries

```text
untrusted repository / issue / review text
                  │
                  ▼
          analysis and prompt boundary
                  │
                  ▼
             external LLM
                  │ untrusted output
                  ▼
      deterministic validation + admission
                  │
                  ▼
             human operator
                  │
                  ▼
              GitHub write
```

The LLM is never an authorization authority. Repository content can influence analysis and
generation, but cannot grant write capability, manufacture consent, expand a permit, or approve its
own output.

The read-only [agent harness](AGENT_HARNESS.md) has a smaller capability boundary:
an immutable operator-selected snapshot, strict JSON actions, and only list/read
tools. No model response grants execution or write access. Its content-minimized
SQLite events are diagnostics, not admission evidence or signed attestations.
Existing provider transport buffers are not memory-bounded by the harness, and
snapshot loading assumes a stable source filesystem rather than an adversarial
concurrent writer. Cancellation stops the local asynchronous wait, not remote
billing or already-started inference. See the harness guide for exact limits.

## Primary threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| Prompt injection in source or issue text | hardened prompts, untrusted-data framing, deterministic admission | model may still produce poor or misleading candidates |
| Unsolicited PR/issue/comment volume | read-only defaults, explicit capability flags, consent, daily limits | an operator can misuse low-level credentials outside ContribAI |
| Scope expansion | strict manifest schema, canonical paths, literal-separator globs, path/size budgets, protected paths | semantic impact can exceed line count |
| Evidence substitution after review | full-candidate fingerprint, scope recomputation, expiry and check validation at the write boundary | a compromised local process can bypass ContribAI and call GitHub directly |
| Admission history mutation | content-minimized append-only records, SHA-256 receipts linked in order, full-chain verification | receipts are unsigned; an attacker controlling the database and all retained checkpoints can replace the whole chain |
| Consent revocation during generation/review | manifest or issue state is re-read immediately before the first write | revocation after the write workflow begins cannot undo fork artifacts |
| TOCTOU on default branch | permit records base SHA; fork branch starts at exact SHA | upstream may advance before review, requiring rebase |
| Duplicate non-idempotent writes | POST/PATCH retries disabled; duplicate checks and memory | network ambiguity can still require manual reconciliation |
| Credential disclosure | redacted config debug, bounded HTTP errors, gitignored secrets | external providers receive intentionally selected context |
| Execution of malicious generated code | deterministic argv command classification, bounded workspace, timeouts, approval-gated non-safe commands | classification limits known-bad invocations; a determined candidate can still harm within an allowed test command |
| Run-state forgery | persisted run states with guarded transitions; submission requires reviewed fingerprint equal to candidate fingerprint | a compromised local database or process can fabricate run records |
| Challenger/repair collusion | solver and challenger are separate model calls; challenger verdict is evidence, not admission; deterministic checks re-run after repair | a model that both writes and reviews can still mislead if the operator trusts it uncritically |
| Unbounded repair loops | `max_repair_iterations` bound; each repair re-enters deterministic validation | a bounded loop can still waste operator time before failing closed |
| Workspace escape | canonical-path resolution, component-wise traversal rejection, canonicalized-root containment | symlink or OS-level quirks outside path validation remain possible |
| MCP confused-deputy writes | read-only advertisement by default; explicit write mode; PR and CLA tools non-delegable | write-enabled MCP tools can mutate the operator's fork or existing PR state |
| Legal impersonation | no automated CLA signing; human review identity | DCO/commit metadata still depends on operator configuration |
| Unauthenticated dashboard exposure | localhost default; refuse non-loopback bind without API keys; optional TLS | reverse proxy and key management remain operator responsibilities |
| Supply-chain compromise | lockfile, pinned actions, dependency audit/review, checksums, release attestations | compromised compiler, runner, or maintainer account remains possible |

## Protected paths

Generated proposals cannot modify license, contribution policy, code of conduct, security policy,
CODEOWNERS, funding, GitHub workflows, consent files, AI policy files, or agent instruction files.
This prevents a candidate from changing the rules used to admit itself.

Repository paths are accepted only in canonical relative POSIX form. Traversal, encoded URI
metacharacters, ambiguous separators, duplicate targets, and unsupported deletions fail closed
before any write-capable API call.

## Data handling

ContribAI may send repository fragments, issue descriptions, and review context to the selected LLM
provider. Operators are responsible for provider terms, retention settings, geographic restrictions,
and authorization to process private code. Local Ollama mode can reduce external disclosure but does
not remove local execution or model-supply-chain risk.

Persistent local data includes repository analysis, PR outcomes, admission audit metadata, working
memory, caches, event logs, and contribution-run records. Run state and lifecycle events persist in
SQLite; run artifacts (permit, task spec, reproduction and validation output, challenge report,
contribution, review surface, evidence capsule) persist under `run.runs_root`. Admission records
exclude generated file contents but include repository names and changed paths. Operators should
protect the ContribAI data directory as they would source-code metadata.

Admission appends verify all retained receipts and indexed fields in one SQLite write transaction.
Corruption or malformed payloads prevent appending, and approved submissions fail closed when
their receipt cannot be recorded. This does not detect suffix truncation or full replacement
without an independently retained receipt. Audit parsing errors omit stored values. See
[audit recovery](AUDIT_RECOVERY.md) before restoring or investigating damaged state.

## Out of scope assumptions

- The operating system and Rust toolchain are not already compromised.
- The operator controls the configured GitHub and model accounts.
- GitHub correctly enforces repository permissions and issue-label authority.
- Maintainers still review and decide whether to accept the draft.

## Security regression requirements

Changes to admission, authentication, GitHub writes, sandboxing, installers, workflows, or release
logic require explicit deny-path tests. A missing test is itself a review blocker for a guardrail
change.
