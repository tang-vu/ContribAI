# Five-minute safety walkthrough

This walkthrough proves ContribAI's admission boundary before you configure a provider, add a
GitHub token, or allow any network request. It does not generate or publish a contribution.

## Minute 1: install or build

Use the published container image — a container runtime is the only requirement,
and the demo inside needs no configuration, token, or network:

    docker run --rm ghcr.io/tang-vu/contribai:latest demo

Or use a release installer:

    curl -fsSL https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.sh | bash

On Windows PowerShell:

    irm https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.ps1 | iex

For the current development version:

    git clone https://github.com/tang-vu/ContribAI.git
    cd ContribAI
    cargo install --path crates/contribai-rs --locked

The installers verify the downloaded release binary before placing it on <code>PATH</code>. Their
release asset paths are exercised on Linux, macOS, and Windows by CI.

## Minute 2: exercise the real policy engine offline

    contribai demo

The command uses the production consent parser, permit binding, admission controller, evidence v2
builder, and evidence validator. It admits a two-file code-and-test candidate, then probes a
protected workflow path and confirms that policy denies it.

It deliberately does **not**:

- read configuration or environment secrets;
- contact GitHub or an LLM provider;
- modify the bundled fixture or another checkout;
- grant human approval;
- create a branch or pull request.

## Minute 3: inspect the receipt

    contribai demo --json

Check these fields:

- <code>mode</code> is <code>offline_read_only</code>;
- <code>candidate.admission_allowed</code> and <code>candidate.evidence_valid</code> are
  <code>true</code>;
- <code>protected_path_probe.admission_allowed</code> is <code>false</code>;
- <code>human_review</code> is <code>required</code>;
- <code>submission</code> is <code>not_attempted</code>;
- <code>external_writes_enabled</code> is <code>false</code>.

## Minute 4: inspect maintainer scope

Open the bundled [quickstart repository](../examples/quickstart-repository/README.md) and its
[consent manifest](../examples/quickstart-repository/.github/contribai.yml). The manifest is short
enough to review as policy:

    schema_version: 1
    enabled: true
    max_files: 2
    max_changed_lines: 40
    allowed_paths:
      - src/**
      - tests/**

Validate that file with the same Rust parser used at the admission boundary:

    contribai demo --manifest examples/quickstart-repository/.github/contribai.yml --json

Only a target repository maintainer should add this file. Merge it through that repository's normal
review process; do not ask an agent to add its own authorization.

## Minute 5: choose whether to connect

If the offline proof matches your expectations, configure local credentials:

    contribai init
    contribai doctor

Then inspect consent on a repository you control or one whose maintainer already opted in:

    contribai consent-check owner/repo --json --require-consent
    contribai analyze https://github.com/owner/repo

Neither command grants submission capability. A real draft proposal additionally requires explicit
<code>--submit</code>, current maintainer consent, an exact base SHA, bounded scope, passing checks,
evidence, and interactive human review.

## Installation options

The container image published with each release is the lowest-friction path:
`docker run --rm ghcr.io/tang-vu/contribai:latest demo`. Pin a version with the
release tag (`ghcr.io/tang-vu/contribai:v6.10.0`), or build locally with
`docker build -t contribai:local .` — neither path needs a Rust toolchain.

Installers select GitHub's latest published stable release, independently of the version on `main`.
Linux and macOS require Bash, curl, and either sha256sum or shasum. Windows supports Windows
PowerShell 5.1 and PowerShell 7. Published binaries target Linux x86_64, Windows x86_64,
macOS Intel, and macOS Apple Silicon; other targets require a source build.

To pin an existing stable release on Linux or macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.sh | CONTRIBAI_VERSION=v6.10.0 bash
```

On Windows:

```powershell
$env:CONTRIBAI_VERSION = 'v6.10.0'
irm https://raw.githubusercontent.com/tang-vu/ContribAI/main/install.ps1 | iex
Remove-Item Env:CONTRIBAI_VERSION
```

Pins accept exact stable tags in `vX.Y.Z` form and bypass latest-release lookup. Missing releases,
lookup failures, and invalid checksums stop installation before replacing an existing binary.
Set `CONTRIBAI_INSTALL_DIR` for a custom location. On Windows, set `CONTRIBAI_NO_PATH_UPDATE=1`
when testing an isolated installation; invoke the installed executable by its full path.

The installer and checksums are retrieved over HTTPS from this repository's release infrastructure.
Checksum verification detects download mismatches; it does not independently establish provenance.
For provenance verification, see [release artifact verification](RELEASING.md#verify-downloaded-artifacts).
