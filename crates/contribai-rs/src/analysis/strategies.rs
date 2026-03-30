//! Framework-specific analysis strategies.
//!
//! Port from Python `analysis/strategies.py`.
//! Detects frameworks (Django, Flask, FastAPI, React, Express)
//! and applies targeted analysis rules.

use crate::core::models::RepoContext;

/// Detected framework metadata.
#[derive(Debug, Clone)]
pub struct FrameworkInfo {
    pub name: String,
    pub version: Option<String>,
    pub config_file: Option<String>,
}

/// A framework analysis strategy.
pub trait FrameworkStrategy: Send + Sync {
    /// Framework name.
    fn name(&self) -> &str;
    /// Detect if this framework is used in the repo.
    fn detect(&self, context: &RepoContext) -> Option<FrameworkInfo>;
    /// Get framework-specific analysis prompt.
    fn get_analysis_prompt(&self, info: &FrameworkInfo) -> String;
    /// Get framework-specific critical files to analyze.
    fn get_critical_files(&self, context: &RepoContext) -> Vec<String>;
}

// ── Django ─────────────────────────────────────────────

struct DjangoStrategy;

impl FrameworkStrategy for DjangoStrategy {
    fn name(&self) -> &str { "Django" }

    fn detect(&self, context: &RepoContext) -> Option<FrameworkInfo> {
        for f in &context.file_tree {
            if f.path.ends_with("manage.py") || f.path.ends_with("settings.py") {
                return Some(FrameworkInfo {
                    name: "Django".into(),
                    version: None,
                    config_file: Some(f.path.clone()),
                });
            }
        }
        for (path, content) in &context.relevant_files {
            if (path.to_lowercase().contains("requirements") || path == "pyproject.toml")
                && content.to_lowercase().contains("django")
            {
                return Some(FrameworkInfo {
                    name: "Django".into(),
                    version: None,
                    config_file: Some(path.clone()),
                });
            }
        }
        None
    }

    fn get_analysis_prompt(&self, _info: &FrameworkInfo) -> String {
        "Analyze this Django project for:\n\
         1. Security: CSRF, SQL injection, DEBUG=True, SECRET_KEY exposure\n\
         2. Best Practices: Missing migrations, N+1 queries, fat views\n\
         3. Common Issues: Missing __str__, no admin registration\n\
         4. Performance: Unbounded querysets, missing indexes".into()
    }

    fn get_critical_files(&self, context: &RepoContext) -> Vec<String> {
        let patterns = ["settings.py", "urls.py", "views.py", "models.py", "forms.py"];
        context.file_tree.iter()
            .filter(|f| patterns.iter().any(|p| f.path.ends_with(p)))
            .map(|f| f.path.clone())
            .collect()
    }
}

// ── Flask ──────────────────────────────────────────────

struct FlaskStrategy;

impl FrameworkStrategy for FlaskStrategy {
    fn name(&self) -> &str { "Flask" }

    fn detect(&self, context: &RepoContext) -> Option<FrameworkInfo> {
        for (path, content) in &context.relevant_files {
            if content.contains("from flask") || content.contains("import flask") {
                return Some(FrameworkInfo { name: "Flask".into(), version: None, config_file: Some(path.clone()) });
            }
        }
        None
    }

    fn get_analysis_prompt(&self, _info: &FrameworkInfo) -> String {
        "Analyze this Flask project for:\n\
         1. Security: Missing CSRF, SQL injection, debug mode, SECRET_KEY\n\
         2. Best Practices: Missing error handlers, no blueprints\n\
         3. Common Issues: No request validation, missing CORS\n\
         4. Performance: No connection pooling, missing caching".into()
    }

    fn get_critical_files(&self, context: &RepoContext) -> Vec<String> {
        let patterns = ["app.py", "wsgi.py", "config.py", "routes.py"];
        context.file_tree.iter()
            .filter(|f| patterns.iter().any(|p| f.path.ends_with(p)))
            .map(|f| f.path.clone())
            .collect()
    }
}

// ── FastAPI ────────────────────────────────────────────

struct FastAPIStrategy;

impl FrameworkStrategy for FastAPIStrategy {
    fn name(&self) -> &str { "FastAPI" }

    fn detect(&self, context: &RepoContext) -> Option<FrameworkInfo> {
        for (path, content) in &context.relevant_files {
            if content.contains("from fastapi") || content.contains("import fastapi") {
                return Some(FrameworkInfo { name: "FastAPI".into(), version: None, config_file: Some(path.clone()) });
            }
        }
        None
    }

    fn get_analysis_prompt(&self, _info: &FrameworkInfo) -> String {
        "Analyze this FastAPI project for:\n\
         1. Security: Missing auth, CORS misconfiguration, exposed debug\n\
         2. Best Practices: Missing response models, sync in async endpoints\n\
         3. Common Issues: Missing Depends(), no health check\n\
         4. Performance: Missing async DB driver, blocking I/O".into()
    }

    fn get_critical_files(&self, context: &RepoContext) -> Vec<String> {
        let patterns = ["main.py", "app.py", "routers/", "schemas.py", "models.py"];
        context.file_tree.iter()
            .filter(|f| patterns.iter().any(|p| f.path.contains(p)))
            .map(|f| f.path.clone())
            .collect()
    }
}

// ── React ──────────────────────────────────────────────

struct ReactStrategy;

impl FrameworkStrategy for ReactStrategy {
    fn name(&self) -> &str { "React" }

