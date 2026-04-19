//! JSON extraction and change-parsing from LLM responses.

use regex::Regex;
use tracing::{info, warn};

use crate::core::models::{FileChange, RepoContext};

use super::engine::ContributionGenerator;
use super::fuzzy_match::apply_single_edit;

// ── JSON extraction ──────────────────────────────────────────────────────────

impl ContributionGenerator<'_> {
    /// Robustly extract JSON from LLM response.
    ///
    /// Three strategies (matching Python `_extract_json`):
    /// 1. Extract from ` ```json ` fences
    /// 2. Plain ` ``` ` fences containing a JSON object
    /// 3. Bracket-counting fallback from first `{"changes"`, `{`, or `[`
    pub fn extract_json(response: &str) -> Option<String> {
        // Strategy 1: ```json ... ``` fenced blocks
        if let Ok(re) = Regex::new(r"(?s)```json\s*\n(.*?)\n\s*```") {
            if let Some(cap) = re.captures(response) {
                if let Some(m) = cap.get(1) {
                    return Some(m.as_str().trim().to_string());
                }
            }
        }

        // Strategy 2: plain ``` ... ``` fence containing a JSON object
        if let Ok(re) = Regex::new(r"(?s)```\s*\n(\{.*?\})\s*\n\s*```") {
            if let Some(cap) = re.captures(response) {
                if let Some(m) = cap.get(1) {
                    return Some(m.as_str().trim().to_string());
                }
            }
        }

        // Strategy 3: bracket-counting — prefer `{"changes"` anchor, then first `[` or `{`
        // whichever comes first in the text (so bare arrays are not missed).
        let start = response.find(r#"{"changes""#).or_else(|| {
            let brace = response.find('{');
            let bracket = response.find('[');
            match (brace, bracket) {
                (Some(b), Some(k)) => Some(b.min(k)),
                (Some(b), None) => Some(b),
                (None, Some(k)) => Some(k),
                _ => None,
            }
        });

        let start = start?;

        let open_ch = response.as_bytes().get(start).copied()? as char;
        let close_ch = if open_ch == '{' { '}' } else { ']' };

        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut prev_ch = '\0';

        for (i, ch) in response[start..].char_indices() {
            if ch == '"' && prev_ch != '\\' {
                in_string = !in_string;
            }
            if in_string {
                prev_ch = ch;
                continue;
            }
            if ch == open_ch {
                depth += 1;
            } else if ch == close_ch {
                depth -= 1;
                if depth == 0 {
                    let end = start + i + ch.len_utf8();
                    return Some(response[start..end].to_string());
                }
            }
            prev_ch = ch;
        }

        None
    }

    // ── Change parsing ───────────────────────────────────────────────────────

    /// Parse LLM response into FileChange objects.
    ///
    /// Supports two formats (matching Python engine):
    /// 1. `{"changes": [{"path":..., "edits": [{"search":..., "replace":...}]}]}`
    ///    — search/replace applied to original file content from `context`
    /// 2. `{"changes": [{"path":..., "content":..., "is_new_file": true}]}`
    ///    — full content for new files
    ///
    /// Falls back to bare JSON array `[{"path":..., "new_content":...}]` for
    /// backward compatibility.
    pub(crate) fn parse_changes(
        &self,
        response: &str,
        context: &RepoContext,
    ) -> Option<Vec<FileChange>> {
        let json_text = Self::extract_json(response)?;

        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_text) {
            // Try canonical `{"changes": [...]}` wrapper format first
            if let Some(raw_changes) = data.get("changes").and_then(|v| v.as_array()) {
                let changes = self.apply_changes_from_json(raw_changes, context);
                if !changes.is_empty() {
                    return Some(changes);
                }
            }

            // Bare array fallback: [{path, new_content, is_new_file}]
            if let Some(items) = data.as_array() {
                let changes = Self::parse_bare_array(items);
                if !changes.is_empty() {
                    return Some(changes);
                }
            }
        }

        None
    }

    /// Apply search/replace edits or full-content changes from JSON items.
    fn apply_changes_from_json(
        &self,
        items: &[serde_json::Value],
        context: &RepoContext,
    ) -> Vec<FileChange> {
        let mut changes: Vec<FileChange> = Vec::new();

        for item in items {
            let path = match item.get("path").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => continue,
            };

            if let Some(edits) = item.get("edits").and_then(|v| v.as_array()) {
                // Search/replace mode — requires original file content
                let original = match context.relevant_files.get(&path) {
                    Some(c) => c.clone(),
                    None => {
                        warn!(path = %path, "No original content for search/replace edits");
                        continue;
                    }
                };

                let mut new_content = original.clone();
                let edits_total = edits.len();
                let mut edits_applied: usize = 0;

                for edit in edits {
                    let search = edit
                        .get("search")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let replace = edit
                        .get("replace")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if search.is_empty() {
                        continue;
                    }

                    if let Some(updated) = apply_single_edit(&new_content, &search, &replace, &path)
                    {
                        new_content = updated;
                        edits_applied += 1;
                    } else {
                        warn!(
                            path = %path,
                            search_len = search.len(),
                            search_preview = %&search[..search.len().min(80)].replace('\n', "\\n"),
                            "Search text not found (tried exact + 3 fuzzy strategies)"
                        );
                    }
                }

                info!(
                    path = %path,
                    applied = edits_applied,
                    total = edits_total,
                    "Edits applied"
                );

                if edits_applied == 0 {
                    warn!(path = %path, "No edits applied, skipping file");
                    continue;
                }

                changes.push(FileChange {
                    path,
                    original_content: Some(original),
                    new_content,
                    is_new_file: false,
                    is_deleted: false,
                });
            } else if let Some(content) = item.get("content").and_then(|v| v.as_str()) {
                // Full-content mode (new files or fallback)
                changes.push(FileChange {
                    path,
                    original_content: None,
                    new_content: content.to_string(),
                    is_new_file: true,
                    is_deleted: false,
                });
            }
        }

        // Enforce max files limit
        let max = self.config.max_changes_per_pr;
        if changes.len() > max {
            warn!(
                actual = changes.len(),
                limit = max,
                "Too many files changed, truncating"
            );
            changes.truncate(max);
        }

        changes
    }

    /// Parse legacy bare-array format `[{"path":..., "new_content":..., "is_new_file":...}]`.
    fn parse_bare_array(items: &[serde_json::Value]) -> Vec<FileChange> {
        items
            .iter()
            .filter_map(|item| {
                let path = item.get("path")?.as_str()?.to_string();
                let new_content = item.get("new_content")?.as_str()?.to_string();
                let is_new_file = item
                    .get("is_new_file")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Some(FileChange {
                    path,
                    original_content: None,
                    new_content,
                    is_new_file,
                    is_deleted: false,
                })
            })
            .collect()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::generator::engine::tests::{mock_gen, test_context};

    #[test]
    fn test_extract_json_fenced() {
        let response = "some text\n```json\n{\"changes\": []}\n```\ntrailing text";
        let result = ContributionGenerator::extract_json(response);
        assert_eq!(result, Some("{\"changes\": []}".to_string()));
    }

    #[test]
    fn test_extract_json_raw() {
        let response = r#"Here is the fix: {"changes": [{"path": "x.py"}]}"#;
        let result = ContributionGenerator::extract_json(response);
        assert!(result.is_some());
        assert!(result.unwrap().contains("changes"));
    }

    #[test]
    fn test_extract_json_bare_array() {
        let response = r#"[{"path": "x.py", "new_content": "hello"}]"#;
        let result = ContributionGenerator::extract_json(response);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with('['));
    }

    #[test]
    fn test_extract_json_none() {
        let result = ContributionGenerator::extract_json("no json here at all");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_changes_valid() {
        let gen = mock_gen();
        let ctx = test_context(HashMap::new());
        let response =
            r#"[{"path": "src/main.py", "new_content": "print('fixed')", "is_new_file": false}]"#;
        let changes = gen.parse_changes(response, &ctx);
        assert!(changes.is_some());
        assert_eq!(changes.unwrap().len(), 1);
    }

    #[test]
    fn test_parse_changes_invalid() {
        let gen = mock_gen();
        let ctx = test_context(HashMap::new());
        let response = "This is not valid JSON at all";
        let changes = gen.parse_changes(response, &ctx);
        assert!(changes.is_none());
    }

    #[test]
    fn test_parse_changes_search_replace() {
        let gen = mock_gen();
        let mut files = HashMap::new();
        files.insert(
            "src/main.py".to_string(),
            "def foo():\n    x = 1\n    return x\n".to_string(),
        );
        let ctx = test_context(files);

        let response = r#"{"changes": [{"path": "src/main.py", "is_new_file": false, "edits": [{"search": "x = 1", "replace": "x = 2"}]}]}"#;
        let changes = gen.parse_changes(response, &ctx);
        assert!(changes.is_some());
        let ch = changes.unwrap();
        assert_eq!(ch.len(), 1);
        assert!(ch[0].new_content.contains("x = 2"));
        assert!(!ch[0].new_content.contains("x = 1"));
    }

    #[test]
    fn test_validate_json_schema() {
        // Valid change schema
        let valid = r#"{"changes": [{"path": "test.py", "is_new_file": false, "edits": [{"search": "old", "replace": "new"}]}]}"#;
        let parsed: serde_json::Value = serde_json::from_str(valid).unwrap();
        assert!(validate_change_schema(&parsed));

        // Missing required field
        let invalid = r#"{"changes": [{"path": "test.py"}]}"#;
        let parsed: serde_json::Value = serde_json::from_str(invalid).unwrap();
        assert!(!validate_change_schema(&parsed));

        // Wrong type
        let invalid2 = r#"{"changes": "not an array"}"#;
        let parsed: serde_json::Value = serde_json::from_str(invalid2).unwrap();
        assert!(!validate_change_schema(&parsed));
    }
}

