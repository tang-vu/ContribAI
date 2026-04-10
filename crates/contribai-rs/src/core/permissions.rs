//! Rule-based permission system for file/shell operations.
//!
//! Replaces binary sandbox on/off with granular, glob-pattern-based
//! access control. Each action type (file_read, file_edit, file_create,
//! shell_command, etc.) has its own rules.
//!
//! Rules are evaluated in order — first match wins.
//! Wildcard `"*"` matches everything.

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Permission action result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAction {
    /// Always allow without asking.
    Allow,
    /// Ask user for confirmation (interactive) or deny (non-interactive).
    Ask,
    /// Always deny.
    Deny,
}

impl std::fmt::Display for PermissionAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermissionAction::Allow => write!(f, "allow"),
            PermissionAction::Ask => write!(f, "ask"),
            PermissionAction::Deny => write!(f, "deny"),
        }
    }
}

/// A single permission rule: pattern → action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub pattern: String,
    pub action: PermissionAction,
}

impl PermissionRule {
    pub fn new(pattern: &str, action: PermissionAction) -> Self {
        Self {
            pattern: pattern.to_string(),
            action,
        }
    }

    /// Check if a path matches this rule's pattern.
    /// Supports glob-like patterns: `*`, `**`, `?`.
    pub fn matches(&self, path: &str) -> bool {
        glob_match(&self.pattern, path)
    }
}

/// Permission configuration for a specific action type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSet {
    pub rules: Vec<PermissionRule>,
}

impl PermissionSet {
    /// Create a new permission set with a single rule.
    pub fn single(action: PermissionAction) -> Self {
        Self {
            rules: vec![PermissionRule::new("*", action)],
        }
    }

    /// Evaluate the permission for a given path.
    /// Returns the action of the first matching rule.
    /// If no rule matches, defaults to Deny.
    pub fn evaluate(&self, path: &str) -> PermissionAction {
        for rule in &self.rules {
            if rule.matches(path) {
                debug!(pattern = %rule.pattern, action = %rule.action, path, "Permission rule matched");
                return rule.action;
            }
        }
        debug!(path, "No permission rule matched — defaulting to deny");
        PermissionAction::Deny
    }
}

/// Top-level permission configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionConfig {
    /// File read permissions.
    #[serde(default = "PermissionSet::allow_all")]
    pub file_read: PermissionSet,
    /// File edit permissions.
    #[serde(default = "PermissionSet::ask_all")]
    pub file_edit: PermissionSet,
    /// File create permissions.
    #[serde(default = "PermissionSet::ask_all")]
    pub file_create: PermissionSet,
    /// File delete permissions.
    #[serde(default = "PermissionSet::deny_all")]
    pub file_delete: PermissionSet,
    /// Shell command permissions.
    #[serde(default = "PermissionSet::deny_all")]
    pub shell_command: PermissionSet,
    /// PR creation permissions.
    #[serde(default = "PermissionSet::allow_all")]
    pub pr_create: PermissionSet,
}

impl PermissionSet {
    pub fn allow_all() -> Self {
        Self::single(PermissionAction::Allow)
    }
    pub fn ask_all() -> Self {
        Self::single(PermissionAction::Ask)
    }
    pub fn deny_all() -> Self {
        Self::single(PermissionAction::Deny)
    }
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            file_read: PermissionSet::allow_all(),
            file_edit: PermissionSet::ask_all(),
            file_create: PermissionSet::ask_all(),
            file_delete: PermissionSet::deny_all(),
            shell_command: PermissionSet::deny_all(),
            pr_create: PermissionSet::allow_all(),
        }
    }
}

/// Simple glob matching — supports `*`, `**`, `?`.
fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    if pattern == path {
        return true;
    }

    // Handle `**` — matches any path segment(s)
    if pattern.contains("**") {
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0].trim_end_matches('/');
            let suffix_pattern = parts[1].trim_start_matches('/');
            // Check prefix match
            if !prefix.is_empty() && !path.starts_with(&format!("{}/", prefix)) && path != prefix {
                return false;
            }
            // Check suffix with glob matching
            if !suffix_pattern.is_empty() {
                let suffix_regex = format!(
                    "^{}$",
                    suffix_pattern.replace('*', "[^/]*").replace('?', ".")
                );
                // Try matching against full path and just filename
                let full_match = regex::Regex::new(&suffix_regex)
                    .ok()
                    .map(|r| r.is_match(path))
                    .unwrap_or(false);
                let file_match = path
                    .rsplit('/')
                    .next()
                    .and_then(|f| regex::Regex::new(&suffix_regex).ok().map(|r| r.is_match(f)))
                    .unwrap_or(false);
                if !full_match && !file_match {
                    return false;
                }
            }
            return true;
        }
    }

    // Convert glob pattern to regex
    let regex_pattern = format!("^{}$", pattern.replace('*', "[^/]*").replace('?', "."));

    match regex::Regex::new(&regex_pattern) {
        Ok(re) => re.is_match(path),
        Err(_) => pattern == path,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_star_matches_anything() {
        assert!(glob_match("*", "any/path/file.rs"));
        assert!(glob_match("**", "any/deep/path/file.rs"));
    }

    #[test]
    fn test_glob_double_star_matches_nested() {
        assert!(glob_match("src/**/*.rs", "src/main.rs"));
        assert!(glob_match("src/**/*.rs", "src/sub/dir/file.rs"));
        assert!(!glob_match("src/**/*.rs", "tests/main.rs"));
    }

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_match("Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("Cargo.toml", "Cargo.lock"));
    }

    #[test]
    fn test_glob_question_mark() {
        assert!(glob_match("file?.txt", "file1.txt"));
        assert!(glob_match("file?.txt", "fileX.txt"));
        assert!(!glob_match("file?.txt", "file12.txt"));
    }

    #[test]
    fn test_permission_set_evaluate() {
        let set = PermissionSet {
            rules: vec![
                PermissionRule::new("src/**/*.rs", PermissionAction::Allow),
                PermissionRule::new("tests/**", PermissionAction::Deny),
                PermissionRule::new("*", PermissionAction::Ask),
            ],
        };

        assert_eq!(set.evaluate("src/main.rs"), PermissionAction::Allow);
        assert_eq!(set.evaluate("tests/unit.rs"), PermissionAction::Deny);
        assert_eq!(set.evaluate("Cargo.toml"), PermissionAction::Ask);
        assert_eq!(set.evaluate("README.md"), PermissionAction::Ask);
    }

    #[test]
    fn test_permission_config_defaults() {
        let config = PermissionConfig::default();
        assert_eq!(
            config.file_read.evaluate("any/file.rs"),
            PermissionAction::Allow
        );
        assert_eq!(
            config.file_edit.evaluate("src/main.rs"),
            PermissionAction::Ask
        );
        assert_eq!(
            config.file_delete.evaluate("src/main.rs"),
            PermissionAction::Deny
        );
    }
}
