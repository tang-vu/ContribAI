//! Maintainer-controlled admission for AI-mediated contributions.
//!
//! This module deliberately separates generating a change from earning permission
//! to publish it. A contribution is publishable only when it carries an explicit,
//! time-bounded permit rooted in repository or maintainer consent and passes the
//! scope policy. The resulting evidence capsule is deterministic and auditable.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::models::{Contribution, FileChange, Issue, Repository};
use crate::github::client::GitHubClient;

/// Repository files that explicitly enable ContribAI writes.
///
/// The YAML manifest is canonical. The marker-style paths remain readable for
/// compatibility with the experimental v1 protocol.
pub const CONSENT_PATHS: &[&str] = &[
    ".github/contribai.yml",
    ".github/CONTRIBAI_ALLOW",
    "CONTRIBAI_ALLOW",
];

/// Labels that only repository collaborators with triage permission can normally apply.
pub const MAINTAINER_APPROVAL_LABELS: &[&str] = &[
    "agent-ready",
    "contribai-approved",
    "ai-contribution-approved",
];

const DEFAULT_MAX_FILES: usize = 5;
const DEFAULT_MAX_CHANGED_LINES: usize = 250;
const DEFAULT_PERMIT_TTL_HOURS: i64 = 24;
const DEFAULT_MAX_RUNTIME_SECONDS: u64 = 900;
const CONSENT_SCHEMA_VERSION: u8 = 1;
const CONSENT_SCHEMA_VERSION_V2: u8 = 2;
const EVIDENCE_SCHEMA_VERSION: u8 = 2;
const ADMISSION_AUDIT_SCHEMA_VERSION: u8 = 1;
const ADMISSION_AUDIT_SCHEMA_VERSION_V2: u8 = 2;

/// Where a run's deterministic checks may execute.
///
/// Consenting to a proposal does not imply consenting to arbitrary local
/// execution — but v6 already ran local sandbox validation, so `local` is the
/// backward-compatible default. `off` disables command execution entirely
/// (every command-dependent check is then recorded as skipped, never passed).
/// `container` requires the container backend; it is not silently downgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Off,
    Local,
    Container,
}

impl ExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Local => "local",
            Self::Container => "container",
        }
    }
}

/// Source of the maintainer's consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsentSource {
    RepositoryManifest { path: String },
    MaintainerLabel { issue: i64, label: String },
}

/// Parsed repository-side consent and its review budget.
///
/// Schema 1 fields carry their v6 meaning exactly. Schema 2 adds the
/// maintainer controls that cannot be expressed in schema 1; its defaults are
/// restrictive (opt-in) so an incomplete manifest never widens permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryConsent {
    pub source: ConsentSource,
    /// Manifest schema that produced this consent (`1` or `2`). Label-derived
    /// consent reports `1` — it predates the schema system.
    pub schema_version: u8,
    pub max_files: usize,
    pub max_changed_lines: usize,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Additional maintainer deny-globs, evaluated after protected paths.
    /// Schema 2 only; always empty for schema 1.
    #[serde(default)]
    pub denied_paths: Vec<String>,
    /// Named validation checks that must appear and pass before admission.
    /// Empty means "no specific checks required".
    #[serde(default)]
    pub required_checks: Vec<String>,
    /// Whether dependency manifests/lockfiles may change. `true` for schema 1
    /// (v6 allowed in-scope manifest edits); schema 2 defaults to `false`.
    pub allow_dependency_changes: bool,
    /// Whether new files may be added. `true` for schema 1; schema 2 keeps
    /// `true` as the default since a contribution that cannot create files is
    /// rarely useful — maintainers tighten it explicitly.
    pub allow_new_files: bool,
    /// Whether test files may change.
    pub allow_test_changes: bool,
    /// Whether the run must carry reproduction evidence to be admissible.
    pub required_reproduction: bool,
    /// Where deterministic checks may execute.
    pub execution_mode: ExecutionMode,
    /// Wall-clock bound for a single run.
    pub max_runtime_seconds: u64,
    /// Extra maintainer label names treated as approval labels for
    /// issue-scoped consent. Schema 2 only.
    #[serde(default)]
    pub allowed_issue_labels: Vec<String>,
    pub draft_only: bool,
}

impl RepositoryConsent {
    /// Parse the consent manifest.
    ///
    /// A manifest is valid only when it contains `enabled: true`. Unknown
    /// fields and unsupported schema versions fail closed so typos cannot
    /// widen scope. A schema-1 manifest containing schema-2 fields is rejected
    /// outright — the maintainer must declare `schema_version: 2`.
    pub fn parse(path: &str, content: &str) -> Option<Self> {
        let probe: SchemaProbe = serde_yaml::from_str(content).ok()?;
        match probe.schema_version.unwrap_or(CONSENT_SCHEMA_VERSION) {
            CONSENT_SCHEMA_VERSION => Self::parse_v1(path, content),
            CONSENT_SCHEMA_VERSION_V2 => Self::parse_v2(path, content),
            _ => None,
        }
    }

    fn parse_v1(path: &str, content: &str) -> Option<Self> {
        let manifest: ConsentManifest = serde_yaml::from_str(content).ok()?;
        if !manifest.enabled {
            return None;
        }
        let (max_files, max_changed_lines) =
            budgets(manifest.max_files, manifest.max_changed_lines)?;
        let allowed_paths = manifest.allowed_paths.into_paths();
        if allowed_paths
            .iter()
            .any(|pattern| !is_safe_allow_pattern(pattern))
        {
            return None;
        }
        Some(Self {
            source: ConsentSource::RepositoryManifest {
                path: path.to_string(),
            },
            schema_version: CONSENT_SCHEMA_VERSION,
            max_files,
            max_changed_lines,
            allowed_paths,
            denied_paths: Vec::new(),
            required_checks: Vec::new(),
            // v6 had no dependency control; schema 1 preserves its semantics.
            allow_dependency_changes: true,
            allow_new_files: true,
            allow_test_changes: true,
            required_reproduction: false,
            execution_mode: ExecutionMode::Local,
            max_runtime_seconds: DEFAULT_MAX_RUNTIME_SECONDS,
            allowed_issue_labels: Vec::new(),
            draft_only: true,
        })
    }

    fn parse_v2(path: &str, content: &str) -> Option<Self> {
        let manifest: ConsentManifestV2 = serde_yaml::from_str(content).ok()?;
        if !manifest.enabled {
            return None;
        }
        let (max_files, max_changed_lines) =
            budgets(manifest.max_files, manifest.max_changed_lines)?;
        let allowed_paths = manifest.allowed_paths.into_paths();
        let denied_paths = manifest.denied_paths.into_paths();
        if allowed_paths
            .iter()
            .chain(denied_paths.iter())
            .any(|pattern| !is_safe_allow_pattern(pattern))
        {
            return None;
        }
        // Denied paths must not include protected paths redundantly, but
        // overlap is harmless — protected paths are checked first anyway.
        // Required checks are a maintainer-declared admission requirement:
        // a name that cannot be represented safely must reject the
        // manifest, not silently drop the requirement. Names are stored in
        // canonical form (`cargo test` → `cargo_test`) so they match the
        // validation-graph nodes that ran them.
        let mut required_checks = Vec::new();
        for name in manifest.required_checks {
            let canonical = crate::core::validation_graph::canonical_check_name(&name);
            if canonical.is_empty() {
                continue;
            }
            if !is_safe_check_name(&canonical) {
                return None;
            }
            required_checks.push(canonical);
        }
        let allowed_issue_labels = manifest
            .allowed_issue_labels
            .into_iter()
            .map(|label| label.trim().to_string())
            .filter(|label| is_safe_label_name(label))
            .collect::<Vec<_>>();
        let max_runtime_seconds = manifest
            .max_runtime_seconds
            .unwrap_or(DEFAULT_MAX_RUNTIME_SECONDS);
        if max_runtime_seconds == 0 || max_runtime_seconds > 86_400 {
            return None;
        }
        Some(Self {
            source: ConsentSource::RepositoryManifest {
                path: path.to_string(),
            },
            schema_version: CONSENT_SCHEMA_VERSION_V2,
            max_files,
            max_changed_lines,
            allowed_paths,
            denied_paths,
            required_checks,
            allow_dependency_changes: manifest.allow_dependency_changes,
            allow_new_files: manifest.allow_new_files.unwrap_or(true),
            allow_test_changes: manifest.allow_test_changes.unwrap_or(true),
            required_reproduction: manifest.required_reproduction,
            execution_mode: manifest.execution_mode.unwrap_or(ExecutionMode::Local),
            max_runtime_seconds,
            allowed_issue_labels,
            draft_only: true,
        })
    }

    /// Construct consent from a maintainer-controlled issue label.
    pub fn from_issue(issue: &Issue) -> Option<Self> {
        Self::from_issue_with_labels(issue, &[])
    }

    /// Label-derived consent, honoring manifest-extended label names.
    ///
    /// Label consent gets the conservative schema-1 profile: v6 semantics
    /// plus the schema-2 restrictive default for dependency changes kept as
    /// `true` for parity with schema 1.
    pub fn from_issue_with_labels(issue: &Issue, extra_labels: &[String]) -> Option<Self> {
        if !issue.state.eq_ignore_ascii_case("open") {
            return None;
        }
        let label = issue.labels.iter().find(|label| {
            MAINTAINER_APPROVAL_LABELS
                .iter()
                .any(|allowed| label.eq_ignore_ascii_case(allowed))
                || extra_labels
                    .iter()
                    .any(|allowed| label.eq_ignore_ascii_case(allowed))
        })?;
        Some(Self::from_label(issue.number, label))
    }

