//! Progressive skill loading for analysis.
//!
//! Port from Python `analysis/skills.py`.
//! Skills (analyzer prompts) are loaded on-demand based on detected
//! language/framework, keeping context lean.

use std::collections::HashSet;

/// A single analysis skill/prompt.
#[derive(Debug, Clone)]
pub struct AnalysisSkill {
    pub name: String,
    pub description: String,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub priority: u8, // 1=highest, 10=lowest
}

impl AnalysisSkill {
    /// Check if this skill is relevant for given language/frameworks.
    pub fn matches(&self, language: &str, frameworks_detected: &HashSet<String>) -> bool {
        // Universal skills always match
        if self.languages.is_empty() && self.frameworks.is_empty() {
            return true;
        }

        let lang_lower = language.to_lowercase();
        let lang_matches = self.languages.is_empty()
            || self.languages.iter().any(|l| l.to_lowercase() == lang_lower);

        // If this skill requires specific frameworks, they must be detected
        if !self.frameworks.is_empty() {
            let fw_lower: HashSet<String> = self.frameworks.iter().map(|f| f.to_lowercase()).collect();
            let fw_matches = frameworks_detected
                .iter()
                .any(|f| fw_lower.contains(&f.to_lowercase()));
            return lang_matches && fw_matches;
        }

        // Language-only skills
        lang_matches
    }
}

/// Get the built-in skill registry.
pub fn builtin_skills() -> Vec<AnalysisSkill> {
    vec![
        // Universal skills (always loaded)
        AnalysisSkill {
            name: "security".into(),
            description: "Detect hardcoded secrets, SQL injection, XSS, command injection".into(),
            languages: vec![],
            frameworks: vec![],
            priority: 1,
        },
        AnalysisSkill {
            name: "code_quality".into(),
            description: "Find dead code, missing error handling, complexity issues".into(),
            languages: vec![],
            frameworks: vec![],
            priority: 2,
        },
        // Language-specific
        AnalysisSkill {
            name: "python_specific".into(),
            description: "Python antipatterns: mutable defaults, bare except, f-string issues".into(),
            languages: vec!["python".into()],
            frameworks: vec![],
            priority: 3,
        },
        AnalysisSkill {
            name: "javascript_specific".into(),
            description: "JS/TS issues: callback hell, promise misuse, prototype pollution".into(),
            languages: vec!["javascript".into(), "typescript".into()],
            frameworks: vec![],
            priority: 3,
        },
        AnalysisSkill {
            name: "go_specific".into(),
            description: "Go issues: goroutine leaks, unchecked errors, defer in loops".into(),
            languages: vec!["go".into()],
            frameworks: vec![],
            priority: 3,
        },
        AnalysisSkill {
            name: "rust_specific".into(),
            description: "Rust: unwrap abuse, unnecessary clones, unsafe misuse".into(),
            languages: vec!["rust".into()],
            frameworks: vec![],
            priority: 3,
        },
        AnalysisSkill {
            name: "java_specific".into(),
            description: "Java: resource leaks, null handling, serialization issues".into(),
            languages: vec!["java".into(), "kotlin".into()],
            frameworks: vec![],
            priority: 3,
        },
        // Framework-specific
        AnalysisSkill {
            name: "django_security".into(),
            description: "Django: CSRF, ORM injection, settings exposure, debug mode".into(),
            languages: vec!["python".into()],
            frameworks: vec!["django".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "flask_security".into(),
            description: "Flask: template injection, secret key exposure, debug mode".into(),
            languages: vec!["python".into()],
            frameworks: vec!["flask".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "react_patterns".into(),
            description: "React: hook rules, key props, memo misuse, state management".into(),
            languages: vec!["javascript".into(), "typescript".into()],
            frameworks: vec!["react".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "nextjs_patterns".into(),
            description: "Next.js: SSR issues, data fetching patterns, routing".into(),
            languages: vec!["javascript".into(), "typescript".into()],
            frameworks: vec!["nextjs".into(), "next".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "fastapi_patterns".into(),
            description: "FastAPI: dependency injection, validation, async patterns".into(),
            languages: vec!["python".into()],
            frameworks: vec!["fastapi".into()],
            priority: 4,
        },
    ]
}

/// Select relevant skills for a language + detected frameworks.
pub fn select_skills(language: &str, frameworks: &HashSet<String>) -> Vec<AnalysisSkill> {
    let mut skills: Vec<_> = builtin_skills()
        .into_iter()
        .filter(|s| s.matches(language, frameworks))
        .collect();
    skills.sort_by_key(|s| s.priority);
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_skills_always_match() {
        let skills = select_skills("unknown_lang", &HashSet::new());
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"security"));
        assert!(names.contains(&"code_quality"));
    }

    #[test]
    fn test_python_gets_python_skills() {
        let skills = select_skills("python", &HashSet::new());
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"python_specific"));
        assert!(!names.contains(&"go_specific"));
    }

    #[test]
    fn test_framework_skills_loaded() {
        let mut fw = HashSet::new();
        fw.insert("django".to_string());
        let skills = select_skills("python", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"django_security"));
        assert!(!names.contains(&"flask_security"));
    }

    #[test]
    fn test_skills_sorted_by_priority() {
        let skills = select_skills("python", &HashSet::new());
        for i in 1..skills.len() {
            assert!(skills[i].priority >= skills[i - 1].priority);
        }
    }
}
