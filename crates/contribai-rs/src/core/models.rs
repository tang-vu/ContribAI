//! Core data models for ContribAI.
//!
//! Direct port from Python `core/models.py` — all Pydantic models become
//! serde-derivable Rust structs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ── Enums ──────────────────────────────────────────────────────────────────────

/// Types of contributions the agent can make.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionType {
    SecurityFix,
    FeatureAdd,
    DocsImprove,
    UiUxFix,
    PerformanceOpt,
    Refactor,
    CodeQuality,
}

impl fmt::Display for ContributionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecurityFix => write!(f, "security_fix"),
            Self::FeatureAdd => write!(f, "feature_add"),
            Self::DocsImprove => write!(f, "docs_improve"),
            Self::UiUxFix => write!(f, "ui_ux_fix"),
            Self::PerformanceOpt => write!(f, "performance_opt"),
            Self::Refactor => write!(f, "refactor"),
            Self::CodeQuality => write!(f, "code_quality"),
        }
    }
}

impl ContributionType {
    /// Map an analyzer name to its contribution type.
    pub fn from_analyzer(name: &str) -> Self {
        match name {
            "security" | "django_security" | "flask_security" => Self::SecurityFix,
            "performance" => Self::PerformanceOpt,
            "docs" | "documentation" => Self::DocsImprove,
            "ui_ux" => Self::UiUxFix,
            "refactor" => Self::Refactor,
            _ => Self::CodeQuality,
        }
    }
}

/// Severity level for findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Weight for priority scoring.
    pub fn weight(self) -> f64 {
        match self {
            Self::Low => 1.0,
            Self::Medium => 2.0,
            Self::High => 3.0,
            Self::Critical => 4.0,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Status of a submitted pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrStatus {
    Pending,
    Open,
    Merged,
    Closed,
    ReviewRequested,
}

// ── GitHub Models ──────────────────────────────────────────────────────────────

/// GitHub repository metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub languages: HashMap<String, i64>,
    #[serde(default)]
    pub stars: i64,
    #[serde(default)]
    pub forks: i64,
    #[serde(default)]
    pub open_issues: i64,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub clone_url: String,
    #[serde(default)]
    pub has_contributing: bool,
    #[serde(default)]
    pub has_license: bool,
    pub last_push_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

fn default_branch() -> String {
    "main".to_string()
}

impl Repository {
    pub fn url(&self) -> String {
        format!("https://github.com/{}", self.full_name)
    }
}

/// GitHub issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default = "default_state")]
    pub state: String,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub html_url: String,
}

fn default_state() -> String {
    "open".to_string()
}

/// A file or directory in the repo tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: String, // "blob" or "tree"
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub sha: String,
}

// ── Analysis Models ────────────────────────────────────────────────────────────

/// A code symbol extracted by tree-sitter AST analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
}

/// Kind of code symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Class,
    Method,
    Interface,
    Struct,
    Enum,
    Constant,
    Variable,
    Import,
}

/// An individual issue found during analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type")]
    pub finding_type: ContributionType,
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub file_path: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub suggestion: Option<String>,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// Signals that contributed to the priority score.
    #[serde(default)]
    pub priority_signals: Vec<String>,
}

fn default_confidence() -> f64 {
    0.8
}

impl Finding {
    /// Simple priority score (severity × confidence).
    pub fn priority_score(&self) -> f64 {
        self.severity.weight() * self.confidence
    }
}

/// Aggregated results from code analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub repo: Repository,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub analyzed_files: usize,
    #[serde(default)]
    pub skipped_files: usize,
    #[serde(default)]
    pub analysis_duration_sec: f64,
}

impl AnalysisResult {
    /// Findings sorted by priority, highest first.
    pub fn top_findings(&self) -> Vec<&Finding> {
        let mut sorted: Vec<&Finding> = self.findings.iter().collect();
        sorted.sort_by(|a, b| {
            b.priority_score()
                .partial_cmp(&a.priority_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted
    }

    /// Filter findings by type.
    pub fn filter_by_type(&self, contrib_type: &ContributionType) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| &f.finding_type == contrib_type)
            .collect()
    }

    /// Filter findings by minimum severity.
    pub fn filter_by_severity(&self, min: Severity) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity >= min)
            .collect()
    }
}

