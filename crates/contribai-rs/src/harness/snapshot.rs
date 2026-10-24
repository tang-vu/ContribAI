//! Immutable, bounded inputs for the read-only harness.
//!
//! Only explicitly selected files are loaded; directories are never enumerated.
//! Path policy is a conservative filename filter, not a content secret scanner.
//! The portable metadata/canonicalization checks reject existing symlinks, but
//! cannot eliminate concurrent filesystem check/open races. Callers must keep
//! the source tree and its ancestors stable while loading; this is not an OS
//! sandbox for an adversarial process mutating the filesystem concurrently.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::core::admission::repository_path_error;
use crate::core::error::{ContribError, Result};

const MAX_FILES: usize = 32;
const MAX_PATH_BYTES: usize = 1024;
const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_TOTAL_BYTES: usize = 256 * 1024;

/// A deterministic, immutable collection of explicitly allowlisted UTF-8 files.
///
/// Content is copied at construction and is never refreshed from the filesystem.
/// Debug output deliberately omits file content.
#[derive(Clone)]
pub struct HarnessSnapshot {
    files: BTreeMap<String, String>,
}

impl fmt::Debug for HarnessSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HarnessSnapshot")
            .field("paths", &self.paths())
            .field(
                "total_bytes",
                &self.files.values().map(String::len).sum::<usize>(),
            )
            .finish()
    }
}

impl HarnessSnapshot {
    /// Load only `paths` under `root`, rejecting symlinks and non-regular files.
    ///
    /// Paths must be unique canonical relative POSIX paths of at most 1024 bytes.
    /// At most 32 files, 64 KiB per file, and 256 KiB total are accepted. Reads
    /// are byte-bounded even if a file grows after its metadata was inspected. All path policy
    /// checks run before reading any file, and partial results are never returned.
    ///
    /// The root and every existing path component must not be symlinks. Keep the
    /// tree stable during loading: portable checks cannot close the concurrent
    /// filesystem check/open race described in this module's documentation.
    pub async fn load(root: &Path, paths: &[String]) -> Result<Self> {
        check_file_count(paths.len())?;
        let mut selected = BTreeSet::new();
        for path in paths {
            check_path(path)?;
            if !selected.insert(path.as_str()) {
                return Err(invalid(format!("duplicate snapshot path: {path:?}")));
            }
        }

        let root = checked_root(root).await?;
        let canonical_root = tokio::fs::canonicalize(&root).await?;
        let mut files = BTreeMap::new();
        let mut total_bytes = 0;
        for path in selected {
            let file_path = checked_file_path(&root, &canonical_root, path).await?;
            let file = tokio::fs::File::open(&file_path).await?;
            let metadata = file.metadata().await?;
            if !metadata.is_file() {
                return Err(invalid(format!(
                    "snapshot path is not a regular file: {path:?}"
                )));
            }
            let remaining = MAX_TOTAL_BYTES - total_bytes;
            let byte_limit = MAX_FILE_BYTES.min(remaining);
            if metadata.len() > byte_limit as u64 {
                return Err(invalid("snapshot file or total byte budget exceeded"));
            }
            // `take` bounds actual I/O, not just an untrusted metadata estimate.
            // The extra byte distinguishes a limit-sized file from truncation.
            let mut bytes = Vec::new();
            file.take(byte_limit as u64 + 1)
                .read_to_end(&mut bytes)
                .await?;
            if bytes.len() > byte_limit {
                return Err(invalid("snapshot file or total byte budget exceeded"));
            }
            let content = String::from_utf8(bytes)
                .map_err(|_| invalid(format!("snapshot file is not UTF-8: {path:?}")))?;
            total_bytes += content.len();
            files.insert(path.to_owned(), content);
        }
        Self::from_files(files)
    }

    /// Construct a deterministic snapshot without accessing the filesystem.
    ///
    /// The same filename and byte-budget policy as [`Self::load`] applies. A
    /// `BTreeMap` already guarantees unique paths; strings are necessarily UTF-8.
    pub fn from_files(files: BTreeMap<String, String>) -> Result<Self> {
        check_file_count(files.len())?;
        let mut total_bytes = 0;
        for (path, content) in &files {
            check_path(path)?;
            if content.len() > MAX_FILE_BYTES {
                return Err(invalid("snapshot file exceeds the 64 KiB byte budget"));
            }
            total_bytes += content.len();
            if total_bytes > MAX_TOTAL_BYTES {
                return Err(invalid("snapshot exceeds the 256 KiB total byte budget"));
            }
        }
        Ok(Self { files })
    }

