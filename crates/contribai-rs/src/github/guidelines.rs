//! Fetch and parse repository contribution guidelines.
//!
//! Port from Python `github/guidelines.py`.
//! Reads CONTRIBUTING.md, PR templates, and adapts PR format.
//!
//! Enhanced in v6.4.1 (Sprint 22.5):
//! - CONTRIBUTAI_BLOCK file detection
//! - Enhanced anti-AI phrase detection (10+ patterns)

use regex::Regex;
use tracing::info;

use crate::github::client::GitHubClient;

/// Common PR template locations.
const PR_TEMPLATE_PATHS: &[&str] = &[
    ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/pull_request_template.md",
    "PULL_REQUEST_TEMPLATE.md",
    "pull_request_template.md",
    "docs/PULL_REQUEST_TEMPLATE.md",
    ".github/PULL_REQUEST_TEMPLATE/default.md",
];

const CONTRIBUTING_PATHS: &[&str] = &[
    "CONTRIBUTING.md",
    "contributing.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
];

/// Paths where maintainers can place a block file to opt-out of ContribAI.
const CONTRIBAI_BLOCK_PATHS: &[&str] = &[
    ".github/CONTRIBUTAI_BLOCK",
    "CONTRIBUTAI_BLOCK",
    ".github/CONTRIBUTAI.md",
    "CONTRIBUTAI.md",
];

/// Parsed contribution guidelines for a repository.
#[derive(Debug, Clone, Default)]
pub struct RepoGuidelines {
    pub contributing_md: String,
    pub pr_template: String,
    pub commit_format: String,
    pub commit_scopes: Vec<String>,
    pub pr_title_format: String,
    pub required_sections: Vec<String>,
    pub uses_conventional_commits: bool,
    pub uses_angular_commits: bool,
    pub requires_scope: bool,
    pub allowed_types: Vec<String>,
}

impl RepoGuidelines {
    pub fn has_guidelines(&self) -> bool {
        !self.contributing_md.is_empty() || !self.pr_template.is_empty()
    }
}

/// Fetch and parse contribution guidelines from a repo.
pub async fn fetch_repo_guidelines(
    github: &GitHubClient,
    owner: &str,
    repo: &str,
) -> RepoGuidelines {
    let mut guidelines = RepoGuidelines {
        commit_format: "default".into(),
        pr_title_format: "default".into(),
        ..Default::default()
    };

    // Fetch CONTRIBUTING.md
    for path in CONTRIBUTING_PATHS {
        match github.get_file_content(owner, repo, path, None).await {
            Ok(content) if !content.is_empty() => {
                guidelines.contributing_md = content;
                info!(owner, repo, path, "Found contributing guide");
                break;
            }
            _ => continue,
        }
    }

    // Fetch PR template
    for path in PR_TEMPLATE_PATHS {
        match github.get_file_content(owner, repo, path, None).await {
            Ok(content) if !content.is_empty() => {
                guidelines.pr_template = content;
                info!(owner, repo, path, "Found PR template");
                break;
            }
            _ => continue,
        }
    }

    // Parse conventions
    parse_commit_format(&mut guidelines);
    parse_pr_template_sections(&mut guidelines);

    if guidelines.has_guidelines() {
        info!(
            commit_format = %guidelines.commit_format,
            pr_title_format = %guidelines.pr_title_format,
            scopes = ?guidelines.commit_scopes,
            sections = guidelines.required_sections.len(),
            "Repo guidelines parsed"
        );
    }

    guidelines
}

fn parse_commit_format(guidelines: &mut RepoGuidelines) {
    let text = guidelines.contributing_md.to_lowercase();

    // Detect conventional commits
    let patterns = [
        r"conventional\s*commit",
        r"feat\s*[:(]",
        r"fix\s*[:(]",
        r"chore\s*[:(]",
        r"docs\s*[:(]",
        r"refactor\s*[:(]",
    ];
    let matches: usize = patterns
        .iter()
        .filter(|p| Regex::new(p).map(|re| re.is_match(&text)).unwrap_or(false))
        .count();

    if matches >= 2 {
        guidelines.uses_conventional_commits = true;
        guidelines.commit_format = "conventional".into();
        guidelines.pr_title_format = "conventional".into();
    }

    // Detect angular format (with scope)
    if let Ok(re) = Regex::new(r"feat\s*\(\s*\w+\s*\)") {
        if re.is_match(&text) {
            guidelines.uses_angular_commits = true;
            guidelines.commit_format = "angular".into();
            guidelines.pr_title_format = "conventional".into();
            guidelines.requires_scope = true;
        }
    }

    // Extract allowed types
    if let Ok(re) = Regex::new(
        r"(?m)^\s*[-*]\s*`?(feat|fix|docs|chore|refactor|test|perf|ci|style|build|revert)`?\b",
    ) {
        let mut types: Vec<String> = re
            .captures_iter(&text)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
        types.dedup();
        guidelines.allowed_types = types;
    }

    // Extract scopes
    if let Ok(re) = Regex::new(r"(?:feat|fix|docs|chore|refactor|test|perf)\((\w+)\)") {
        let mut scopes: Vec<String> = re
            .captures_iter(&guidelines.contributing_md)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
        scopes.dedup();
        guidelines.commit_scopes = scopes;
    }
}