// ── Contribution Models ────────────────────────────────────────────────────────

/// A single file change in a contribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub original_content: Option<String>,
    pub new_content: String,
    #[serde(default)]
    pub is_new_file: bool,
    #[serde(default)]
    pub is_deleted: bool,
}

/// A generated contribution ready to be submitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub finding: Finding,
    pub contribution_type: ContributionType,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub changes: Vec<FileChange>,
    #[serde(default)]
    pub commit_message: String,
    #[serde(default)]
    pub tests_added: Vec<FileChange>,
    #[serde(default)]
    pub branch_name: String,
    #[serde(default = "Utc::now")]
    pub generated_at: DateTime<Utc>,
}

impl Contribution {
    pub fn total_files_changed(&self) -> usize {
        self.changes.len() + self.tests_added.len()
    }
}

/// Result of creating a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrResult {
    pub repo: Repository,
    pub contribution: Contribution,
    pub pr_number: i64,
    pub pr_url: String,
    #[serde(default)]
    pub status: PrStatus,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub branch_name: String,
    #[serde(default)]
    pub fork_full_name: String,
}

impl Default for PrStatus {
    fn default() -> Self {
        Self::Open
    }
}

// ── Discovery Models ──────────────────────────────────────────────────────────

/// Criteria for discovering repositories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryCriteria {
    #[serde(default = "default_languages")]
    pub languages: Vec<String>,
    #[serde(default = "default_stars_min")]
    pub stars_min: i64,
    #[serde(default = "default_stars_max")]
    pub stars_max: i64,
    #[serde(default = "default_min_activity_days")]
    pub min_last_activity_days: i64,
    #[serde(default)]
    pub require_contributing_guide: bool,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default)]
    pub exclude_repos: Vec<String>,
}

fn default_languages() -> Vec<String> {
    vec!["python".to_string()]
}
fn default_stars_min() -> i64 {
    50
}
fn default_stars_max() -> i64 {
    10000
}
fn default_min_activity_days() -> i64 {
    30
}
fn default_max_results() -> usize {
    20
}

impl Default for DiscoveryCriteria {
    fn default() -> Self {
        Self {
            languages: default_languages(),
            stars_min: default_stars_min(),
            stars_max: default_stars_max(),
            min_last_activity_days: default_min_activity_days(),
            require_contributing_guide: false,
            topics: Vec::new(),
            max_results: default_max_results(),
            exclude_repos: Vec::new(),
        }
    }
}

/// Full context about a repository for LLM prompting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoContext {
    pub repo: Repository,
    #[serde(default)]
    pub file_tree: Vec<FileNode>,
    pub readme_content: Option<String>,
    pub contributing_guide: Option<String>,
    #[serde(default)]
    pub relevant_files: HashMap<String, String>,
    #[serde(default)]
    pub open_issues: Vec<Issue>,
    pub coding_style: Option<String>,
    /// AST symbol map: file_path → symbols (from tree-sitter).
    #[serde(default)]
    pub symbol_map: HashMap<String, Vec<Symbol>>,
    /// PageRank scores: file_path → importance score.
    #[serde(default)]
    pub file_ranks: HashMap<String, f64>,
}

// ── Triage Models (NEW — inspired by RedAmon) ─────────────────────────────────

/// Structured remediation spec produced by triage, consumed by generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationSpec {
    pub finding: Finding,
    /// Weighted priority score (lower = higher priority, 0 = top).
    pub priority_score: f64,
    /// Category: sqli, xss, rce, quality, perf, docs, etc.
    pub category: String,
    /// Estimated fix complexity.
    pub fix_complexity: FixComplexity,
    /// Affected symbols from AST analysis.
    #[serde(default)]
    pub affected_symbols: Vec<String>,
    /// Concrete evidence this is a real issue.
    #[serde(default)]
    pub evidence: String,
    /// Suggested fix approach.
    #[serde(default)]
    pub solution_hint: String,
    /// Signals that contributed to the score.
    #[serde(default)]
    pub scoring_signals: Vec<ScoringSignal>,
}

/// Fix complexity estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FixComplexity {
    Low,
    Medium,
    High,
    Critical,
}

/// A scoring signal with its weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringSignal {
    pub name: String,
    pub weight: i32,
    pub reason: String,
}

