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
            || self
                .languages
                .iter()
                .any(|l| l.to_lowercase() == lang_lower);

        // If this skill requires specific frameworks, they must be detected
        if !self.frameworks.is_empty() {
            let fw_lower: HashSet<String> =
                self.frameworks.iter().map(|f| f.to_lowercase()).collect();
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
            description: "Python antipatterns: mutable defaults, bare except, f-string issues"
                .into(),
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
        AnalysisSkill {
            name: "csharp_specific".into(),
            description:
                "C#: IDisposable misuse, async void, null-coalescing pitfalls, LINQ deferred-execution bugs"
                    .into(),
            languages: vec!["csharp".into(), "c#".into(), "c_sharp".into()],
            frameworks: vec![],
            priority: 3,
        },
        AnalysisSkill {
            name: "ruby_specific".into(),
            description:
                "Ruby: monkey-patching risk, frozen_string_literal, block-vs-Proc semantics, eval/send injection"
                    .into(),
            languages: vec!["ruby".into()],
            frameworks: vec![],
            priority: 3,
        },
        AnalysisSkill {
            name: "php_specific".into(),
            description:
                "PHP: SQL injection in raw queries, type juggling (==), error-suppression `@`, deprecated mysql_*"
                    .into(),
            languages: vec!["php".into()],
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
        AnalysisSkill {
            name: "express_security".into(),
            description: "Express.js: middleware order, CORS, helmet, input sanitization".into(),
            languages: vec!["javascript".into(), "typescript".into()],
            frameworks: vec!["express".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "vue_patterns".into(),
            description:
                "Vue 3: reactivity (ref vs reactive), v-html XSS, lifecycle ordering, computed/watch misuse"
                    .into(),
            languages: vec!["javascript".into(), "typescript".into()],
            frameworks: vec!["vue".into(), "vuejs".into(), "vue3".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "rails_security".into(),
            description:
                "Rails: mass-assignment (strong params), SQL injection via raw `find_by_sql`, CSRF-token gaps, secret_key_base exposure"
                    .into(),
            languages: vec!["ruby".into()],
            frameworks: vec!["rails".into(), "ruby_on_rails".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "laravel_security".into(),
            description:
                "Laravel: mass-assignment ($fillable/$guarded), unprotected routes, CSRF middleware bypass, raw DB::raw injection"
                    .into(),
            languages: vec!["php".into()],
            frameworks: vec!["laravel".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "spring_security".into(),
            description:
                "Spring/Spring Boot: AuthN/AuthZ filter chain holes, JPA query injection, exposed actuator endpoints, CSRF misconfig"
                    .into(),
            languages: vec!["java".into(), "kotlin".into()],
            frameworks: vec!["spring".into(), "spring_boot".into(), "springboot".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "dockerfile_security".into(),
            description:
                "Dockerfile: `latest` tag, running as root, secrets baked into layers, missing HEALTHCHECK, large attack surface"
                    .into(),
            languages: vec![],
            frameworks: vec!["docker".into(), "dockerfile".into()],
            priority: 4,
        },
        AnalysisSkill {
            name: "github_actions_security".into(),
            description:
                "GitHub Actions: untrusted `${{ github.event.* }}` injection, exposed secrets in logs, pwn-request via pull_request_target, missing permissions: scope"
                    .into(),
            languages: vec![],
            frameworks: vec!["github_actions".into(), "actions".into()],
            priority: 4,
        },
        // Additional universal skills
        AnalysisSkill {
            name: "docs".into(),
            description: "Missing or outdated docstrings, README gaps, incorrect examples".into(),
            languages: vec![],
            frameworks: vec![],
            priority: 5,
        },
        AnalysisSkill {
            name: "ui_ux".into(),
            description: "Accessibility (a11y), responsive design, color contrast, ARIA labels"
                .into(),
            languages: vec![],
            frameworks: vec![],
            priority: 5,
        },
        AnalysisSkill {
            name: "performance".into(),
            description: "N+1 queries, unnecessary allocations, blocking I/O, cache misses".into(),
            languages: vec![],
            frameworks: vec![],
            priority: 5,
        },
        AnalysisSkill {
            name: "refactor".into(),
            description: "Unused imports, dead code, overly complex functions, duplicated logic"
                .into(),
            languages: vec![],
            frameworks: vec![],
            priority: 5,
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

    #[test]
    fn test_csharp_gets_csharp_skill() {
        let skills = select_skills("csharp", &HashSet::new());
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"csharp_specific"));
    }

    #[test]
    fn test_ruby_with_rails_gets_rails_security() {
        let mut fw = HashSet::new();
        fw.insert("rails".to_string());
        let skills = select_skills("ruby", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"ruby_specific"));
        assert!(names.contains(&"rails_security"));
    }

    #[test]
    fn test_php_with_laravel_gets_laravel_security() {
        let mut fw = HashSet::new();
        fw.insert("laravel".to_string());
        let skills = select_skills("php", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"php_specific"));
        assert!(names.contains(&"laravel_security"));
    }

    #[test]
    fn test_java_with_spring_gets_spring_security() {
        let mut fw = HashSet::new();
        fw.insert("spring".to_string());
        let skills = select_skills("java", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"java_specific"));
        assert!(names.contains(&"spring_security"));
    }

    #[test]
    fn test_vue_loads_vue_patterns() {
        let mut fw = HashSet::new();
        fw.insert("vue".to_string());
        let skills = select_skills("javascript", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"vue_patterns"));
    }

    #[test]
    fn test_dockerfile_skill_loads_for_docker_framework_any_lang() {
        let mut fw = HashSet::new();
        fw.insert("docker".to_string());
        // Dockerfile-specific skill should match regardless of "primary" language
        let skills = select_skills("python", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"dockerfile_security"));
    }

    #[test]
    fn test_github_actions_skill_loads_when_detected() {
        let mut fw = HashSet::new();
        fw.insert("github_actions".to_string());
        let skills = select_skills("yaml", &fw);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"github_actions_security"));
    }

    #[test]
    fn test_no_framework_skill_leaks_into_unrelated_language() {
        // Loading rails_security shouldn't trigger when only Python is detected
        let skills = select_skills("python", &HashSet::new());
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(!names.contains(&"rails_security"));
        assert!(!names.contains(&"laravel_security"));
        assert!(!names.contains(&"spring_security"));
    }
}
