//! Contribution quality scorer.
//!
//! Port from Python `generator/scorer.py`.
//! Evaluates generated contributions before submission
//! to prevent low-quality PRs.

use regex::Regex;
use std::sync::LazyLock;
use tracing::info;

use crate::core::models::Contribution;

static RE_CONVENTIONAL_COMMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(feat|fix|docs|refactor|perf|test|chore)\(?.*\)?: .+").unwrap()
});

/// Quality assessment of a contribution.
#[derive(Debug, Clone)]
pub struct QualityReport {
    pub score: f64, // 0.0 - 1.0
    pub passed: bool,
    pub checks: Vec<CheckResult>,
}

impl QualityReport {
    pub fn summary(&self) -> String {
        let passed = self.checks.iter().filter(|c| c.passed).count();
        format!(
            "{}/{} checks passed (score: {:.0}%)",
            passed,
            self.checks.len(),
            self.score * 100.0
        )
    }
}

/// Result of a single quality check.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub score: f64, // 0.0 - 1.0
    pub reason: String,
}

/// Evaluates contribution quality before PR submission.
pub struct QualityScorer {
    min_score: f64,
}

impl QualityScorer {
    pub fn new(min_score: f64) -> Self {
        Self { min_score }
    }

    /// Run all quality checks on a contribution.
    pub fn evaluate(&self, contribution: &Contribution) -> QualityReport {
        let checks = vec![
            self.check_has_changes(contribution),
            self.check_change_size(contribution),
            self.check_commit_message(contribution),
            self.check_description(contribution),
            self.check_no_debug_code(contribution),
            self.check_no_placeholders(contribution),
            self.check_file_coherence(contribution),
        ];

        let total_score: f64 = checks.iter().map(|c| c.score).sum::<f64>() / checks.len() as f64;
        let passed = total_score >= self.min_score;

        let report = QualityReport {
            score: total_score,
            passed,
            checks,
        };

        info!(summary = %report.summary(), "Quality check");
        report
    }

    fn check_has_changes(&self, c: &Contribution) -> CheckResult {
        let has = !c.changes.is_empty()
            && c.changes
                .iter()
                .any(|ch| !ch.new_content.trim().is_empty());
        CheckResult {
            name: "has_changes".into(),
            passed: has,
            score: if has { 1.0 } else { 0.0 },
            reason: if has {
                "Has file changes".into()
            } else {
                "No file changes".into()
            },
        }
    }

    fn check_change_size(&self, c: &Contribution) -> CheckResult {
        let total_lines: usize = c
            .changes
            .iter()
            .map(|ch| ch.new_content.lines().count())
            .sum();

        if total_lines == 0 {
            CheckResult {
                name: "change_size".into(),
                passed: false,
                score: 0.0,
                reason: "Empty changes".into(),
            }
        } else if total_lines < 3 {
            CheckResult {
                name: "change_size".into(),
                passed: false,
                score: 0.3,
                reason: format!("Very small change ({} lines)", total_lines),
            }
        } else if total_lines > 500 {
            CheckResult {
                name: "change_size".into(),
                passed: false,
                score: 0.4,
                reason: format!("Very large change ({} lines)", total_lines),
            }
        } else if total_lines > 200 {
            CheckResult {
                name: "change_size".into(),
                passed: true,
                score: 0.7,
                reason: format!("Large change ({} lines)", total_lines),
            }
        } else {
            CheckResult {
                name: "change_size".into(),
                passed: true,
                score: 1.0,
                reason: format!("Good change size ({} lines)", total_lines),
            }
        }
    }

    fn check_commit_message(&self, c: &Contribution) -> CheckResult {
        let msg = &c.commit_message;
        if msg.is_empty() {
            return CheckResult {
                name: "commit_message".into(),
                passed: false,
                score: 0.0,
                reason: "Empty commit message".into(),
            };
        }

        if RE_CONVENTIONAL_COMMIT.is_match(msg) {
            CheckResult {
                name: "commit_message".into(),
                passed: true,
                score: 1.0,
                reason: "Follows conventional commits".into(),
            }
        } else if msg.len() > 10 {
            CheckResult {
                name: "commit_message".into(),
                passed: true,
                score: 0.7,
                reason: "Descriptive but not conventional".into(),
            }
        } else {
            CheckResult {
                name: "commit_message".into(),
                passed: false,
                score: 0.3,
                reason: "Poor commit message".into(),
            }
        }
    }

    fn check_description(&self, c: &Contribution) -> CheckResult {
        if c.description.is_empty() {
            CheckResult {
                name: "description".into(),
                passed: false,
                score: 0.0,
                reason: "Empty description".into(),
            }
        } else if c.description.len() < 20 {
            CheckResult {
                name: "description".into(),
                passed: false,
                score: 0.3,
                reason: "Description too short".into(),
            }
        } else {
            CheckResult {
                name: "description".into(),
                passed: true,
                score: 1.0,
                reason: "Good description".into(),
            }
        }
    }

