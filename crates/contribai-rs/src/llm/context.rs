//! Context management for LLM calls.
//!
//! Port from Python `llm/context.py`.
//! Token estimation, context window chunking, prompt building.

use crate::core::models::FileNode;
use tracing::debug;

/// Rough token estimate: ~4 chars per token.
const CHARS_PER_TOKEN: usize = 4;

/// Budget tracker for context window.
#[derive(Debug)]
pub struct ContextBudget {
    pub max_tokens: usize,
    pub used_tokens: usize,
    pub sections: Vec<(String, usize)>,
}

impl ContextBudget {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            used_tokens: 0,
            sections: Vec::new(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.max_tokens.saturating_sub(self.used_tokens)
    }

    pub fn can_fit(&self, text: &str) -> bool {
        estimate_tokens(text) <= self.remaining()
    }

    pub fn add(&mut self, section_name: &str, text: &str) -> bool {
        let tokens = estimate_tokens(text);
        if tokens > self.remaining() {
            return false;
        }
        self.used_tokens += tokens;
        self.sections.push((section_name.to_string(), tokens));
        true
    }
}

/// Rough token estimate based on character count.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / CHARS_PER_TOKEN
}

/// Truncate text to fit within token budget.
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * CHARS_PER_TOKEN;
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}\n... [truncated]")
    }
}

/// Build a compact prompt summarizing repository context.
///
/// Priority: README > file tree > contributing guide > relevant files > coding style.
pub fn build_repo_context_prompt(
    repo_name: &str,
    language: &str,
    stars: u64,
    description: &str,
    readme: Option<&str>,
    file_tree: Option<&[String]>,
    contributing_guide: Option<&str>,
    relevant_files: Option<&[(&str, &str)]>,
    coding_style: Option<&str>,
    max_tokens: usize,
) -> String {
    let mut budget = ContextBudget::new(max_tokens);
    let mut parts: Vec<String> = Vec::new();

    // 1. Repo metadata (always included)
    let meta = format!(
        "## Repository: {repo_name}\n- Language: {language}\n- Stars: {stars}\n- Description: {description}\n"
    );
    budget.add("metadata", &meta);
    parts.push(meta);

    // 2. README (high priority)
    if let Some(readme) = readme {
        let readme_text = truncate_to_tokens(readme, 2000.min(budget.remaining()));
        if budget.add("readme", &readme_text) {
            parts.push(format!("## README\n{readme_text}"));
        }
    }

    // 3. File tree
    if let Some(tree) = file_tree {
        let tree_text: String = tree.iter().take(100).cloned().collect::<Vec<_>>().join("\n");
        let tree_text = truncate_to_tokens(&tree_text, 1000.min(budget.remaining()));
        if budget.add("file_tree", &tree_text) {
            parts.push(format!("## File Structure\n```\n{tree_text}\n```"));
        }
    }

    // 4. Contributing guide
    if let Some(guide) = contributing_guide {
        let guide_text = truncate_to_tokens(guide, 800.min(budget.remaining()));
        if budget.add("contributing", &guide_text) {
            parts.push(format!("## Contributing Guide\n{guide_text}"));
        }
    }

    // 5. Relevant files
    if let Some(files) = relevant_files {
        parts.push("## Relevant Source Files".into());
        for (path, content) in files {
            let truncated = truncate_to_tokens(content, 500.min(budget.remaining()));
            if budget.add(&format!("file:{path}"), &truncated) {
                parts.push(format!("### {path}\n```\n{truncated}\n```"));
            } else {
                break;
            }
        }
    }

    // 6. Coding style
    if let Some(style) = coding_style {
        if budget.can_fit(style) {
            budget.add("style", style);
            parts.push(format!("## Coding Conventions\n{style}"));
        }
    }

    debug!(
        tokens_used = budget.used_tokens,
        sections = budget.sections.len(),
        "Context built"
    );

    parts.join("\n\n")
}

/// Format file tree nodes into a readable string.
///
/// Port of Python `format_file_tree()` from `context.py`.
pub fn format_file_tree(nodes: &[FileNode], max_depth: usize) -> String {
    let mut sorted: Vec<&FileNode> = nodes.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let mut lines = Vec::new();
    for node in &sorted {
        let depth = node.path.matches('/').count();
        if depth > max_depth {
            continue;
        }
        let prefix = if node.node_type == "tree" { "D " } else { "F " };
        let indent = "  ".repeat(depth);
        let name = node.path.rsplit('/').next().unwrap_or(&node.path);
        lines.push(format!("{indent}{prefix}{name}"));
        if lines.len() >= 100 {
            break;
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello world"), 2); // 11 chars / 4
    }

    #[test]
    fn test_truncate_short() {
        let text = "short";
        assert_eq!(truncate_to_tokens(text, 100), "short");
    }

    #[test]
    fn test_truncate_long() {
        let text = "a".repeat(100);
        let result = truncate_to_tokens(&text, 5); // 20 chars max
        assert!(result.len() < 100);
        assert!(result.contains("[truncated]"));
    }

    #[test]
    fn test_context_budget() {
        let mut budget = ContextBudget::new(100);
        assert!(budget.add("test", "hello")); // 1 token
        assert_eq!(budget.remaining(), 99);
        assert_eq!(budget.sections.len(), 1);
    }

    #[test]
    fn test_context_budget_overflow() {
        let mut budget = ContextBudget::new(2);
        let long_text = "a".repeat(100); // 25 tokens
        assert!(!budget.add("big", &long_text));
    }

    #[test]
    fn test_build_context_prompt() {
        let prompt = build_repo_context_prompt(
            "owner/repo",
            "Python",
            100,
            "A test repo",
            Some("# README\nThis is a test"),
            None,
            None,
            None,
            None,
            6000,
        );
        assert!(prompt.contains("owner/repo"));
        assert!(prompt.contains("README"));
    }

    #[test]
    fn test_format_file_tree() {
        let nodes = vec![
            FileNode { path: "src".into(), node_type: "tree".into(), size: 0, sha: String::new() },
            FileNode { path: "src/main.rs".into(), node_type: "blob".into(), size: 100, sha: String::new() },
            FileNode { path: "src/lib.rs".into(), node_type: "blob".into(), size: 200, sha: String::new() },
            FileNode { path: "README.md".into(), node_type: "blob".into(), size: 50, sha: String::new() },
        ];
        let result = format_file_tree(&nodes, 3);
        assert!(result.contains("F README.md"));
        assert!(result.contains("D src"));
        assert!(result.contains("  F main.rs"));
    }

    #[test]
    fn test_format_file_tree_depth_limit() {
        let nodes = vec![
            FileNode { path: "a/b/c/d/deep.rs".into(), node_type: "blob".into(), size: 0, sha: String::new() },
        ];
        let result = format_file_tree(&nodes, 2);
        assert!(result.is_empty()); // depth 4 > max 2
    }

    #[test]
    fn test_build_context_with_files() {
        let files = vec![("src/main.py", "print('hello')")];
        let prompt = build_repo_context_prompt(
            "test/repo", "Python", 50, "desc",
            None, None, None,
            Some(&files),
            None, 6000,
        );
        assert!(prompt.contains("src/main.py"));
    }
}