fn parse_pr_template_sections(guidelines: &mut RepoGuidelines) {
    if guidelines.pr_template.is_empty() {
        return;
    }

    // Find markdown headers
    if let Ok(re) = Regex::new(r"(?m)^#{1,3}\s+(.+)$") {
        let headers: Vec<String> = re
            .captures_iter(&guidelines.pr_template)
            .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
            .collect();
        guidelines.required_sections = headers;
    }

    // Also check HTML comment sections
    if let Ok(re) = Regex::new(r"<!--\s*(.+?)\s*-->") {
        for cap in re.captures_iter(&guidelines.pr_template) {
            if let Some(section) = cap.get(1) {
                let s = section.as_str().trim().to_string();
                if !guidelines.required_sections.contains(&s) {
                    guidelines.required_sections.push(s);
                }
            }
        }
    }
}

/// Adapt PR title to match repo conventions.
pub fn adapt_pr_title(
    finding_title: &str,
    contribution_type: &str,
    guidelines: &RepoGuidelines,
    scope: &str,
) -> String {
    let type_map: std::collections::HashMap<&str, &str> = [
        ("security_fix", "fix"),
        ("code_quality", "refactor"),
        ("docs_improve", "docs"),
        ("ui_ux_fix", "fix"),
        ("performance_opt", "perf"),
        ("feature_add", "feat"),
        ("refactor", "refactor"),
    ]
    .into_iter()
    .collect();

    let cc_type = type_map.get(contribution_type).copied().unwrap_or("fix");

    if guidelines.uses_conventional_commits || guidelines.uses_angular_commits {
        let mut final_type = cc_type.to_string();
        if !guidelines.allowed_types.is_empty() && !guidelines.allowed_types.contains(&final_type) {
            final_type = if guidelines.allowed_types.contains(&"fix".to_string()) {
                "fix".into()
            } else {
                guidelines
                    .allowed_types
                    .first()
                    .cloned()
                    .unwrap_or("fix".into())
            };
        }

        if !scope.is_empty() {
            format!("{final_type}({scope}): {}", finding_title.to_lowercase())
        } else {
            format!("{final_type}: {}", finding_title.to_lowercase())
        }
    } else {
        let label = match contribution_type {
            "security_fix" => "Security",
            "code_quality" => "Quality",
            "docs_improve" => "Docs",
            "ui_ux_fix" => "UI/UX",
            "performance_opt" => "Performance",
            "feature_add" => "Feature",
            "refactor" => "Refactor",
            _ => "Fix",
        };
        format!("{label}: {finding_title}")
    }
}

/// Extract scope from file path.
pub fn extract_scope_from_path(file_path: &str, guidelines: &RepoGuidelines) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();

    // Match known scopes
    for part in &parts {
        if guidelines.commit_scopes.contains(&part.to_string()) {
            return part.to_string();
        }
    }

    // Infer from packages/X, apps/X, libs/X
    if parts.len() >= 2 && ["packages", "apps", "libs", "modules"].contains(&parts[0]) {
        return parts[1].to_string();
    }

    // Infer from src/X
    if parts.len() >= 2 && parts[0] == "src" {
        return parts[1].to_string();
    }

    // First meaningful directory
    for part in &parts[..parts.len().saturating_sub(1)] {
        if ![".", "..", "src", "lib", "app"].contains(part) {
            return part.to_string();
        }
    }

    String::new()
}

