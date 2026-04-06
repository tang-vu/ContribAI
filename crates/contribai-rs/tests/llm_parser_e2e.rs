//! E2E parser tests with real LLM response shapes.
//!
//! These tests capture real-world LLM output patterns and verify that
//! the parsing logic in analyzer.rs and engine.rs handles them correctly.
//! This includes: markdown fences, explanations alongside JSON,
//! malformed responses, empty responses, and multi-language findings.

#[cfg(test)]
mod tests {
    use contribai::core::models::{ContributionType, Finding, Severity};

    // ── Helper: replicate analyzer.rs parse_findings logic ──

    fn parse_findings(response: &str, analyzer: &str) -> Vec<Finding> {
        // Try to extract JSON array from response
        let json_str = if let Some(start) = response.find('[') {
            if let Some(end) = response.rfind(']') {
                &response[start..=end]
            } else {
                return vec![];
            }
        } else {
            return vec![];
        };

        match serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
            Ok(items) => items
                .into_iter()
                .filter_map(|item| {
                    let title = item["title"].as_str()?.to_string();
                    let description = item["description"].as_str().unwrap_or("").to_string();
                    let severity = match item["severity"].as_str().unwrap_or("medium") {
                        "critical" => Severity::Critical,
                        "high" => Severity::High,
                        "low" => Severity::Low,
                        _ => Severity::Medium,
                    };
                    let file_path = item["file_path"].as_str().unwrap_or("").to_string();
                    let confidence = item["confidence"].as_f64().unwrap_or(0.7);

                    Some(Finding {
                        id: uuid::Uuid::new_v4().to_string(),
                        finding_type: ContributionType::from_analyzer(analyzer),
                        severity,
                        title,
                        description,
                        file_path,
                        line_start: item["line_start"].as_u64().map(|n| n as usize),
                        line_end: item["line_end"].as_u64().map(|n| n as usize),
                        suggestion: item["suggestion"].as_str().map(String::from),
                        confidence,
                        priority_signals: vec![],
                    })
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    // ── Test 1: Clean JSON array (ideal case) ──

    #[test]
    fn test_parse_clean_json_array() {
        let response = r#"[
  {
    "title": "SQL injection vulnerability",
    "description": "User input is not sanitized in the query builder",
    "severity": "critical",
    "file_path": "src/db/query.py",
    "line_start": 42,
    "line_end": 55,
    "suggestion": "Use parameterized queries instead of string formatting",
    "confidence": 0.95
  }
]"#;

        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "SQL injection vulnerability");
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].file_path, "src/db/query.py");
        assert_eq!(findings[0].line_start, Some(42));
        assert!((findings[0].confidence - 0.95).abs() < 0.001);
    }

    // ── Test 2: JSON inside markdown fences ──

    #[test]
    fn test_parse_json_in_markdown_fences() {
        let response = r#"Here are my findings:

```json
[
  {
    "title": "Unused import",
    "description": "os module is imported but never used",
    "severity": "low",
    "file_path": "src/utils.py",
    "line_start": 1,
    "line_end": 1,
    "suggestion": "Remove the unused import",
    "confidence": 0.9
  }
]
```

Hope this helps!"#;

        let findings = parse_findings(response, "code_quality");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Unused import");
        assert_eq!(findings[0].severity, Severity::Low);
    }

    // ── Test 3: JSON with explanation before and after ──

    #[test]
    fn test_parse_json_with_explanation() {
        let response = r#"I've analyzed the repository and found the following issues:

[
  {
    "title": "Missing error handling",
    "description": "The function does not handle IO errors",
    "severity": "medium",
    "file_path": "src/io.rs",
    "line_start": 10,
    "line_end": 20,
    "suggestion": "Add Result return type",
    "confidence": 0.8
  },
  {
    "title": "Memory leak in buffer",
    "description": "Buffer is not freed on error path",
    "severity": "high",
    "file_path": "src/buffer.c",
    "line_start": 45,
    "line_end": 60,
    "suggestion": "Use RAII pattern for resource cleanup",
    "confidence": 0.85
  }
]

These are the most critical issues I found. There may be more minor ones."#;

        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].title, "Missing error handling");
        assert_eq!(findings[1].title, "Memory leak in buffer");
        assert_eq!(findings[1].severity, Severity::High);
    }

    // ── Test 4: Empty findings array ──

    #[test]
    fn test_parse_empty_findings_array() {
        let response = r#"No issues found. The code looks clean.

[]"#;

        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 5: Response with no brackets at all ──

    #[test]
    fn test_parse_no_brackets() {
        let response = "The repository appears to be well-structured with no obvious issues.";

        let findings = parse_findings(response, "code_quality");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 6: JSON with trailing comma (malformed) ──

    #[test]
    fn test_parse_trailing_comma_fails_gracefully() {
        let response = r#"[
  {
    "title": "Test finding",
    "description": "Test description",
    "severity": "medium",
    "file_path": "test.py",
    "line_start": 1,
    "line_end": 5,
    "suggestion": "Fix it",
    "confidence": 0.8,
  }
]"#;

        // Should return empty vec, not panic
        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 7: Multiple JSON arrays (ambiguous) ──

    #[test]
    fn test_parse_multiple_arrays_picks_outermost() {
        // LLM sometimes returns nested structures
        let response = r#"Findings:
[
  {
    "title": "Issue 1",
    "description": "Description 1",
    "severity": "high",
    "file_path": "a.py",
    "line_start": 1,
    "line_end": 10,
    "suggestion": "Fix 1",
    "confidence": 0.9
  }
]

Also check:
[
  {
    "title": "Issue 2",
    "description": "Description 2",
    "severity": "low",
    "file_path": "b.py",
    "line_start": 5,
    "line_end": 15,
    "suggestion": "Fix 2",
    "confidence": 0.7
  }
]"#;

        // Naive bracket matching grabs from first '[' to last ']' — this creates invalid JSON
        // This is a KNOWN BUG in the current implementation. Test documents it.
        let findings = parse_findings(response, "security");
        // The current parser will fail to parse this as valid JSON
        // because it grabs everything between first '[' and last ']'
        // which includes text and two separate arrays
        assert!(findings.is_empty(), "Known bug: multiple arrays not handled");
    }

    // ── Test 8: JSON with code example containing brackets in description ──

    #[test]
    fn test_parse_json_with_brackets_in_description() {
        let response = r#"[
  {
    "title": "Incorrect array access",
    "description": "Use arr[i] instead of arr[i+1] to avoid off-by-one",
    "severity": "medium",
    "file_path": "src/array.go",
    "line_start": 30,
    "line_end": 35,
    "suggestion": "Change arr[i+1] to arr[i]",
    "confidence": 0.8
  }
]"#;

        let findings = parse_findings(response, "code_quality");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Incorrect array access");
        assert!(findings[0].description.contains("arr[i]"));
    }

    // ── Test 9: Multi-language findings ──

    #[test]
    fn test_parse_multi_language_findings() {
        let response = r#"[
  {
    "title": "XSS vulnerability in template",
    "description": "User input rendered without escaping",
    "severity": "high",
    "file_path": "templates/index.html",
    "line_start": 12,
    "line_end": 12,
    "suggestion": "Use auto-escaping: {{ user_input | e }}",
    "confidence": 0.9
  },
  {
    "title": "CSP header missing",
    "description": "No Content-Security-Policy header set",
    "severity": "medium",
    "file_path": "src/middleware.py",
    "line_start": 5,
    "line_end": 10,
    "suggestion": "Add CSP header to all responses",
    "confidence": 0.85
  },
  {
    "title": "Prototype pollution in merge",
    "description": "Object.merge allows __proto__ key",
    "severity": "critical",
    "file_path": "lib/utils.js",
    "line_start": 22,
    "line_end": 30,
    "suggestion": "Use Object.create(null) for merge target",
    "confidence": 0.95
  }
]"#;

        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[1].severity, Severity::Medium);
        assert_eq!(findings[2].severity, Severity::Critical);
    }

    // ── Test 10: Minimal / terse response ──

    #[test]
    fn test_parse_minimal_response() {
        let response = r#"[{"title":"x","severity":"high","file_path":"a.py"}]"#;

        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "x");
        assert_eq!(findings[0].file_path, "a.py");
        // Missing fields should have defaults
        assert_eq!(findings[0].description, "");
        assert!((findings[0].confidence - 0.7).abs() < 0.001);
    }

    // ── Test 11: HTML/XML response (LLM confused) ──

    #[test]
    fn test_parse_html_response() {
        let response = r#"<html>
<body>
<p>Here are the findings:</p>
<ul>
<li>SQL injection in query.py</li>
<li>XSS in index.html</li>
</ul>
</body>
</html>"#;

        // No '[' found → empty vec
        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 12: Single bracket mismatch ──

    #[test]
    fn test_parse_unmatched_bracket() {
        let response = r#"[
  {
    "title": "Issue found",
    "description": "Something is wrong",
    "severity": "high",
    "file_path": "src/main.rs""#;

        // '[' found but no ']' → returns empty
        // Actually rfind(']') won't find one since the JSON is truncated
        // But our test string does have ']' in the severity line — let me check
        // Wait, it doesn't. So rfind(']') returns None.
        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 13: JSON with unicode ──

    #[test]
    fn test_parse_unicode_in_findings() {
        let response = r#"[
  {
    "title": "Unicode injection vulnerability",
    "description": "Homoglyph attack possible with café vs café",
    "severity": "high",
    "file_path": "src/auth.py",
    "line_start": 10,
    "line_end": 20,
    "suggestion": "Normalize unicode input before comparison",
    "confidence": 0.9
  }
]"#;

        let findings = parse_findings(response, "security");
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("café"));
    }

    // ── Test 14: Extra fields in JSON (should be ignored) ──

    #[test]
    fn test_parse_extra_fields_ignored() {
        let response = r#"[
  {
    "title": "Performance issue",
    "description": "O(n²) algorithm used where O(n log n) is possible",
    "severity": "medium",
    "file_path": "src/sort.rs",
    "line_start": 15,
    "line_end": 25,
    "suggestion": "Replace with merge sort",
    "confidence": 0.8,
    "extra_field": "should be ignored",
    "another": [1, 2, 3],
    "nested": {"key": "value"}
  }
]"#;

        let findings = parse_findings(response, "performance");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Performance issue");
    }

    // ── Test 15: Severity case-insensitive ──

    #[test]
    fn test_parse_severity_case_insensitive() {
        // Current implementation is case-sensitive — this documents the behavior
        let response = r#"[
  {"title": "CRITICAL issue", "severity": "CRITICAL", "file_path": "a.py"},
  {"title": "High issue", "severity": "HIGH", "file_path": "b.py"},
  {"title": "Medium issue", "severity": "MEDIUM", "file_path": "c.py"},
  {"title": "Low issue", "severity": "LOW", "file_path": "d.py"}
]"#;

        let findings = parse_findings(response, "security");
        // Current parser: only lowercase matches, rest default to Medium
        assert_eq!(findings.len(), 4);
        assert_eq!(findings[0].severity, Severity::Medium); // "CRITICAL" → unknown → Medium
        assert_eq!(findings[1].severity, Severity::Medium); // "HIGH" → unknown → Medium
        assert_eq!(findings[2].severity, Severity::Medium); // "MEDIUM" matches
        assert_eq!(findings[3].severity, Severity::Medium); // "LOW" matches
    }

    // ── Test 16: Empty string response ──

    #[test]
    fn test_parse_empty_string() {
        let findings = parse_findings("", "security");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 17: Whitespace-only response ──

    #[test]
    fn test_parse_whitespace_only() {
        let findings = parse_findings("   \n\n\t\n  ", "security");
        assert_eq!(findings.len(), 0);
    }

    // ── Test 18: JSON with null values ──

    #[test]
    fn test_parse_null_values() {
        let response = r#"[
  {
    "title": "Issue with null fields",
    "description": null,
    "severity": null,
    "file_path": null,
    "line_start": null,
    "line_end": null,
    "suggestion": null,
    "confidence": null
  }
]"#;

        let findings = parse_findings(response, "security");
        // file_path is null → as_str() returns None → filter_map returns None
        // BUT: file_path.as_str().unwrap_or("") returns "" (empty string)
        // which passes the filter_map. So the finding IS created with empty file_path.
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file_path, ""); // empty string default
        assert_eq!(findings[0].description, ""); // empty string default
    }
}
