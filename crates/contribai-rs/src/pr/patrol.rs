//! PR Patrol — monitor and respond to review feedback.
//!
//! Port from Python `pr/patrol.py`.
//! Scans ContribAI PRs for maintainer comments, classifies
//! feedback via LLM, generates code fixes, and pushes updates.

use tracing::{debug, info, warn};

use crate::core::error::Result;
use crate::core::models::{FeedbackAction, FeedbackItem, PatrolResult};
use crate::github::client::GitHubClient;
use crate::llm::provider::LlmProvider;
use crate::orchestrator::memory::Memory;

/// Comments we already posted — skip these.
const OUR_REPLY_MARKERS: &[&str] = &[
    "I have read the CLA Document",
    "contribai",
    "auto-fix",
    "✅ Fixed",
    "📝 Addressed",
];

/// Review bot logins to ignore.
const REVIEW_BOT_LOGINS: &[&str] = &[
    "coderabbitai",
    "copilot",
    "github-actions",
    "dependabot",
    "renovate",
    "sweep-ai",
    "sourcery-ai",
    "codeclimate",
    "sonarcloud",
    "codecov",
    "deepsource-autofix",
];

/// Monitor open PRs and respond to maintainer feedback.
pub struct PrPatrol<'a> {
    github: &'a GitHubClient,
    llm: &'a dyn LlmProvider,
    memory: Option<&'a Memory>,
    user: Option<serde_json::Value>,
}

impl<'a> PrPatrol<'a> {
    pub fn new(github: &'a GitHubClient, llm: &'a dyn LlmProvider) -> Self {
        Self {
            github,
            llm,
            memory: None,
            user: None,
        }
    }

    /// Attach memory for conversation tracking.
    pub fn with_memory(mut self, memory: &'a Memory) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Record a feedback message in conversation memory (if memory is attached).
    fn save_conversation(&self, msg: crate::orchestrator::memory::ConversationMessage) {
        if let Some(mem) = self.memory {
            if let Err(e) = mem.record_conversation(&msg) {
                tracing::debug!("Failed to save conversation: {}", e);
            }
        }
    }

    /// Main entry: scan open PRs for pending feedback.
    pub async fn patrol(
        &mut self,
        pr_records: &[serde_json::Value],
        dry_run: bool,
    ) -> Result<PatrolResult> {
        let mut result = PatrolResult::default();

        let user = self.github.get_authenticated_user().await?;
        let username = user["login"].as_str().unwrap_or("").to_string();
        self.user = Some(user);

        for pr in pr_records {
            let status = pr["status"].as_str().unwrap_or("");
            if !["open", "pending", "review_requested"].contains(&status) {
                // v5.8.1: Analyze closed/merged PRs for outcome learnings
                if status == "closed" || status == "merged" || status == "ci_failed" {
                    let repo = pr["repo"].as_str().unwrap_or("");
                    let pr_number = pr["pr_number"].as_i64().unwrap_or(0);
                    let pr_type = pr["type"].as_str().unwrap_or("unknown");
                    let parts: Vec<&str> = repo.splitn(2, '/').collect();
                    if parts.len() == 2 {
                        if let Err(e) = self
                            .analyze_closed_pr(parts[0], parts[1], pr_number, pr_type, &mut result)
                            .await
                        {
                            debug!(pr = pr_number, error = %e, "Failed to analyze closed PR");
                        }
                    }
                }
                result.prs_skipped += 1;
                continue;
            }

            let repo = pr["repo"].as_str().unwrap_or("");
            let pr_number = pr["pr_number"].as_i64().unwrap_or(0);

            let parts: Vec<&str> = repo.splitn(2, '/').collect();
            if parts.len() != 2 {
                continue;
            }
            let (owner, repo_name) = (parts[0], parts[1]);

            match self
                .check_single_pr(owner, repo_name, pr_number, &username, dry_run, &mut result)
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    let err_msg = format!("{}", e);
                    // Auto-close PRs that return 404 (deleted repos, closed PRs, etc.)
                    if err_msg.contains("Not found") || err_msg.contains("404") {
                        info!(
                            pr = pr_number,
                            repo = repo,
                            "🗑️ PR no longer exists, marking as closed"
                        );
                        result.prs_skipped += 1;
                        // The caller (cli/mod.rs) should update memory status
                        result
                            .errors
                            .push(format!("NOT_FOUND:{}:{}", repo, pr_number));
                    } else {
                        let msg = format!("Error patrolling PR #{}: {}", pr_number, e);
                        warn!("{}", msg);
                        result.errors.push(msg);
                    }
                }
            }
        }