/// Generate ContribAI attribution footer.
pub fn contribai_attribution() -> String {
    "\n---\n\n\
     <details>\n\
     <summary>🤖 About this PR</summary>\n\n\
     This pull request was generated by \
     [ContribAI](https://github.com/tang-vu/ContribAI), an AI agent\n\
     that helps improve open source projects. The change was:\n\n\
     1. **Discovered** by automated code analysis\n\
     2. **Generated** by AI with context-aware code generation\n\
     3. **Self-reviewed** by AI quality checks\n\n\
     If you have questions or feedback about this PR, please comment below.\n\
     We appreciate your time reviewing this contribution!\n\n\
     </details>\n"
        .into()
}

// ── Anti-AI & Block Detection (Sprint 22.5) ────────────────────────────────

/// Check if a repository has a CONTRIBUTAI_BLOCK file.
///
/// Maintainers can place a file at any of these paths to opt-out:
/// - `.github/CONTRIBUTAI_BLOCK`
/// - `CONTRIBUTAI_BLOCK`
/// - `.github/CONTRIBUTAI.md`
/// - `CONTRIBUTAI.md`
///
/// Returns `true` if a block file exists.
pub async fn has_contribai_block(github: &GitHubClient, owner: &str, repo: &str) -> bool {
    for path in CONTRIBAI_BLOCK_PATHS {
        match github.get_file_content(owner, repo, path, None).await {
            Ok(content) if !content.is_empty() => {
                info!(owner, repo, path, "CONTRIBUTAI_BLOCK found — skipping repo");
                return true;
            }
            _ => continue,
        }
    }
    false
}

/// Check if contribution guidelines contain anti-AI contribution phrases.
///
/// Detects 15+ patterns that indicate the repo doesn't want automated/AI contributions:
/// - "no AI", "no automated", "no bot", "no generated"
/// - "manual contributions only"
/// - "no spam"
/// - "human only"
///
/// Returns `true` if an anti-AI phrase is detected.
pub fn detects_ai_ban(guidelines: &str) -> bool {
    let text = guidelines.to_lowercase();

    // 15 anti-AI/anti-automation patterns
    let patterns = [
        r"no\s+ai(\s|\.|,|!)?\s*(contrib|pr|submission|code|generated)",
        r"no\s+automated\s+(pr|contribution|submissions|pull\s*request|code)",
        r"no\s+bot\s+(contrib|pr|submission|pull\s*request)",
        r"no\s+ai[-\s]?generated",
        r"ai[-\s]?generated\s+content\s+(is\s+)?(not\s+)?(allowed|welcome|accepted)",
        r"manual\s+contributions?\s+only",
        r"human[-\s]?only\s+(contrib|pr|code)",
        r"no\s+spam\s+(pr|contribution|pull\s*request|automated)",
        r"no\s+(automated|auto|ai|bot|machine)\s+pull\s*requests?",
        r"automated\s+pr[s]?\s+are\s+(not\s+)?(allowed|welcome|accepted)",
        r"do\s+not\s+(accept|allow|welcome)\s+(ai|bot|automated|generated)",
        r"(we\s+)?(do\s+not\s+)?(accept|welcome)\s+(ai|bot|automated)\s+(contributions?|pr[s]?|pull\s*requests?)",
        r"no\s+(machine|automated|scripted|generated)\s+contributions?",
        r"(contributions?|pr[s]?)\s+must\s+be\s+(written|made|done)\s+by\s+humans?",
        r"no\s+(llm|gpt|claude|copilot|gemini)\s+(contrib|pr|generated|code)",
        // Catches "GPT generated code", "LLM-written code", etc. without requiring "no" prefix
        r"\b(llm|gpt|claude|copilot|gemini)[-\s]+(generated|written|created)\s+(code|contributions?|pr[s]?)",
        // Catches "only accept manual contributions" / "only allow human PRs"
        r"only\s+(accept|allow|welcome|approve)\s+(manual|human|hand[-\s]?written)",
    ];

    for pattern in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(&text) {
                info!(pattern, "AI contribution ban detected");
                return true;
            }
        }
    }

    // Also check for AI_POLICY.md ban content
    detect_ai_policy_ban(&text)
}

