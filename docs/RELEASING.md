# Releasing ContribAI

This procedure applies to releases of this repository. It does not grant permission to submit
contributions to other repositories. The [product contract](../AGENTS.md) remains in force.

The root `Cargo.lock` is the single tracked lockfile for this workspace, including source installs
with `cargo install --path crates/contribai-rs --locked`. Do not keep a member-level lockfile:
Cargo packages generate their own lockfile from the workspace dependency resolution.

## Prepare a candidate

1. Choose a semantic version and describe the user-visible changes, migration requirements,
   safety impact, and known limitations in [CHANGELOG.md](../CHANGELOG.md).
2. Update the Rust crate version and the `contribai` entry in `Cargo.lock` together. Installers
   resolve the latest published release by default; release smoke jobs pin `CONTRIBAI_VERSION` to
   the exact tag. Do not update unrelated dependencies during a version bump.
3. Review the exact diff. Disclose AI assistance accurately; do not claim a human reviewed or
   verified work unless they did. Preserve existing local user changes outside the release commit.
4. Run the required checks from the workspace root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
cargo audit --deny warnings
node scripts/check-installers.mjs
node --test scripts/test-installers.mjs
node --test scripts/check-release-assets.test.mjs
node scripts/check-site.mjs
node --check site/app.js
npm test --prefix examples/quickstart-repository
```

Check that configuration examples deserialize, action references use full commit SHAs, workflow
permissions are minimal, Docker/installer paths target Rust, and licensing metadata agrees.
For workflow changes, also run `actionlint` against the changed workflows.

Commit and push the verified candidate according to repository policy. Wait for CI on that exact
commit, including all supported test platforms, MSRV, package validation, and the dependency audit.
A green check on an earlier commit is insufficient.

## Tag and publish

Create an annotated `vX.Y.Z` tag on the verified candidate commit and push that tag. Never move an
existing published tag to different source. The [release workflow](../.github/workflows/release.yml):

1. Tests the tagged source and builds Linux x86_64, Windows x86_64, macOS Intel, and macOS ARM64.
2. Runs each exact release binary's version check and offline safety demo on its platform.
3. Stages binaries and SHA-256 sidecars with read-only build-job permissions.
4. Requires all builds to succeed, verifies exactly eight staged files and their checksums, and
   generates GitHub artifact attestations.
5. Uploads to a draft GitHub Release, then publishes only after uploads succeed.
6. Builds the locked Dockerfile, smoke-tests the exact image's version and offline safety
   demo, then pushes it to `ghcr.io/tang-vu/contribai` tagged with the release version and
   `latest`, and attests the pushed image digest.
7. Installs the published release on all four platforms and verifies its version and safety demo.

The container and installer checks run **after publication**. Do not declare the phase complete
until they pass.

The first image push creates a private GHCR package. The owner must set the `contribai`
package's visibility to public once (package settings under the owner's GitHub profile) or
external users cannot pull the image. The Dockerfile's `org.opencontainers.image.source`
label already links the package to this repository.

Copy the relevant changelog entry into the release notes and include the exact validation evidence
and any remaining limitations. Record the tag, commit SHA, workflow URL, and publication result.

## Verify downloaded artifacts

For example, after v6.10.0 has been published:

```bash
gh release download v6.10.0 --repo tang-vu/ContribAI --pattern 'contribai-*' --dir release-assets
node scripts/check-release-assets.mjs release-assets v6.10.0
gh attestation verify release-assets/contribai-v6.10.0-linux-x86_64 --repo tang-vu/ContribAI
```

Use a fresh download directory. Check provenance for each binary being distributed. Checksums
detect mismatches; attestations identify the producing workflow. Neither proves code correctness
or eliminates the risk of a compromised build environment.

## If a gate fails

Before publication, leave the candidate unpublished and inspect the failed job. Retry a transient
infrastructure failure only after checking the existing run's state. Fix source defects in a new
commit and use a new version if the original tag has already been published.

After publication, identify the affected platform or installer in the release notes, point users
to a verified version, and prepare a patch release. Do not silently replace published binaries or
retag source to make failed checks appear successful. Preserve the failed run as evidence.

Release artifacts and local admission receipts are separate: admission receipts do not authorize
a release, and a successful release does not authorize outbound contributions.
