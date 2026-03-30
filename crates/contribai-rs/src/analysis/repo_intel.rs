//! Repo Intelligence — profile repositories before contributing.
//!
//! Port from Python `analysis/repo_intel.py`.
//! Analyzes contribution culture: merged PRs, actionable issues,
//! review speed, and maintainer activity.

use std::collections::{HashMap, HashSet};
use tracing::{debug, info};

use crate::github::client::GitHubClient;

/// Intelligence gathered about a repository's contribution culture.
#[derive(Debug, Clone, Default)]
pub struct RepoProfile {
    pub repo: String,
    pub merged_pr_types: Vec<String>,
    pub actionable_issues: Vec<ActionableIssue>,
    pub avg_review_hours: f64,
    pub is_active: bool,
    pub preferred_types: Vec<String>,
    pub rejected_types: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ActionableIssue {
    pub number: i64,
    pub title: String,
    pub labels: Vec<String>,
    pub score: i64,
    pub comments: i64,
}

impl RepoProfile {
    /// Format profile as context for LLM prompts.
    pub fn to_prompt_context(&self) -> String {
        let mut parts = vec![format!("REPO INTELLIGENCE for {}:", self.repo)];

        if !self.preferred_types.is_empty() {
            parts.push(format!(
                "- Preferred contributions: {}",
                self.preferred_types.join(", ")
            ));
        }
        if !self.rejected_types.is_empty() {
            parts.push(format!(
                "- Rejected types (AVOID): {}",
                self.rejected_types.join(", ")
            ));
        }
        if !self.actionable_issues.is_empty() {
            parts.push("- High-value open issues:".into());
            for issue in self.actionable_issues.iter().take(5) {
                parts.push(format!(
                    "  #{}: {} [{}]",
                    issue.number,
                    issue.title,
                    issue.labels.join(", ")
                ));
            }
        }
        if self.avg_review_hours > 0.0 {
            parts.push(format!(
                "- Avg review time: {:.0}h",
                self.avg_review_hours
            ));
        }

        parts.join("\n")
    }
}

/// PR type classification keywords.
fn type_keywords() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m = HashMap::new();
    m.insert("security", vec!["security", "vulnerability", "cve", "xss", "injection", "auth"]);
    m.insert("bug_fix", vec!["fix", "bug", "crash", "error", "issue", "broken", "null", "none"]);
    m.insert("test", vec!["test", "coverage", "spec", "unittest", "pytest"]);
    m.insert("docs", vec!["doc", "readme", "changelog", "comment", "docstring"]);
    m.insert("refactor", vec!["refactor", "cleanup", "simplify", "extract", "reorganize"]);
    m.insert("performance", vec!["perf", "performance", "speed", "optimize", "cache", "memory"]);
    m.insert("feature", vec!["add", "feat", "feature", "support", "implement", "new"]);
    m.insert("ci", vec!["ci", "workflow", "github action", "pipeline", "build"]);
    m.insert("deps", vec!["bump", "upgrade", "dependency", "update", "version"]);
    m
}

fn high_value_labels() -> HashSet<&'static str> {
    [
        "good first issue", "help wanted", "bug", "enhancement",
        "easy", "beginner", "low-hanging fruit", "contributions welcome",
        "hacktoberfest",
    ]
    .into_iter()
    .collect()
}

/// Gather intelligence about a repo before contributing.
pub struct RepoIntelligence<'a> {
    github: &'a GitHubClient,
}

impl<'a> RepoIntelligence<'a> {
    pub fn new(github: &'a GitHubClient) -> Self {
        Self { github }
    }

    /// Build a comprehensive profile of a repo's contribution culture.
    pub async fn profile(&self, owner: &str, repo: &str) -> RepoProfile {
        let full_name = format!("{}/{}", owner, repo);
        let mut profile = RepoProfile {
            repo: full_name.clone(),
            is_active: true,
            ..Default::default()
        };

        // 1. Analyze recently merged PRs
        match self.analyze_pr_history(owner, repo).await {
            Ok((merged, rejected, avg)) => {
                profile.preferred_types = merged.iter().cloned().collect::<HashSet<_>>().into_iter().collect();
                profile.rejected_types = rejected.iter().cloned().collect::<HashSet<_>>().into_iter().collect();
                profile.merged_pr_types = merged;
                profile.avg_review_hours = avg;
            }
            Err(e) => debug!(repo = %full_name, error = %e, "Could not analyze PR history"),
        }

        // 2. Find actionable issues
        match self.find_actionable_issues(owner, repo).await {
            Ok(issues) => profile.actionable_issues = issues,
            Err(e) => debug!(repo = %full_name, error = %e, "Could not fetch issues"),
        }

        // 3. Build summary
        profile.summary = profile.to_prompt_context();
        info!(
            repo = %full_name,
            preferred = profile.preferred_types.len(),
            issues = profile.actionable_issues.len(),
            "🧠 Repo intel complete"
        );

        profile
    }