// ── JSON Schema Validation (Sprint 22) ──────────────────────────────────────

/// Validate that parsed JSON matches the expected change schema.
///
/// Required structure:
/// ```json
/// {
///   "changes": [
///     {
///       "path": "string",
///       "is_new_file": boolean,
///       "edits": [{"search": "string", "replace": "string"}]  // for existing files
///       // OR
///       "content": "string"  // for new files
///     }
///   ]
/// }
/// ```
pub fn validate_change_schema(value: &serde_json::Value) -> bool {
    // Must have "changes" key with array value
    let changes = match value.get("changes").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return false,
    };

    if changes.is_empty() {
        return false;
    }

    for change in changes {
        // Must have "path" as string
        if change.get("path").and_then(|v| v.as_str()).is_none() {
            return false;
        }

        // Must have "is_new_file" as boolean
        if change
            .get("is_new_file")
            .and_then(|v| v.as_bool())
            .is_none()
        {
            return false;
        }

        let is_new = change["is_new_file"].as_bool().unwrap_or(false);

        if is_new {
            // New files must have "content"
            if change.get("content").and_then(|v| v.as_str()).is_none() {
                return false;
            }
        } else {
            // Existing files must have "edits" array
            let edits = match change.get("edits").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => return false,
            };

            for edit in edits {
                if edit.get("search").and_then(|v| v.as_str()).is_none() {
                    return false;
                }
                if edit.get("replace").and_then(|v| v.as_str()).is_none() {
                    return false;
                }
            }
        }
    }

    true
}