    /// Label-derived consent for a known issue number and label name.
    /// Same conservative schema-1 profile as `from_issue_with_labels` —
    /// the label only authorizes when it was verified upstream.
    pub fn from_label(issue: i64, label: &str) -> Self {
        Self {
            source: ConsentSource::MaintainerLabel {
                issue,
                label: label.to_string(),
            },
            schema_version: CONSENT_SCHEMA_VERSION,
            max_files: DEFAULT_MAX_FILES,
            max_changed_lines: DEFAULT_MAX_CHANGED_LINES,
            allowed_paths: Vec::new(),
            denied_paths: Vec::new(),
            required_checks: Vec::new(),
            allow_dependency_changes: true,
            allow_new_files: true,
            allow_test_changes: true,
            required_reproduction: false,
            execution_mode: ExecutionMode::Local,
            max_runtime_seconds: DEFAULT_MAX_RUNTIME_SECONDS,
            allowed_issue_labels: Vec::new(),
            draft_only: true,
        }
    }
}

fn budgets(max_files: Option<usize>, max_changed_lines: Option<usize>) -> Option<(usize, usize)> {
    let max_files = max_files.unwrap_or(DEFAULT_MAX_FILES);
    let max_changed_lines = max_changed_lines.unwrap_or(DEFAULT_MAX_CHANGED_LINES);
    if max_files == 0 || max_changed_lines == 0 {
        return None;
    }
    Some((max_files, max_changed_lines))
}

/// A check name is a safe identifier the validation graph can carry.
fn is_safe_check_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// A label name a maintainer may add; conservative charset only.
fn is_safe_label_name(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 64
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ' | ':' | '/'))
}

#[derive(Debug, Deserialize)]
struct SchemaProbe {
    schema_version: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentManifest {
    /// Retained so `deny_unknown_fields` still accepts the `schema_version`
    /// key; dispatch happens in [`SchemaProbe`].
    #[allow(dead_code)]
    schema_version: Option<u8>,
    #[serde(default)]
    enabled: bool,
    max_files: Option<usize>,
    max_changed_lines: Option<usize>,
    #[serde(default)]
    allowed_paths: AllowedPaths,
}

/// Schema 2 adds maintainer controls schema 1 cannot express. Every field is
/// optional; every default is restrictive or matches schema-1 semantics.
/// Unknown fields fail closed via `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentManifestV2 {
    /// See [`ConsentManifest::schema_version`].
    #[allow(dead_code)]
    schema_version: Option<u8>,
    #[serde(default)]
    enabled: bool,
    max_files: Option<usize>,
    max_changed_lines: Option<usize>,
    #[serde(default)]
    allowed_paths: AllowedPaths,
    #[serde(default)]
    denied_paths: AllowedPaths,
    #[serde(default)]
    required_checks: Vec<String>,
    /// Default `false`: dependency manifests and lockfiles are denied.
    #[serde(default)]
    allow_dependency_changes: bool,
    allow_new_files: Option<bool>,
    allow_test_changes: Option<bool>,
    /// Default `false`: reproduction evidence is recommended, not required.
    #[serde(default)]
    required_reproduction: bool,
    execution_mode: Option<ExecutionMode>,
    max_runtime_seconds: Option<u64>,
    #[serde(default)]
    allowed_issue_labels: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum AllowedPaths {
    List(Vec<String>),
    CommaSeparated(String),
    #[default]
    Empty,
}

impl AllowedPaths {
    fn into_paths(self) -> Vec<String> {
        let paths = match self {
            Self::List(paths) => paths,
            Self::CommaSeparated(paths) => paths.split(',').map(str::to_string).collect(),
            Self::Empty => Vec::new(),
        };
        paths
            .into_iter()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .collect()
    }
}

/// Discover explicit repository-side consent. Missing, empty, or malformed files
/// are a denial, never an implicit approval.
pub async fn discover_repository_consent(
    github: &GitHubClient,
    owner: &str,
    repo: &str,
) -> Option<RepositoryConsent> {
    for path in CONSENT_PATHS {
        if let Ok(content) = github.get_file_content(owner, repo, path, None).await {
            if let Some(consent) = RepositoryConsent::parse(path, &content) {
                return Some(consent);
            }
        }
    }
    None
}

/// A time-bounded capability to propose exactly one contribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributionPermit {
    pub id: String,
    pub repository: String,
    pub base_sha: String,
    pub source: ConsentSource,
    /// Consent schema that authorized this permit (1 or 2).
    #[serde(default = "default_schema_version")]
    pub consent_schema_version: u8,
    pub issue: Option<i64>,
    pub allowed_paths: Vec<String>,
    /// Maintainer deny-globs evaluated after protected paths.
    #[serde(default)]
    pub denied_paths: Vec<String>,
    /// Named checks the run must pass before admission.
    #[serde(default)]
    pub required_checks: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_dependency_changes: bool,
    #[serde(default = "default_true")]
    pub allow_new_files: bool,
    #[serde(default = "default_true")]
    pub allow_test_changes: bool,
    #[serde(default)]
    pub required_reproduction: bool,
    #[serde(default = "default_execution_mode")]
    pub execution_mode: ExecutionMode,
    #[serde(default = "default_max_runtime")]
    pub max_runtime_seconds: u64,
    pub max_files: usize,
    pub max_changed_lines: usize,
    pub draft_only: bool,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

fn default_schema_version() -> u8 {
    CONSENT_SCHEMA_VERSION
}
fn default_true() -> bool {
    true
}
fn default_execution_mode() -> ExecutionMode {
    ExecutionMode::Local
}
fn default_max_runtime() -> u64 {
    DEFAULT_MAX_RUNTIME_SECONDS
}

impl ContributionPermit {
    pub fn issue(
        repository: &Repository,
        base_sha: impl Into<String>,
        consent: RepositoryConsent,
        issue: Option<i64>,
    ) -> Self {
        let issued_at = Utc::now();
        let mut permit = Self {
            id: String::new(),
            repository: repository.full_name.clone(),
            base_sha: base_sha.into(),
            source: consent.source,
            consent_schema_version: consent.schema_version,
            issue,
            allowed_paths: consent.allowed_paths,
            denied_paths: consent.denied_paths,
            required_checks: consent.required_checks,
            allow_dependency_changes: consent.allow_dependency_changes,
            allow_new_files: consent.allow_new_files,
            allow_test_changes: consent.allow_test_changes,
            required_reproduction: consent.required_reproduction,
            execution_mode: consent.execution_mode,
            max_runtime_seconds: consent.max_runtime_seconds,
            max_files: consent.max_files,
            max_changed_lines: consent.max_changed_lines,
            draft_only: consent.draft_only,
            issued_at,
            expires_at: issued_at + Duration::hours(DEFAULT_PERMIT_TTL_HOURS),
        };
        permit.id = permit.fingerprint();
        permit
    }

    fn fingerprint(&self) -> String {
        let material = format!(
            "v3\n{}\n{}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.repository,
            self.base_sha,
            self.source,
            self.issue,
            self.allowed_paths,
            self.denied_paths,
            self.required_checks,
            self.allow_dependency_changes,
            self.allow_new_files,
            self.allow_test_changes,
            self.required_reproduction,
            self.execution_mode.as_str(),
            self.max_runtime_seconds,
            self.max_files,
            self.max_changed_lines,
            self.draft_only,
            self.issued_at.timestamp(),
            self.expires_at.timestamp()
        );
        short_sha256(material.as_bytes())
    }
}

/// Stable reason why a contribution did not earn admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionViolation {
    ExternalWritesNotEnabled,
    MissingConsent,
    MissingBaseRevision,
    InvalidBaseRevision,
    PermitExpired,
    RepositoryMismatch,
    TooManyFiles {
        actual: usize,
        maximum: usize,
    },
    TooManyChangedLines {
        actual: usize,
        maximum: usize,
    },
    ProtectedPath {
        path: String,
    },
    InvalidPath {
        path: String,
        reason: String,
    },
    DuplicatePath {
        path: String,
    },
    UnsupportedDeletion {
        path: String,
    },
    PathOutsidePermit {
        path: String,
    },
    /// Path matched a maintainer `denied_paths` glob (schema 2).
    DeniedPath {
        path: String,
    },
    /// Dependency manifest/lockfile change without `allow_dependency_changes`.
    DependencyChangeNotAllowed {
        path: String,
    },
    /// New file without `allow_new_files`.
    NewFilesNotAllowed {
        path: String,
    },
    /// Test-file change without `allow_test_changes`.
    TestChangeNotAllowed {
        path: String,
    },
    /// A `required_checks` entry did not pass (evaluated at run admission).
    MissingRequiredCheck {
        name: String,
    },
    /// `required_reproduction` set but no reproduction evidence exists.
    ReproductionRequired,
}

