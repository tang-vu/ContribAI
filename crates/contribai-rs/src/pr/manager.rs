//! Pull Request lifecycle manager.
//!
//! Port from Python `pr/manager.py`.
//! Handles: fork → branch → commit → PR → compliance → CLA.

use regex::Regex;
use std::sync::LazyLock;
use tracing::{info, warn};

use crate::core::error::Result;

static RE_SLUG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
static RE_CONVENTIONAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]+(\([^)]+\))?!?: .+").unwrap());
use crate::core::models::{
    Contribution, ContributionType, PrResult, PrStatus, Repository,
};
use crate::github::client::GitHubClient;
use crate::github::guidelines::RepoGuidelines;

/// How long (seconds) to wait for bots to post compliance comments after PR creation.
const COMPLIANCE_BOT_WAIT_SECS: u64 = 15;

/// Manage the full pull request lifecycle.
pub struct PrManager<'a> {
    github: &'a GitHubClient,
    user: Option<serde_json::Value>,
}

impl<'a> PrManager<'a> {
    pub fn new(github: &'a GitHubClient) -> Self {
        Self {
            github,
            user: None,
        }
    }

    /// Get and cache the authenticated user.
    async fn get_user(&mut self) -> Result<&serde_json::Value> {
        if self.user.is_none() {
            let user = self.github.get_authenticated_user().await?;
            self.user = Some(user);
        }
        Ok(self.user.as_ref().unwrap())
    }

    /// Build DCO Signed-off-by string.
    fn build_signoff(user: &serde_json::Value) -> Option<String> {
        let name = user["name"]
            .as_str()
            .or(user["login"].as_str())
            .unwrap_or("");
        let email = user["email"].as_str().map(String::from).unwrap_or_else(|| {
            let uid = user["id"].as_i64().unwrap_or(0);
            let login = user["login"].as_str().unwrap_or("");
            format!("{}+{}@users.noreply.github.com", uid, login)
        });

        if name.is_empty() {
            None
        } else {
            Some(format!("{} <{}>", name, email))
        }
    }

