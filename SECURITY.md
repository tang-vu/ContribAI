# Security Policy

## Supported versions

Security fixes are provided for the latest Rust release line. The Python v4 implementation is
archived and unsupported except for disclosure coordination.

| Version | Status |
|---|---|
| Latest v6.x release | Supported |
| Older Rust releases | Upgrade required |
| Python v4.x and earlier | Unsupported / reference only |

## Report a vulnerability privately

Use [GitHub private vulnerability reporting](https://github.com/tang-vu/ContribAI/security/advisories/new).
Do not open a public issue for an unpatched vulnerability and do not include secrets or private
repository content in a report.

Include, when available:

- affected version and platform;
- impact and attack prerequisites;
- minimal reproduction steps;
- whether a GitHub token, LLM credential, webhook secret, or private repository is involved;
- suggested remediation or compensating controls.

We aim to acknowledge complete reports within 72 hours, provide an initial assessment within seven
days, and coordinate a fix and disclosure based on severity. These are targets, not contractual
service-level guarantees.

## Security boundaries

ContribAI processes untrusted repositories, issue text, review comments, tool output, and model
output. It may hold credentials capable of reading or writing GitHub resources. The principal risks
are therefore:

- prompt injection through repository content;
- unwanted or oversized external writes;
- token leakage through logs, prompts, subprocesses, or HTTP errors;
- malicious generated code executed during validation;
- stale-base or time-of-check/time-of-use races;
- supply-chain compromise of dependencies, workflows, installers, or release artifacts;
- unauthenticated exposure of the dashboard API.

Current controls include deterministic admission policy, explicit write capabilities, repository
consent, time-bounded permits, exact base-SHA branching, protected paths, draft-only PR creation,
bounded error bodies, retry restrictions for non-idempotent requests, deterministic command
classification, bounded workspace execution, an independent challenger stage, fingerprint-bound
human review, run-bound evidence capsules, secret redaction, MCP capability filtering, and
refusal of unauthenticated public web binds.

See [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) for assumptions, abuse cases, and residual risk.

The [agent harness](docs/AGENT_HARNESS.md) exposes only immutable, explicitly selected
file reads. It cannot execute commands, edit files, or submit contributions. Its
metadata journal omits raw tasks, responses, and file contents. Selected content
is still sent to the configured model provider; filename filtering is not secret
scanning. Token reservations are not a monetary cap, and provider transport
buffering and concurrent filesystem races remain documented limitations.

## Operator guidance

- Prefer a fine-grained GitHub token scoped only to repositories and operations you need.
- Start with read-only commands and inspect output before using `--submit`, `--respond`, or
  `--allow-writes`.
- Keep the dashboard on localhost. Public binds require configured API keys and should be placed
  behind TLS and a trusted reverse proxy.
- Use Docker validation for untrusted generated code; local/AST validation is not isolation.
- Keep secrets in environment variables or a secret manager, never committed YAML.
- Verify release checksums and GitHub artifact attestations when available.
- Revoke credentials and preserve logs immediately if compromise is suspected.

## Scope exclusions

Reports about model quality, false positives, prompt style, or a maintainer declining an admitted
proposal are generally product issues rather than vulnerabilities. A bypass of consent, protected
paths, authentication, draft-only behavior, or credential boundaries is a security issue.
