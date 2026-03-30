//! Fuzzy matching strategies for applying LLM-generated search/replace edits.

use std::collections::HashMap;

use regex::Regex;
use tracing::{debug, info};

use crate::core::models::{Finding, RepoContext};

use super::engine::ContributionGenerator;

// ── Fuzzy matching ───────────────────────────────────────────────────────────

/// Apply a single search/replace edit using 4 strategies with graceful fallback.
///
/// Strategy order (matches Python `_parse_changes` edit loop):
/// 1. Exact substring match
/// 2. Normalized trailing-whitespace match (per line)
/// 3. Stripped leading/trailing whitespace match
/// 4. Token-based similarity (word overlap ratio >= 0.8)
pub fn apply_single_edit(
    content: &str,
    search: &str,
    replace: &str,
    path: &str,
) -> Option<String> {
    // Strategy 1: exact
    if content.contains(search) {
        return Some(content.replacen(search, replace, 1));
    }

    // Strategy 2: normalize trailing whitespace per line
    {
        let norm_search: String = search
            .split('\n')
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        let norm_content: String = content
            .split('\n')
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        if let Some(idx) = norm_content.find(&norm_search) {
            let start_line = norm_content[..idx].matches('\n').count();
            let end_line = start_line + norm_search.matches('\n').count();
            let mut lines: Vec<&str> = content.split('\n').collect();
            let replace_lines: Vec<&str> = replace.split('\n').collect();
            lines.splice(start_line..=end_line, replace_lines);
            debug!(path = %path, "Fuzzy match (whitespace normalized)");
            return Some(lines.join("\n"));
        }
    }

    // Strategy 3: strip all leading/trailing whitespace
    {
        let stripped = search.trim();
        if stripped.len() > 20 && content.contains(stripped) {
            debug!(path = %path, "Fuzzy match (stripped)");
            return Some(content.replacen(stripped, replace.trim(), 1));
        }
    }

    // Strategy 4: token-based similarity (word overlap Dice coefficient >= 0.8)
    if search.len() > 20 {
        if let Some(result) = fuzzy_replace(content, search, replace) {
            debug!(path = %path, "Fuzzy match (token similarity)");
            return Some(result);
        }
    }

    None
}

/// Find the best-matching block in `content` using word-overlap similarity.
///
/// Slides a window the same number of lines as `search` over `content`.
/// Uses Dice coefficient on word sets: `2 * |intersection| / (|A| + |B|)`.
/// Returns modified content if best ratio >= 0.8, otherwise `None`.
pub fn fuzzy_replace(content: &str, search: &str, replace: &str) -> Option<String> {
    let search_lines: Vec<&str> = search.lines().collect();
    let content_lines: Vec<&str> = content.lines().collect();
    let search_len = search_lines.len();

    if search_len == 0 || search_len > content_lines.len() {
        return None;
    }

    let search_words: Vec<&str> = search.split_whitespace().collect();

    let mut best_ratio = 0.0_f64;
    let mut best_start: Option<usize> = None;

    for i in 0..=(content_lines.len() - search_len) {
        let window = content_lines[i..i + search_len].join("\n");
        let window_words: Vec<&str> = window.split_whitespace().collect();

        let ratio = word_overlap_ratio(&search_words, &window_words);
        if ratio > best_ratio {
            best_ratio = ratio;
            best_start = Some(i);
        }
    }

    if best_ratio >= 0.8 {
        if let Some(start) = best_start {
            let replace_lines: Vec<&str> = replace.lines().collect();
            let mut result = content_lines[..start].to_vec();
            result.extend_from_slice(&replace_lines);
            result.extend_from_slice(&content_lines[start + search_len..]);
            return Some(result.join("\n"));
        }
    }

    None
}

/// Compute Dice coefficient (word overlap ratio) between two word slices.
///
/// Formula: `2 * |intersection| / (|a| + |b|)` where intersection is the
/// multiset intersection (accounts for repeated words).
pub fn word_overlap_ratio(a: &[&str], b: &[&str]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    // Count word frequencies in b
    let mut b_counts: HashMap<&str, usize> = HashMap::new();
    for w in b {
        *b_counts.entry(w).or_insert(0) += 1;
    }

    // Count intersection (limited by min frequency in each side)
    let mut a_counts: HashMap<&str, usize> = HashMap::new();
    let mut intersection: usize = 0;
    for w in a {
        let a_c = a_counts.entry(w).or_insert(0);
        *a_c += 1;
        let b_c = b_counts.get(w).copied().unwrap_or(0);
        if *a_c <= b_c {
            intersection += 1;
        }
    }

    2.0 * intersection as f64 / (a.len() + b.len()) as f64
}

// ── Cross-file detection ─────────────────────────────────────────────────────