    async fn analyze_pr_history(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<(Vec<String>, Vec<String>, f64), String> {
        let prs = self
            .github
            .list_pull_requests(owner, repo, "closed", 30)
            .await
            .map_err(|e| format!("{e}"))?;

        let keywords = type_keywords();
        let mut merged: Vec<String> = Vec::new();
        let mut rejected: Vec<String> = Vec::new();
        let mut review_hours: Vec<f64> = Vec::new();

        for pr in &prs {
            let title = pr
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let pr_type = classify_pr(&title, &keywords);

            if pr.get("merged_at").and_then(|v| v.as_str()).is_some() {
                merged.push(pr_type);
                let created = pr.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                let merged_at = pr.get("merged_at").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(hours) = time_diff_hours(created, merged_at) {
                    if hours < 720.0 {
                        review_hours.push(hours);
                    }
                }
            } else {
                rejected.push(pr_type);
            }
        }

        let avg = if review_hours.is_empty() {
            0.0
        } else {
            review_hours.iter().sum::<f64>() / review_hours.len() as f64
        };

        Ok((merged, rejected, avg))
    }

    async fn find_actionable_issues(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<ActionableIssue>, String> {
        let issues = self
            .github
            .get_open_issues(owner, repo, 30)
            .await
            .map_err(|e| format!("{e}"))?;

        let high_value = high_value_labels();
        let mut actionable: Vec<ActionableIssue> = Vec::new();

        for issue in &issues {
            let labels_lower: Vec<String> = issue.labels.iter().map(|l| l.to_lowercase()).collect();
            let score: i64 = labels_lower
                .iter()
                .filter(|l| high_value.contains(l.as_str()))
                .count() as i64;

            if score > 0 {
                actionable.push(ActionableIssue {
                    number: issue.number,
                    title: issue.title.clone(),
                    labels: labels_lower,
                    score,
                    comments: 0,
                });
            }
        }

        actionable.sort_by(|a, b| b.score.cmp(&a.score));
        Ok(actionable.into_iter().take(10).collect())
    }
}

fn classify_pr(title: &str, keywords: &HashMap<&str, Vec<&str>>) -> String {
    let title_lower = title.to_lowercase();
    for (pr_type, kws) in keywords {
        if kws.iter().any(|kw| title_lower.contains(kw)) {
            return pr_type.to_string();
        }
    }
    "other".into()
}

fn time_diff_hours(created: &str, merged: &str) -> Option<f64> {
    use chrono::NaiveDateTime;
    let fmt = "%Y-%m-%dT%H:%M:%SZ";
    let c = NaiveDateTime::parse_from_str(created, fmt).ok()?;
    let m = NaiveDateTime::parse_from_str(merged, fmt).ok()?;
    Some((m - c).num_seconds() as f64 / 3600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_pr_bugfix() {
        let kw = type_keywords();
        assert_eq!(classify_pr("fix crash on startup", &kw), "bug_fix");
    }

    #[test]
    fn test_classify_pr_feature() {
        let kw = type_keywords();
        assert_eq!(classify_pr("add support for YAML", &kw), "feature");
    }

    #[test]
    fn test_classify_pr_unknown() {
        let kw = type_keywords();
        assert_eq!(classify_pr("hello world", &kw), "other");
    }

    #[test]
    fn test_time_diff_hours() {
        let h = time_diff_hours("2024-01-01T00:00:00Z", "2024-01-01T03:00:00Z").unwrap();
        assert!((h - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_time_diff_invalid() {
        assert!(time_diff_hours("bad", "data").is_none());
    }

    #[test]
    fn test_repo_profile_prompt_context() {
        let profile = RepoProfile {
            repo: "test/repo".into(),
            preferred_types: vec!["bug_fix".into(), "security".into()],
            rejected_types: vec!["docs".into()],
            actionable_issues: vec![ActionableIssue {
                number: 42,
                title: "Fix login".into(),
                labels: vec!["bug".into()],
                score: 3,
                comments: 5,
            }],
            avg_review_hours: 24.0,
            ..Default::default()
        };

        let ctx = profile.to_prompt_context();
        assert!(ctx.contains("test/repo"));
        assert!(ctx.contains("bug_fix"));
        assert!(ctx.contains("AVOID"));
        assert!(ctx.contains("#42"));
        assert!(ctx.contains("24h"));
    }

    #[test]
    fn test_high_value_labels() {
        let labels = high_value_labels();
        assert!(labels.contains("good first issue"));
        assert!(labels.contains("help wanted"));
        assert!(!labels.contains("wontfix"));
    }
}
