# Deployment guide

This guide covers the Rust workspace and the checked-in Docker configuration. Begin with the
[five-minute walkthrough](QUICKSTART.md) and its offline `contribai demo` before adding credentials.
External submission remains disabled by default; deployment does not grant maintainer consent.

## Install and verify

Use the [release installation commands and version pins](QUICKSTART.md#installation-options).
After installation:

```bash
contribai --version
contribai demo
```

For a source build, run from the repository root with Rust 1.88 or later, a supported native C/C++
toolchain, and the platform dependencies used by [CI](../.github/workflows/ci.yml):

```bash
cargo build --workspace --release --locked
./target/release/contribai demo
# Alternatively, install the workspace application on PATH:
cargo install --path crates/contribai-rs --locked
```

On Windows the built executable is `target\release\contribai.exe`. The root `Cargo.lock` controls
workspace dependencies. On machines with limited disk throughput, set `CARGO_BUILD_JOBS=2` in the
build environment to reduce competing compiler/linker jobs.

| Release binary | Installer | Verification |
|---|---|---|
| Linux x86_64 | Bash | Tagged build, version, demo, installer smoke |
| Windows x86_64 | PowerShell | Tagged build, version, demo, installer smoke |
| macOS Intel | Bash | Native Intel tagged build and installer smoke |
| macOS Apple Silicon | Bash | Native ARM64 tagged build and installer smoke |

See the [release workflow](../.github/workflows/release.yml) for runner versions and gates.
Windows installer behavior is also tested with Windows PowerShell 5.1 and PowerShell 7. Other
architectures require a source build and are not covered by the release matrix. Successful hosted
runner tests do not establish compatibility with every older OS distribution or libc version.

## Configuration and credentials

Copy [config.example.yaml](../config.example.yaml) to `config.yaml` and retain its safe defaults:
`pipeline.dry_run: true`, `pipeline.agent_mode: plan`, and an initially disabled scheduler.
Supply GitHub/provider credentials through environment variables or your secret manager when a
network operation needs them. Keep real credentials out of committed YAML and diagnostic reports.

The example uses Gemini (`GEMINI_API_KEY`); choose the provider/model you intend to operate.
`GITHUB_TOKEN` supplies GitHub credentials. Review [SECURITY.md](../SECURITY.md) before enabling
network services or validation of untrusted generated code. Local/AST validation is not a sandbox;
use the supported Docker validation mode when isolation is required.

## Local dashboard

```bash
contribai --config config.yaml web-server --host 127.0.0.1 --port 8787
```

This guide explicitly selects port 8787. The bare `web-server` command defaults to port 5000.
Open `http://127.0.0.1:8787`; check liveness at `/api/health`.

Set `CONTRIBAI_WEB_API_KEY` to a securely generated secret to authenticate protected API routes.
Clients should send it in the `X-API-Key` header, not in a URL that may enter browser or proxy logs.
Without configured keys, a loopback server permits local access without authentication. A bind to
`0.0.0.0` or another non-loopback address refuses startup without keys. For remote access, use TLS
and a trusted reverse proxy; do not expose an unauthenticated localhost service through a tunnel.
Health checks prove server liveness, not valid credentials, repository consent, or admission readiness.

## Docker Compose

Use the repository [Dockerfile](../Dockerfile) and [Compose file](../docker-compose.yml). They build
the locked Rust workspace, run as an unprivileged user, mount configuration read-only, and persist
application state in the named `contribai-data` volume. Releases also publish the same verified
image to `ghcr.io/tang-vu/contribai` (`:<version>` and `:latest`), for example
`docker run --rm ghcr.io/tang-vu/contribai:latest demo`; point the Compose `image` at a pinned tag
instead of `build:` to skip the local build.

Create `config.yaml` first. Set `CONTRIBAI_WEB_API_KEY` in the Compose environment: the container
binds to `0.0.0.0` internally even though the published host port is restricted to localhost.

```bash
docker compose config --quiet
docker compose up --build -d dashboard
docker compose ps
# Inspect recent service logs; sanitize them before sharing:
docker compose logs --tail 50 dashboard
```

The dashboard is available at `http://127.0.0.1:8787`. Keep the existing host-port restriction,
read-only filesystem, dropped capabilities, and `no-new-privileges` setting. An isolated one-shot
offline demonstration uses the optional CLI service:

```bash
docker compose --profile cli run --rm runner demo
```

The scheduler is an explicit opt-in profile, separate from starting the dashboard. Before enabling
it, configure and review its repository scope, credentials, quotas, and read-only behavior:

```bash
docker compose --profile scheduler up -d scheduler
```

Starting a scheduler does not authorize submission. Submission still requires explicit local
capability, current upstream consent, an exact base SHA, passing checks, and human review of the
exact candidate and evidence. See the [consent protocol](CONSENT_PROTOCOL.md).

## Upgrades, recovery, and support

Before upgrading, stop processes writing application state and preserve the database together with
any SQLite WAL/SHM sidecars. For Docker, preserve the named volume; `docker compose down -v` removes
it and is not an ordinary upgrade command. Follow [audit recovery](AUDIT_RECOVERY.md) for damaged
history, and retain an independent audit receipt before planned maintenance.

Verify the installed version, offline demo, dashboard health, and audit integrity after an upgrade.
For provenance checks, use the [release verification procedure](RELEASING.md#verify-downloaded-artifacts).
Do not reset or re-hash damaged history merely to make validation pass.

For an installation problem, report the release tag, OS/architecture, shell version, failing command,
and a sanitized error. For a build problem, also include `rustc --version` and `cargo --version`.
Use the private reporting route in [SECURITY.md](../SECURITY.md) for vulnerabilities; omit tokens,
private repository content, and raw configuration. The latest v6.x release is the supported line.