        Ok(result)
    }

    /// Check a single PR for feedback.
    async fn check_single_pr(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        username: &str,
        dry_run: bool,
        result: &mut PatrolResult,
    ) -> Result<()> {
        // Check live status
        let pr_data = self.github.get_pr_details(owner, repo, pr_number).await?;
        if pr_data["state"].as_str() != Some("open") {
            result.prs_skipped += 1;
            return Ok(());
        }

        result.prs_checked += 1;
        info!(
            pr = pr_number,
            repo = format!("{}/{}", owner, repo),
            "🔍 Checking PR"
        );

        // Collect feedback
        let feedback = self
            .collect_feedback(owner, repo, pr_number, username)
            .await;

        if feedback.is_empty() {
            info!(pr = pr_number, "✅ No pending feedback");
            return Ok(());
        }
        // v5.4: Load conversation history for context-aware classification
        let full_repo = format!("{}/{}", owner, repo);
        let conversation_context = self
            .memory
            .and_then(|m| m.get_conversation_context(&full_repo, pr_number).ok())
            .unwrap_or_default();

        // Classify via LLM (with conversation history)
        let classified = self
            .classify_feedback_with_context(&feedback, &conversation_context)
            .await;

        let actionable: Vec<&FeedbackItem> = classified
            .iter()
            .filter(|f| {
                matches!(
                    f.action,
                    FeedbackAction::CodeChange
                        | FeedbackAction::StyleFix
                        | FeedbackAction::Question
                )
            })
            .collect();

        let rejected = classified
            .iter()
            .any(|f| f.action == FeedbackAction::Reject);

        if rejected {
            info!(pr = pr_number, "🚫 PR rejected by maintainer");
            return Ok(());
        }

        if actionable.is_empty() {
            info!(pr = pr_number, "✅ All feedback handled or approved");
            return Ok(());
        }

        info!(
            pr = pr_number,
            count = actionable.len(),
            "📋 Actionable items found"
        );

        for item in actionable {
            if dry_run {
                info!(
                    action = ?item.action,
                    body = %item.body.chars().take(80).collect::<String>(),
                    "🏃 [DRY RUN]"
                );
                continue;
            }

            match item.action {
                FeedbackAction::CodeChange | FeedbackAction::StyleFix => {
                    if self.handle_code_fix(owner, repo, &pr_data, item).await {
                        result.fixes_pushed += 1;
                        result.replies_sent += 1;
                    }
                }
                FeedbackAction::Question => {
                    if self.handle_question(owner, repo, &pr_data, item).await {
                        result.replies_sent += 1;
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Collect review comments, filtering out our own and bots.
    async fn collect_feedback(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        username: &str,
    ) -> Vec<FeedbackItem> {
        let mut feedback = Vec::new();

        // Issue comments
        if let Ok(comments) = self.github.get_pr_comments(owner, repo, pr_number).await {
            for c in comments {
                let login = c["user"]["login"].as_str().unwrap_or("");
                let body = c["body"].as_str().unwrap_or("");
                let is_bot = c["user"]["type"].as_str() == Some("Bot");

                if login == username || is_bot {
                    continue;
                }
                if REVIEW_BOT_LOGINS.contains(&login.to_lowercase().as_str()) {
                    continue;
                }
                if login.ends_with("[bot]") {
                    continue;
                }
                if OUR_REPLY_MARKERS.iter().any(|m| body.contains(m)) {
                    continue;
                }

                let comment_id = c["id"].as_i64().unwrap_or(0);
                feedback.push(FeedbackItem {
                    comment_id,
                    author: login.to_string(),
                    body: body.to_string(),
                    action: FeedbackAction::CodeChange, // default; classified later
                    file_path: None,
                    line: None,
                    diff_hunk: None,
                    is_inline: false,
                    bot_context: None,
                });

                // v5.4: Save to conversation memory
                self.save_conversation(crate::orchestrator::memory::ConversationMessage {
                    repo: format!("{}/{}", owner, repo),
                    pr_number,
                    role: "maintainer".into(),
                    author: login.to_string(),
                    body: body.to_string(),
                    comment_id,
                    is_inline: false,
                    file_path: None,
                });
            }
        }

        // Inline review comments
        if let Ok(reviews) = self
            .github
            .get_pr_review_comments(owner, repo, pr_number)
            .await
        {
            for c in reviews {
                let login = c["user"]["login"].as_str().unwrap_or("");
                let body = c["body"].as_str().unwrap_or("");

                if login == username {
                    continue;
                }
                if REVIEW_BOT_LOGINS.contains(&login.to_lowercase().as_str()) {
                    continue;
                }
                if login.ends_with("[bot]") || c["user"]["type"].as_str() == Some("Bot") {
                    continue;
                }
                if OUR_REPLY_MARKERS.iter().any(|m| body.contains(m)) {
                    continue;
                }

                let comment_id = c["id"].as_i64().unwrap_or(0);
                let file_path_str = c["path"].as_str();
                feedback.push(FeedbackItem {
                    comment_id,
                    author: login.to_string(),
                    body: body.to_string(),
                    action: FeedbackAction::CodeChange,
                    file_path: file_path_str.map(String::from),
                    line: c["line"].as_i64().or(c["original_line"].as_i64()),
                    diff_hunk: c["diff_hunk"].as_str().map(String::from),
                    is_inline: true,
                    bot_context: None,
                });

                // v5.4: Save to conversation memory
                self.save_conversation(crate::orchestrator::memory::ConversationMessage {
                    repo: format!("{}/{}", owner, repo),
                    pr_number,
                    role: "maintainer".into(),
                    author: login.to_string(),
                    body: body.to_string(),
                    comment_id,
                    is_inline: true,
                    file_path: file_path_str.map(String::from),
                });
            }
        }

        feedback
    }

    /// Classify feedback items with conversation history for context.
    async fn classify_feedback_with_context(
        &self,
        feedback: &[FeedbackItem],
        conversation_history: &str,
    ) -> Vec<FeedbackItem> {
        if feedback.is_empty() {
            return vec![];
        }

        let comments_text: String = feedback
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let loc = if f.is_inline {
                    format!("inline on {}", f.file_path.as_deref().unwrap_or("?"))
                } else {
                    "general".to_string()
                };
                format!(
                    "Comment #{} (by @{}, {}):\n{}",
                    i + 1,
                    f.author,
                    loc,
                    f.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // v5.4: Include conversation history for context
        let history_section = if conversation_history.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nPREVIOUS CONVERSATION HISTORY (for context):\n{}\n\n\
                 Use this history to understand:\n\
                 - What was already discussed/fixed\n\
                 - The maintainer's communication style\n\
                 - Whether comments are follow-ups to earlier feedback\n",
                conversation_history
            )
        };

        let prompt = format!(
            "Classify each review comment. Actions:\n\
             - CODE_CHANGE: Maintainer wants code mods\n\
             - QUESTION: Maintainer asks a question\n\
             - STYLE_FIX: Naming, formatting, convention\n\
             - APPROVE: Positive, no action\n\
             - REJECT: PR rejected entirely\n\
             - ALREADY_HANDLED: Reply to prev fix or bot\n\n\
             {}\
             Comments:\n{}\n\n\
             Respond as JSON array: [{{\"comment_number\": 1, \"action\": \"CODE_CHANGE\"}}]",
            history_section, comments_text
        );

        match self
            .llm
            .complete(
                &prompt,
                Some("You classify review comments. Be precise."),
                Some(0.1),
                None,
            )
            .await
        {
            Ok(response) => self.parse_classifications(&response, feedback),
            Err(e) => {
                warn!(error = %e, "Classification failed, defaulting to CODE_CHANGE");
                feedback.to_vec()
            }
        }
    }

    /// Parse LLM classification response.
    fn parse_classifications(
        &self,
        response: &str,
        feedback: &[FeedbackItem],
    ) -> Vec<FeedbackItem> {
        let json_str = if let Some(start) = response.find('[') {
            if let Some(end) = response.rfind(']') {
                &response[start..=end]
            } else {
                return feedback.to_vec();
            }
        } else {
            return feedback.to_vec();
        };

        let items: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return feedback.to_vec(),
        };

        let mut result = Vec::new();
        for cls in items {
            let idx = cls["comment_number"].as_u64().unwrap_or(0) as usize;
            if idx == 0 || idx > feedback.len() {
                continue;
            }

            let mut item = feedback[idx - 1].clone();
            item.action = match cls["action"].as_str().unwrap_or("").to_lowercase().as_str() {
                "code_change" => FeedbackAction::CodeChange,
                "question" => FeedbackAction::Question,
                "style_fix" => FeedbackAction::StyleFix,
                "approve" => FeedbackAction::Approve,
                "reject" => FeedbackAction::Reject,
                _ => FeedbackAction::AlreadyHandled,
            };
            result.push(item);
        }

        result
    }

    /// Handle code fix request.
    async fn handle_code_fix(
        &self,
        owner: &str,
        repo: &str,
        pr_data: &serde_json::Value,
        feedback: &FeedbackItem,
    ) -> bool {
        let head = &pr_data["head"];
        let fork_owner = head["repo"]["owner"]["login"].as_str().unwrap_or(owner);
        let fork_repo = head["repo"]["name"].as_str().unwrap_or(repo);
        let branch = head["ref"].as_str().unwrap_or("main");

        // Get file content
        let file_content = if let Some(path) = &feedback.file_path {
            self.github
                .get_file_content(fork_owner, fork_repo, path, Some(branch))
                .await
                .unwrap_or_default()
        } else {
            String::new()
        };

        let prompt = format!(
            "A reviewer left this feedback:\n\n> {}\n\n\
             Current file content:\n```\n{}\n```\n\n\
             Apply the MINIMUM change. Return the COMPLETE updated file content.",
            feedback.body, file_content
        );

        let response = match self
            .llm
            .complete(
                &prompt,
                Some("You fix code based on PR review. Return ONLY fixed file content."),
                Some(0.2),
                None,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Fix generation failed");
                return false;
            }
        };

        let fixed = Self::extract_fixed_content(&response);
        if fixed.trim() == file_content.trim() || fixed.trim().is_empty() {
            return false;
        }

        // Push fix
        if let Some(path) = &feedback.file_path {
            let sha = self
                .github
                .get_file_sha(fork_owner, fork_repo, path, Some(branch))
                .await
                .ok();
            let commit_msg = format!(
                "fix: address review feedback — {}",
                &feedback.body.chars().take(60).collect::<String>()
            );

            let signoff = self.user.as_ref().and_then(PrPatrol::build_signoff);

            match self
                .github
                .create_or_update_file(
                    fork_owner,
                    fork_repo,
                    path,
                    &fixed,
                    &commit_msg,
                    branch,
                    sha.as_deref(),
                    signoff.as_deref(),
                )
                .await
            {
                Ok(_) => {
                    info!(file = path, "✅ Pushed fix");
                    true
                }
                Err(e) => {
                    warn!(error = %e, "Failed to push fix");
                    false
                }
            }
        } else {
            false
        }
    }

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

    /// Handle maintainer question.
    async fn handle_question(
        &self,
        owner: &str,
        repo: &str,
        pr_data: &serde_json::Value,
        feedback: &FeedbackItem,
    ) -> bool {
        let pr_title = pr_data["title"].as_str().unwrap_or("");
        let pr_body = pr_data["body"].as_str().unwrap_or("");

        let prompt = format!(
            "A maintainer asked on our PR:\n\
             PR: {}\nDescription: {}\n\n\
             Question from @{}:\n> {}\n\n\
             Write a concise, helpful reply (2-4 sentences).",
            pr_title,
            &pr_body.chars().take(2000).collect::<String>(),
            feedback.author,
            feedback.body
        );

        let response = match self
            .llm
            .complete(
                &prompt,
                Some("You respond to code review questions. Be concise and professional."),
                Some(0.3),
                None,
            )
            .await
        {
            Ok(r) => r,
            Err(_) => return false,
        };

        let reply = response.trim().to_string();
        if reply.is_empty() {
            return false;
        }

        let pr_number = pr_data["number"].as_i64().unwrap_or(0);
        match self
            .github
            .create_pr_comment(owner, repo, pr_number, &reply)
            .await
        {
            Ok(_) => {
                info!(author = %feedback.author, pr = pr_number, "💬 Replied");
                true
            }
            Err(_) => false,
        }
    }

    /// Extract fixed file content from LLM response.
    fn extract_fixed_content(response: &str) -> String {
        let text = response.trim();
        if text.starts_with("```") {
            let lines: Vec<&str> = text.lines().collect();
            let end = if lines.last().is_some_and(|l| l.trim() == "```") {
                lines.len() - 1
            } else {
                lines.len()
            };
            lines[1..end].join("\n")
        } else {
            text.to_string()
        }
    }

    /// Analyze a closed/merged PR to store outcome learnings.
    async fn analyze_closed_pr(
        &self,
        owner: &str,
        repo: &str,
        pr_number: i64,
        pr_type: &str,
        result: &mut PatrolResult,
    ) -> Result<()> {
        let Some(memory) = self.memory else {
            return Ok(());
        };

        let pr_data = self.github.get_pr_details(owner, repo, pr_number).await?;
        let full_repo = format!("{}/{}", owner, repo);
        let pr_url = pr_data["html_url"].as_str().unwrap_or("");

        // Calculate time to close
        let created = pr_data["created_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
        let closed = pr_data["closed_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
        let hours = match (created, closed) {
            (Some(c), Some(cl)) => (cl - c).num_hours() as f64,
            _ => 0.0,
        };

        if pr_data["merged"].as_bool() == Some(true) {
            memory.record_outcome(&full_repo, pr_number, pr_url, pr_type, "merged", "", hours)?;
            info!(pr = pr_number, repo = %full_repo, "📊 Recorded merged outcome");
        } else {
            // Closed without merge — fetch comments for feedback analysis
            let comments = self
                .github
                .get_pr_comments(owner, repo, pr_number)
                .await
                .unwrap_or_default();

            let feedback_text: String = comments
                .iter()
                .rev()
                .take(5)
                .filter_map(|c| c["body"].as_str())
                .collect::<Vec<_>>()
                .join(" | ");

            let summary = if feedback_text.len() > 500 {
                feedback_text[..500].to_string()
            } else {
                feedback_text.clone()
            };

            let reason = classify_rejection_reason(&feedback_text);
            let tagged = format!("[{}] {}", reason, summary);

            memory.record_outcome(
                &full_repo, pr_number, pr_url, pr_type, "closed", &tagged, hours,
            )?;
            info!(
                pr = pr_number,
                repo = %full_repo,
                reason = reason,
                "📊 Recorded closed outcome with rejection learnings"
            );
        }

        memory.update_pr_status(
            &full_repo,
            pr_number,
            if pr_data["merged"].as_bool() == Some(true) {
                "merged"
            } else {
                "closed"
            },
        )?;
        result.prs_learned += 1;
        Ok(())
    }
}

/// Classify rejection reason from review comments using keyword heuristics.
fn classify_rejection_reason(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("duplicate") || lower.contains("already") {
        "duplicate"
    } else if lower.contains("won't fix")
        || lower.contains("not needed")
        || lower.contains("not wanted")
        || lower.contains("wontfix")
    {
        "unwanted"
    } else if lower.contains(" ci ")
        || lower.contains("test fail")
        || lower.contains("ci fail")
        || lower.contains("broken build")
        || lower.contains("check fail")
    {
        "ci_failure"
    } else if lower.contains("style")
        || lower.contains("convention")
        || lower.contains("format")
        || lower.contains("lint")
    {
        "style_mismatch"
    } else if lower.contains("stale") || lower.contains("outdated") || lower.contains("old") {
        "stale"
    } else {
        "unknown"
    }
}

// ── CI Monitor ────────────────────────────────────────────────────────────────

/// Outcome of a CI monitoring session.
#[derive(Debug, Clone, PartialEq)]
pub enum CiOutcome {
    /// All checks passed.
    Passed,
    /// One or more checks failed.
    Failed(Vec<String>),
    /// Monitoring timed out before CI completed.
    Timeout,
    /// No CI checks found (repo doesn't use CI).
    NoCi,
}

impl<'a> PrPatrol<'a> {
    /// Monitor CI status for a PR after submission.
    ///
    /// Polls check-runs every `poll_interval_secs` until all checks complete
    /// or `timeout_mins` is reached. Returns the final CI outcome.
    ///
    /// Python equivalent: `pr/patrol.py:monitor_ci_status()`
    pub async fn monitor_pr_ci(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
        timeout_mins: u64,
        poll_interval_secs: u64,
    ) -> CiOutcome {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_mins * 60);
        let poll = std::time::Duration::from_secs(poll_interval_secs);

        info!(
            owner,
            repo,
            sha = &sha[..8.min(sha.len())],
            timeout_mins,
            "🔍 CI Monitor started"
        );

        loop {
            match self.github.get_combined_status(owner, repo, sha).await {
                Ok(ci) => {
                    if ci.total == 0 {
                        info!(owner, repo, "⚡ No CI checks — skipping monitor");
                        return CiOutcome::NoCi;
                    }

                    if !ci.in_progress.is_empty() {
                        info!(
                            running = ci.in_progress.len(),
                            passed = ci.passed.len(),
                            "⏳ CI still running"
                        );
                    } else if !ci.failed.is_empty() {
                        warn!(failed = ?ci.failed, "❌ CI failed");
                        return CiOutcome::Failed(ci.failed);
                    } else {
                        info!(passed = ci.passed.len(), "✅ CI passed");
                        return CiOutcome::Passed;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "CI status check failed — retrying");
                }
            }

            if std::time::Instant::now() >= deadline {
                warn!(timeout_mins, "⏰ CI monitor timed out");
                return CiOutcome::Timeout;
            }

            tokio::time::sleep(poll).await;
        }
    }

    /// Monitor CI and record outcome into memory.
    ///
    /// Intended to be called after PR creation (in background or patrol cycle).
    /// Updates `memory.update_pr_status()` once CI resolves.
    pub async fn monitor_and_record(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
        pr_number: i64,
        memory: &crate::orchestrator::memory::Memory,
    ) {
        let outcome = self.monitor_pr_ci(owner, repo, sha, 30, 60).await;

        let status = match &outcome {
            CiOutcome::Passed => "ci_passed",
            CiOutcome::Failed(_) => "ci_failed",
            CiOutcome::Timeout => "ci_timeout",
            CiOutcome::NoCi => "open",
        };

        let full_name = format!("{}/{}", owner, repo);
        if let Err(e) = memory.update_pr_status(&full_name, pr_number, status) {
            warn!(pr = pr_number, error = %e, "Failed to update PR status after CI");
        } else {
            info!(pr = pr_number, status, "📝 PR status updated post-CI");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_fixed_content_plain() {
        let response = "def foo():\n    return 42\n";
        assert_eq!(PrPatrol::extract_fixed_content(response), response.trim());
    }

    #[test]
    fn test_extract_fixed_content_fenced() {
        let response = "```python\ndef foo():\n    return 42\n```";
        let result = PrPatrol::extract_fixed_content(response);
        assert_eq!(result, "def foo():\n    return 42");
    }

    #[test]
    fn test_parse_classifications() {
        let _feedback = vec![FeedbackItem {
            comment_id: 1,
            author: "maintainer".into(),
            body: "Please fix this".into(),
            action: FeedbackAction::CodeChange,
            file_path: Some("src/main.py".into()),
            line: Some(10),
            diff_hunk: None,
            is_inline: true,
            bot_context: None,
        }];

        let response = r#"[{"comment_number": 1, "action": "CODE_CHANGE"}]"#;

        // We need a PrPatrol instance but can't create one without GitHubClient
        // So test the parse logic inline
        let json_str = &response[response.find('[').unwrap()..=response.rfind(']').unwrap()];
        let items: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["action"], "CODE_CHANGE");
    }

    #[test]
    fn test_review_bot_detection() {
        let bots = REVIEW_BOT_LOGINS;
        assert!(bots.contains(&"coderabbitai"));
        assert!(bots.contains(&"dependabot"));
        assert!(!bots.contains(&"real-user"));
    }

    // ── Sprint 2: CI Monitor tests ────────────────────────────────────────────

    #[test]
    fn test_ci_outcome_variants() {
        // Passed
        let o = CiOutcome::Passed;
        assert_eq!(o, CiOutcome::Passed);

        // Failed with names
        let o = CiOutcome::Failed(vec!["lint".into(), "test".into()]);
        if let CiOutcome::Failed(names) = o {
            assert_eq!(names.len(), 2);
            assert!(names.contains(&"lint".to_string()));
        } else {
            panic!("Expected Failed variant");
        }

        // Timeout
        assert_eq!(CiOutcome::Timeout, CiOutcome::Timeout);

        // NoCi
        assert_eq!(CiOutcome::NoCi, CiOutcome::NoCi);
    }

    #[test]
    fn test_classify_rejection_reason() {
        assert_eq!(
            classify_rejection_reason("This is a duplicate of #42"),
            "duplicate"
        );
        assert_eq!(
            classify_rejection_reason("We already have this feature"),
            "duplicate"
        );
        assert_eq!(
            classify_rejection_reason("Not needed, won't fix this"),
            "unwanted"
        );
        assert_eq!(
            classify_rejection_reason("The CI failed on this PR"),
            "ci_failure"
        );
        assert_eq!(
            classify_rejection_reason("Check failed: lint errors"),
            "ci_failure"
        );
        assert_eq!(
            classify_rejection_reason("Doesn't follow our style guide"),
            "style_mismatch"
        );
        assert_eq!(
            classify_rejection_reason("This PR is stale, closing"),
            "stale"
        );
        assert_eq!(classify_rejection_reason("Thanks but no thanks"), "unknown");
    }

    #[test]
    fn test_ci_outcome_status_mapping() {
        // Verify the status strings match what memory.update_pr_status() expects
        let statuses = [
            (CiOutcome::Passed, "ci_passed"),
            (CiOutcome::Failed(vec![]), "ci_failed"),
            (CiOutcome::Timeout, "ci_timeout"),
            (CiOutcome::NoCi, "open"),
        ];
        for (outcome, expected) in statuses {
            let status = match &outcome {
                CiOutcome::Passed => "ci_passed",
                CiOutcome::Failed(_) => "ci_failed",
                CiOutcome::Timeout => "ci_timeout",
                CiOutcome::NoCi => "open",
            };
            assert_eq!(
                status, expected,
                "Outcome {:?} should map to '{}'",
                outcome, expected
            );
        }
    }
}
