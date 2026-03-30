//! Template registry for common contribution patterns.
//!
//! Port from Python `templates/registry.py`.
//! Templates are YAML-defined patterns that describe common
//! fixes the agent can apply without full LLM generation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};

/// A contribution template definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub template_type: String,
    pub pattern: String,
    pub fix_template: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_severity() -> String {
    "medium".into()
}

/// Built-in templates (embedded in binary).
fn builtin_templates() -> Vec<Template> {
    vec![
        Template {
            name: "security-headers".into(),
            description: "Add missing HTTP security headers to web applications".into(),
            template_type: "security_fix".into(),
            pattern: "Web server missing security headers".into(),
            fix_template: "Add security headers:\n- Content-Security-Policy\n- X-Content-Type-Options: nosniff\n- X-Frame-Options: DENY\n- Strict-Transport-Security".into(),
            severity: "high".into(),
            languages: vec!["python".into(), "javascript".into(), "typescript".into()],
            tags: vec!["security".into(), "headers".into(), "web".into()],
        },
        Template {
            name: "add-gitignore".into(),
            description: "Add or improve .gitignore file".into(),
            template_type: "code_quality".into(),
            pattern: "Missing .gitignore or incomplete patterns".into(),
            fix_template: "Add comprehensive .gitignore for the project's language".into(),
            severity: "low".into(),
            languages: vec![],
            tags: vec!["git".into(), "config".into()],
        },
        Template {
            name: "add-type-hints".into(),
            description: "Add Python type hints to function signatures".into(),
            template_type: "code_quality".into(),
            pattern: "Function missing type annotations".into(),
            fix_template: "Add type hints to function parameters and return types".into(),
            severity: "low".into(),
            languages: vec!["python".into()],
            tags: vec!["types".into(), "python".into()],
        },
        Template {
            name: "fix-readme-badges".into(),
            description: "Fix broken or missing README badges".into(),
            template_type: "docs_improve".into(),
            pattern: "README missing CI/coverage badges".into(),
            fix_template: "Add shields.io badges for CI status, coverage, license".into(),
            severity: "low".into(),
            languages: vec![],
            tags: vec!["docs".into(), "readme".into()],
        },
        Template {
            name: "add-license".into(),
            description: "Add missing LICENSE file".into(),
            template_type: "docs_improve".into(),
            pattern: "Repository missing LICENSE file".into(),
            fix_template: "Add appropriate open source license (MIT/Apache-2.0)".into(),
            severity: "medium".into(),
            languages: vec![],
            tags: vec!["license".into(), "legal".into()],
        },
    ]
}

/// Registry that manages contribution templates.
pub struct TemplateRegistry {
    templates: HashMap<String, Template>,
}

impl TemplateRegistry {
    /// Create a new registry with built-in templates loaded.
    pub fn new() -> Self {
        let mut registry = Self {
            templates: HashMap::new(),
        };
        for tpl in builtin_templates() {
            registry.templates.insert(tpl.name.clone(), tpl);
        }
        info!(
            count = registry.templates.len(),
            "Loaded built-in templates"
        );
        registry
    }

    /// Load templates from YAML strings.
    pub fn load_yaml(&mut self, yaml_str: &str) {
        match serde_yaml::from_str::<Template>(yaml_str) {
            Ok(tpl) => {
                self.templates.insert(tpl.name.clone(), tpl);
            }
            Err(e) => {
                warn!(error = %e, "Failed to load template YAML");
            }
        }
    }

    /// Get a template by name.
    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }

    /// List all loaded templates.
    pub fn list_all(&self) -> Vec<&Template> {
        self.templates.values().collect()
    }

    /// Filter templates by contribution type.
    pub fn filter_by_type(&self, contrib_type: &str) -> Vec<&Template> {
        self.templates
            .values()
            .filter(|t| t.template_type == contrib_type)
            .collect()
    }

    /// Filter templates applicable to a language.
    pub fn filter_by_language(&self, language: &str) -> Vec<&Template> {
        let lang = language.to_lowercase();
        self.templates
            .values()
            .filter(|t| {
                t.languages.is_empty()
                    || t.languages.iter().any(|l| l.to_lowercase() == lang)
            })
            .collect()
    }

    /// Total number of templates.
    pub fn count(&self) -> usize {
        self.templates.len()
    }
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_loaded() {
        let r = TemplateRegistry::new();
        assert_eq!(r.count(), 5);
    }

    #[test]
    fn test_get_template() {
        let r = TemplateRegistry::new();
        let tpl = r.get("security-headers").unwrap();
        assert_eq!(tpl.severity, "high");
        assert!(tpl.languages.contains(&"python".to_string()));
    }

    #[test]
    fn test_get_missing_template() {
        let r = TemplateRegistry::new();
        assert!(r.get("nonexistent").is_none());
    }

    #[test]
    fn test_filter_by_type() {
        let r = TemplateRegistry::new();
        let security = r.filter_by_type("security_fix");
        assert_eq!(security.len(), 1);
        assert_eq!(security[0].name, "security-headers");
    }

    #[test]
    fn test_filter_by_language_python() {
        let r = TemplateRegistry::new();
        let python = r.filter_by_language("python");
        // Should include python-specific + language-agnostic templates
        assert!(python.len() >= 3);
    }

    #[test]
    fn test_filter_by_language_case_insensitive() {
        let r = TemplateRegistry::new();
        let py1 = r.filter_by_language("Python");
        let py2 = r.filter_by_language("python");
        assert_eq!(py1.len(), py2.len());
    }

    #[test]
    fn test_list_all() {
        let r = TemplateRegistry::new();
        assert_eq!(r.list_all().len(), 5);
    }
}