impl std::fmt::Display for AdmissionViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExternalWritesNotEnabled => write!(formatter, "external writes were not enabled"),
            Self::MissingConsent => write!(formatter, "maintainer consent was not found"),
            Self::MissingBaseRevision => write!(formatter, "base revision could not be attested"),
            Self::InvalidBaseRevision => write!(formatter, "base revision is not a full Git SHA"),
            Self::PermitExpired => write!(formatter, "contribution permit expired"),
            Self::RepositoryMismatch => write!(formatter, "permit belongs to another repository"),
            Self::TooManyFiles { actual, maximum } => {
                write!(formatter, "changed {actual} files; permit allows {maximum}")
            }
            Self::TooManyChangedLines { actual, maximum } => {
                write!(formatter, "changed {actual} lines; permit allows {maximum}")
            }
            Self::ProtectedPath { path } => write!(formatter, "protected path: {path}"),
            Self::InvalidPath { path, reason } => {
                write!(formatter, "invalid repository path {path:?}: {reason}")
            }
            Self::DuplicatePath { path } => write!(formatter, "duplicate repository path: {path}"),
            Self::UnsupportedDeletion { path } => {
                write!(formatter, "file deletion is not supported: {path}")
            }
            Self::PathOutsidePermit { path } => write!(formatter, "path outside permit: {path}"),
            Self::DeniedPath { path } => {
                write!(formatter, "path denied by maintainer policy: {path}")
            }
            Self::DependencyChangeNotAllowed { path } => {
                write!(formatter, "dependency change not authorized: {path}")
            }
            Self::NewFilesNotAllowed { path } => {
                write!(formatter, "new files not authorized: {path}")
            }
            Self::TestChangeNotAllowed { path } => {
                write!(formatter, "test changes not authorized: {path}")
            }
            Self::MissingRequiredCheck { name } => {
                write!(formatter, "required check did not pass: {name}")
            }
            Self::ReproductionRequired => {
                write!(formatter, "maintainer requires reproduction evidence")
            }
        }
    }
}

/// Complete admission report. An empty violation list is the only allow state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionReport {
    pub allowed: bool,
    pub violations: Vec<AdmissionViolation>,
    pub file_count: usize,
    pub changed_lines: usize,
    pub paths: Vec<String>,
}

pub struct AdmissionController;

impl AdmissionController {
    pub fn evaluate(
        repository: &Repository,
        contribution: &Contribution,
        permit: &ContributionPermit,
        now: DateTime<Utc>,
    ) -> AdmissionReport {
        let changes: Vec<&FileChange> = contribution
            .changes
            .iter()
            .chain(contribution.tests_added.iter())
            .collect();
        let paths: Vec<String> = changes.iter().map(|change| change.path.clone()).collect();
        let changed_lines = changes
            .iter()
            .map(|change| changed_line_count(change))
            .sum();
        let mut violations = Vec::new();

        if permit.repository != repository.full_name {
            violations.push(AdmissionViolation::RepositoryMismatch);
        }
        if permit.base_sha.trim().is_empty() {
            violations.push(AdmissionViolation::MissingBaseRevision);
        } else if !is_full_commit_sha(&permit.base_sha) {
            violations.push(AdmissionViolation::InvalidBaseRevision);
        }
        if now > permit.expires_at {
            violations.push(AdmissionViolation::PermitExpired);
        }
        if paths.len() > permit.max_files {
            violations.push(AdmissionViolation::TooManyFiles {
                actual: paths.len(),
                maximum: permit.max_files,
            });
        }
        if changed_lines > permit.max_changed_lines {
            violations.push(AdmissionViolation::TooManyChangedLines {
                actual: changed_lines,
                maximum: permit.max_changed_lines,
            });
        }

        let mut unique_paths = BTreeSet::new();
        for change in changes {
            let path = &change.path;
            if let Some(reason) = repository_path_error(path) {
                violations.push(AdmissionViolation::InvalidPath {
                    path: path.clone(),
                    reason: reason.to_string(),
                });
                continue;
            }
            if !unique_paths.insert(path.clone()) {
                violations.push(AdmissionViolation::DuplicatePath { path: path.clone() });
            }
            if change.is_deleted {
                violations.push(AdmissionViolation::UnsupportedDeletion { path: path.clone() });
            }
            if is_protected_path(path) {
                violations.push(AdmissionViolation::ProtectedPath { path: path.clone() });
            } else {
                if !permit.allowed_paths.is_empty()
                    && !permit
                        .allowed_paths
                        .iter()
                        .any(|pattern| path_matches(pattern, path))
                {
                    violations.push(AdmissionViolation::PathOutsidePermit { path: path.clone() });
                }
                if permit
                    .denied_paths
                    .iter()
                    .any(|pattern| path_matches(pattern, path))
                {
                    violations.push(AdmissionViolation::DeniedPath { path: path.clone() });
                }
            }
            if !permit.allow_dependency_changes && is_dependency_path(path) {
                violations
                    .push(AdmissionViolation::DependencyChangeNotAllowed { path: path.clone() });
            }
            if !permit.allow_new_files && change.is_new_file {
                violations.push(AdmissionViolation::NewFilesNotAllowed { path: path.clone() });
            }
            if !permit.allow_test_changes && is_test_path(path) {
                violations.push(AdmissionViolation::TestChangeNotAllowed { path: path.clone() });
            }
        }

        AdmissionReport {
            allowed: violations.is_empty(),
            violations,
            file_count: paths.len(),
            changed_lines,
            paths,
        }
    }
}

/// One independently checkable claim in an evidence capsule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCheck {
    pub name: String,
    pub passed: bool,
    pub details: String,
}

/// Pipeline boundary at which an admission attempt reached a terminal decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionAuditStage {
    Capability,
    Permission,
    Consent,
    BaseRevision,
    Evidence,
    Admission,
    HumanReview,
}

impl AdmissionAuditStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "capability",
            Self::Permission => "permission",
            Self::Consent => "consent",
            Self::BaseRevision => "base_revision",
            Self::Evidence => "evidence",
            Self::Admission => "admission",
            Self::HumanReview => "human_review",
        }
    }
}

/// Terminal result of one admission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionAuditDecision {
    Approved,
    Blocked,
    Rejected,
    Skipped,
    Error,
}

impl AdmissionAuditDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Blocked => "blocked",
            Self::Rejected => "rejected",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }

    pub fn is_valid_filter(value: &str) -> bool {
        matches!(
            value,
            "approved" | "blocked" | "rejected" | "skipped" | "error"
        )
    }
}

/// Content-minimized, integrity-checkable record of an admission decision.
///
/// The record stores candidate metadata and hashes, never generated file contents. Receipts are
/// linked to the preceding local record. This detects accidental edits and broken ordering when
/// the complete local chain is verified; it is not a signature or remote attestation.
///
/// Schema 1 records verify against the original material. Schema 2 adds the
/// `run_id` binding; v2 material covers it. Verification is version-aware so
/// a v2 record can never be downgraded into a valid v1 receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionAuditRecord {
    pub schema_version: u8,
    pub receipt: String,
    pub previous_receipt: Option<String>,
    pub repository: String,
    /// Contribution Run this decision belongs to (schema 2). `None` for
    /// pre-run-pipeline records.
    #[serde(default)]
    pub run_id: Option<String>,
    pub contribution_fingerprint: String,
    pub stage: AdmissionAuditStage,
    pub decision: AdmissionAuditDecision,
    pub reason: String,
    pub base_sha: Option<String>,
    pub permit_id: Option<String>,
    pub issue: Option<i64>,
    pub file_count: usize,
    pub changed_lines: usize,
    pub paths: Vec<String>,
    pub violations: Vec<AdmissionViolation>,
    pub checks: Vec<EvidenceCheck>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct AdmissionAuditMaterial<'a> {
    schema_version: u8,
    previous_receipt: &'a Option<String>,
    repository: &'a str,
    contribution_fingerprint: &'a str,
    stage: AdmissionAuditStage,
    decision: AdmissionAuditDecision,
    reason: &'a str,
    base_sha: &'a Option<String>,
    permit_id: &'a Option<String>,
    issue: Option<i64>,
    file_count: usize,
    changed_lines: usize,
    paths: &'a [String],
    violations: &'a [AdmissionViolation],
    checks: &'a [EvidenceCheck],
    recorded_at: DateTime<Utc>,
}

/// Schema-2 receipt material: identical to v1 plus `run_id`.
#[derive(Serialize)]
struct AdmissionAuditMaterialV2<'a> {
    schema_version: u8,
    previous_receipt: &'a Option<String>,
    repository: &'a str,
    run_id: &'a Option<String>,
    contribution_fingerprint: &'a str,
    stage: AdmissionAuditStage,
    decision: AdmissionAuditDecision,
    reason: &'a str,
    base_sha: &'a Option<String>,
    permit_id: &'a Option<String>,
    issue: Option<i64>,
    file_count: usize,
    changed_lines: usize,
    paths: &'a [String],
    violations: &'a [AdmissionViolation],
    checks: &'a [EvidenceCheck],
    recorded_at: DateTime<Utc>,
}

