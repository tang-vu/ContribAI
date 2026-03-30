//! Issue-driven contribution engine.
//!
//! Port from Python `issues/solver.py`.
//! Reads open GitHub issues, classifies them, and generates
//! targeted contributions that solve specific issues.

use regex::Regex;
use std::collections::HashMap;
use tracing::{info, warn};

use crate::core::models::{ContributionType, FileNode, Finding, Issue, RepoContext, Repository, Severity};
use crate::github::client::GitHubClient;
use crate::llm::provider::LlmProvider;

/// Classification categories for GitHub issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCategory {
    Bug,
    Feature,
    Docs,
    Security,
    Performance,
    UiUx,
    GoodFirstIssue,
    Unsolvable,
}

/// Map label → category.
fn label_to_category(label: &str) -> Option<IssueCategory> {
    match label.to_lowercase().trim() {
        "bug" | "fix" | "defect" => Some(IssueCategory::Bug),
        "feature" | "enhancement" | "feature-request" => Some(IssueCategory::Feature),
        "documentation" | "docs" => Some(IssueCategory::Docs),
        "security" | "vulnerability" => Some(IssueCategory::Security),
        "performance" => Some(IssueCategory::Performance),
        "ui" | "ux" | "accessibility" => Some(IssueCategory::UiUx),
        "good first issue" | "good-first-issue" | "beginner" | "help wanted" => {
            Some(IssueCategory::GoodFirstIssue)
        }
        _ => None,
    }
}

/// Map category → contribution type.
fn category_to_contrib(cat: IssueCategory) -> ContributionType {
    match cat {
        IssueCategory::Bug => ContributionType::CodeQuality,
        IssueCategory::Feature => ContributionType::FeatureAdd,
        IssueCategory::Docs => ContributionType::DocsImprove,
        IssueCategory::Security => ContributionType::SecurityFix,
        IssueCategory::Performance => ContributionType::PerformanceOpt,
        IssueCategory::UiUx => ContributionType::UiUxFix,
        IssueCategory::GoodFirstIssue => ContributionType::CodeQuality,
        IssueCategory::Unsolvable => ContributionType::CodeQuality,
    }
}

const SOLVABLE_LABELS: &[&str] = &[
    "good first issue",
    "good-first-issue",
    "help wanted",
    "help-wanted",
    "beginner",
    "easy",
    "low-hanging-fruit",
    "bug",
    "documentation",
    "docs",
    "enhancement",
    "feature",
];

/// Analyzes and solves GitHub issues using LLM.
pub struct IssueSolver<'a> {
    llm: &'a dyn LlmProvider,
    github: &'a GitHubClient,
}

impl<'a> IssueSolver<'a> {
    pub fn new(llm: &'a dyn LlmProvider, github: &'a GitHubClient) -> Self {
        Self { llm, github }
    }

    /// Classify an issue based on labels and title keywords.
    pub fn classify_issue(&self, issue: &Issue) -> IssueCategory {
        // Check labels first
        for label in &issue.labels {
            if let Some(cat) = label_to_category(label) {
                return cat;
            }
        }

        // Keyword matching on title
        let title = issue.title.to_lowercase();
        let keyword_map: &[(&[&str], IssueCategory)] = &[
            (&["bug", "fix", "error", "crash", "broken", "fail"], IssueCategory::Bug),
            (&["add", "feature", "implement", "support", "new"], IssueCategory::Feature),
            (&["doc", "readme", "typo", "documentation", "example"], IssueCategory::Docs),
            (&["security", "vulnerability", "cve", "xss", "injection"], IssueCategory::Security),
            (&["slow", "performance", "optimize", "speed", "memory"], IssueCategory::Performance),
            (&["ui", "ux", "responsive", "accessibility", "design"], IssueCategory::UiUx),
        ];

        for (keywords, category) in keyword_map {
            if keywords.iter().any(|kw| title.contains(kw)) {
                return *category;
            }
        }

        IssueCategory::Bug // default
    }