// ── Patrol Models ──────────────────────────────────────────────────────────

/// What action to take for a review comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackAction {
    CodeChange,
    Question,
    StyleFix,
    Approve,
    Reject,
    AlreadyHandled,
}

/// A classified review comment requiring action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackItem {
    pub comment_id: i64,
    pub author: String,
    pub body: String,
    pub action: FeedbackAction,
    pub file_path: Option<String>,
    pub line: Option<i64>,
    pub diff_hunk: Option<String>,
    #[serde(default)]
    pub is_inline: bool,
    pub bot_context: Option<String>,
}

/// Result of a patrol run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatrolResult {
    pub prs_checked: usize,
    pub fixes_pushed: usize,
    pub replies_sent: usize,
    pub cla_signed: usize,
    pub prs_skipped: usize,
    pub issues_found: usize,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn test_severity_weight() {
        assert_eq!(Severity::Low.weight(), 1.0);
        assert_eq!(Severity::Critical.weight(), 4.0);
    }

    #[test]
    fn test_finding_priority_score() {
        let finding = Finding {
            id: String::new(),
            finding_type: ContributionType::SecurityFix,
            severity: Severity::Critical,
            title: "SQL Injection".to_string(),
            description: "test".to_string(),
            file_path: "src/db.py".to_string(),
            line_start: Some(42),
            line_end: Some(45),
            suggestion: None,
            confidence: 0.9,
            priority_signals: vec![],
        };
        assert!((finding.priority_score() - 3.6).abs() < 0.01);
    }

    #[test]
    fn test_analysis_result_filter_by_severity() {
        let repo = Repository {
            owner: "test".into(),
            name: "repo".into(),
            full_name: "test/repo".into(),
            description: None,
            language: Some("python".into()),
            languages: HashMap::new(),
            stars: 100,
            forks: 10,
            open_issues: 5,
            topics: vec![],
            default_branch: "main".into(),
            html_url: String::new(),
            clone_url: String::new(),
            has_contributing: false,
            has_license: true,
            last_push_at: None,
            created_at: None,
        };

        let result = AnalysisResult {
            repo,
            findings: vec![
                Finding {
                    id: "1".into(),
                    finding_type: ContributionType::SecurityFix,
                    severity: Severity::Critical,
                    title: "Critical issue".into(),
                    description: "desc".into(),
                    file_path: "a.py".into(),
                    line_start: None,
                    line_end: None,
                    suggestion: None,
                    confidence: 0.9,
                    priority_signals: vec![],
                },
                Finding {
                    id: "2".into(),
                    finding_type: ContributionType::CodeQuality,
                    severity: Severity::Low,
                    title: "Minor issue".into(),
                    description: "desc".into(),
                    file_path: "b.py".into(),
                    line_start: None,
                    line_end: None,
                    suggestion: None,
                    confidence: 0.5,
                    priority_signals: vec![],
                },
            ],
            analyzed_files: 10,
            skipped_files: 5,
            analysis_duration_sec: 1.5,
        };

        assert_eq!(result.filter_by_severity(Severity::High).len(), 1);
        assert_eq!(result.filter_by_severity(Severity::Low).len(), 2);
    }

    #[test]
    fn test_discovery_criteria_defaults() {
        let dc = DiscoveryCriteria::default();
        assert_eq!(dc.stars_min, 50);
        assert_eq!(dc.stars_max, 10000);
        assert_eq!(dc.languages, vec!["python"]);
    }

    #[test]
    fn test_contribution_type_display() {
        assert_eq!(ContributionType::SecurityFix.to_string(), "security_fix");
        assert_eq!(ContributionType::CodeQuality.to_string(), "code_quality");
    }

    #[test]
    fn test_repo_url() {
        let repo = Repository {
            owner: "tang-vu".into(),
            name: "ContribAI".into(),
            full_name: "tang-vu/ContribAI".into(),
            description: None,
            language: None,
            languages: HashMap::new(),
            stars: 0,
            forks: 0,
            open_issues: 0,
            topics: vec![],
            default_branch: "main".into(),
            html_url: String::new(),
            clone_url: String::new(),
            has_contributing: false,
            has_license: false,
            last_push_at: None,
            created_at: None,
        };
        assert_eq!(repo.url(), "https://github.com/tang-vu/ContribAI");
    }
}