impl AdmissionAuditRecord {
    /// Build an unsealed record for the local audit store.
    ///
    /// New records are written at schema 2 so they can carry the run binding.
    /// Pre-run call sites pass `None` for `run_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_attempt(
        repository: &Repository,
        contribution: &Contribution,
        stage: AdmissionAuditStage,
        decision: AdmissionAuditDecision,
        reason: impl Into<String>,
        permit: Option<&ContributionPermit>,
        report: Option<&AdmissionReport>,
        checks: Vec<EvidenceCheck>,
        recorded_at: DateTime<Utc>,
    ) -> Self {
        Self::from_attempt_with_run(
            repository,
            contribution,
            stage,
            decision,
            reason,
            permit,
            report,
            checks,
            recorded_at,
            None,
        )
    }

    /// Build an unsealed schema-2 record bound to a Contribution Run.
    #[allow(clippy::too_many_arguments)]
    pub fn from_attempt_with_run(
        repository: &Repository,
        contribution: &Contribution,
        stage: AdmissionAuditStage,
        decision: AdmissionAuditDecision,
        reason: impl Into<String>,
        permit: Option<&ContributionPermit>,
        report: Option<&AdmissionReport>,
        checks: Vec<EvidenceCheck>,
        recorded_at: DateTime<Utc>,
        run_id: Option<&str>,
    ) -> Self {
        let changes: Vec<&FileChange> = contribution
            .changes
            .iter()
            .chain(contribution.tests_added.iter())
            .collect();
        Self {
            schema_version: ADMISSION_AUDIT_SCHEMA_VERSION_V2,
            receipt: String::new(),
            previous_receipt: None,
            repository: repository.full_name.clone(),
            run_id: run_id.map(str::to_string),
            contribution_fingerprint: contribution_fingerprint(contribution),
            stage,
            decision,
            reason: reason.into(),
            base_sha: permit.map(|value| value.base_sha.clone()),
            permit_id: permit.map(|value| value.id.clone()),
            issue: permit.and_then(|value| value.issue),
            file_count: changes.len(),
            changed_lines: changes
                .iter()
                .map(|change| changed_line_count(change))
                .sum(),
            paths: changes.iter().map(|change| change.path.clone()).collect(),
            violations: report
                .map(|value| value.violations.clone())
                .unwrap_or_default(),
            checks,
            recorded_at,
        }
    }

    /// Bind this record to the preceding receipt and calculate its SHA-256 receipt.
    pub fn seal(
        mut self,
        previous_receipt: Option<String>,
    ) -> std::result::Result<Self, serde_json::Error> {
        self.previous_receipt = previous_receipt;
        self.receipt = self.calculate_receipt()?;
        Ok(self)
    }

    /// Recompute the record receipt from its stored fields.
    ///
    /// Versions 1 and 2 are both accepted; each verifies against its own
    /// material so a stored v2 record cannot be weakened into a v1 receipt
    /// by deleting `run_id`.
    pub fn verify_receipt(&self) -> bool {
        matches!(
            self.schema_version,
            ADMISSION_AUDIT_SCHEMA_VERSION | ADMISSION_AUDIT_SCHEMA_VERSION_V2
        ) && self.receipt.len() == 64
            && self.receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
            && self
                .calculate_receipt()
                .is_ok_and(|expected| expected == self.receipt)
    }

    fn calculate_receipt(&self) -> std::result::Result<String, serde_json::Error> {
        let encoded = match self.schema_version {
            ADMISSION_AUDIT_SCHEMA_VERSION_V2 => {
                let material = AdmissionAuditMaterialV2 {
                    schema_version: self.schema_version,
                    previous_receipt: &self.previous_receipt,
                    repository: &self.repository,
                    run_id: &self.run_id,
                    contribution_fingerprint: &self.contribution_fingerprint,
                    stage: self.stage,
                    decision: self.decision,
                    reason: &self.reason,
                    base_sha: &self.base_sha,
                    permit_id: &self.permit_id,
                    issue: self.issue,
                    file_count: self.file_count,
                    changed_lines: self.changed_lines,
                    paths: &self.paths,
                    violations: &self.violations,
                    checks: &self.checks,
                    recorded_at: self.recorded_at,
                };
                serde_json::to_vec(&material)?
            }
            _ => {
                let material = AdmissionAuditMaterial {
                    schema_version: self.schema_version,
                    previous_receipt: &self.previous_receipt,
                    repository: &self.repository,
                    contribution_fingerprint: &self.contribution_fingerprint,
                    stage: self.stage,
                    decision: self.decision,
                    reason: &self.reason,
                    base_sha: &self.base_sha,
                    permit_id: &self.permit_id,
                    issue: self.issue,
                    file_count: self.file_count,
                    changed_lines: self.changed_lines,
                    paths: &self.paths,
                    violations: &self.violations,
                    checks: &self.checks,
                    recorded_at: self.recorded_at,
                };
                serde_json::to_vec(&material)?
            }
        };
        Ok(hex::encode(Sha256::digest(encoded)))
    }
}

/// Result of checking the complete local admission audit chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionAuditVerification {
    pub valid: bool,
    pub records_checked: usize,
    pub first_invalid_receipt: Option<String>,
}

/// Audit artifact attached to every admitted draft PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCapsule {
    pub schema_version: u8,
    pub permit_id: String,
    pub repository: String,
    pub base_sha: String,
    pub contribution_fingerprint: String,
    pub generated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consent: ConsentSource,
    pub issue: Option<i64>,
    pub draft_only: bool,
    pub file_count: usize,
    pub changed_lines: usize,
    pub paths: Vec<String>,
    pub checks: Vec<EvidenceCheck>,
}

/// Stable reason why an evidence capsule cannot authorize a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceViolation {
    UnsupportedSchema {
        actual: u8,
    },
    InvalidPermitId,
    RepositoryMismatch,
    MissingBaseRevision,
    InvalidBaseRevision,
    ContributionMismatch,
    ScopeMismatch,
    InvalidValidityWindow,
    Expired,
    NotDraftOnly,
    InvalidConsentSource,
    MissingAdmissionCheck,
    DuplicateCheck {
        name: String,
    },
    FailedCheck {
        name: String,
    },
    InvalidPath {
        path: String,
    },
    DuplicatePath {
        path: String,
    },
    UnsupportedDeletion {
        path: String,
    },
    /// Evidence belongs to a different run than the live record (v3).
    RunBindingMismatch,
    /// The run is not in an approved/submitted state (v3).
    RunNotApproved,
    /// Task spec fingerprint does not match the run record (v3).
    TaskBindingMismatch,
    /// Human approval does not cover the exact current candidate (v3).
    ReviewBindingMismatch,
    /// Required checks were skipped or absent — evidence incomplete (v3).
    ValidationIncomplete,
    /// A required deterministic check failed (v3).
    ValidationFailed,
    /// The adversarial challenger never ran (v3).
    ChallengerNotRun,
    /// Unresolved critical/high challenger findings remain (v3).
    UnresolvedChallengeConcerns,
    /// A named required check from the permit did not pass (v3).
    MissingRequiredCheck {
        name: String,
    },
    /// Required reproduction evidence is absent or negative (v3).
    ReproductionMissing,
    /// The workspace the evidence was produced in was provably
    /// incomplete — skipped in-scope files, manifests, or a truncated
    /// listing (v3).
    IncompleteWorkspace,
}

impl std::fmt::Display for EvidenceViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema { actual } => {
                write!(formatter, "unsupported evidence schema version {actual}")
            }
            Self::InvalidPermitId => write!(formatter, "permit identifier is malformed"),
            Self::RepositoryMismatch => write!(formatter, "evidence belongs to another repository"),
            Self::MissingBaseRevision => write!(formatter, "evidence has no base revision"),
            Self::InvalidBaseRevision => write!(formatter, "evidence base is not a full Git SHA"),
            Self::ContributionMismatch => {
                write!(
                    formatter,
                    "evidence fingerprint does not match the contribution"
                )
            }
            Self::ScopeMismatch => {
                write!(formatter, "evidence scope does not match the contribution")
            }
            Self::InvalidValidityWindow => {
                write!(formatter, "evidence validity window is malformed")
            }
            Self::Expired => write!(formatter, "evidence capsule expired"),
            Self::NotDraftOnly => {
                write!(formatter, "evidence does not require a draft pull request")
            }
            Self::InvalidConsentSource => write!(formatter, "evidence consent source is invalid"),
            Self::MissingAdmissionCheck => write!(formatter, "admission policy check is missing"),
            Self::DuplicateCheck { name } => write!(formatter, "duplicate evidence check: {name}"),
            Self::FailedCheck { name } => write!(formatter, "evidence check failed: {name}"),
            Self::InvalidPath { path } => {
                write!(formatter, "evidence contains invalid path: {path}")
            }
            Self::DuplicatePath { path } => {
                write!(formatter, "evidence contains duplicate path: {path}")
            }
            Self::UnsupportedDeletion { path } => {
                write!(formatter, "evidence contains unsupported deletion: {path}")
            }
            Self::RunBindingMismatch => {
                write!(formatter, "evidence does not match the live run record")
            }
            Self::RunNotApproved => {
                write!(formatter, "run is not in an approved state")
            }
            Self::TaskBindingMismatch => {
                write!(
                    formatter,
                    "evidence task fingerprint does not match the run"
                )
            }
            Self::ReviewBindingMismatch => {
                write!(
                    formatter,
                    "human approval does not cover the exact current candidate"
                )
            }
            Self::ValidationIncomplete => {
                write!(formatter, "required validation evidence is incomplete")
            }
            Self::ValidationFailed => {
                write!(formatter, "a required validation check failed")
            }
            Self::ChallengerNotRun => {
                write!(formatter, "adversarial challenger never ran")
            }
            Self::UnresolvedChallengeConcerns => {
                write!(
                    formatter,
                    "unresolved critical/high challenger findings remain"
                )
            }
            Self::MissingRequiredCheck { name } => {
                write!(formatter, "required check did not pass: {name}")
            }
            Self::ReproductionMissing => {
                write!(formatter, "required reproduction evidence is missing")
            }
            Self::IncompleteWorkspace => {
                write!(formatter, "workspace materialization had unresolved gaps")
            }
        }
    }
}

