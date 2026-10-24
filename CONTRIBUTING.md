# Contributing to ContribAI

The [read-only agent harness](docs/AGENT_HARNESS.md) is part of the Rust product.
Changes must preserve explicit file selection, bounded execution, metadata-only
history, and the separation from contribution admission. Test live adapters with
loopback fixtures; never describe a fixture as a verified real-model integration.

Thanks for helping build a healthier interface between AI agents and open source maintainers.

## Before you start

- Search existing issues and pull requests.
- For a substantial feature, policy change, new provider, or architecture change, open a design
  issue first.
- Security vulnerabilities must be reported privately through
  [GitHub Security Advisories](https://github.com/tang-vu/ContribAI/security/advisories/new).
- Read [AGENTS.md](AGENTS.md), [GOVERNANCE.md](GOVERNANCE.md), and
  [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

## AI-assisted contributions

AI assistance is welcome; unaccountable output is not. The person opening the pull request must:

1. disclose meaningful AI assistance;
2. understand and review every submitted change;
3. verify that generated material is compatible with the project license;
4. run the relevant checks;
5. respond to reviewer questions without delegating identity or legal attestations to an agent.

Do not paste secrets, private source code, vulnerability details, or third-party personal data into
an external model without authorization.

## Development setup

```bash
git clone https://github.com/tang-vu/ContribAI.git
cd ContribAI
rustup component add rustfmt clippy
cargo build --workspace
cargo test --workspace
```

Rust under `crates/contribai-rs/` is the maintained implementation. Python under `python/` is
legacy reference code; changes there should be limited to critical security or archival fixes.

## Change workflow

1. Create a focused branch from `main`.
2. Keep the change small and explain its user or maintainer value.
3. Add tests for behavior and guardrail changes.
4. Update public documentation and examples when behavior changes.
5. Run the required verification.
6. Open a pull request using the repository template.

Use conventional commit prefixes: `feat`, `fix`, `refactor`, `docs`, `test`, `perf`, `security`,
or `chore`.

## Required verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
contribai conformance
```

`contribai conformance` runs the offline safety-invariant suite; it must report zero failures.

Additional expectations by change type:

| Area | Required evidence |
|---|---|
| Admission or permissions | allow and deny tests; fail-closed behavior |
| GitHub writes | idempotency/retry analysis; mocked request assertion |
| CLI capability | default-off parser test and help text |
| Configuration | deserialize the shipped example/template |
| Web/API | authentication and non-loopback exposure tests |
| Workflow/release | least privilege, immutable action pins, reproducibility notes |
| Documentation | commands, links, versions, and claims checked against code |

## Design rules

- Authorization is deterministic policy code, never an LLM decision.
- Reading and generating do not imply permission to publish.
- Missing consent, evidence, identity, base revision, or validation is a denial.
- Generated proposals cannot change governance, workflow, consent, funding, security-policy, or
  agent-instruction files.
- A change that increases maintainer review burden needs a stronger justification than one that
  reduces it.
- Do not add vanity metrics that reward issue/PR volume. Prefer acceptance rate, review burden,
  false-positive rate, time-to-decision, and policy-denial telemetry.

## Review and merge

Maintainers may request changes or close proposals that are technically correct but poorly scoped,
uninvited, hard to verify, or inconsistent with the product contract. A maintainer other than the
author should approve security-sensitive or governance changes whenever practical.

The project uses squash merge unless preserving individual commits materially improves the history.
Release notes should describe user-visible behavior and migrations rather than internal activity.

## Developer certificate and CLA

ContribAI does not require an automated CLA. By contributing, you represent that you have the right
to submit the work under the project license. DCO sign-off may be used where repository rules require
it, but no bot or agent may make that attestation on a person's behalf.
