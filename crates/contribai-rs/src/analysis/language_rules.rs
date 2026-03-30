//! Multi-language analysis rules and prompts.
//!
//! Port from Python `analysis/language_rules.py`.
//! Language-specific security, code quality, and performance rules
//! for JavaScript/TypeScript, Go, and Rust.

/// A language-specific analysis rule.
#[derive(Debug, Clone)]
pub struct LanguageRule {
    pub language: &'static str,
    pub category: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub pattern: &'static str,
    pub severity: &'static str,
    pub fix_hint: &'static str,
}

// ── JavaScript / TypeScript ──────────────────────────

const JS_TS_RULES: &[LanguageRule] = &[
    LanguageRule {
        language: "javascript", category: "security", name: "eval-usage",
        description: "Use of eval() is a security risk",
        pattern: "eval(", severity: "critical",
        fix_hint: "Replace eval() with JSON.parse() or a safe alternative",
    },
    LanguageRule {
        language: "javascript", category: "security", name: "innerHTML-xss",
        description: "innerHTML can lead to XSS attacks",
        pattern: "innerHTML", severity: "high",
        fix_hint: "Use textContent or DOMPurify",
    },
    LanguageRule {
        language: "javascript", category: "security", name: "no-prototype-pollution",
        description: "Prototype pollution via __proto__",
        pattern: "__proto__", severity: "critical",
        fix_hint: "Use Object.create(null) or validate input keys",
    },
    LanguageRule {
        language: "typescript", category: "code_quality", name: "no-any-type",
        description: "Avoid 'any' type - defeats TypeScript's purpose",
        pattern: ": any", severity: "medium",
        fix_hint: "Use specific types or 'unknown' with type guards",
    },
    LanguageRule {
        language: "javascript", category: "code_quality", name: "no-var",
        description: "Use const/let instead of var",
        pattern: "var ", severity: "low",
        fix_hint: "Replace 'var' with 'const' or 'let'",
    },
    LanguageRule {
        language: "javascript", category: "performance", name: "no-sync-fs",
        description: "Synchronous fs operations block the event loop",
        pattern: "readFileSync", severity: "medium",
        fix_hint: "Use async fs.readFile() or fs.promises.readFile()",
    },
    LanguageRule {
        language: "javascript", category: "security", name: "no-hardcoded-jwt",
        description: "Hardcoded JWT secret key",
        pattern: "jwt.sign(", severity: "high",
        fix_hint: "Use environment variables for JWT secrets",
    },
];

// ── Go ───────────────────────────────────────────────

const GO_RULES: &[LanguageRule] = &[
    LanguageRule {
        language: "go", category: "security", name: "sql-injection",
        description: "Potential SQL injection via string formatting",
        pattern: "fmt.Sprintf(\"SELECT", severity: "critical",
        fix_hint: "Use parameterized queries with db.Query(sql, args...)",
    },
    LanguageRule {
        language: "go", category: "code_quality", name: "unchecked-error",
        description: "Ignoring error return values",
        pattern: "_ = ", severity: "medium",
        fix_hint: "Handle errors explicitly: if err != nil { return err }",
    },
    LanguageRule {
        language: "go", category: "code_quality", name: "defer-in-loop",
        description: "Defer inside loop can cause resource leaks",
        pattern: "defer ", severity: "medium",
        fix_hint: "Move defer outside the loop or use a wrapper function",
    },
    LanguageRule {
        language: "go", category: "security", name: "tls-insecure-skip",
        description: "TLS verification disabled",
        pattern: "InsecureSkipVerify: true", severity: "critical",
        fix_hint: "Remove InsecureSkipVerify or use proper CA certificates",
    },
    LanguageRule {
        language: "go", category: "performance", name: "goroutine-leak",
        description: "Goroutine without context cancellation",
        pattern: "go func()", severity: "medium",
        fix_hint: "Use context.WithCancel and select for graceful shutdown",
    },
];

// ── Rust ─────────────────────────────────────────────