impl EvidenceCapsule {
    pub fn build(
        contribution: &Contribution,
        permit: &ContributionPermit,
        report: &AdmissionReport,
        mut checks: Vec<EvidenceCheck>,
    ) -> Self {
        checks.insert(
            0,
            EvidenceCheck {
                name: "admission_policy".to_string(),
                passed: report.allowed,
                details: if report.allowed {
                    "permit, scope, and protected-path checks passed".to_string()
                } else {
                    report
                        .violations
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                },
            },
        );

        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            permit_id: permit.id.clone(),
            repository: permit.repository.clone(),
            base_sha: permit.base_sha.clone(),
            contribution_fingerprint: contribution_fingerprint(contribution),
            generated_at: Utc::now(),
            expires_at: permit.expires_at,
            consent: permit.source.clone(),
            issue: permit.issue,
            draft_only: permit.draft_only,
            file_count: report.file_count,
            changed_lines: report.changed_lines,
            paths: report.paths.clone(),
            checks,
        }
    }

    /// Recompute every locally checkable claim before a GitHub write begins.
    ///
    /// This prevents a stale or unrelated capsule from being paired with a
    /// different repository or contribution after human review.
    pub fn validate_for_submission(
        &self,
        contribution: &Contribution,
        repository: &Repository,
        now: DateTime<Utc>,
    ) -> std::result::Result<(), Vec<EvidenceViolation>> {
        let mut violations = Vec::new();
        if self.schema_version != EVIDENCE_SCHEMA_VERSION {
            violations.push(EvidenceViolation::UnsupportedSchema {
                actual: self.schema_version,
            });
        }
        if self.permit_id.len() != 24
            || !self.permit_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            violations.push(EvidenceViolation::InvalidPermitId);
        }
        if self.repository != repository.full_name {
            violations.push(EvidenceViolation::RepositoryMismatch);
        }
        if self.base_sha.trim().is_empty() {
            violations.push(EvidenceViolation::MissingBaseRevision);
        } else if !is_full_commit_sha(&self.base_sha) {
            violations.push(EvidenceViolation::InvalidBaseRevision);
        }
        if self.contribution_fingerprint != contribution_fingerprint(contribution) {
            violations.push(EvidenceViolation::ContributionMismatch);
        }
        if self.expires_at <= self.generated_at {
            violations.push(EvidenceViolation::InvalidValidityWindow);
        } else if now > self.expires_at {
            violations.push(EvidenceViolation::Expired);
        }
        if !self.draft_only {
            violations.push(EvidenceViolation::NotDraftOnly);
        }

        let source_is_valid = match &self.consent {
            ConsentSource::RepositoryManifest { path } => CONSENT_PATHS.contains(&path.as_str()),
            ConsentSource::MaintainerLabel { issue, label } => {
                self.issue == Some(*issue)
                    && MAINTAINER_APPROVAL_LABELS
                        .iter()
                        .any(|allowed| label.eq_ignore_ascii_case(allowed))
            }
        };
        if !source_is_valid {
            violations.push(EvidenceViolation::InvalidConsentSource);
        }

        let changes: Vec<&FileChange> = contribution
            .changes
            .iter()
            .chain(contribution.tests_added.iter())
            .collect();
        let paths: Vec<String> = changes.iter().map(|change| change.path.clone()).collect();
        let changed_lines = changes
            .iter()
            .map(|change| changed_line_count(change))
            .sum::<usize>();
        if self.file_count != paths.len()
            || self.changed_lines != changed_lines
            || self.paths != paths
        {
            violations.push(EvidenceViolation::ScopeMismatch);
        }

        let mut unique_paths = BTreeSet::new();
        for change in changes {
            if repository_path_error(&change.path).is_some() {
                violations.push(EvidenceViolation::InvalidPath {
                    path: change.path.clone(),
                });
            }
            if !unique_paths.insert(change.path.clone()) {
                violations.push(EvidenceViolation::DuplicatePath {
                    path: change.path.clone(),
                });
            }
            if change.is_deleted {
                violations.push(EvidenceViolation::UnsupportedDeletion {
                    path: change.path.clone(),
                });
            }
        }

        let mut check_names = BTreeSet::new();
        let mut has_admission_check = false;
        for check in &self.checks {
            if !check_names.insert(check.name.clone()) {
                violations.push(EvidenceViolation::DuplicateCheck {
                    name: check.name.clone(),
                });
            }
            if check.name == "admission_policy" {
                has_admission_check = true;
            }
            if !check.passed {
                violations.push(EvidenceViolation::FailedCheck {
                    name: check.name.clone(),
                });
            }
        }
        if !has_admission_check {
            violations.push(EvidenceViolation::MissingAdmissionCheck);
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    /// Compact Markdown for the pull request description.
    pub fn to_markdown(&self) -> String {
        let consent = match &self.consent {
            ConsentSource::RepositoryManifest { path } => format!("repository manifest `{path}`"),
            ConsentSource::MaintainerLabel { issue, label } => {
                format!("maintainer label `{label}` on #{issue}")
            }
        };
        let checks = self
            .checks
            .iter()
            .map(|check| {
                format!(
                    "- [{}] **{}** — {}",
                    if check.passed { "x" } else { " " },
                    check.name,
                    check.details
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "## ContribAI Evidence\n\n\
             - **Permit**: `{}`\n\
             - **Consent**: {}\n\
             - **Base revision**: `{}`\n\
             - **Change fingerprint**: `{}`\n\
             - **Evidence expires**: `{}`\n\
             - **Scope**: {} files / {} changed lines\n\
             - **Submission mode**: draft only\n\n\
             {}",
            self.permit_id,
            consent,
            self.base_sha,
            self.contribution_fingerprint,
            self.expires_at.to_rfc3339(),
            self.file_count,
            self.changed_lines,
            checks
        )
    }
}

pub fn contribution_fingerprint(contribution: &Contribution) -> String {
    let mut digest = Sha256::new();
    hash_field(&mut digest, b"contribai-contribution-v2");
    hash_field(
        &mut digest,
        format!("{:?}", contribution.contribution_type).as_bytes(),
    );
    hash_field(&mut digest, contribution.title.as_bytes());
    hash_field(&mut digest, contribution.description.as_bytes());
    hash_field(&mut digest, contribution.commit_message.as_bytes());
    hash_field(&mut digest, contribution.branch_name.as_bytes());

    let finding = &contribution.finding;
    hash_field(
        &mut digest,
        format!("{:?}", finding.finding_type).as_bytes(),
    );
    hash_field(&mut digest, format!("{:?}", finding.severity).as_bytes());
    hash_field(&mut digest, finding.title.as_bytes());
    hash_field(&mut digest, finding.description.as_bytes());
    hash_field(&mut digest, finding.file_path.as_bytes());
    hash_field(&mut digest, format!("{:?}", finding.line_start).as_bytes());
    hash_field(&mut digest, format!("{:?}", finding.line_end).as_bytes());
    hash_field(&mut digest, format!("{:?}", finding.suggestion).as_bytes());
    hash_field(&mut digest, &finding.confidence.to_bits().to_be_bytes());
    hash_field(
        &mut digest,
        &(finding.priority_signals.len() as u64).to_be_bytes(),
    );
    for signal in &finding.priority_signals {
        hash_field(&mut digest, signal.as_bytes());
    }

    hash_changes(&mut digest, b"changes", &contribution.changes);
    hash_changes(&mut digest, b"tests_added", &contribution.tests_added);
    hex::encode(digest.finalize())
}

fn hash_changes(digest: &mut Sha256, section: &[u8], changes: &[FileChange]) {
    hash_field(digest, section);
    hash_field(digest, &(changes.len() as u64).to_be_bytes());
    for change in changes {
        hash_field(digest, change.path.as_bytes());
        match &change.original_content {
            Some(content) => {
                hash_field(digest, b"original:some");
                hash_field(digest, content.as_bytes());
            }
            None => hash_field(digest, b"original:none"),
        }
        hash_field(digest, change.new_content.as_bytes());
        hash_field(digest, &[u8::from(change.is_new_file)]);
        hash_field(digest, &[u8::from(change.is_deleted)]);
    }
}

fn hash_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

pub fn changed_line_count(change: &FileChange) -> usize {
    let new_lines: Vec<&str> = change.new_content.lines().collect();
    let Some(original) = &change.original_content else {
        return new_lines.len();
    };
    let old_lines: Vec<&str> = original.lines().collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(old, new)| old == new)
        .count();
    let max_suffix = old_lines.len().min(new_lines.len()).saturating_sub(prefix);
    let suffix = old_lines
        .iter()
        .rev()
        .zip(new_lines.iter().rev())
        .take(max_suffix)
        .take_while(|(old, new)| old == new)
        .count();
    old_lines.len().saturating_sub(prefix + suffix)
        + new_lines.len().saturating_sub(prefix + suffix)
}

/// Whether a path is a dependency manifest or lockfile. Used by the
/// `allow_dependency_changes` policy and the review-surface estimator.
pub fn is_dependency_path(path: &str) -> bool {
    is_dependency_manifest(path) || is_lockfile(path)
}

/// Dependency manifests (not lockfiles) across supported ecosystems.
pub fn is_dependency_manifest(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "pnpm-workspace.yaml"
            | "pyproject.toml"
            | "setup.py"
            | "setup.cfg"
            | "requirements.txt"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "Gemfile"
            | "composer.json"
            | "pubspec.yaml"
            | "mix.exs"
            | "Package.swift"
            | "Package.resolved"
    ) || name.ends_with(".csproj")
        || name.ends_with(".fsproj")
}

