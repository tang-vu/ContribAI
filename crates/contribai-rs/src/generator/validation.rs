//! Validation of LLM-generated file changes.

use std::collections::HashMap;

use tracing::debug;

use crate::core::models::FileChange;

use super::engine::ContributionGenerator;

impl ContributionGenerator<'_> {
    /// Validate generated changes for basic sanity.
    ///
    /// Checks:
    /// - Non-empty content for new files
    /// - No-op detection (original == new)
    /// - Balanced brackets (string/comment-aware)
    pub(crate) fn validate_changes(&self, changes: &[FileChange]) -> bool {
        if changes.is_empty() {
            return false;
        }

        for change in changes {
            let content = &change.new_content;

            // New file must have non-trivial content
            if change.is_new_file && content.trim().len() < 10 {
                debug!(
                    path = %change.path,
                    len = content.trim().len(),
                    "Validation: new file content too short"
                );
                return false;
            }

            // Detect no-op edits
            if let Some(orig) = &change.original_content {
                if content == orig {
                    debug!(path = %change.path, "Validation: new_content identical to original (no-op)");
                    return false;
                }
            }

            // Balanced bracket check (string/comment-aware)
            if !content.is_empty() {
                let unbalanced = Self::count_unbalanced_brackets(content);
                if unbalanced > 5 {
                    debug!(
                        path = %change.path,
                        unbalanced = unbalanced,
                        "Validation: too many unbalanced brackets"
                    );
                    return false;
                }
            }
        }

        true
    }

    /// Count unbalanced brackets, ignoring those inside strings and comments.
    ///
    /// Handles:
    /// - Single-line comments: `#` (Python) and `//` (C-like)
    /// - Block comments: `/* ... */`
    /// - String literals delimited by `"` or `'` (with backslash-escape tracking)
    pub fn count_unbalanced_brackets(code: &str) -> usize {
        let open_to_close: HashMap<char, char> =
            [('(', ')'), ('[', ']'), ('{', '}')].into_iter().collect();
        let closers: std::collections::HashSet<char> = [')', ']', '}'].into_iter().collect();

        let mut stack: Vec<char> = Vec::new();
        let mut in_string: Option<char> = None; // current quote character
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let chars: Vec<char> = code.chars().collect();
        let n = chars.len();
        let mut i = 0;

        while i < n {
            let ch = chars[i];
            let next = chars.get(i + 1).copied();

            // Newline resets line comments
            if ch == '\n' {
                in_line_comment = false;
                i += 1;
                continue;
            }

            // Skip chars inside line comments
            if in_line_comment {
                i += 1;
                continue;
            }

            // Handle block comment end
            if in_block_comment {
                if ch == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2; // consume "*/"
                    continue;
                }
                i += 1;
                continue;
            }

            // Handle block comment start (outside strings)
            if in_string.is_none() && ch == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }

            // Handle line comment start: `#` or `//`
            if in_string.is_none() {
                if ch == '#' {
                    in_line_comment = true;
                    i += 1;
                    continue;
                }
                if ch == '/' && next == Some('/') {
                    in_line_comment = true;
                    i += 2;
                    continue;
                }
            }

            // Handle string boundaries (skip escaped quotes)
            if (ch == '"' || ch == '\'') && (i == 0 || chars[i - 1] != '\\') {
                match in_string {
                    None => in_string = Some(ch),
                    Some(q) if q == ch => in_string = None,
                    _ => {} // inside a different-quote string
                }
                i += 1;
                continue;
            }

            // Skip chars inside strings
            if in_string.is_some() {
                i += 1;
                continue;
            }

            // Count brackets
            if let Some(&close) = open_to_close.get(&ch) {
                stack.push(close);
            } else if closers.contains(&ch) {
                if stack.last() == Some(&ch) {
                    stack.pop();
                }
            }

            i += 1;
        }

        stack.len()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::engine::tests::mock_gen;

    #[test]
    fn test_count_unbalanced_balanced() {
        let code = "fn foo() { let x = (1 + 2); }";
        assert_eq!(ContributionGenerator::count_unbalanced_brackets(code), 0);
    }

    #[test]
    fn test_count_unbalanced_simple_imbalance() {
        // missing `)` and `}`
        let code = "fn foo() { let x = (1 + 2;";
        assert_eq!(ContributionGenerator::count_unbalanced_brackets(code), 2);
    }

    #[test]
    fn test_count_unbalanced_ignores_string() {
        // Brackets inside string literals must be ignored
        let code = r#"let s = "hello { world }"; fn foo() {}"#;
        assert_eq!(ContributionGenerator::count_unbalanced_brackets(code), 0);
    }

    #[test]
    fn test_count_unbalanced_ignores_line_comment_slash() {
        let code = "let x = 1; // unmatched { bracket\nlet y = 2;";
        assert_eq!(ContributionGenerator::count_unbalanced_brackets(code), 0);
    }

    #[test]
    fn test_count_unbalanced_ignores_line_comment_hash() {
        let code = "x = 1  # unmatched { bracket\ny = 2";
        assert_eq!(ContributionGenerator::count_unbalanced_brackets(code), 0);
    }

    #[test]
    fn test_count_unbalanced_ignores_block_comment() {
        let code = "let x = 1; /* unmatched { */ let y = 2;";
        assert_eq!(ContributionGenerator::count_unbalanced_brackets(code), 0);
    }

    #[test]
    fn test_validate_changes_good() {
        let gen = mock_gen();
        let good = vec![FileChange {
            path: "test.py".into(),
            original_content: None,
            new_content: "def foo():\n    return 42\n".into(),
            is_new_file: false,
            is_deleted: false,
        }];
        assert!(gen.validate_changes(&good));
    }

    #[test]
    fn test_validate_changes_noop() {
        let gen = mock_gen();
        let noop = vec![FileChange {
            path: "test.py".into(),
            original_content: Some("same content".into()),
            new_content: "same content".into(),
            is_new_file: false,
            is_deleted: false,
        }];
        assert!(!gen.validate_changes(&noop));
    }

    #[test]
    fn test_validate_changes_new_file_empty() {
        let gen = mock_gen();
        let empty_new = vec![FileChange {
            path: "test.py".into(),
            original_content: None,
            new_content: "   ".into(),
            is_new_file: true,
            is_deleted: false,
        }];
        assert!(!gen.validate_changes(&empty_new));
    }
}