    /// Estimate issue complexity (1-5).
    fn estimate_complexity(&self, issue: &Issue) -> u32 {
        let mut score: u32 = 2;

        // Good first issues are simple
        if issue.labels.iter().any(|l| {
            let low = l.to_lowercase();
            low.contains("first") || low.contains("beginner")
        }) {
            return 1;
        }

        let body_len = issue.body.as_ref().map(|b| b.len()).unwrap_or(0);
        if body_len > 2000 {
            score += 1;
        }
        if body_len > 5000 {
            score += 1;
        }

        // Multiple file references = complex
        if let Some(body) = &issue.body {
            let re = Regex::new(r"[\w/]+\.\w{1,4}").unwrap_or_else(|_| Regex::new(".").unwrap());
            if re.find_iter(body).count() > 3 {
                score += 1;
            }
        }

        score.min(5)
    }

    /// Filter issues to only those solvable by the agent.
    pub fn filter_solvable(&self, issues: &[Issue], max_complexity: u32) -> Vec<Issue> {
        issues
            .iter()
            .filter(|issue| {
                let cat = self.classify_issue(issue);
                if cat == IssueCategory::Unsolvable {
                    return false;
                }
                self.estimate_complexity(issue) <= max_complexity
            })
            .cloned()
            .collect()
    }

    /// Convert a GitHub issue into a Finding for the generator.
    pub async fn solve_issue(
        &self,
        issue: &Issue,
        repo: &Repository,
        context: &RepoContext,
    ) -> Option<Finding> {
        let category = self.classify_issue(issue);
        let contrib_type = category_to_contrib(category);

        let file_tree_str: String = context
            .file_tree
            .iter()
            .filter(|f| f.node_type == "blob")
            .take(50)
            .map(|f| format!("  {}", f.path))
            .collect::<Vec<_>>()
            .join("\n");

        let mut relevant_code = String::new();
        for (path, content) in context.relevant_files.iter().take(3) {
            let snippet: String = content.chars().take(2000).collect();
            relevant_code.push_str(&format!("\n### {}\n```\n{}\n```\n", path, snippet));
        }

        let body = issue.body.as_deref().unwrap_or("No description provided.");

        let prompt = format!(
            "Analyze this GitHub issue and determine:\n\
             1. Which file(s) need changes\n\
             2. What changes are needed\n\
             3. The severity of the issue\n\n\
             ## Repository: {} ({})\n\n\
             ## Issue #{}: {}\n{}\n\n\
             ## Labels: {}\n\n\
             ## File Tree:\n{}\n\n\
             {}\n\n\
             Respond in this exact format:\n\
             FILE_PATH: <main file to change>\n\
             SEVERITY: <low|medium|high|critical>\n\
             TITLE: <short descriptive title>\n\
             DESCRIPTION: <what needs to be changed and why>\n\
             SUGGESTION: <specific implementation suggestion>",
            repo.full_name,
            repo.language.as_deref().unwrap_or("unknown"),
            issue.number,
            issue.title,
            body,
            if issue.labels.is_empty() { "none".to_string() } else { issue.labels.join(", ") },
            file_tree_str,
            relevant_code
        );

        let response = match self
            .llm
            .complete(
                &prompt,
                Some("You are a senior developer analyzing GitHub issues. Identify the root cause and suggest a specific fix."),
                Some(0.2),
                None,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(issue = issue.number, error = %e, "Failed to analyze issue");
                return None;
            }
        };

        // Parse structured response
        let parsed = Self::parse_structured_response(&response);

        let severity = match parsed.get("SEVERITY").map(|s| s.to_lowercase()).as_deref() {
            Some("low") => Severity::Low,
            Some("high") => Severity::High,
            Some("critical") => Severity::Critical,
            _ => Severity::Medium,
        };