const RUST_RULES: &[LanguageRule] = &[
    LanguageRule {
        language: "rust", category: "security", name: "unsafe-block",
        description: "Unsafe block bypasses Rust's safety guarantees",
        pattern: "unsafe {", severity: "high",
        fix_hint: "Document why unsafe is necessary; use safe alternatives",
    },
    LanguageRule {
        language: "rust", category: "code_quality", name: "unwrap-panic",
        description: "unwrap() panics on None/Err — use ? operator",
        pattern: ".unwrap()", severity: "medium",
        fix_hint: "Use .unwrap_or(), .unwrap_or_default(), or ? operator",
    },
    LanguageRule {
        language: "rust", category: "code_quality", name: "expect-panic",
        description: "expect() panics with message — use ? in libraries",
        pattern: ".expect(", severity: "low",
        fix_hint: "Use ? operator in library code",
    },
    LanguageRule {
        language: "rust", category: "performance", name: "clone-heavy",
        description: "Excessive .clone() may indicate ownership issues",
        pattern: ".clone()", severity: "low",
        fix_hint: "Use references (&T) or Cow<T> instead",
    },
    LanguageRule {
        language: "rust", category: "code_quality", name: "todo-macro",
        description: "todo!() macro will panic at runtime",
        pattern: "todo!()", severity: "medium",
        fix_hint: "Implement the missing functionality",
    },
];

/// Get analysis rules for a specific language.
pub fn get_rules_for_language(language: &str) -> Vec<&'static LanguageRule> {
    let lang = language.to_lowercase();
    let mut rules: Vec<&LanguageRule> = Vec::new();

    let all: &[&[LanguageRule]] = &[JS_TS_RULES, GO_RULES, RUST_RULES];
    for rule_set in all {
        for rule in *rule_set {
            if rule.language == lang {
                rules.push(rule);
            }
        }
    }

    // TypeScript inherits JavaScript rules
    if lang == "typescript" {
        for rule in JS_TS_RULES {
            if rule.language == "javascript" {
                rules.push(rule);
            }
        }
    }

    rules
}

/// Generate a language-specific analysis prompt.
pub fn get_analysis_prompt(language: &str, file_content: &str, file_path: &str) -> String {
    let rules = get_rules_for_language(language);
    if rules.is_empty() {
        return String::new();
    }

    let rule_descs: String = rules
        .iter()
        .map(|r| format!("- [{}] {}: {} (look for: {})", r.severity.to_uppercase(), r.name, r.description, r.pattern))
        .collect::<Vec<_>>()
        .join("\n");

    let snippet: String = file_content.chars().take(5000).collect();

    format!(
        "Analyze this {} file for issues:\n\n\
         File: {}\n\n\
         Language-specific rules to check:\n{}\n\n\
         For each issue found, provide:\n\
         1. Rule name\n\
         2. Line number(s)\n\
         3. Description\n\
         4. Suggested fix\n\n\
         Code:\n```\n{}\n```",
        language, file_path, rule_descs, snippet
    )
}

/// Get list of languages with specialized rules.
pub fn get_supported_languages() -> Vec<&'static str> {
    let mut langs: Vec<&str> = Vec::new();
    let all: &[&[LanguageRule]] = &[JS_TS_RULES, GO_RULES, RUST_RULES];
    for rule_set in all {
        for rule in *rule_set {
            if !langs.contains(&rule.language) {
                langs.push(rule.language);
            }
        }
    }
    langs.sort();
    langs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_rules_javascript() {
        let rules = get_rules_for_language("javascript");
        assert!(!rules.is_empty());
        assert!(rules.iter().any(|r| r.name == "eval-usage"));
    }

    #[test]
    fn test_get_rules_typescript_inherits_js() {
        let rules = get_rules_for_language("typescript");
        assert!(rules.iter().any(|r| r.name == "no-any-type"));
        assert!(rules.iter().any(|r| r.name == "eval-usage")); // inherited
    }

    #[test]
    fn test_get_rules_go() {
        let rules = get_rules_for_language("go");
        assert!(rules.iter().any(|r| r.name == "sql-injection"));
    }

    #[test]
    fn test_get_rules_rust() {
        let rules = get_rules_for_language("rust");
        assert!(rules.iter().any(|r| r.name == "unwrap-panic"));
    }

    #[test]
    fn test_unknown_language_empty() {
        assert!(get_rules_for_language("brainfuck").is_empty());
    }

    #[test]
    fn test_supported_languages() {
        let langs = get_supported_languages();
        assert!(langs.contains(&"go"));
        assert!(langs.contains(&"rust"));
        assert!(langs.contains(&"javascript"));
    }

    #[test]
    fn test_analysis_prompt_nonempty() {
        let prompt = get_analysis_prompt("rust", "fn main() {}", "src/main.rs");
        assert!(prompt.contains("unwrap-panic"));
        assert!(prompt.contains("src/main.rs"));
    }

    #[test]
    fn test_analysis_prompt_empty_for_unknown() {
        let prompt = get_analysis_prompt("cobol", "code", "file.cob");
        assert!(prompt.is_empty());
    }
}