    /// Return the exact allowlisted paths in lexicographic order.
    pub fn paths(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    /// Borrow the frozen contents of an exact allowlisted path, if present.
    pub fn read(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Return lowercase hex SHA-256 hashes of the exact UTF-8 content bytes.
    pub fn fingerprints(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|(path, content)| {
                (
                    path.clone(),
                    hex::encode(Sha256::digest(content.as_bytes())),
                )
            })
            .collect()
    }
}

fn invalid(message: impl Into<String>) -> ContribError {
    ContribError::Config(message.into())
}

fn check_file_count(count: usize) -> Result<()> {
    if count > MAX_FILES {
        return Err(invalid("snapshot exceeds the 32-file limit"));
    }
    Ok(())
}

fn check_path(path: &str) -> Result<()> {
    if path.len() > MAX_PATH_BYTES {
        return Err(invalid("snapshot path exceeds the 1024-byte limit"));
    }
    if let Some(reason) = repository_path_error(path) {
        return Err(invalid(format!("unsafe snapshot path {path:?}: {reason}")));
    }
    for component in path.split('/') {
        // Reject Windows alternate streams and normalization aliases even when
        // running on Unix, so the policy stays portable and deterministic.
        if component.contains([':', '*']) || component.ends_with(['.', ' ']) {
            return Err(invalid(format!(
                "unsafe snapshot path {path:?}: non-portable path component"
            )));
        }
        let component = component.to_ascii_lowercase();
        if component == ".git" || is_credential_component(&component) {
            return Err(invalid(format!(
                "snapshot path contains repository metadata or a credential filename: {path:?}"
            )));
        }
    }
    Ok(())
}

fn is_credential_component(component: &str) -> bool {
    matches!(
        component,
        ".env"
            | ".envrc"
            | ".ssh"
            | ".aws"
            | ".azure"
            | ".gnupg"
            | ".netrc"
            | "_netrc"
            | ".npmrc"
            | ".pypirc"
            | "config.yaml"
            | "config.yml"
            | "credentials"
            | "credentials.json"
            | "credentials.yaml"
            | "credentials.yml"
            | "secrets.json"
            | "secrets.yaml"
            | "secrets.yml"
            | "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
            | "key"
            | "pem"
    ) || component.starts_with(".env.")
        || [".key", ".pem", ".p12", ".pfx", ".jks"]
            .iter()
            .any(|extension| component.ends_with(extension))
}

async fn checked_root(root: &Path) -> Result<PathBuf> {
    if root
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(invalid("snapshot root must not contain parent traversal"));
    }
    let absolute = std::path::absolute(root)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        if matches!(component, Component::ParentDir) {
            return Err(invalid("snapshot root must not contain parent traversal"));
        }
        current.push(component.as_os_str());
        // A Windows drive/UNC prefix is not a standalone directory. Inspect it
        // after the following root component has completed the absolute path.
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = tokio::fs::symlink_metadata(&current).await?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid(
                "snapshot root components must be non-symlink directories",
            ));
        }
    }
    Ok(absolute)
}

