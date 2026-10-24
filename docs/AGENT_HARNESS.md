# Bounded agent harness

ContribAI now has a general-purpose, read-only repository agent alongside its
maintainer-governed contribution pipeline. This first vertical slice reuses the
Rust `LlmProvider` abstraction, supports local Ollama and explicitly configured
hosted providers, and makes each run inspectable through durable events.

The direction is inspired by [ODS](https://github.com/Osmantic/ODS)'s local and
hybrid AI orchestration, not an ODS fork or a claim of feature parity. No ODS
source code is included. ContribAI's AGPL-3.0-or-later license is unchanged.

## Try the loop without a model

```sh
cargo run --locked -- harness --db /tmp/contribai-demo.db demo --json
cargo run --locked -- harness --db /tmp/contribai-demo.db runs --json
cargo run --locked -- harness --db /tmp/contribai-demo.db inspect agent_ID --json
```

Replace `agent_ID` with the returned run ID. `demo` is explicitly labeled
`demo_fixture/deterministic`: it drives the same runtime, tools, budgets, and
journal with scripted responses. It is not a real model integration test and
never falls back from a failed live run.

## Run a real local model

Install and start Ollama independently, with a model already available. Create
`harness-local.yaml` (model name must match your installed model):

```yaml
llm:
  provider: ollama
  model: your-installed-model
  base_url: http://localhost:11434
```

```sh
contribai --config harness-local.yaml harness run \
  "Explain the architecture using the selected files; cite the paths." \
  --workspace . --file README.md --file crates/contribai-rs/src/lib.rs \
  --max-turns 8 --max-tools 12 --token-budget 8192 --timeout-seconds 120 --json
```

Use a physical workspace path if an ancestor is a symlink (`pwd -P` on Unix).
`--file` is repeatable, exact, and mandatory. The snapshot rejects traversal,
symlinks, non-regular/non-UTF-8 files, `.git`, and common credential filenames.
Only those files can be listed or read, with at most 32 files, 1024 bytes per
path, 64 KiB per file, and 256 KiB in total. Contents are frozen before the first
model call, so later source edits do not change the run's input.

For hosted inference, explicitly select `openai`, `anthropic`, or API-key
`gemini` in the supplied configuration, using existing provider environment
variables. OpenAI-compatible endpoints use `llm.base_url`. The task and selected
file contents are transmitted to that provider. Select only data you are
authorized to share. Filename filtering is not a content secret scanner.
No GitHub token is required by the harness. Vertex/gcloud and Copilot credential
helpers are not supported in this slice. No provider/account fallback or
response cache is used by the harness. Other top-level configuration sections
are ignored; the harness does not resolve GitHub or gcloud credentials.

## Execution and budgets

The model returns exactly one JSON action per turn:

```json
{"action":"list_files"}
{"action":"read_file","path":"README.md"}
{"action":"finish","answer":"The architecture is ..."}
```

These are separate response examples, not a batch. Unknown actions, extra
fields, malformed JSON, empty final answers, and out-of-snapshot reads stop the
run. Repository text and tool results remain untrusted data. No prompt can
enable shell execution, filesystem writes, MCP access, or GitHub submissions.

- Model invocation count, tool count, cumulative requested output-token
  reservations, context bytes, response bytes, and elapsed execution time are
  bounded. Limits are validated against hard ceilings.
- Output tokens are reserved *before* a provider call, including failed calls.
  The final call uses the remaining allowance. These are not measured token
  usage, input-token accounting, or a monetary spending guarantee. Configure
  provider-side spending limits separately.
- Existing OpenAI/Anthropic adapters may perform an internal HTTP-to-SSE
  fallback within one invocation. Invocation limits are not HTTP-request counts.
  The raw provider interface currently exposes text, not usage metadata.
- Context/response byte ceilings apply to accepted conversation data. Existing
  provider adapters buffer HTTP responses before the harness checks their
  length, so this is **not** a transport-memory cap or a hostile-server sandbox.
- Ctrl-C cancels the pending asynchronous provider future and records
  `cancelled`; the deadline records `timed_out`. Cancellation cannot undo a
  request already accepted or billed by a remote provider. The deadline is
  cooperative: synchronous local journal I/O cannot be interrupted, and
  snapshot loading/provider construction happen before the loop deadline.
- Non-completed runs print their receipt and exit nonzero. Journal errors stop
  execution instead of continuing without evidence.

## History and evidence boundaries

Default history is `~/.contribai/harness.db`; override it with `harness --db`.
`runs` and `inspect` require an existing journal and never call a model. Events
include model requests/completions, tool requests/completions, and the terminal
status, with monotonically increasing sequence numbers. Each event and receipt
update are saved in one SQLite transaction. Multiple processes use unique run
IDs. A process killed before terminal persistence leaves `running`, which means
completion is unknown, not that work is still executing or succeeded.

History stores task/output hashes, selected paths and content hashes, resource
counters, configured provider/model labels, timestamps, and safe reason codes.
It deliberately excludes raw task text, file contents, provider error bodies,
model responses, and final answers. Save the `run --json` output yourself if you
need the answer; that output can contain private data. Protect the database and
stdout artifacts with normal local filesystem permissions.

These records are unsigned local diagnostics. A completed agent answer is not
proof of correctness, passing checks, maintainer consent, human approval, or an
admitted contribution. Use the separate `contribute` command for the existing
permit, isolated validation, exact-candidate review, and draft-only write path.
The harness does not weaken any admission gate and cannot merge or sign a CLA.

Snapshot checks reject existing symlinks but portable check/open operations do
not defeat another process racing filesystem mutations. Use a stable trusted
source tree; this is not a kernel sandbox. The runtime's cancellation contract
also requires asynchronous, cancellation-safe provider implementations.

## Verification

Unit tests cover the action schema, privacy, all budgets, unknown paths,
cancellation before/during calls, deadlines, journal failure and restart,
snapshot immutability, credential paths, and symlinks. CLI tests exercise the
offline demo and real provider adapters against loopback HTTP fixtures,
including multi-turn file reads and rejected write-like actions. They do not
perform paid calls or assert real-model quality. Run the repository's normal
format, clippy, workspace test, and release-build gates before release.

Next slices can add a live event-view UI and a separately permissioned candidate
editing workflow. Neither capability is present or implied by this release.