    /// Create a PR from a generated contribution.
    ///
    /// Full workflow: fork → branch → commit → PR
    pub async fn create_pr(
        &mut self,
        contribution: &Contribution,
        target_repo: &Repository,
    ) -> Result<PrResult> {
        let user = self.get_user().await?;
        let username = user["login"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let signoff = Self::build_signoff(user);

        // 1. Fork
        let fork = self.fork_if_needed(&username, target_repo).await?;

        // 2. Create branch
        let branch = if contribution.branch_name.is_empty() {
            Self::human_branch_name(contribution)
        } else {
            contribution.branch_name.clone()
        };

        self.github
            .create_branch(&fork.owner, &fork.name, &branch, None)
            .await?;

        // 3. Commit all file changes
        for change in contribution.changes.iter().chain(contribution.tests_added.iter()) {
            let sha = if !change.is_new_file {
                self.github
                    .get_file_sha(&fork.owner, &fork.name, &change.path, Some(&branch))
                    .await
                    .ok()
            } else {
                None
            };

            self.github
                .create_or_update_file(
                    &fork.owner,
                    &fork.name,
                    &change.path,
                    &change.new_content,
                    &contribution.commit_message,
                    &branch,
                    sha.as_deref(),
                    signoff.as_deref(),
                )
                .await?;
        }

        // 4. Create PR
        let pr_body = self.generate_pr_body(contribution);
        let head = format!("{}:{}", fork.owner, branch);

        let pr_data = self
            .github
            .create_pull_request(
                &target_repo.owner,
                &target_repo.name,
                &contribution.title,
                &pr_body,
                &head,
                Some(target_repo.default_branch.as_str()),
            )
            .await?;

        let pr_number = pr_data["number"].as_i64().unwrap_or(0);
        let pr_url = pr_data["html_url"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let result = PrResult {
            repo: target_repo.clone(),
            contribution: contribution.clone(),
            pr_number,
            pr_url: pr_url.clone(),
            status: PrStatus::Open,
            created_at: chrono::Utc::now(),
            branch_name: branch,
            fork_full_name: fork.full_name,
        };

        info!(pr = pr_number, url = %pr_url, "PR created");
        Ok(result)
    }

    /// Fork if not already forked.
    async fn fork_if_needed(
        &self,
        username: &str,
        repo: &Repository,
    ) -> Result<Repository> {
        // Check if fork exists
        match self.github.get_repo_details(username, &repo.name).await {
            Ok(existing) if existing.owner == username => {
                info!(fork = %existing.full_name, "Fork already exists");
                return Ok(existing);
            }
            _ => {}
        }

        // Create fork
        self.github
            .fork_repository(&repo.owner, &repo.name)
            .await
    }

    /// Generate a natural-looking branch name.
    fn human_branch_name(contribution: &Contribution) -> String {
        let prefix = match contribution.contribution_type {
            ContributionType::SecurityFix => "fix/security",
            ContributionType::CodeQuality => "fix",
            ContributionType::DocsImprove => "docs",
            ContributionType::UiUxFix => "fix/ui",
            ContributionType::PerformanceOpt => "perf",
            ContributionType::FeatureAdd => "feat",
            ContributionType::Refactor => "refactor",
        };

        let lower = contribution.finding.title.to_lowercase();
        let slug = RE_SLUG.replace_all(&lower, "-");
        let slug = slug.trim_matches('-');
        let slug = crate::core::safe_truncate(slug, 50);

        format!("{}/{}", prefix, slug)
    }

    /// Generate PR description.
    fn generate_pr_body(&self, contribution: &Contribution) -> String {
        let finding = &contribution.finding;

        let files_list: String = contribution
            .changes
            .iter()
            .map(|c| {
                let tag = if c.is_new_file { "(new)" } else { "(modified)" };
                format!("- `{}` {}", c.path, tag)
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "## Problem\n\n\
             {}\n\n\
             **Severity**: `{}`\n\
             **File**: `{}`\n\n\
             ## Solution\n\n\
             {}\n\n\
             ## Changes\n\n\
             {}\n\n\
             ## Testing\n\n\
             - [ ] Existing tests pass\n\
             - [ ] Manual review completed\n\
             - [ ] No new warnings/errors introduced\n",
            finding.description,
            finding.severity,
            finding.file_path,
            finding
                .suggestion
                .as_deref()
                .unwrap_or(&contribution.description),
            files_list
        )
    }

    /// Check PR status.
    pub async fn get_pr_status(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
    ) -> Result<PrStatus> {
        let data = self.github.get_pr_details(owner, repo, pr_number).await?;

        let state = data["state"].as_str().unwrap_or("open");
        let merged = data["merged"].as_bool().unwrap_or(false);

        Ok(if merged {
            PrStatus::Merged
        } else if state == "closed" {
            PrStatus::Closed
        } else if data["requested_reviewers"].as_array().map_or(false, |r| !r.is_empty()) {
            PrStatus::ReviewRequested
        } else {
            PrStatus::Open
        })
    }

    // ── Auto Issue Creation ──────────────────────────────────────────────────

    /// Create an issue on the target repo describing the finding.
    ///
    /// Used before submitting a PR when the repository guidelines require an
    /// associated issue.  Returns the new issue number, or `None` if creation
    /// failed (non-fatal — the PR workflow continues without it).
    pub async fn create_issue_for_finding(
        &self,
        contribution: &Contribution,
        target_repo: &Repository,
    ) -> Option<i64> {
        let finding = &contribution.finding;

        let (prefix, label) = issue_type_meta(&finding.finding_type);

        // Build issue title using conventional-commit style.
        // Extract a scope from the file path (e.g. src/api/foo.rs → "api").
        let scope = extract_scope_from_file_path(&finding.file_path);
        let issue_title = if scope.is_empty() {
            format!("{}: {}", prefix, finding.title.to_lowercase())
        } else {
            format!("{}({}): {}", prefix, scope, finding.title.to_lowercase())
        };

        let issue_body = format!(
            "## Description\n\n\
             {}\n\n\
             **Severity**: `{}`\n\
             **File**: `{}`\n\n\
             ## Expected Behavior\n\n\
             The code should handle this case properly to avoid \
             unexpected errors or degraded quality.",
            finding.description, finding.severity, finding.file_path,
        );

        // Try with label first; fall back to no-label if the label is missing.
        let data = match self
            .github
            .create_issue_with_labels(
                &target_repo.owner,
                &target_repo.name,
                &issue_title,
                &issue_body,
                &[label],
            )
            .await
        {
            Ok(d) => d,
            Err(_) => {
                // Label may not exist on repo — retry without labels.
                match self
                    .github
                    .create_issue(
                        &target_repo.owner,
                        &target_repo.name,
                        &issue_title,
                        &issue_body,
                    )
                    .await
                {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(error = %e, "Failed to create issue for finding");
                        return None;
                    }
                }
            }
        };

        let issue_number = data["number"].as_i64().unwrap_or(0);
        info!(
            issue = issue_number,
            repo = %target_repo.full_name,
            title = %issue_title,
            "Created issue for finding"
        );
        Some(issue_number)
    }

    // ── Post-PR Compliance ───────────────────────────────────────────────────

    /// Check bot comments for compliance issues and auto-fix where possible.
    ///
    /// Handles:
    /// - PR title format (conventional commits)
    /// - Missing issue references (`needs:issue` / "no issue referenced")
    /// - CLA signing (CLAAssistant, EasyCLA)
    ///
    /// Returns `true` if the PR is compliant (or was successfully auto-fixed).
    pub async fn check_compliance_and_fix(
        &self,
        pr_result: &PrResult,
        contribution: &Contribution,
        guidelines: Option<&RepoGuidelines>,
    ) -> bool {
        let repo = &pr_result.repo;

        // Give bots time to post their compliance comments.
        tokio::time::sleep(std::time::Duration::from_secs(COMPLIANCE_BOT_WAIT_SECS)).await;

        let comments = match self
            .github
            .get_pr_comments(&repo.owner, &repo.name, pr_result.pr_number)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Could not fetch PR comments");
                return true; // Non-fatal — don't block the workflow.
            }
        };

        let mut bot_issues: Vec<String> = Vec::new();
        let mut cla_comments: Vec<serde_json::Value> = Vec::new();

        for comment in &comments {
            let user = &comment["user"];
            let login = user["login"].as_str().unwrap_or("");
            let body = comment["body"].as_str().unwrap_or("");
            let is_bot =
                user["type"].as_str() == Some("Bot") || login.ends_with("[bot]");

            if !is_bot {
                continue;
            }

            let body_lower = body.to_lowercase();
            let login_lower = login.to_lowercase();

            // Detect CLA bots by login or body keywords.
            if is_cla_bot(&login_lower, &body_lower) {
                cla_comments.push(comment.clone());
                continue;
            }

            // Detect compliance violations reported by other bots.
            if has_compliance_issue(&body_lower) {
                bot_issues.push(body.to_string());
            }
        }

        // Handle CLA signing first (independent of other compliance issues).
        if !cla_comments.is_empty() {
            self.handle_cla_signing(pr_result, &cla_comments).await;
        }

        if bot_issues.is_empty() {
            info!(pr = pr_result.pr_number, "PR passed compliance checks");
            return true;
        }

        info!(
            pr = pr_result.pr_number,
            count = bot_issues.len(),
            "PR has compliance issues, auto-fixing"
        );

        let all_comments = bot_issues.join(" ").to_lowercase();
        let mut fixed_anything = false;

        // ── Fix: PR title format ──
        if all_comments.contains("conventional commit") || all_comments.contains("needs:title") {
            if let Some(new_title) =
                self.build_compliant_title(contribution, guidelines)
            {
                match self
                    .github
                    .update_pull_request(
                        &repo.owner,
                        &repo.name,
                        pr_result.pr_number,
                        Some(&new_title),
                        None,
                    )
                    .await
                {
                    Ok(_) => {
                        info!(title = %new_title, "Fixed PR title");
                        fixed_anything = true;
                    }
                    Err(e) => warn!(error = %e, "Failed to fix PR title"),
                }
            }
        }

        // ── Fix: missing issue reference ──
        if all_comments.contains("no issue referenced") || all_comments.contains("needs:issue") {
            if let Some(issue_number) =
                self.create_issue_for_finding(contribution, &pr_result.repo).await
            {
                // Fetch current PR body and inject the issue link.
                match self
                    .github
                    .get_pr_details(&repo.owner, &repo.name, pr_result.pr_number)
                    .await
                {
                    Ok(pr_data) => {
                        let current_body =
                            pr_data["body"].as_str().unwrap_or("").to_string();
                        let new_body = inject_issue_link(&current_body, issue_number);

                        match self
                            .github
                            .update_pull_request(
                                &repo.owner,
                                &repo.name,
                                pr_result.pr_number,
                                None,
                                Some(&new_body),
                            )
                            .await
                        {
                            Ok(_) => {
                                info!(issue = issue_number, "Linked issue to PR");
                                fixed_anything = true;
                            }
                            Err(e) => warn!(error = %e, "Failed to link issue to PR"),
                        }
                    }
                    Err(e) => warn!(error = %e, "Failed to fetch PR body for issue linking"),
                }
            }
        }

        if fixed_anything {
            info!(pr = pr_result.pr_number, "PR compliance auto-fixed");
        } else {
            warn!(pr = pr_result.pr_number, "PR has unresolved compliance issues");
        }

        fixed_anything
    }