async fn checked_file_path(root: &Path, canonical_root: &Path, path: &str) -> Result<PathBuf> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = Path::new(path).components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(part) = component else {
            return Err(invalid("snapshot path must contain only normal components"));
        };
        current.push(part);
        let metadata = tokio::fs::symlink_metadata(&current).await?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(format!(
                "snapshot path contains a symlink: {path:?}"
            )));
        }
        let is_last = index + 1 == components.len();
        if (is_last && !metadata.is_file()) || (!is_last && !metadata.is_dir()) {
            return Err(invalid(format!(
                "snapshot path is not a regular file with directory ancestors: {path:?}"
            )));
        }
        if is_last && metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(invalid("snapshot file exceeds the 64 KiB byte budget"));
        }
    }
    let canonical = tokio::fs::canonicalize(&current).await?;
    if !canonical.starts_with(canonical_root) {
        return Err(invalid(format!(
            "snapshot path escapes its repository root: {path:?}"
        )));
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(files: &[(&str, &str)]) -> Result<HarnessSnapshot> {
        HarnessSnapshot::from_files(
            files
                .iter()
                .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
                .collect(),
        )
    }

    async fn fixture_root() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        // Temp directories can have platform-provided symlink ancestors (e.g.
        // /var on macOS); fixtures use the physical path rather than an alias.
        let root = tokio::fs::canonicalize(directory.path()).await.unwrap();
        (directory, root)
    }

    #[test]
    fn paths_and_fingerprints_are_deterministic() {
        let snapshot = fixture(&[("z.txt", ""), ("src/a.rs", "abc")]).unwrap();
        assert_eq!(snapshot.paths(), ["src/a.rs", "z.txt"]);
        assert_eq!(snapshot.read("src/a.rs"), Some("abc"));
        assert_eq!(snapshot.read("./src/a.rs"), None);
        assert_eq!(snapshot.read("missing.txt"), None);
        assert_eq!(
            snapshot.fingerprints(),
            BTreeMap::from([
                (
                    "src/a.rs".to_owned(),
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
                ),
                (
                    "z.txt".to_owned(),
                    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned()
                )
            ])
        );
        assert_eq!(snapshot.clone().fingerprints(), snapshot.fingerprints());
        assert!(!format!("{snapshot:?}").contains("abc"));
    }

    #[test]
    fn rejects_unsafe_or_credential_shaped_paths() {
        for path in [
            "",
            "/etc/passwd",
            "../outside.txt",
            "src/../outside.txt",
            "./source.rs",
            "src//lib.rs",
            "src/",
            "C:/Windows/file.txt",
            "src\\lib.rs",
            "src/file:stream",
            "src/*.rs",
            "src/file.",
            "src/file ",
            "src/%2e%2e/file.txt",
            "src/a\n.rs",
            ".git/config",
            "nested/.GIT/HEAD",
            ".env",
            "nested/.ENV.local",
            ".env.example",
            "settings/.envrc",
            ".ssh/id_ed25519",
            ".aws/config",
            "keys/private.KEY",
            "cert/server.pem",
            "config.yaml",
            "nested/CONFIG.YAML",
            "config.yml",
            "credentials.json",
            "secrets.yml",
            "id_rsa",
            ".npmrc",
        ] {
            assert!(fixture(&[(path, "")]).is_err(), "accepted {path:?}");
        }
        assert!(fixture(&[
            ("src/key.rs", ""),
            ("docs/environment.md", ""),
            ("config.example.yaml", ""),
            (".github/contribai.yml", ""),
            ("web/[id].tsx", ""),
        ])
        .is_ok());
    }

    #[test]
    fn in_memory_budgets_include_exact_limits_and_utf8_bytes() {
        assert!(HarnessSnapshot::from_files(BTreeMap::new()).is_ok());
        let files: BTreeMap<_, _> = (0..MAX_FILES)
            .map(|index| (format!("file{index}.txt"), String::new()))
            .collect();
        assert!(HarnessSnapshot::from_files(files.clone()).is_ok());
        let mut too_many = files;
        too_many.insert("extra.txt".to_owned(), String::new());
        assert!(HarnessSnapshot::from_files(too_many).is_err());

        assert!(fixture(&[("file.txt", &"x".repeat(MAX_FILE_BYTES))]).is_ok());
        assert!(fixture(&[("file.txt", &"x".repeat(MAX_FILE_BYTES + 1))]).is_err());
        assert!(fixture(&[("file.txt", &"é".repeat(MAX_FILE_BYTES / 2))]).is_ok());
        assert!(fixture(&[("file.txt", &"é".repeat(MAX_FILE_BYTES / 2 + 1))]).is_err());
        let files: BTreeMap<_, _> = (0..4)
            .map(|index| (format!("file{index}.txt"), "x".repeat(MAX_FILE_BYTES)))
            .collect();
        assert!(HarnessSnapshot::from_files(files.clone()).is_ok());
        let mut oversized = files;
        oversized.insert("extra.txt".to_owned(), "x".to_owned());
        assert!(HarnessSnapshot::from_files(oversized).is_err());
    }

    #[test]
    fn path_budget_counts_bytes_and_includes_the_exact_limit() {
        assert!(fixture(&[(&"x".repeat(MAX_PATH_BYTES), "")]).is_ok());
        assert!(fixture(&[(&"x".repeat(MAX_PATH_BYTES + 1), "")]).is_err());
        assert!(fixture(&[(&"é".repeat(MAX_PATH_BYTES / 2), "")]).is_ok());
        assert!(fixture(&[(&"é".repeat(MAX_PATH_BYTES / 2 + 1), "")]).is_err());
    }

    #[tokio::test]
    async fn load_is_allowlisted_and_frozen() {
        let (_directory, root) = fixture_root().await;
        tokio::fs::create_dir(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/lib.rs"), "before")
            .await
            .unwrap();
        tokio::fs::write(root.join("not-selected.txt"), "not included")
            .await
            .unwrap();
        let snapshot = HarnessSnapshot::load(&root, &["src/lib.rs".to_owned()])
            .await
            .unwrap();
        tokio::fs::write(root.join("src/lib.rs"), "after")
            .await
            .unwrap();
        assert_eq!(snapshot.paths(), ["src/lib.rs"]);
        assert_eq!(snapshot.read("src/lib.rs"), Some("before"));
        assert_eq!(snapshot.read("not-selected.txt"), None);
    }

    #[tokio::test]
    async fn validates_allowlist_before_accessing_root() {
        let absent = Path::new("snapshot-root-that-does-not-exist");
        for paths in [
            vec!["file.txt".to_owned(), "file.txt".to_owned()],
            vec!["../file.txt".to_owned()],
            vec![".env".to_owned()],
            vec!["x".repeat(MAX_PATH_BYTES + 1)],
            (0..=MAX_FILES)
                .map(|index| format!("file{index}.txt"))
                .collect(),
        ] {
            assert!(matches!(
                HarnessSnapshot::load(absent, &paths).await,
                Err(ContribError::Config(_))
            ));
        }
    }

    #[tokio::test]
    async fn rejects_nonfiles_missing_files_and_non_utf8() {
        let (_directory, root) = fixture_root().await;
        tokio::fs::create_dir(root.join("directory")).await.unwrap();
        tokio::fs::write(root.join("binary.txt"), [0xff, 0xfe])
            .await
            .unwrap();
        for path in ["directory", "missing.txt", "binary.txt"] {
            assert!(HarnessSnapshot::load(&root, &[path.to_owned()])
                .await
                .is_err());
        }
        assert!(HarnessSnapshot::load(&root.join("binary.txt"), &[])
            .await
            .is_err());
        assert!(HarnessSnapshot::load(&root.join("directory/.."), &[])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn filesystem_budgets_include_exact_limits() {
        let (_directory, root) = fixture_root().await;
        let mut paths = Vec::new();
        for index in 0..4 {
            let path = format!("file{index}.txt");
            tokio::fs::write(root.join(&path), vec![b'x'; MAX_FILE_BYTES])
                .await
                .unwrap();
            paths.push(path);
        }
        assert!(HarnessSnapshot::load(&root, &paths).await.is_ok());
        tokio::fs::write(root.join("empty.txt"), b"").await.unwrap();
        paths.push("empty.txt".to_owned());
        assert!(HarnessSnapshot::load(&root, &paths).await.is_ok());
        tokio::fs::write(root.join("empty.txt"), b"x")
            .await
            .unwrap();
        assert!(HarnessSnapshot::load(&root, &paths).await.is_err());
        tokio::fs::write(root.join("oversized.txt"), vec![b'x'; MAX_FILE_BYTES + 1])
            .await
            .unwrap();
        assert!(HarnessSnapshot::load(&root, &["oversized.txt".to_owned()])
            .await
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlinks_in_file_directory_root_and_root_ancestor() {
        let (_directory, root) = fixture_root().await;
        let (_outside_directory, outside) = fixture_root().await;
        tokio::fs::write(outside.join("file.txt"), "outside")
            .await
            .unwrap();
        tokio::fs::write(root.join("file.txt"), "inside")
            .await
            .unwrap();
        tokio::fs::create_dir(root.join("real")).await.unwrap();
        tokio::fs::symlink(root.join("file.txt"), root.join("internal-link.txt"))
            .await
            .unwrap();
        tokio::fs::symlink(outside.join("file.txt"), root.join("external-link.txt"))
            .await
            .unwrap();
        tokio::fs::symlink(&outside, root.join("directory-link"))
            .await
            .unwrap();
        tokio::fs::symlink(&root, root.join("root-link"))
            .await
            .unwrap();
        for path in [
            "internal-link.txt",
            "external-link.txt",
            "directory-link/file.txt",
        ] {
            assert!(HarnessSnapshot::load(&root, &[path.to_owned()])
                .await
                .is_err());
        }
        assert!(
            HarnessSnapshot::load(&root.join("root-link"), &["file.txt".to_owned()])
                .await
                .is_err()
        );
        assert!(HarnessSnapshot::load(&root.join("root-link/real"), &[])
            .await
            .is_err());
    }
}