    fn detect(&self, context: &RepoContext) -> Option<FrameworkInfo> {
        for (path, content) in &context.relevant_files {
            if path == "package.json" && (content.contains("\"react\"") || content.contains("\"next\"")) {
                let name = if content.contains("\"next\"") { "Next.js" } else { "React" };
                return Some(FrameworkInfo { name: name.into(), version: None, config_file: Some(path.clone()) });
            }
        }
        for f in &context.file_tree {
            if f.path.ends_with(".jsx") || f.path.ends_with(".tsx") {
                return Some(FrameworkInfo { name: "React".into(), version: None, config_file: None });
            }
        }
        None
    }

    fn get_analysis_prompt(&self, info: &FrameworkInfo) -> String {
        format!(
            "Analyze this {} project for:\n\
             1. Security: XSS via dangerouslySetInnerHTML, exposed API keys\n\
             2. Performance: Missing React.memo, no useMemo/useCallback\n\
             3. Accessibility: Missing alt text, no ARIA labels\n\
             4. Best Practices: Missing types, no error boundaries",
            info.name
        )
    }

    fn get_critical_files(&self, context: &RepoContext) -> Vec<String> {
        let patterns = [".jsx", ".tsx", "App.", "index.", "page.", "package.json"];
        context.file_tree.iter()
            .filter(|f| patterns.iter().any(|p| f.path.contains(p)))
            .take(20)
            .map(|f| f.path.clone())
            .collect()
    }
}

// ── Express ────────────────────────────────────────────

struct ExpressStrategy;

impl FrameworkStrategy for ExpressStrategy {
    fn name(&self) -> &str { "Express" }

    fn detect(&self, context: &RepoContext) -> Option<FrameworkInfo> {
        for (path, content) in &context.relevant_files {
            if (path == "package.json" && content.contains("\"express\""))
                || content.contains("require('express')")
                || content.contains("from \"express\"")
            {
                return Some(FrameworkInfo { name: "Express".into(), version: None, config_file: Some(path.clone()) });
            }
        }
        None
    }

    fn get_analysis_prompt(&self, _info: &FrameworkInfo) -> String {
        "Analyze this Express.js project for:\n\
         1. Security: Missing helmet, no rate limiting, CORS, injection\n\
         2. Best Practices: No error handling middleware, missing async error\n\
         3. Common Issues: Callback hell, no request validation\n\
         4. Performance: No compression, missing caching headers".into()
    }

    fn get_critical_files(&self, context: &RepoContext) -> Vec<String> {
        let patterns = ["app.js", "server.js", "index.js", "routes/", "middleware/"];
        context.file_tree.iter()
            .filter(|f| patterns.iter().any(|p| f.path.contains(p)))
            .take(15)
            .map(|f| f.path.clone())
            .collect()
    }
}

// ── Registry ───────────────────────────────────────────

/// All available framework strategies.
fn all_strategies() -> Vec<Box<dyn FrameworkStrategy>> {
    vec![
        Box::new(DjangoStrategy),
        Box::new(FlaskStrategy),
        Box::new(FastAPIStrategy),
        Box::new(ReactStrategy),
        Box::new(ExpressStrategy),
    ]
}

/// Detect all frameworks used in a repository.
pub fn detect_frameworks(context: &RepoContext) -> Vec<(Box<dyn FrameworkStrategy>, FrameworkInfo)> {
    let mut detected = Vec::new();
    for strategy in all_strategies() {
        if let Some(info) = strategy.detect(context) {
            detected.push((strategy, info));
        }
    }
    detected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::FileNode;
    use std::collections::HashMap;

    fn make_context(files: &[&str], relevant: HashMap<String, String>) -> RepoContext {
        RepoContext {
            repo: crate::core::models::Repository {
                owner: "test".into(),
                name: "test".into(),
                full_name: "test/test".into(),
                description: None,
                language: Some("python".into()),
                languages: HashMap::new(),
                stars: 0,
                forks: 0,
                open_issues: 0,
                topics: vec![],
                default_branch: "main".into(),
                html_url: String::new(),
                clone_url: String::new(),
                has_contributing: false,
                has_license: false,
                last_push_at: None,
                created_at: None,
            },
            file_tree: files.iter().map(|p| FileNode {
                path: p.to_string(),
                node_type: "blob".into(),
                size: 0,
                sha: String::new(),
            }).collect(),
            readme_content: None,
            contributing_guide: None,
            relevant_files: relevant,
            open_issues: vec![],
            coding_style: None,
            symbol_map: HashMap::new(),
            file_ranks: HashMap::new(),
        }
    }

    #[test]
    fn test_detect_django() {
        let ctx = make_context(&["manage.py", "app/settings.py"], HashMap::new());
        let detected = detect_frameworks(&ctx);
        assert!(!detected.is_empty());
        assert_eq!(detected[0].1.name, "Django");
    }

    #[test]
    fn test_detect_react_jsx() {
        let ctx = make_context(&["src/App.jsx", "src/index.tsx"], HashMap::new());
        let detected = detect_frameworks(&ctx);
        assert!(!detected.is_empty());
        assert_eq!(detected[0].1.name, "React");
    }

    #[test]
    fn test_detect_nextjs_via_package() {
        let mut files = HashMap::new();
        files.insert("package.json".into(), r#"{"dependencies": {"next": "14.0"}}"#.into());
        let ctx = make_context(&["package.json"], files);
        let detected = detect_frameworks(&ctx);
        assert!(!detected.is_empty());
        assert_eq!(detected[0].1.name, "Next.js");
    }

    #[test]
    fn test_no_framework_detected() {
        let ctx = make_context(&["main.rs", "Cargo.toml"], HashMap::new());
        let detected = detect_frameworks(&ctx);
        assert!(detected.is_empty());
    }
}