    // ── CLA Auto-signing ─────────────────────────────────────────────────────

    /// Auto-sign CLA when a CLA bot requests it.
    ///
    /// Supports CLAAssistant (posts magic comment) and logs a warning for
    /// EasyCLA (requires a web flow that cannot be automated).
    pub async fn handle_cla_signing(
        &self,
        pr_result: &PrResult,
        cla_comments: &[serde_json::Value],
    ) {
        let repo = &pr_result.repo;

        for comment in cla_comments {
            let login = comment["user"]["login"]
                .as_str()
                .unwrap_or("")
                .to_lowercase();
            let body_lower = comment["body"].as_str().unwrap_or("").to_lowercase();

            // CLAAssistant — post the magic signing comment.
            if login.contains("claassistant") || body_lower.contains("i have read the cla") {
                match self
                    .github
                    .create_pr_comment(
                        &repo.owner,
                        &repo.name,
                        pr_result.pr_number,
                        "I have read the CLA Document and I hereby sign the CLA",
                    )
                    .await
                {
                    Ok(_) => {
                        info!(pr = pr_result.pr_number, "Auto-signed CLA (CLAAssistant)");
                        return;
                    }
                    Err(e) => {
                        warn!(error = %e, pr = pr_result.pr_number, "CLA signing failed");
                    }
                }
            }

            // EasyCLA requires a web flow — flag for manual action.
            if login.contains("easycla") || login.contains("linux-foundation") {
                warn!(
                    pr = pr_result.pr_number,
                    "PR needs EasyCLA — manual signing required at the link in the bot comment"
                );
                return;
            }
        }

        info!(
            pr = pr_result.pr_number,
            "CLA bot detected but no actionable signing method found"
        );
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Build a compliant PR title, applying guidelines if available.
    ///
    /// Returns `None` if the existing title already looks conventional and no
    /// change is needed (avoids a no-op API call).
    fn build_compliant_title(
        &self,
        contribution: &Contribution,
        guidelines: Option<&RepoGuidelines>,
    ) -> Option<String> {
        let title = &contribution.title;

        // If the title already uses lowercase conventional-commit format, keep it.
        if is_conventional_commit_title(title) {
            return None;
        }

        // Default non-conventional prefixes the Python code detects.
        let default_prefixes = [
            "Security:", "Quality:", "Docs:", "UI/UX:",
            "Performance:", "Feature:", "Refactor:", "Fix:",
        ];

        if default_prefixes.iter().any(|p| title.starts_with(p)) {
            if let Some(g) = guidelines {
                if g.has_guidelines() {
                    let scope = crate::github::guidelines::extract_scope_from_path(
                        &contribution.finding.file_path,
                        g,
                    );
                    let new_title = crate::github::guidelines::adapt_pr_title(
                        &contribution.finding.title,
                        &contribution.finding.finding_type.to_string(),
                        g,
                        &scope,
                    );
                    return Some(new_title);
                }
            }
        }

        // No guidelines but title has a non-conventional prefix — convert it.
        if default_prefixes.iter().any(|p| title.starts_with(p)) {
            let (prefix, _label) = issue_type_meta(&contribution.finding.finding_type);
            return Some(format!(
                "{}: {}",
                prefix,
                contribution.finding.title.to_lowercase()
            ));
        }

        None
    }
}

// ── Pure helper functions (no I/O — fully unit-testable) ─────────────────────

/// Return `(commit_prefix, issue_label)` for a contribution type.
pub fn issue_type_meta(ct: &ContributionType) -> (&'static str, &'static str) {
    match ct {
        ContributionType::SecurityFix => ("fix", "bug"),
        ContributionType::CodeQuality => ("fix", "bug"),
        ContributionType::DocsImprove => ("docs", "documentation"),
        ContributionType::UiUxFix => ("fix", "bug"),
        ContributionType::PerformanceOpt => ("perf", "perf"),
        ContributionType::FeatureAdd => ("feat", "enhancement"),
        ContributionType::Refactor => ("refactor", "enhancement"),
    }
}

/// Extract a human-readable scope segment from a file path.
///
/// Mirrors the Python logic: `packages/X/…`, `apps/X/…`, `src/X/…` → X.
pub fn extract_scope_from_file_path(file_path: &str) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();
    if parts.len() >= 2 {
        let root = parts[0];
        if ["packages", "apps", "libs", "src"].contains(&root) {
            return parts[1].to_string();
        }
    }
    String::new()
}