/// Check AI_POLICY.md content for ban keywords.
fn detect_ai_policy_ban(text: &str) -> bool {
    let lowered = text.to_lowercase();
    let ban_phrases = [
        "ai contributions are not allowed",
        "automated contributions prohibited",
        "no ai generated code",
        "ai submissions banned",
    ];

    for phrase in &ban_phrases {
        if lowered.contains(phrase) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapt_pr_title_conventional() {
        let g = RepoGuidelines {
            uses_conventional_commits: true,
            ..Default::default()
        };
        let title = adapt_pr_title("Add error handling", "security_fix", &g, "");
        assert_eq!(title, "fix: add error handling");
    }

    #[test]
    fn test_adapt_pr_title_with_scope() {
        let g = RepoGuidelines {
            uses_angular_commits: true,
            ..Default::default()
        };
        let title = adapt_pr_title("Add error handling", "security_fix", &g, "auth");
        assert_eq!(title, "fix(auth): add error handling");
    }

    #[test]
    fn test_adapt_pr_title_default() {
        let g = RepoGuidelines::default();
        let title = adapt_pr_title("Add error handling", "security_fix", &g, "");
        assert_eq!(title, "Security: Add error handling");
    }

    #[test]
    fn test_extract_scope_packages() {
        let g = RepoGuidelines::default();
        assert_eq!(
            extract_scope_from_path("packages/console/app.ts", &g),
            "console"
        );
    }

    #[test]
    fn test_extract_scope_src() {
        let g = RepoGuidelines::default();
        assert_eq!(extract_scope_from_path("src/utils/helper.py", &g), "utils");
    }

    #[test]
    fn test_extract_scope_known() {
        let g = RepoGuidelines {
            commit_scopes: vec!["auth".into()],
            ..Default::default()
        };
        assert_eq!(extract_scope_from_path("src/auth/login.py", &g), "auth");
    }

    #[test]
    fn test_parse_commit_format_conventional() {
        let mut g = RepoGuidelines {
            contributing_md: "We use conventional commits.\nExamples:\n- feat: add new feature\n- fix: fix bug\n- docs: update readme".into(),
            ..Default::default()
        };
        parse_commit_format(&mut g);
        assert!(g.uses_conventional_commits);
        assert_eq!(g.commit_format, "conventional");
    }

    #[test]
    fn test_parse_pr_template_sections() {
        let mut g = RepoGuidelines {
            pr_template: "## Description\n\n<!-- testing -->\n\n## Changes\n".into(),
            ..Default::default()
        };
        parse_pr_template_sections(&mut g);
        assert!(g.required_sections.contains(&"Description".to_string()));
        assert!(g.required_sections.contains(&"Changes".to_string()));
        assert!(g.required_sections.contains(&"testing".to_string()));
    }

    #[test]
    fn test_attribution() {
        let attr = contribai_attribution();
        assert!(attr.contains("ContribAI"));
        assert!(attr.contains("tang-vu"));
    }

    // ── Sprint 22.5: Anti-AI & Block Detection ─────────────────────────

    #[test]
    fn test_detects_ai_ban_no_ai() {
        let text = "We do not accept AI generated contributions.";
        assert!(detects_ai_ban(text));
    }

    #[test]
    fn test_detects_ai_ban_no_automated() {
        let text = "No automated PRs or bot submissions allowed.";
        assert!(detects_ai_ban(text));
    }

    #[test]
    fn test_detects_ai_ban_manual_only() {
        let text = "We only accept manual contributions from real humans.";
        assert!(detects_ai_ban(text));
    }

    #[test]
    fn test_detects_ai_ban_llm() {
        let text = "No LLM or GPT generated code please.";
        assert!(detects_ai_ban(text));
    }

    #[test]
    fn test_detects_ai_ban_welcome() {
        // Should NOT detect a ban in a welcoming message
        let text = "We welcome all contributions! Please read our guidelines.";
        assert!(!detects_ai_ban(text));
    }

    #[test]
    fn test_detects_ai_ban_normal_contributing() {
        // Normal contributing guide should not trigger
        let text = "Thank you for your interest in contributing! Please follow our code style and write tests.";
        assert!(!detects_ai_ban(text));
    }

    #[test]
    fn test_detects_ai_ban_conventional_commits() {
        // Conventional commits guide should not trigger
        let text = "We use conventional commits. Examples: feat: add feature, fix: fix bug.";
        assert!(!detects_ai_ban(text));
    }

    #[test]
    fn test_detect_ai_policy_ban() {
        assert!(detect_ai_policy_ban("AI contributions are not allowed"));
        assert!(!detect_ai_policy_ban("AI contributions are welcome"));
        assert!(detect_ai_policy_ban("automated contributions prohibited"));
    }
}
