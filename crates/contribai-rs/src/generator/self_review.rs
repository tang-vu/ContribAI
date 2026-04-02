//! LLM self-review gate for generated contributions.

use tracing::{info, warn};

use crate::core::models::{Contribution, RepoContext};
use crate::core::safe_truncate;

use super::engine::ContributionGenerator;

impl ContributionGenerator<'_> {
    /// Have the LLM review the generated contribution and approve or reject it.
    ///
    /// Builds a unified diff for modified files and asks the LLM whether the
    /// change is correct. Defaults to `false` (rejected) on LLM failures to
    /// ensure a fail-closed posture.
    pub(crate) async fn self_review(
        &self,
        contribution: &Contribution,
        context: &RepoContext,
    ) -> bool {
        let changes_summary: String = contribution
            .changes
            .iter()
            .map(|c| {
                format!(
                    "- {} ({})\n",
                    c.path,
                    if c.is_new_file { "new" } else { "modified" }
                )
            })
            .collect();

        let mut prompt = format!(
            "Review the following code contribution for quality:\n\n\
             **Title**: {}\n\
             **Type**: {:?}\n\
             **Finding**: {}\n\
             **Changes**:\n{}\n\n\
             For each changed file:\n",
            contribution.title,
            contribution.contribution_type,
            contribution.finding.description,
            changes_summary
        );

        for change in contribution.changes.iter().take(5) {
            let original = context.relevant_files.get(&change.path);
            if let (Some(orig), false) = (original, change.is_new_file) {
                let diff = unified_diff(orig, &change.new_content, &change.path);
                let diff_snippet = safe_truncate(&diff, 4000);
                prompt.push_str(&format!(
                    "\n### {} (diff)\n```diff\n{}\n```\n",
                    change.path, diff_snippet
                ));
            } else {
                let snippet = safe_truncate(&change.new_content, 4000);
                prompt.push_str(&format!("\n### {}\n```\n{}\n```\n", change.path, snippet));
            }
        }

        prompt.push_str(
            "\nAnswer these questions:\n\
             1. Does the change address the described issue?\n\
             2. Does it introduce any obvious new bugs or security vulnerabilities?\n\
             3. Is the change reasonable and follows existing code style?\n\n\
             IMPORTANT: Be lenient. APPROVE if the change is a net improvement, \
             even if minor improvements could be made. Only REJECT if the change \
             is clearly wrong, introduces a bug, or is completely unrelated to the issue.\n\n\
             Reply with APPROVE or REJECT followed by brief reasoning.",
        );

        match self.llm.complete(&prompt, None, Some(0.1), None).await {
            Ok(response) => {
                let approved = response.to_uppercase().contains("APPROVE");
                if !approved {
                    info!(
                        preview = %&response[..response.len().min(200)],
                        "Self-review rejected"
                    );
                }
                approved
            }
            Err(e) => {
                warn!(error = %e, "Self-review LLM call failed, rejecting by default");
                false
            }
        }
    }
}

/// Build a simple diff string between two text blobs for LLM self-review.
///
/// Emits removed lines (prefixed `-`) and added lines (prefixed `+`).
/// This is sufficient for the LLM to understand the nature of the change.
pub fn unified_diff(original: &str, new_content: &str, path: &str) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let mut output = format!("--- a/{}\n+++ b/{}\n", path, path);

    let orig_set: std::collections::HashSet<&str> = orig_lines.iter().copied().collect();
    let new_set: std::collections::HashSet<&str> = new_lines.iter().copied().collect();

    for line in &orig_lines {
        if !new_set.contains(*line) {
            output.push_str(&format!("-{}\n", line));
        }
    }
    for line in &new_lines {
        if !orig_set.contains(*line) {
            output.push_str(&format!("+{}\n", line));
        }
    }

    output
}