impl ContributionGenerator<'_> {
    /// Find other files in the repo with the same issue pattern.
    ///
    /// Searches `context.relevant_files` for code patterns extracted from the
    /// finding description/suggestion. Returns `{path: content}` for files
    /// with at least 2 keyword matches (capped at 3 extra files).
    pub fn find_cross_file_instances(
        &self,
        finding: &Finding,
        context: &RepoContext,
    ) -> HashMap<String, String> {
        if finding.file_path.is_empty() || context.relevant_files.is_empty() {
            return HashMap::new();
        }

        let keywords = Self::extract_search_patterns(finding);
        if keywords.is_empty() {
            return HashMap::new();
        }

        let mut other_files: HashMap<String, String> = HashMap::new();

        for (fpath, content) in &context.relevant_files {
            if fpath == &finding.file_path {
                continue;
            }
            let content_lower = content.to_lowercase();
            let matches = keywords
                .iter()
                .filter(|kw| content_lower.contains(kw.to_lowercase().as_str()))
                .count();

            if matches >= 2 {
                other_files.insert(fpath.clone(), content.clone());
                if other_files.len() >= 3 {
                    break;
                }
            }
        }

        if !other_files.is_empty() {
            info!(
                count = other_files.len(),
                files = ?other_files.keys().collect::<Vec<_>>(),
                "Found same pattern in other files"
            );
        }

        other_files
    }

    /// Extract code patterns from finding description and suggestion.
    ///
    /// Looks for backtick-quoted snippets (`foo`) and dotted identifiers (foo.bar()).
    fn extract_search_patterns(finding: &Finding) -> Vec<String> {
        let text = format!(
            "{} {}",
            finding.description,
            finding.suggestion.as_deref().unwrap_or("")
        );

        let mut patterns: Vec<String> = Vec::new();

        // Backtick-quoted snippets
        if let Ok(re) = Regex::new(r"`([^`]+)`") {
            for cap in re.captures_iter(&text) {
                if let Some(m) = cap.get(1) {
                    let s = m.as_str().to_string();
                    if s.len() > 3 {
                        patterns.push(s);
                    }
                }
            }
        }

        // Dotted identifiers (e.g., `foo.bar()`, `obj.method!`)
        if let Ok(re) = Regex::new(r"(\w+\.\w+[!?]?(?:\(\))?)") {
            for cap in re.captures_iter(&text) {
                if let Some(m) = cap.get(1) {
                    let s = m.as_str().to_string();
                    if s.len() > 5 {
                        patterns.push(s);
                    }
                }
            }
        }

        patterns.truncate(10);
        patterns
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::generator::engine::tests::{mock_gen, test_context, test_finding};

    #[test]
    fn test_fuzzy_replace_exact_words() {
        let content = "line one\nline two\nline three\n";
        let search = "line one\nline two";
        let replace = "replaced one\nreplaced two";
        let result = fuzzy_replace(content, search, replace);
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.contains("replaced one"));
        assert!(!r.contains("line one"));
    }

    #[test]
    fn test_fuzzy_replace_no_match() {
        let content = "completely different text here";
        let search = "foo bar baz qux quux corge grault garply";
        let replace = "something";
        let result = fuzzy_replace(content, search, replace);
        assert!(result.is_none());
    }

    #[test]
    fn test_fuzzy_replace_empty_search() {
        let result = fuzzy_replace("hello world", "", "replacement");
        assert!(result.is_none());
    }

    #[test]
    fn test_word_overlap_ratio_identical() {
        let words: Vec<&str> = vec!["foo", "bar", "baz"];
        let ratio = word_overlap_ratio(&words, &words);
        assert!((ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_word_overlap_ratio_disjoint() {
        let a = vec!["foo", "bar"];
        let b = vec!["qux", "quux"];
        let ratio = word_overlap_ratio(&a, &b);
        assert!((ratio - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_word_overlap_ratio_partial() {
        let a = vec!["foo", "bar", "baz"];
        let b = vec!["foo", "bar", "qux"];
        let ratio = word_overlap_ratio(&a, &b);
        // intersection = 2, total = 6 -> 2*2/6 ~ 0.667
        assert!(ratio > 0.5 && ratio < 1.0);
    }

    #[test]
    fn test_find_cross_file_instances() {
        let gen = mock_gen();
        let finding = Finding {
            description: "Use `parameterized queries` instead of `string.format`".into(),
            suggestion: Some("Use `cursor.execute` with params".into()),
            ..test_finding()
        };

        let mut files = HashMap::new();
        // Primary file (should be excluded from results)
        files.insert(
            "src/db/queries.py".to_string(),
            "cursor.execute(sql.format(user))".to_string(),
        );
        // Should match: has 2+ keyword hits
        files.insert(
            "src/api/users.py".to_string(),
            "parameterized queries string.format cursor.execute".to_string(),
        );
        // Should NOT match: insufficient keyword overlap
        files.insert(
            "src/api/posts.py".to_string(),
            "unrelated content here xyz".to_string(),
        );

        let ctx = test_context(files);
        let result = gen.find_cross_file_instances(&finding, &ctx);

        assert!(result.contains_key("src/api/users.py"));
        assert!(!result.contains_key("src/db/queries.py")); // primary file excluded
        assert!(!result.contains_key("src/api/posts.py")); // too few matches
    }
}