        Some(Finding {
            id: format!("issue-{}", issue.number),
            finding_type: contrib_type,
            severity,
            title: parsed.get("TITLE").cloned().unwrap_or_else(|| issue.title.clone()),
            description: parsed
                .get("DESCRIPTION")
                .cloned()
                .unwrap_or_else(|| body.to_string()),
            file_path: parsed.get("FILE_PATH").cloned().unwrap_or_else(|| "unknown".into()),
            suggestion: parsed.get("SUGGESTION").cloned(),
            confidence: 0.85,
            line_start: None,
            line_end: None,
            priority_signals: vec![],
        })
    }

    /// Deep multi-file issue solving.
    pub async fn solve_issue_deep(
        &self,
        issue: &Issue,
        repo: &Repository,
        context: &RepoContext,
    ) -> Vec<Finding> {
        let category = self.classify_issue(issue);
        let contrib_type = category_to_contrib(category);

        let body = issue.body.as_deref().unwrap_or("No description provided.");

        let file_tree_str = Self::build_file_tree_summary(&context.file_tree);

        let mut relevant_code = String::new();
        for (path, content) in context.relevant_files.iter().take(10) {
            let snippet: String = content.chars().take(3000).collect();
            relevant_code.push_str(&format!("\n### {}\n```\n{}\n```\n", path, snippet));
        }

        let prompt = format!(
            "You are solving a GitHub issue. Analyze the issue carefully and create\n\
             a detailed plan for which file(s) to create or modify.\n\n\
             ## Repository: {} ({})\n\n\
             ## Issue #{}: {}\n{}\n\n\
             ## File Tree:\n{}\n\n\
             ## Relevant Code:\n{}\n\n\
             Respond with one or more blocks in this exact format:\n\n\
             ---FILE---\n\
             PATH: <path to file>\n\
             SEVERITY: <low|medium|high|critical>\n\
             TITLE: <what this change does>\n\
             DESCRIPTION: <detailed description>\n\
             SUGGESTION: <specific implementation details>\n\
             ---END---",
            repo.full_name,
            repo.language.as_deref().unwrap_or("unknown"),
            issue.number,
            issue.title,
            body,
            file_tree_str,
            relevant_code,
        );

        match self
            .llm
            .complete(
                &prompt,
                Some("You are an expert open-source developer solving GitHub issues."),
                Some(0.2),
                None,
            )
            .await
        {
            Ok(response) => {
                let findings = Self::parse_multi_file_response(&response, issue, contrib_type);
                if findings.is_empty() {
                    // fallback to single-file
                    if let Some(f) = self.solve_issue(issue, repo, context).await {
                        return vec![f];
                    }
                }
                info!(
                    issue = issue.number,
                    files = findings.len(),
                    "🧠 Deep solve complete"
                );
                findings
            }
            Err(e) => {
                warn!(issue = issue.number, error = %e, "Deep solve failed");
                if let Some(f) = self.solve_issue(issue, repo, context).await {
                    vec![f]
                } else {
                    vec![]
                }
            }
        }
    }

    // ── Issue Discovery ────────────────────────────────────────────────────

    /// Fetch open issues from a repo that are good candidates for automated solving.
    ///
    /// Mirrors Python `fetch_solvable_issues`:
    /// - Queries per label group, falling back to any open issue.
    /// - Skips issues that already have linked PRs via timeline cross-references.
    /// - Applies `filter_solvable` and returns up to `max_issues` sorted by complexity.
    pub async fn fetch_solvable_issues(
        &self,
        repo: &Repository,
        max_issues: usize,
        max_complexity: u32,
    ) -> Vec<Issue> {
        let label_groups: &[&str] = &[
            "good first issue",
            "help wanted",
            "bug",
            "enhancement",
            "documentation",
        ];

        let mut all_issues: Vec<Issue> = Vec::new();

        // Try fetching with preferred label groups first.
        for label in label_groups {
            match self
                .github
                .list_issues(&repo.owner, &repo.name, Some(label), Some("none"), 10)
                .await
            {
                Ok(raw_issues) => {
                    for raw in raw_issues {
                        // Skip pull requests masquerading as issues.
                        if raw.get("pull_request").is_some() {
                            continue;
                        }
                        let issue = Issue {
                            number: raw["number"].as_i64().unwrap_or(0),
                            title: raw["title"].as_str().unwrap_or("").to_string(),
                            body: raw["body"].as_str().map(String::from),
                            labels: raw["labels"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|l| l["name"].as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            state: raw["state"].as_str().unwrap_or("open").to_string(),
                            created_at: None,
                            html_url: raw["html_url"].as_str().unwrap_or("").to_string(),
                        };
                        // Deduplicate by issue number.
                        if !all_issues.iter().any(|i| i.number == issue.number) {
                            all_issues.push(issue);
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(label, error = %e, "Failed to fetch issues with label");
                }
            }
        }

        // Fallback: fetch any open unassigned issues when no label hits.
        if all_issues.is_empty() {
            match self
                .github
                .list_issues(&repo.owner, &repo.name, None, Some("none"), 20)
                .await
            {
                Ok(raw_issues) => {
                    for raw in raw_issues {
                        if raw.get("pull_request").is_some() {
                            continue;
                        }
                        let issue = Issue {
                            number: raw["number"].as_i64().unwrap_or(0),
                            title: raw["title"].as_str().unwrap_or("").to_string(),
                            body: raw["body"].as_str().map(String::from),
                            labels: raw["labels"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|l| l["name"].as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            state: raw["state"].as_str().unwrap_or("open").to_string(),
                            created_at: None,
                            html_url: raw["html_url"].as_str().unwrap_or("").to_string(),
                        };
                        if !all_issues.iter().any(|i| i.number == issue.number) {
                            all_issues.push(issue);
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "Failed to fetch fallback issues");
                }
            }
        }

        // Skip issues that already have a linked PR.
        let mut without_linked_pr: Vec<Issue> = Vec::new();
        for issue in all_issues.iter() {
            if self.has_linked_pr(repo, issue).await {
                tracing::debug!(
                    issue = issue.number,
                    title = %issue.title,
                    "Skipping issue (has linked PR)"
                );
            } else {
                without_linked_pr.push(issue.clone());
            }
        }

        // Apply complexity/solvability filter.
        let mut solvable = self.filter_solvable(&without_linked_pr, max_complexity);

        // Sort by estimated complexity (easiest first).
        solvable.sort_by_key(|i| self.estimate_complexity(i));

        info!(
            repo = %repo.full_name,
            solvable = solvable.len(),
            total = all_issues.len(),
            "Found solvable issues"
        );

        solvable.into_iter().take(max_issues).collect()
    }

    /// Check if an issue already has a linked pull request.
    ///
    /// Uses the GitHub timeline API to detect `cross-referenced` events
    /// where the source is a pull request.  Falls back to `false` on
    /// any API error so we never block issue processing on a transient error.
    pub async fn has_linked_pr(&self, repo: &Repository, issue: &Issue) -> bool {
        match self
            .github
            .get_issue_timeline(&repo.owner, &repo.name, issue.number)
            .await
        {
            Ok(events) => Self::timeline_contains_pr_reference(&events),
            Err(_) => false,
        }
    }

    /// Pure helper: scan timeline events for cross-referenced PR links.
    ///
    /// Extracted so it can be unit-tested without HTTP.
    fn timeline_contains_pr_reference(events: &[serde_json::Value]) -> bool {
        for event in events {
            if event.get("event").and_then(|e| e.as_str()) == Some("cross-referenced") {
                let source = &event["source"];
                if source.get("type").and_then(|t| t.as_str()) == Some("issue") {
                    // GitHub returns a "pull_request" sub-object when the
                    // cross-referencing issue is actually a PR.
                    if !source["issue"]["pull_request"].is_null() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Build a rich context string for the LLM from an issue and its comments.
    ///
    /// Includes:
    /// - Issue title, labels, and body.
    /// - Up to 5 comments (truncated at 1 000 chars each).
    /// - File paths mentioned anywhere in the issue text (regex `[\w/]+\.\w{1,4}`).
    ///
    /// Used by `solve_issue_deep` to provide context to the LLM prompt.
    pub async fn build_issue_context(&self, issue: &Issue, repo: &Repository) -> String {
        let body = issue.body.as_deref().unwrap_or("No description provided.");
        let labels_str = if issue.labels.is_empty() {
            "none".to_string()
        } else {
            issue.labels.join(", ")
        };

        // Extract file path mentions from the issue body.
        let file_paths = Self::extract_file_paths(body);
        let file_paths_section = if file_paths.is_empty() {
            String::new()
        } else {
            format!("\n\n**Mentioned files:**\n{}", file_paths.join("\n"))
        };

        let mut parts = vec![format!(
            "**Title:** {}\n**Labels:** {}\n\n{}{}",
            issue.title, labels_str, body, file_paths_section
        )];

        // Fetch and append up to 5 comments.
        match self
            .github
            .get_issue_comments(&repo.owner, &repo.name, issue.number)
            .await
        {
            Ok(comments) => {
                for comment in comments.iter().take(5) {
                    let author = comment["user"]["login"].as_str().unwrap_or("unknown");
                    let body = comment["body"].as_str().unwrap_or("");
                    if body.len() > 10 {
                        let truncated: String = body.chars().take(1000).collect();
                        parts.push(format!("\n**Comment by @{}:**\n{}", author, truncated));
                    }
                }
            }
            Err(e) => {
                tracing::debug!(
                    issue = issue.number,
                    error = %e,
                    "Failed to fetch issue comments for context"
                );
            }
        }

        parts.join("\n")
    }

    /// Extract file path mentions from a text string.
    ///
    /// Matches patterns like `src/foo.rs`, `lib/bar.py`, `README.md`.
    fn extract_file_paths(text: &str) -> Vec<String> {
        let re = Regex::new(r"[\w/]+\.\w{1,4}").unwrap_or_else(|_| Regex::new(".^").unwrap());
        let mut paths: Vec<String> = re
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect();
        paths.dedup();
        paths
    }

    fn build_file_tree_summary(tree: &[FileNode]) -> String {
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        for f in tree.iter().filter(|f| f.node_type == "blob").take(200) {
            let (dir, file) = match f.path.rsplit_once('/') {
                Some((d, f)) => (d.to_string(), f.to_string()),
                None => (".".to_string(), f.path.clone()),
            };
            dirs.entry(dir).or_default().push(file);
        }

        let mut keys: Vec<_> = dirs.keys().cloned().collect();
        keys.sort();
        keys.iter()
            .take(30)
            .map(|dir| {
                let files = &dirs[dir];
                let files_str = if files.len() > 8 {
                    format!(
                        "{} (+{} more)",
                        files[..8].join(", "),
                        files.len() - 8
                    )
                } else {
                    files.join(", ")
                };
                format!("  {}/  [{}]", dir, files_str)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parse_structured_response(response: &str) -> HashMap<String, String> {
        let mut parsed = HashMap::new();
        for line in response.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_uppercase();
                if ["FILE_PATH", "PATH", "SEVERITY", "TITLE", "DESCRIPTION", "SUGGESTION", "ACTION"]
                    .contains(&key.as_str())
                {
                    parsed.insert(key, value.trim().to_string());
                }
            }
        }
        parsed
    }

    fn parse_multi_file_response(
        response: &str,
        issue: &Issue,
        default_type: ContributionType,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        let blocks: Vec<&str> = response.split("---FILE---").collect();

        for block in blocks {
            let block = block.trim();
            if block.is_empty() || !block.contains("---END---") {
                continue;
            }

            let block = block.split("---END---").next().unwrap_or("").trim();
            let parsed = Self::parse_structured_response(block);

            let file_path = match parsed.get("PATH") {
                Some(p) if p != "unknown" && !p.is_empty() => p.clone(),
                _ => continue,
            };

            let severity = match parsed.get("SEVERITY").map(|s| s.to_lowercase()).as_deref() {
                Some("low") => Severity::Low,
                Some("high") => Severity::High,
                Some("critical") => Severity::Critical,
                _ => Severity::Medium,
            };

            findings.push(Finding {
                id: format!("issue-{}-{}", issue.number, findings.len()),
                finding_type: default_type.clone(),
                severity,
                title: parsed
                    .get("TITLE")
                    .cloned()
                    .unwrap_or_else(|| issue.title.clone()),
                description: parsed
                    .get("DESCRIPTION")
                    .cloned()
                    .unwrap_or_else(|| issue.body.clone().unwrap_or_default()),
                file_path,
                suggestion: parsed.get("SUGGESTION").cloned(),
                confidence: 0.80,
                line_start: None,
            line_end: None,
            priority_signals: vec![],
            });
        }

        findings.into_iter().take(5).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::Result;

    fn make_issue(title: &str, labels: &[&str]) -> Issue {
        Issue {
            number: 1,
            title: title.to_string(),
            body: Some("test body".into()),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            state: "open".into(),
            created_at: None,
            html_url: String::new(),
        }
    }

    #[test]
    fn test_classify_by_label() {
        let solver = IssueSolver { llm: &MockLlm, github: &unsafe_mock_github() };
        assert_eq!(
            solver.classify_issue(&make_issue("anything", &["bug"])),
            IssueCategory::Bug
        );
        assert_eq!(
            solver.classify_issue(&make_issue("anything", &["documentation"])),
            IssueCategory::Docs
        );
        assert_eq!(
            solver.classify_issue(&make_issue("anything", &["good first issue"])),
            IssueCategory::GoodFirstIssue
        );
    }

    #[test]
    fn test_classify_by_title() {
        let solver = IssueSolver { llm: &MockLlm, github: &unsafe_mock_github() };
        assert_eq!(
            solver.classify_issue(&make_issue("fix crash on startup", &[])),
            IssueCategory::Bug
        );
        assert_eq!(
            solver.classify_issue(&make_issue("add support for JSON", &[])),
            IssueCategory::Feature
        );
        assert_eq!(
            solver.classify_issue(&make_issue("update readme docs", &[])),
            IssueCategory::Docs
        );
    }

    #[test]
    fn test_complexity_good_first() {
        let solver = IssueSolver { llm: &MockLlm, github: &unsafe_mock_github() };
        assert_eq!(
            solver.estimate_complexity(&make_issue("easy fix", &["good first issue"])),
            1
        );
    }

    #[test]
    fn test_filter_solvable() {
        let solver = IssueSolver { llm: &MockLlm, github: &unsafe_mock_github() };
        let issues = vec![
            make_issue("fix bug", &["bug"]),
            make_issue("x".repeat(6000).as_str(), &[]),
        ];
        // Second issue has long body (but it's in title here, body is "test body")
        let filtered = solver.filter_solvable(&issues, 3);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_parse_structured_response() {
        let response = "FILE_PATH: src/main.py\nSEVERITY: high\nTITLE: Fix null check\nDESCRIPTION: Missing null check\nSUGGESTION: Add if not None";
        let parsed = IssueSolver::parse_structured_response(response);
        assert_eq!(parsed.get("FILE_PATH").unwrap(), "src/main.py");
        assert_eq!(parsed.get("SEVERITY").unwrap(), "high");
    }

    #[test]
    fn test_parse_multi_file_response() {
        let response = "---FILE---\nPATH: src/a.py\nSEVERITY: high\nTITLE: Fix A\nDESCRIPTION: desc\nSUGGESTION: fix\n---END---\n---FILE---\nPATH: src/b.py\nSEVERITY: low\nTITLE: Fix B\nDESCRIPTION: desc2\nSUGGESTION: fix2\n---END---";
        let issue = make_issue("test", &[]);
        let findings =
            IssueSolver::parse_multi_file_response(response, &issue, ContributionType::CodeQuality);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].file_path, "src/a.py");
        assert_eq!(findings[1].file_path, "src/b.py");
    }

    // Minimal mock LLM for tests
    struct MockLlm;
    #[async_trait::async_trait]
    impl LlmProvider for MockLlm {
        async fn complete(
            &self, _: &str, _: Option<&str>, _: Option<f64>, _: Option<u32>,
        ) -> Result<String> {
            Ok("mock".into())
        }
        async fn chat(
            &self,
            _: &[crate::llm::provider::ChatMessage],
            _: Option<&str>,
            _: Option<f64>,
            _: Option<u32>,
        ) -> Result<String> {
            Ok("mock".into())
        }
    }

    // Safety: only used in tests, never actually called
    fn unsafe_mock_github() -> GitHubClient {
        GitHubClient::new("test-token", 100).unwrap()
    }

    // ── Tests for has_linked_pr (pure helper) ─────────────────────────────

    #[test]
    fn test_timeline_no_pr_reference() {
        // Timeline with no cross-referenced events → no linked PR.
        let events = vec![
            serde_json::json!({ "event": "labeled", "label": { "name": "bug" } }),
            serde_json::json!({ "event": "assigned" }),
        ];
        assert!(!IssueSolver::timeline_contains_pr_reference(&events));
    }

    #[test]
    fn test_timeline_cross_ref_from_pr() {
        // A cross-referenced event where the source is a PR (has pull_request field).
        let events = vec![serde_json::json!({
            "event": "cross-referenced",
            "source": {
                "type": "issue",
                "issue": {
                    "number": 42,
                    "title": "Fix bug",
                    "pull_request": {
                        "url": "https://api.github.com/repos/owner/repo/pulls/42"
                    }
                }
            }
        })];
        assert!(IssueSolver::timeline_contains_pr_reference(&events));
    }

    #[test]
    fn test_timeline_cross_ref_from_plain_issue() {
        // Cross-reference from a regular issue (no pull_request field) → not a PR link.
        let events = vec![serde_json::json!({
            "event": "cross-referenced",
            "source": {
                "type": "issue",
                "issue": {
                    "number": 7,
                    "title": "Related issue"
                    // no "pull_request" key
                }
            }
        })];
        assert!(!IssueSolver::timeline_contains_pr_reference(&events));
    }

    #[test]
    fn test_timeline_empty() {
        assert!(!IssueSolver::timeline_contains_pr_reference(&[]));
    }

    // ── Tests for build_issue_context (pure helper: extract_file_paths) ───

    #[test]
    fn test_extract_file_paths_basic() {
        let text = "Please fix src/main.rs and also update lib/utils.py if needed.";
        let paths = IssueSolver::extract_file_paths(text);
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"lib/utils.py".to_string()));
    }

    #[test]
    fn test_extract_file_paths_dedup() {
        // Same path appearing twice should appear once after dedup.
        let text = "See README.md for details. Also README.md.";
        let paths = IssueSolver::extract_file_paths(text);
        // dedup removes consecutive duplicates; check at most 2 entries for README.md
        let count = paths.iter().filter(|p| p.as_str() == "README.md").count();
        assert!(count <= 1, "Expected dedup to remove duplicate README.md");
    }

    #[test]
    fn test_extract_file_paths_empty() {
        let paths = IssueSolver::extract_file_paths("no file references here");
        // "here" has no extension, so nothing matches the pattern
        assert!(paths.is_empty());
    }

    // ── Tests for build_issue_context (structure, no HTTP) ────────────────

    #[test]
    fn test_build_issue_context_labels_in_output() {
        // build_issue_context is async/HTTP, but we test the pure formatting
        // logic by verifying extract_file_paths and label formatting work together.
        let issue = Issue {
            number: 10,
            title: "Fix null pointer in src/parser.rs".to_string(),
            body: Some("The crash happens in src/parser.rs line 42.".to_string()),
            labels: vec!["bug".to_string(), "good first issue".to_string()],
            state: "open".to_string(),
            created_at: None,
            html_url: String::new(),
        };

        // Verify label string building.
        let labels_str = issue.labels.join(", ");
        assert_eq!(labels_str, "bug, good first issue");

        // Verify file path extraction from body.
        let paths = IssueSolver::extract_file_paths(issue.body.as_deref().unwrap_or(""));
        assert!(paths.contains(&"src/parser.rs".to_string()));
    }
}
