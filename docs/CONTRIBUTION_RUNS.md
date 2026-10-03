# Contribution Runs

Status: experimental. A contribution run is the v7 execution unit of the consent protocol: a
persisted, inspectable lifecycle that turns an authorized issue into at most one reviewed draft
proposal.

ContribAI is not a patch farm. A run exists to produce a small, well-evidenced candidate that a
maintainer can verify cheaply — or to stop early with a recorded reason.

## Lifecycle

```text
Discovered → Authorized → Prepared → Reproducing → Planned → Executing
          → Validating → Challenging → (Repairing) → ReadyForReview
          → Approved → Submitted
```

Fail-closed exits from any nonterminal state: `Blocked`, `NeedsAuthorization`, `Failed`,
`Expired`, `Cancelled`. Terminal states cannot transition.

| State | Meaning |
|---|---|
| `Discovered` | Run record created for a specific repository issue |
| `Authorized` | Consent verified, base SHA attested, `ContributionPermit` issued |
| `Prepared` | Isolated workspace materialized at the attested base revision |
| `Reproducing` | Reproduction evidence recorded (or skipped where inapplicable) |
| `Planned` | Bounded execution plan produced |
| `Executing` | Solver generated a candidate inside the workspace |
| `Validating` | Deterministic checks ran against the workspace |
| `Challenging` | Independent challenger reviewed the candidate |
| `Repairing` | Bounded repair applied; run re-enters `Validating` |
| `ReadyForReview` | Evidence packaged; awaiting human decision |
| `Approved` | Human approved the exact candidate fingerprint |
| `Submitted` | Draft pull request created through the evidence-bound write path |

## Transition guards

- `Authorized` requires a valid permit and a full 40- or 64-hex base SHA.
- `ReadyForReview` requires a candidate fingerprint.
- `Approved` requires the reviewed fingerprint to equal the candidate fingerprint.
- `Submitted` requires an approved run and an evidence-bound write.
- Validation is never vacuous: every run includes a required `admission_scope` check that
  re-evaluates path and scope policy even when no ecosystem checks apply.

## Operator commands

```bash
contribai contribute owner/repo --issue 123
contribai contribute owner/repo --issue 123 --dry-run
contribai contribute owner/repo --issue 123 --submit
contribai contribute owner/repo --issue 123 --json
contribai runs [--repository owner/repo] [--state ready_for_review] [--limit 20] [--json]
contribai inspect-run <run-id> [--json]
contribai conformance [--json]
```

- `--dry-run` stops after evidence packaging: no review prompt, no external writes.
- `--submit` is the only way to grant write capability, per invocation. It is ignored with
  `--dry-run` and cannot be enabled from configuration.
- `runs` and `inspect-run` are read-only and never contact GitHub or an LLM.
- `conformance` executes the offline safety-invariant suite and exits non-zero on any failure.

## Stage behavior

### Authorize

Fetches the consent manifest (schema 1 or 2) or derives consent from a maintainer-applied issue
label, attests the default-branch SHA, and issues a time-bounded permit bound to the repository
and that exact revision. Missing or invalid consent ends the run as `NeedsAuthorization`.

### Prepare

Materializes an isolated workspace containing a bounded snapshot of repository files at the
attested base SHA. File count and per-file size are limited by `run.snapshot_file_limit` and
internal byte caps; binary files are skipped.

### Understand

Builds a `TaskSpec` from the issue body, comments, and source digests with provenance separation
between maintainer text, community text, and generated context. The task fingerprint is recorded
in the evidence capsule.

### Reproduce and plan

Runs ecosystem reproduction commands where applicable (classified by the command-safety engine),
then produces a bounded plan. A run that cannot be understood or planned fails closed.

### Solve

The solver model proposes file changes against the workspace snapshot. Output is treated as
untrusted: it must parse as structured JSON and survive admission scope checks.

### Validate

The runner executes detected ecosystem checks (build, test, lint where detected) plus the always
present `admission_scope` policy check. Commands are classified before execution: `Safe`
commands run, `RequiresApproval` commands run only when `run.allow_approval_commands` is set,
`Forbidden` commands never run. Missing or failed checks fail closed.

The command deadline bounds the runner's async call, including direct-child execution and
stdout/stderr capture; the earlier whole-run deadline takes precedence. Output is drained to EOF
while retaining at most 64 KiB per stream, so verbose checks do not fail just because their captured
evidence is capped. A timeout or capture error cannot count as a passing check. Direct-child cleanup
has a one-second bounded grace period.

This is not a hard CLI-process shutdown deadline. On Windows, Tokio uses blocking pipe reads that
can survive cancellation of their async wrapper. A descendant retaining stdout/stderr can therefore
delay Tokio runtime destruction and CLI exit after the runner has returned a timeout. The runner
does not terminate descendant processes as a group and is not an OS sandbox; use an appropriate
isolated execution environment for untrusted code.

### Challenge and repair

An independent challenger model reviews the candidate and produces a structured verdict.
Challenge output is evidence, not authorization. If the candidate needs repair, the run enters
`Repairing` at most `run.max_repair_iterations` times; every repair re-enters deterministic
validation.

### Evidence and review

`EvidenceCapsuleV3` binds run ID, permit ID, base SHA, task/candidate/review fingerprints,
validation checks and verdict, reproduction evidence, challenge evidence, scope totals, review
surface, and model labels. The human reviewer sees the complete candidate; approval binds the
review fingerprint to the candidate fingerprint.

### Submit

`PrManager::create_pr_with_evidence_v3` recomputes the candidate fingerprint and scope, verifies
capsule/run/permit consistency, re-reads live consent, creates the branch at the attested SHA,
and opens a draft pull request. No other write path exists for run submissions.

## Persistence and inspection

- SQLite (`contribution_runs`, `run_events`): lifecycle state, fingerprints, timestamps, challenge
  summary, atomic transitions with appended events.
- `run.runs_root` (default: `runs/` beside the storage database): per-run JSON artifacts —
  permit, task spec, reproduction, validation, challenge, contribution, review surface, evidence
  capsule.
- `contribai inspect-run` combines both into a readable or `--json` report.
- MCP exposes read-only `list_runs`, `inspect_run`, `get_run_evidence`, `inspect_consent`, and
  `estimate_review_surface` tools; no run tool can write.

## Configuration

```yaml
run:
  runs_root: ""                # empty → runs/ next to the storage database
  run_ttl_seconds: 7200        # per-run expiry bound
  max_repair_iterations: 2     # challenger-driven repair bound
  command_timeout_secs: 120    # per-command wall clock bound
  allow_approval_commands: false
  snapshot_file_limit: 200
```

There is deliberately no `submit` key: write capability is an invocation-level grant only.

## Security properties

- Consent and base revision are re-validated at the write boundary, not just at authorize time.
- The reviewed candidate is the submitted candidate (fingerprint equality enforced).
- Every terminal admission decision is appended to the receipt-chained audit ledger with the
  `run_id`; a record that cannot persist fails the approval closed.
- All repository text, issue bodies, comments, tool output, and model output are untrusted inputs;
  authorization decisions are deterministic code, not model judgment.