/// Lockfiles — a lockfile-only diff is still a dependency change.
pub fn is_lockfile(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lockb"
            | "bun.lock"
            | "go.sum"
            | "Gemfile.lock"
            | "poetry.lock"
            | "composer.lock"
            | "pubspec.lock"
    )
}

/// Whether a path looks like a test file (used by `allow_test_changes`).
pub fn is_test_path(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered.contains("/tests/")
        || lowered.contains("/test/")
        || lowered.contains("__tests__/")
        || lowered.contains("/benches/")
        || lowered.starts_with("tests/")
        || lowered.starts_with("test/")
        || lowered.ends_with("_test.rs")
        || lowered.ends_with("_test.go")
        || lowered.ends_with("_test.py")
        || lowered.ends_with(".test.js")
        || lowered.ends_with(".test.ts")
        || lowered.ends_with(".spec.js")
        || lowered.ends_with(".spec.ts")
        || lowered.ends_with("test.rb")
        || lowered.contains("/spec/")
}

/// Evaluate the permit requirements that need run evidence: named checks
/// that must have passed and required reproduction evidence.
///
/// `check_passed` reports whether a named check exists in the validation
/// graph and passed — a missing or skipped check never counts as passed.
/// `reproduced` is the run's reproduction classification.
pub fn evaluate_run_requirements(
    permit: &ContributionPermit,
    check_passed: impl Fn(&str) -> bool,
    reproduced: bool,
) -> Vec<AdmissionViolation> {
    let mut violations = Vec::new();
    for name in &permit.required_checks {
        if !check_passed(name) {
            violations.push(AdmissionViolation::MissingRequiredCheck { name: name.clone() });
        }
    }
    if permit.required_reproduction && !reproduced {
        violations.push(AdmissionViolation::ReproductionRequired);
    }
    violations
}

pub fn is_protected_path(path: &str) -> bool {
    let normalized = path.to_ascii_lowercase();
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(
        file_name,
        "license"
            | "license.md"
            | "license.txt"
            | "contributing.md"
            | "code_of_conduct.md"
            | "security.md"
            | "codeowners"
            | "agents.md"
            | "ai_policy.md"
            | "contribai_allow"
            | "contribai_block"
    ) || normalized == ".github/contribai.yml"
        || normalized == ".github/contribai.yaml"
        || normalized.starts_with(".github/workflows/")
        || normalized == ".github/funding.yml"
}

/// Return why a generated repository path is not a canonical relative POSIX path.
pub fn repository_path_error(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return Some("path is empty");
    }
    if path.starts_with('/') {
        return Some("absolute paths are forbidden");
    }
    if path.len() >= 2 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic() {
        return Some("drive-letter paths are forbidden");
    }
    if path.contains('\\') {
        return Some("backslash separators are forbidden");
    }
    if path.contains(['%', '?', '#']) {
        return Some("URI metacharacters are forbidden");
    }
    if path.chars().any(char::is_control) {
        return Some("control characters are forbidden");
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Some("empty, dot, and parent components are forbidden");
    }
    None
}

/// GitHub currently exposes full SHA-1 object IDs and may expose SHA-256 IDs.
pub fn is_full_commit_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_safe_allow_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && !pattern.starts_with('/')
        && !pattern.contains('\\')
        && !pattern.contains(['%', '?', '#'])
        && !pattern.chars().any(char::is_control)
        && !pattern
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        && Pattern::new(pattern).is_ok()
}

/// Whether `pattern` (an allowed/denied glob) matches `path` under the
/// admission matching rules (literal separators and leading dots).
pub fn path_matches(pattern: &str, path: &str) -> bool {
    Pattern::new(pattern)
        .map(|compiled| {
            compiled.matches_with(
                path,
                MatchOptions {
                    case_sensitive: true,
                    require_literal_separator: true,
                    require_literal_leading_dot: true,
                },
            )
        })
        .unwrap_or(false)
}

fn short_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)[..24].to_string()
}

