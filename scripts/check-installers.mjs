import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = dirname(dirname(fileURLToPath(import.meta.url)));

function read(path) {
  return readFileSync(join(repositoryRoot, path), "utf8").replace(/\r\n/g, "\n");
}

function assert(condition, message) {
  if (!condition) {
    console.error("Installer contract failed: " + message);
    process.exit(1);
  }
}

const cargoToml = read("crates/contribai-rs/Cargo.toml");
const cargoLock = read("Cargo.lock");
const shellInstaller = read("install.sh");
const powershellInstaller = read("install.ps1");
const releaseWorkflow = read(".github/workflows/release.yml");
const version = cargoToml.match(/^version = "([0-9]+\.[0-9]+\.[0-9]+)"$/m)?.[1];

assert(version, "crate version is missing");
assert(
  cargoLock.includes('name = "contribai"\nversion = "' + version + '"'),
  "workspace lockfile version does not match the crate"
);
assert(
  shellInstaller.includes('VERSION="${CONTRIBAI_VERSION:-}"') &&
    powershellInstaller.includes('$Version = $env:CONTRIBAI_VERSION'),
  "installers must support explicit version pins"
);
assert(
  shellInstaller.includes('/releases/latest') && powershellInstaller.includes('/releases/latest'),
  "ordinary installs must resolve a published release"
);
assert(
  (releaseWorkflow.match(/CONTRIBAI_VERSION: \$\{\{ github.ref_name \}\}/g) || []).length === 2,
  "both installer smoke steps must pin the exact release tag"
);

for (const asset of [
  "linux-x86_64",
  "macos-aarch64",
  "macos-x86_64",
  "windows-x86_64.exe",
]) {
  assert(releaseWorkflow.includes("asset: " + asset), "release matrix is missing " + asset);
}

assert(
  shellInstaller.includes('CONTRIBAI_INSTALL_DIR:-/usr/local/bin'),
  "install.sh must support isolated installs"
);
assert(
  powershellInstaller.includes("CONTRIBAI_INSTALL_DIR"),
  "install.ps1 must support isolated installs"
);
assert(
  powershellInstaller.includes("CONTRIBAI_NO_PATH_UPDATE"),
  "install.ps1 must support a no-PATH-update smoke mode"
);
assert(
  shellInstaller.includes('CHECKSUM_URL="$URL.sha256"') &&
    powershellInstaller.includes('$ChecksumUrl = "$Url.sha256"'),
  "installers must consume release checksum sidecars"
);
assert(
  releaseWorkflow.includes('contribai-${{ github.ref_name }}-${{ matrix.asset }}.sha256'),
  "release workflow must publish checksum sidecars"
);
assert(
  releaseWorkflow.includes("installer-smoke:") &&
    releaseWorkflow.includes("needs: release"),
  "release workflow must smoke-test installers after every asset is published"
);

assert(
  releaseWorkflow.includes("needs: build") &&
    releaseWorkflow.includes("Smoke-test exact release binary") &&
    releaseWorkflow.includes("cargo test --workspace --locked") &&
    releaseWorkflow.includes("node scripts/check-release-assets.mjs"),
  "publishing must wait for all tested binaries and verify staged checksums"
);
assert(
  releaseWorkflow.includes("draft: true") &&
    releaseWorkflow.includes('gh release edit "$RELEASE_TAG" --draft=false --latest'),
  "release uploads must remain draft until all assets are uploaded"
);
const buildJob = releaseWorkflow.split("  build:\n")[1]?.split("  release:\n")[0];
const pytestInstall = buildJob?.indexOf("python -m pip install --user pytest") ?? -1;
const workspaceTests = buildJob?.indexOf("cargo test --workspace --locked") ?? -1;
assert(
  pytestInstall >= 0 && workspaceTests > pytestInstall,
  "release build jobs must install pytest before running the workspace e2e tests"
);
assert(
  buildJob && !buildJob.includes("contents: write") && !buildJob.includes("action-gh-release@"),
  "build jobs must not publish releases or hold repository write permission"
);

console.log("Installer contract passed for ContribAI v" + version + ".");