    fn check_no_debug_code(&self, c: &Contribution) -> CheckResult {
        let debug_patterns = [
            r"\bprint\s*\(",
            r"\bconsole\.log\s*\(",
            r"\bdebugger\b",
            r"\bpdb\.set_trace\(",
            r"\bbreakpoint\(\)",
            r"#\s*TODO\b",
            r"#\s*FIXME\b",
            r"#\s*HACK\b",
        ];

        for change in &c.changes {
            for pattern in &debug_patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(&change.new_content) {
                        return CheckResult {
                            name: "no_debug_code".into(),
                            passed: false,
                            score: 0.3,
                            reason: format!("Debug code found: {}", pattern),
                        };
                    }
                }
            }
        }

        CheckResult {
            name: "no_debug_code".into(),
            passed: true,
            score: 1.0,
            reason: "No debug code found".into(),
        }
    }

    fn check_no_placeholders(&self, c: &Contribution) -> CheckResult {
        let placeholder_patterns = [
            "TODO: implement",
            "PLACEHOLDER",
            "YOUR_CODE_HERE",
            "pass  # TODO",
            "raise NotImplementedError",
            "...",
        ];

        for change in &c.changes {
            for pattern in &placeholder_patterns {
                if change.new_content.contains(pattern) {
                    return CheckResult {
                        name: "no_placeholders".into(),
                        passed: false,
                        score: 0.2,
                        reason: format!("Placeholder found: {}", pattern),
                    };
                }
            }
        }

        CheckResult {
            name: "no_placeholders".into(),
            passed: true,
            score: 1.0,
            reason: "No placeholders found".into(),
        }
    }

    fn check_file_coherence(&self, c: &Contribution) -> CheckResult {
        // Check that all changed files relate to the finding
        let finding_dir = c
            .finding
            .file_path
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or("");

        let unrelated = c
            .changes
            .iter()
            .filter(|ch| {
                let change_dir = ch.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                !change_dir.starts_with(finding_dir) && !finding_dir.starts_with(change_dir)
            })
            .count();

        if unrelated > 0 {
            CheckResult {
                name: "file_coherence".into(),
                passed: false,
                score: 0.5,
                reason: format!("{} unrelated file(s) changed", unrelated),
            }
        } else {
            CheckResult {
                name: "file_coherence".into(),
                passed: true,
                score: 1.0,
                reason: "All changes are related".into(),
            }
        }
    }
}

impl Default for QualityScorer {
    fn default() -> Self {
        Self::new(0.6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::core::models::{ContributionType, FileChange, Finding, Severity};

    fn test_contribution() -> Contribution {
        Contribution {
            finding: Finding {
                id: "1".into(),
                finding_type: ContributionType::SecurityFix,
                severity: Severity::High,
                title: "SQL injection".into(),
                description: "User input is not sanitized before use in SQL queries".into(),
                file_path: "src/db/queries.py".into(),
                line_start: Some(10),
                line_end: Some(15),
                suggestion: Some("Use parameterized queries".into()),
                confidence: 0.9,
                priority_signals: vec![],
            },
            contribution_type: ContributionType::SecurityFix,
            title: "fix: sql injection in user query".into(),
            description: "User input is not sanitized before use in SQL queries".into(),
            changes: vec![FileChange {
                path: "src/db/queries.py".into(),
                original_content: None,
                new_content: "def query(user_id):\n    cursor.execute('SELECT * FROM users WHERE id = %s', (user_id,))\n".into(),
                is_new_file: false,
                is_deleted: false,
            }],
            commit_message: "fix(db): sanitize sql query parameters".into(),
            tests_added: vec![],
            branch_name: "fix/security/sql-injection".into(),
            generated_at: Utc::now(),
        }
    }

    #[test]
    fn test_quality_scorer_good_contribution() {
        let scorer = QualityScorer::default();
        let c = test_contribution();
        let report = scorer.evaluate(&c);

        assert!(report.passed, "Good contribution should pass: {}", report.summary());
        assert!(report.score >= 0.6);
    }

    #[test]
    fn test_quality_scorer_empty_changes() {
        let scorer = QualityScorer::default();
        let mut c = test_contribution();
        c.changes = vec![];

        let report = scorer.evaluate(&c);
        let check = report.checks.iter().find(|c| c.name == "has_changes").unwrap();
        assert!(!check.passed, "has_changes should fail with no changes");
        assert_eq!(check.score, 0.0);
    }

    #[test]
    fn test_quality_scorer_debug_code() {
        let scorer = QualityScorer::default();
        let mut c = test_contribution();
        c.changes = vec![FileChange {
            path: "src/db/queries.py".into(),
            original_content: None,
            new_content: "print('debugging here')\ndef query():\n    pass\n".into(),
            is_new_file: false,
            is_deleted: false,
        }];

        let report = scorer.evaluate(&c);
        let debug_check = report.checks.iter().find(|c| c.name == "no_debug_code");
        assert!(!debug_check.unwrap().passed, "Should catch debug code");
    }

    #[test]
    fn test_quality_scorer_conventional_commit() {
        let scorer = QualityScorer::default();
        let c = test_contribution();
        let report = scorer.evaluate(&c);

        let commit_check = report.checks.iter().find(|c| c.name == "commit_message");
        assert!(commit_check.unwrap().passed);
        assert_eq!(commit_check.unwrap().score, 1.0);
    }
}