/// Return `true` when a bot comment body indicates a compliance problem.
pub fn has_compliance_issue(body_lower: &str) -> bool {
    let keywords = [
        "doesn't follow conventional commit",
        "no issue referenced",
        "doesn't fully meet",
        "pr title",
        "needs:title",
        "needs:issue",
        "needs:compliance",
    ];
    keywords.iter().any(|kw| body_lower.contains(kw))
}

/// Return `true` when a login or body text matches a CLA bot.
pub fn is_cla_bot(login_lower: &str, body_lower: &str) -> bool {
    let login_keywords = ["cla", "easycla", "claassistant"];
    let body_keywords = [
        "contributor license agreement",
        "sign our cla",
        "cla not signed",
        "please sign",
        "i have read the cla",
    ];
    login_keywords.iter().any(|kw| login_lower.contains(kw))
        || body_keywords.iter().any(|kw| body_lower.contains(kw))
}

/// Return `true` if `title` already follows conventional commits format
/// (e.g. `fix: ...`, `feat(scope): ...`).
pub fn is_conventional_commit_title(title: &str) -> bool {
    // Pattern: type[(scope)]: description
    RE_CONVENTIONAL.is_match(title)
}

/// Inject `Closes #N` into a PR body, replacing placeholders or prepending.
pub fn inject_issue_link(body: &str, issue_number: i64) -> String {
    let closes = format!("Closes #{}", issue_number);

    // Replace "Closes N/A" placeholder.
    if body.contains("Closes N/A") {
        return body.replace("Closes N/A", &closes);
    }

    // Already present — nothing to do.
    if body.contains(&closes) {
        return body.to_string();
    }

    // Prepend.
    format!("{}\n\n{}", closes, body)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{FileChange, Finding, Severity};
    use chrono::Utc;

    fn test_contribution() -> Contribution {
        Contribution {
            finding: Finding {
                id: "1".into(),
                finding_type: ContributionType::SecurityFix,
                severity: Severity::High,
                title: "SQL injection vulnerability".into(),
                description: "Unsafe query construction".into(),
                file_path: "src/db.py".into(),
                line_start: Some(10),
                line_end: Some(15),
                suggestion: Some("Use parameterized queries".into()),
                confidence: 0.9,
                priority_signals: vec![],
            },
            contribution_type: ContributionType::SecurityFix,
            title: "fix: sql injection".into(),
            description: "Fixed unsafe query".into(),
            changes: vec![FileChange {
                path: "src/db.py".into(),
                original_content: None,
                new_content: "fixed code".into(),
                is_new_file: false,
                is_deleted: false,
            }],
            commit_message: "fix: sanitize sql queries".into(),
            tests_added: vec![],
            branch_name: String::new(),
            generated_at: Utc::now(),
        }
    }

    #[test]
    fn test_human_branch_name() {
        let c = test_contribution();
        let branch = PrManager::human_branch_name(&c);
        assert!(branch.starts_with("fix/security/"));
        assert!(branch.contains("sql-injection"));
    }

    #[test]
    fn test_build_signoff() {
        let user = serde_json::json!({
            "login": "testuser",
            "name": "Test User",
            "email": "test@example.com",
            "id": 12345
        });
        let signoff = PrManager::build_signoff(&user);
        assert_eq!(signoff, Some("Test User <test@example.com>".to_string()));
    }

    #[test]
    fn test_build_signoff_noreply() {
        let user = serde_json::json!({
            "login": "testuser",
            "name": "Test User",
            "id": 12345
        });
        let signoff = PrManager::build_signoff(&user);
        assert!(signoff.unwrap().contains("noreply.github.com"));
    }

    // ── issue_type_meta ──

    #[test]
    fn test_issue_type_meta_security() {
        let (prefix, label) = issue_type_meta(&ContributionType::SecurityFix);
        assert_eq!(prefix, "fix");
        assert_eq!(label, "bug");
    }

    #[test]
    fn test_issue_type_meta_docs() {
        let (prefix, label) = issue_type_meta(&ContributionType::DocsImprove);
        assert_eq!(prefix, "docs");
        assert_eq!(label, "documentation");
    }

    #[test]
    fn test_issue_type_meta_feature() {
        let (prefix, label) = issue_type_meta(&ContributionType::FeatureAdd);
        assert_eq!(prefix, "feat");
        assert_eq!(label, "enhancement");
    }

    // ── extract_scope_from_file_path ──

    #[test]
    fn test_extract_scope_src() {
        assert_eq!(extract_scope_from_file_path("src/api/handler.rs"), "api");
    }

    #[test]
    fn test_extract_scope_packages() {
        assert_eq!(
            extract_scope_from_file_path("packages/auth/index.ts"),
            "auth"
        );
    }

    #[test]
    fn test_extract_scope_no_match() {
        assert_eq!(extract_scope_from_file_path("main.rs"), "");
        assert_eq!(extract_scope_from_file_path(""), "");
    }

    #[test]
    fn test_extract_scope_libs() {
        assert_eq!(extract_scope_from_file_path("libs/core/mod.rs"), "core");
    }

    // ── has_compliance_issue ──

    #[test]
    fn test_has_compliance_issue_conventional_commit() {
        assert!(has_compliance_issue("doesn't follow conventional commit format"));
    }

    #[test]
    fn test_has_compliance_issue_no_issue() {
        assert!(has_compliance_issue("no issue referenced in this pr"));
    }

    #[test]
    fn test_has_compliance_issue_needs_title() {
        assert!(has_compliance_issue("needs:title needs:issue"));
    }

    #[test]
    fn test_has_compliance_issue_clean() {
        assert!(!has_compliance_issue("looks good to me!"));
    }

    // ── is_cla_bot ──

    #[test]
    fn test_is_cla_bot_login() {
        assert!(is_cla_bot("claassistant[bot]", ""));
        assert!(is_cla_bot("easycla[bot]", ""));
    }

    #[test]
    fn test_is_cla_bot_body() {
        assert!(is_cla_bot("somebot", "please sign our cla to continue"));
        assert!(is_cla_bot(
            "somebot",
            "contributor license agreement required"
        ));
    }

    #[test]
    fn test_is_cla_bot_false_positive_guard() {
        assert!(!is_cla_bot("dependabot[bot]", "bumped dependency version"));
    }

    // ── is_conventional_commit_title ──

    #[test]
    fn test_conventional_commit_valid() {
        assert!(is_conventional_commit_title("fix: correct null pointer"));
        assert!(is_conventional_commit_title("feat(auth): add oauth support"));
        assert!(is_conventional_commit_title("docs(readme): update setup"));
        assert!(is_conventional_commit_title("fix!: breaking change"));
    }

    #[test]
    fn test_conventional_commit_invalid() {
        assert!(!is_conventional_commit_title("Security: Fix SQL injection"));
        assert!(!is_conventional_commit_title("Fix some bug"));
        assert!(!is_conventional_commit_title(""));
    }

    // ── inject_issue_link ──

    #[test]
    fn test_inject_issue_link_placeholder() {
        let body = "Closes N/A\n\nSome details.";
        let result = inject_issue_link(body, 42);
        assert!(result.contains("Closes #42"));
        assert!(!result.contains("Closes N/A"));
    }

    #[test]
    fn test_inject_issue_link_prepend() {
        let body = "## Problem\n\nSomething is broken.";
        let result = inject_issue_link(body, 7);
        assert!(result.starts_with("Closes #7"));
    }

    #[test]
    fn test_inject_issue_link_already_present() {
        let body = "Closes #42\n\nDetails.";
        let result = inject_issue_link(body, 42);
        // Should not duplicate
        assert_eq!(result.matches("Closes #42").count(), 1);
    }

    // ── build_compliant_title (via PrManager) — pure logic test ──

    #[test]
    fn test_build_compliant_title_already_conventional() {
        // This tests the is_conventional_commit_title guard directly.
        assert!(is_conventional_commit_title("fix: sql injection vulnerability"));
        // PrManager::build_compliant_title would return None for this.
    }

    #[test]
    fn test_issue_title_construction_with_scope() {
        // Simulates what create_issue_for_finding builds.
        let scope = extract_scope_from_file_path("src/api/routes.py");
        let (prefix, _) = issue_type_meta(&ContributionType::CodeQuality);
        let title = format!("{}({}): {}", prefix, scope, "unused variable");
        assert_eq!(title, "fix(api): unused variable");
    }

    #[test]
    fn test_issue_title_construction_no_scope() {
        let scope = extract_scope_from_file_path("main.py");
        let (prefix, _) = issue_type_meta(&ContributionType::DocsImprove);
        let title = if scope.is_empty() {
            format!("{}: {}", prefix, "missing docstring")
        } else {
            format!("{}({}): {}", prefix, scope, "missing docstring")
        };
        assert_eq!(title, "docs: missing docstring");
    }
}