/// Return unique paths in deterministic order, useful to external policy adapters.
pub fn contribution_paths(contribution: &Contribution) -> Vec<String> {
    contribution
        .changes
        .iter()
        .chain(contribution.tests_added.iter())
        .map(|change| change.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{ContributionType, Finding, Severity};

    const TEST_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn repository(name: &str) -> Repository {
        Repository {
            owner: name.split('/').next().unwrap_or("owner").to_string(),
            name: name.split('/').nth(1).unwrap_or("repo").to_string(),
            full_name: name.to_string(),
            description: None,
            language: Some("Rust".to_string()),
            languages: Default::default(),
            stars: 0,
            forks: 0,
            open_issues: 0,
            topics: Vec::new(),
            default_branch: "main".to_string(),
            html_url: String::new(),
            clone_url: String::new(),
            has_contributing: false,
            has_license: true,
            last_push_at: None,
            created_at: None,
        }
    }

    fn contribution(path: &str, original: Option<&str>, new_content: &str) -> Contribution {
        Contribution {
            finding: Finding {
                id: "f1".to_string(),
                finding_type: ContributionType::CodeQuality,
                severity: Severity::Medium,
                title: "Fix parser".to_string(),
                description: "Parser bug".to_string(),
                file_path: path.to_string(),
                line_start: Some(1),
                line_end: Some(1),
                suggestion: None,
                confidence: 0.9,
                priority_signals: Vec::new(),
            },
            contribution_type: ContributionType::CodeQuality,
            title: "fix: parser".to_string(),
            description: "Fix parser".to_string(),
            changes: vec![FileChange {
                path: path.to_string(),
                original_content: original.map(str::to_string),
                new_content: new_content.to_string(),
                is_new_file: original.is_none(),
                is_deleted: false,
            }],
            commit_message: "fix: parser".to_string(),
            tests_added: Vec::new(),
            branch_name: String::new(),
            generated_at: Utc::now(),
        }
    }

    fn consent(paths: &[&str]) -> RepositoryConsent {
        RepositoryConsent {
            source: ConsentSource::RepositoryManifest {
                path: CONSENT_PATHS[0].to_string(),
            },
            schema_version: 1,
            max_files: 5,
            max_changed_lines: 250,
            allowed_paths: paths.iter().map(|path| path.to_string()).collect(),
            denied_paths: Vec::new(),
            required_checks: Vec::new(),
            allow_dependency_changes: true,
            allow_new_files: true,
            allow_test_changes: true,
            required_reproduction: false,
            execution_mode: ExecutionMode::Local,
            max_runtime_seconds: 900,
            allowed_issue_labels: Vec::new(),
            draft_only: true,
        }
    }

    #[test]
    fn manifest_requires_explicit_true() {
        assert!(RepositoryConsent::parse(CONSENT_PATHS[0], "max_files: 2").is_none());
        assert!(RepositoryConsent::parse(CONSENT_PATHS[0], "enabled: false").is_none());
        assert!(RepositoryConsent::parse(CONSENT_PATHS[0], "enabled: true").is_some());
    }

    #[test]
    fn manifest_parses_budgets_and_paths() {
        let parsed = RepositoryConsent::parse(
            CONSENT_PATHS[0],
            "enabled: true\nmax_files: 2\nmax_changed_lines: 80\nallowed_paths: src/**, tests/**",
        )
        .expect("valid consent");
        assert_eq!(parsed.max_files, 2);
        assert_eq!(parsed.max_changed_lines, 80);
        assert_eq!(parsed.allowed_paths, vec!["src/**", "tests/**"]);
        assert!(parsed.draft_only);
    }

    #[test]
    fn canonical_yaml_manifest_accepts_path_lists() {
        let parsed = RepositoryConsent::parse(
            ".github/contribai.yml",
            "schema_version: 1\nenabled: true\nallowed_paths:\n  - src/**\n  - tests/**\n",
        )
        .expect("valid consent");
        assert_eq!(parsed.allowed_paths, vec!["src/**", "tests/**"]);
    }

    #[test]
    fn manifest_rejects_zero_budgets_and_malformed_yaml() {
        assert!(
            RepositoryConsent::parse(".github/contribai.yml", "enabled: true\nmax_files: 0")
                .is_none()
        );
        assert!(RepositoryConsent::parse(".github/contribai.yml", "enabled: [true").is_none());
    }

    #[test]
    fn manifest_rejects_unknown_schema_fields_and_unsafe_patterns() {
        // Schema 2 is supported as of v7; versions beyond it fail closed.
        assert!(RepositoryConsent::parse(
            ".github/contribai.yml",
            "schema_version: 99\nenabled: true",
        )
        .is_none());
        assert!(RepositoryConsent::parse(
            ".github/contribai.yml",
            "schema_version: 1\nenabled: true\nmax_file: 1",
        )
        .is_none());
        for pattern in ["../**", "src\\**", "src/[", "src//**"] {
            let content = format!("schema_version: 1\nenabled: true\nallowed_paths: ['{pattern}']");
            assert!(
                RepositoryConsent::parse(".github/contribai.yml", &content).is_none(),
                "unsafe pattern {pattern:?} must deny consent"
            );
        }
    }

    #[test]
    fn issue_consent_requires_maintainer_label() {
        let mut issue = Issue {
            number: 42,
            title: "Bug".to_string(),
            body: None,
            labels: vec!["bug".to_string()],
            state: "open".to_string(),
            created_at: None,
            html_url: String::new(),
        };
        assert!(RepositoryConsent::from_issue(&issue).is_none());
        issue.labels.push("agent-ready".to_string());
        assert!(matches!(
            RepositoryConsent::from_issue(&issue).map(|value| value.source),
            Some(ConsentSource::MaintainerLabel { issue: 42, .. })
        ));
        issue.state = "closed".to_string();
        assert!(RepositoryConsent::from_issue(&issue).is_none());
    }

    #[test]
    fn admission_allows_scoped_change() {
        let repo = repository("owner/repo");
        let change = contribution("src/parser.rs", Some("old\n"), "new\n");
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&["src/**"]), None);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        assert!(report.allowed, "{:?}", report.violations);
        assert_eq!(report.changed_lines, 2);
    }

    #[test]
    fn admission_blocks_governance_and_out_of_scope_paths() {
        let repo = repository("owner/repo");
        for path in ["AGENTS.md", ".github/contribai.yml"] {
            let change = contribution(path, Some("old"), "new");
            let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&["**"]), None);
            let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
            assert!(!report.allowed, "{path} must remain protected");
            assert!(report
                .violations
                .iter()
                .any(|violation| matches!(violation, AdmissionViolation::ProtectedPath { .. })));
        }
    }

    #[test]
    fn admission_blocks_expired_and_cross_repo_permits() {
        let repo = repository("owner/repo");
        let other = repository("other/repo");
        let change = contribution("src/lib.rs", Some("old"), "new");
        let mut permit = ContributionPermit::issue(&other, TEST_SHA, consent(&[]), None);
        permit.expires_at = Utc::now() - Duration::seconds(1);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        assert!(report
            .violations
            .contains(&AdmissionViolation::RepositoryMismatch));
        assert!(report
            .violations
            .contains(&AdmissionViolation::PermitExpired));
    }

    #[test]
    fn admission_rejects_noncanonical_duplicate_and_deleted_paths() {
        let repo = repository("owner/repo");
        for path in [
            "../SECURITY.md",
            "src/../.github/workflows/release.yml",
            "/src/lib.rs",
            "C:/src/lib.rs",
            "d:lib.rs",
            "src\\lib.rs",
            "src//lib.rs",
            "src/%2e%2e/SECURITY.md",
        ] {
            let change = contribution(path, Some("old"), "new");
            let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&["**"]), None);
            let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
            assert!(
                report
                    .violations
                    .iter()
                    .any(|violation| matches!(violation, AdmissionViolation::InvalidPath { .. })),
                "unsafe path {path:?} must be rejected: {:?}",
                report.violations
            );
        }

        let mut duplicate = contribution("src/lib.rs", Some("old"), "new");
        duplicate.tests_added = duplicate.changes.clone();
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&["src/**"]), None);
        let report = AdmissionController::evaluate(&repo, &duplicate, &permit, Utc::now());
        assert!(report
            .violations
            .iter()
            .any(|violation| matches!(violation, AdmissionViolation::DuplicatePath { .. })));

        let mut deletion = contribution("src/lib.rs", Some("old"), "");
        deletion.changes[0].is_deleted = true;
        let report = AdmissionController::evaluate(&repo, &deletion, &permit, Utc::now());
        assert!(report
            .violations
            .iter()
            .any(|violation| matches!(violation, AdmissionViolation::UnsupportedDeletion { .. })));
    }

    #[test]
    fn allowlist_globs_do_not_cross_unmatched_directory_boundaries() {
        let repo = repository("owner/repo");
        let nested = contribution("src/parser/mod.rs", Some("old"), "new");
        let shallow = ContributionPermit::issue(&repo, TEST_SHA, consent(&["src/*"]), None);
        let recursive = ContributionPermit::issue(&repo, TEST_SHA, consent(&["src/**"]), None);
        assert!(!AdmissionController::evaluate(&repo, &nested, &shallow, Utc::now()).allowed);
        assert!(AdmissionController::evaluate(&repo, &nested, &recursive, Utc::now()).allowed);
    }

    #[test]
    fn admission_requires_a_full_commit_object_id() {
        let repo = repository("owner/repo");
        let change = contribution("src/lib.rs", Some("old"), "new");
        let permit = ContributionPermit::issue(&repo, "abc123", consent(&[]), None);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        assert!(report
            .violations
            .contains(&AdmissionViolation::InvalidBaseRevision));
    }

    #[test]
    fn permit_identifier_binds_path_scope() {
        let repo = repository("owner/repo");
        let source = ContributionPermit::issue(&repo, TEST_SHA, consent(&["src/**"]), None);
        let docs = ContributionPermit::issue(&repo, TEST_SHA, consent(&["docs/**"]), None);
        assert_ne!(source.id, docs.id);
    }

    #[test]
    fn changed_line_count_trims_common_prefix_and_suffix() {
        let change = FileChange {
            path: "src/lib.rs".to_string(),
            original_content: Some("same\nold\ntail\n".to_string()),
            new_content: "same\nnew\ntail\n".to_string(),
            is_new_file: false,
            is_deleted: false,
        };
        assert_eq!(changed_line_count(&change), 2);
    }

    #[test]
    fn evidence_is_deterministic_for_same_change() {
        let first = contribution("src/lib.rs", Some("old"), "new");
        let second = first.clone();
        assert_eq!(
            contribution_fingerprint(&first),
            contribution_fingerprint(&second)
        );
    }

    #[test]
    fn evidence_markdown_discloses_consent_and_attestation() {
        let repo = repository("owner/repo");
        let change = contribution("src/lib.rs", Some("old"), "new");
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&[]), None);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        let capsule = EvidenceCapsule::build(&change, &permit, &report, Vec::new());
        let markdown = capsule.to_markdown();
        assert!(markdown.contains(&permit.id));
        assert!(markdown.contains(TEST_SHA));
        assert!(markdown.contains("draft only"));
    }

    #[test]
    fn evidence_validation_binds_the_exact_candidate_and_repository() {
        let repo = repository("owner/repo");
        let change = contribution("src/lib.rs", Some("old"), "new");
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&[]), None);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        let capsule = EvidenceCapsule::build(&change, &permit, &report, Vec::new());
        assert!(capsule
            .validate_for_submission(&change, &repo, Utc::now())
            .is_ok());

        let mut tampered = change.clone();
        tampered.commit_message = "chore: unrelated rewrite".to_string();
        let violations = capsule
            .validate_for_submission(&tampered, &repo, Utc::now())
            .expect_err("mutated candidate must not reuse reviewed evidence");
        assert!(violations.contains(&EvidenceViolation::ContributionMismatch));

        let other = repository("other/repo");
        let violations = capsule
            .validate_for_submission(&change, &other, Utc::now())
            .expect_err("cross-repository evidence must be rejected");
        assert!(violations.contains(&EvidenceViolation::RepositoryMismatch));

        let issue_permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&[]), Some(42));
        let issue_report = AdmissionController::evaluate(&repo, &change, &issue_permit, Utc::now());
        let issue_capsule =
            EvidenceCapsule::build(&change, &issue_permit, &issue_report, Vec::new());
        assert!(issue_capsule
            .validate_for_submission(&change, &repo, Utc::now())
            .is_ok());
    }

    #[test]
    fn evidence_validation_rejects_expired_or_failed_capsules() {
        let repo = repository("owner/repo");
        let change = contribution("src/lib.rs", Some("old"), "new");
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&[]), None);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        let mut capsule = EvidenceCapsule::build(&change, &permit, &report, Vec::new());
        capsule.generated_at = Utc::now() - Duration::hours(2);
        capsule.expires_at = Utc::now() - Duration::hours(1);
        let violations = capsule
            .validate_for_submission(&change, &repo, Utc::now())
            .expect_err("expired evidence must be rejected");
        assert!(violations.contains(&EvidenceViolation::Expired));

        capsule.expires_at = Utc::now() + Duration::hours(1);
        capsule.checks[0].passed = false;
        let violations = capsule
            .validate_for_submission(&change, &repo, Utc::now())
            .expect_err("failed evidence must be rejected");
        assert!(violations.contains(&EvidenceViolation::FailedCheck {
            name: "admission_policy".to_string(),
        }));
    }

    #[test]
    fn admission_audit_receipt_binds_decision_and_predecessor() {
        let repo = repository("owner/repo");
        let change = contribution("src/lib.rs", Some("old"), "new");
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent(&[]), None);
        let report = AdmissionController::evaluate(&repo, &change, &permit, Utc::now());
        let recorded_at = Utc::now();
        let record = AdmissionAuditRecord::from_attempt(
            &repo,
            &change,
            AdmissionAuditStage::Admission,
            AdmissionAuditDecision::Approved,
            "admission and review passed",
            Some(&permit),
            Some(&report),
            Vec::new(),
            recorded_at,
        )
        .seal(Some("a".repeat(64)))
        .expect("audit record should serialize");
        assert!(record.verify_receipt());

        let mut changed_decision = record.clone();
        changed_decision.decision = AdmissionAuditDecision::Blocked;
        assert!(!changed_decision.verify_receipt());

        let mut changed_predecessor = record;
        changed_predecessor.previous_receipt = None;
        assert!(!changed_predecessor.verify_receipt());
    }

    #[test]
    fn admission_audit_stores_metadata_without_file_contents() {
        let repo = repository("owner/repo");
        let change = contribution("src/lib.rs", Some("private old"), "private new");
        let record = AdmissionAuditRecord::from_attempt(
            &repo,
            &change,
            AdmissionAuditStage::Consent,
            AdmissionAuditDecision::Blocked,
            "maintainer consent was not found",
            None,
            None,
            Vec::new(),
            Utc::now(),
        )
        .seal(None)
        .expect("audit record should serialize");
        let json = serde_json::to_string(&record).expect("audit record should serialize");
        assert!(json.contains("src/lib.rs"));
        assert!(!json.contains("private old"));
        assert!(!json.contains("private new"));
    }

    // ── Consent schema 2 ─────────────────────────────────────────────────

    const V2_MANIFEST: &str = "\
schema_version: 2
enabled: true
max_files: 8
max_changed_lines: 400
allowed_paths:
  - src/**
  - tests/**
denied_paths:
  - src/secrets/**
required_checks: [cargo_test, cargo_clippy]
allow_dependency_changes: false
allow_new_files: false
allow_test_changes: true
required_reproduction: true
execution_mode: local
max_runtime_seconds: 600
allowed_issue_labels: [contribai-v2-ready]
";

    fn v2_consent() -> RepositoryConsent {
        RepositoryConsent::parse(CONSENT_PATHS[0], V2_MANIFEST)
            .expect("schema-2 manifest should parse")
    }

    #[test]
    fn schema2_manifest_parses_all_controls() {
        let consent = v2_consent();
        assert_eq!(consent.schema_version, 2);
        assert_eq!(consent.max_files, 8);
        assert_eq!(consent.denied_paths, vec!["src/secrets/**"]);
        assert_eq!(consent.required_checks, vec!["cargo_test", "cargo_clippy"]);
        assert!(!consent.allow_dependency_changes);
        assert!(!consent.allow_new_files);
        assert!(consent.allow_test_changes);
        assert!(consent.required_reproduction);
        assert_eq!(consent.execution_mode, ExecutionMode::Local);
        assert_eq!(consent.max_runtime_seconds, 600);
        assert_eq!(consent.allowed_issue_labels, vec!["contribai-v2-ready"]);
    }

    #[test]
    fn schema1_manifest_keeps_v6_semantics() {
        let consent = RepositoryConsent::parse(
            CONSENT_PATHS[0],
            "enabled: true\nmax_files: 2\nallowed_paths: src/**",
        )
        .unwrap();
        assert_eq!(consent.schema_version, 1);
        assert!(consent.allow_dependency_changes);
        assert!(consent.denied_paths.is_empty());
        assert_eq!(consent.execution_mode, ExecutionMode::Local);
    }

    #[test]
    fn schema1_manifest_with_v2_fields_fails_closed() {
        // denied_paths in a schema-1 manifest is an unknown field — the whole
        // manifest must be rejected, never silently narrowed.
        assert!(RepositoryConsent::parse(
            CONSENT_PATHS[0],
            "enabled: true\ndenied_paths: [src/**]",
        )
        .is_none());
    }

    #[test]
    fn schema2_rejects_unknown_fields_and_bad_versions() {
        assert!(RepositoryConsent::parse(
            CONSENT_PATHS[0],
            "schema_version: 2\nenabled: true\nallow_magic: true",
        )
        .is_none());
        assert!(
            RepositoryConsent::parse(CONSENT_PATHS[0], "schema_version: 3\nenabled: true",)
                .is_none()
        );
        assert!(RepositoryConsent::parse(
            CONSENT_PATHS[0],
            "schema_version: 2\nenabled: true\nmax_runtime_seconds: 0",
        )
        .is_none());
    }

    #[test]
    fn schema2_evaluate_denies_denied_dependency_newfile_paths() {
        let repo = repository("owner/repo");
        let consent = v2_consent();
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent, None);

        // denied_paths glob.
        let mut c = contribution("src/secrets/keys.rs", None, "x");
        let report = AdmissionController::evaluate(&repo, &c, &permit, Utc::now());
        assert!(report.violations.contains(&AdmissionViolation::DeniedPath {
            path: "src/secrets/keys.rs".into()
        }));

        // dependency manifest without allow_dependency_changes.
        c = contribution("src/../Cargo.toml", None, "x");
        c.changes[0].path = "Cargo.toml".to_string();
        let report = AdmissionController::evaluate(&repo, &c, &permit, Utc::now());
        assert!(report
            .violations
            .contains(&AdmissionViolation::DependencyChangeNotAllowed {
                path: "Cargo.toml".into()
            }));

        // new file without allow_new_files.
        c = contribution("src/new.rs", None, "x");
        c.changes[0].is_new_file = true;
        let report = AdmissionController::evaluate(&repo, &c, &permit, Utc::now());
        assert!(report
            .violations
            .contains(&AdmissionViolation::NewFilesNotAllowed {
                path: "src/new.rs".into()
            }));
    }

    #[test]
    fn schema1_permit_still_allows_in_scope_manifest_edit() {
        let repo = repository("owner/repo");
        let consent = consent(&["**"]);
        let permit = ContributionPermit::issue(&repo, TEST_SHA, consent, None);
        let c = contribution("Cargo.toml", Some("[package]\n"), "[package]\n# comment\n");
        let report = AdmissionController::evaluate(&repo, &c, &permit, Utc::now());
        assert!(report
            .violations
            .iter()
            .all(|v| !matches!(v, AdmissionViolation::DependencyChangeNotAllowed { .. })));
    }

    #[test]
    fn run_requirements_evaluate_checks_and_reproduction() {
        let repo = repository("owner/repo");
        let permit = ContributionPermit::issue(&repo, TEST_SHA, v2_consent(), None);

        // Both required checks pass + reproduction evidence → clean.
        let violations = evaluate_run_requirements(
            &permit,
            |name| matches!(name, "cargo_test" | "cargo_clippy"),
            true,
        );
        assert!(violations.is_empty());

        // Missing check fails closed — a check that never ran does not pass.
        let violations = evaluate_run_requirements(&permit, |name| name == "cargo_test", true);
        assert_eq!(
            violations,
            vec![AdmissionViolation::MissingRequiredCheck {
                name: "cargo_clippy".into()
            }]
        );

        // No reproduction evidence when required → violation.
        let violations = evaluate_run_requirements(&permit, |_| true, false);
        assert_eq!(violations, vec![AdmissionViolation::ReproductionRequired]);
    }

    #[test]
    fn schema2_extends_issue_labels_via_manifest() {
        let mut issue = issue_with_labels(7, &["contribai-v2-ready"]);
        assert!(RepositoryConsent::from_issue(&issue).is_none());
        let consent =
            RepositoryConsent::from_issue_with_labels(&issue, &v2_consent().allowed_issue_labels)
                .unwrap();
        assert!(matches!(
            consent.source,
            ConsentSource::MaintainerLabel { issue: 7, .. }
        ));
        // Built-in labels still work without manifest extension.
        issue.labels = vec!["contribai-approved".to_string()];
        assert!(RepositoryConsent::from_issue(&issue).is_some());
    }

    fn issue_with_labels(number: i64, labels: &[&str]) -> Issue {
        Issue {
            number,
            title: "issue".into(),
            body: Some("body".into()),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            state: "open".into(),
            created_at: Some(Utc::now()),
            html_url: String::new(),
        }
    }

    #[test]
    fn audit_v2_receipts_cover_run_id_and_verify_versioned() {
        let repo = repository("owner/repo");
        let c = contribution("src/lib.rs", None, "new");
        let record_v2 = AdmissionAuditRecord::from_attempt_with_run(
            &repo,
            &c,
            AdmissionAuditStage::Admission,
            AdmissionAuditDecision::Approved,
            "ok",
            None,
            None,
            Vec::new(),
            Utc::now(),
            Some("run_abc123"),
        )
        .seal(None)
        .unwrap();
        assert!(record_v2.verify_receipt());
        assert_eq!(record_v2.schema_version, 2);

        // Tampering with run_id invalidates the receipt.
        let mut tampered = record_v2.clone();
        tampered.run_id = Some("run_evil".into());
        assert!(!tampered.verify_receipt());

        // A v1 record still verifies against v1 material.
        let mut record_v1 = AdmissionAuditRecord::from_attempt(
            &repo,
            &c,
            AdmissionAuditStage::Consent,
            AdmissionAuditDecision::Blocked,
            "no consent",
            None,
            None,
            Vec::new(),
            Utc::now(),
        );
        record_v1.schema_version = 1;
        let record_v1 = record_v1.seal(None).unwrap();
        assert!(record_v1.verify_receipt());
        // But downgrading a v2 receipt by changing schema_version fails.
        let mut downgraded = record_v2.clone();
        downgraded.schema_version = 1;
        assert!(!downgraded.verify_receipt());
    }
}
