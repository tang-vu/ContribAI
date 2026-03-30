//! Plugin system — extensible analyzer/generator plugins.
//!
//! Port from Python `plugins/base.py`.
//! Uses trait-based plugin architecture (instead of Python entry points).

use async_trait::async_trait;
use tracing::{info, warn};

use crate::core::error::Result;
use crate::core::models::{Contribution, Finding, RepoContext};

// ── Base traits ──────────────────────────────────────

/// Base trait for analyzer plugins.
#[async_trait]
pub trait AnalyzerPlugin: Send + Sync {
    /// Plugin name for identification.
    fn name(&self) -> &str;

    /// Plugin version.
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// Analyze a repository and return findings.
    async fn analyze(&self, context: &RepoContext) -> Result<Vec<Finding>>;
}

/// Base trait for generator plugins.
#[async_trait]
pub trait GeneratorPlugin: Send + Sync {
    /// Plugin name for identification.
    fn name(&self) -> &str;

    /// Plugin version.
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// Generate a contribution for a finding.
    async fn generate(
        &self,
        finding: &Finding,
        context: &RepoContext,
    ) -> Result<Option<Contribution>>;
}

// ── Registry ─────────────────────────────────────────

/// Discovers and manages plugins.
pub struct PluginRegistry {
    analyzers: Vec<Box<dyn AnalyzerPlugin>>,
    generators: Vec<Box<dyn GeneratorPlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            analyzers: Vec::new(),
            generators: Vec::new(),
        }
    }

    /// Register an analyzer plugin.
    pub fn register_analyzer(&mut self, plugin: Box<dyn AnalyzerPlugin>) {
        info!(
            name = plugin.name(),
            version = plugin.version(),
            "Registered analyzer plugin"
        );
        self.analyzers.push(plugin);
    }

    /// Register a generator plugin.
    pub fn register_generator(&mut self, plugin: Box<dyn GeneratorPlugin>) {
        info!(
            name = plugin.name(),
            version = plugin.version(),
            "Registered generator plugin"
        );
        self.generators.push(plugin);
    }

    /// List all registered analyzer plugins.
    pub fn analyzer_names(&self) -> Vec<&str> {
        self.analyzers.iter().map(|p| p.name()).collect()
    }

    /// List all registered generator plugins.
    pub fn generator_names(&self) -> Vec<&str> {
        self.generators.iter().map(|p| p.name()).collect()
    }

    /// Run all analyzer plugins on a context.
    pub async fn run_analyzers(&self, context: &RepoContext) -> Vec<Finding> {
        let mut all_findings = Vec::new();
        for plugin in &self.analyzers {
            match plugin.analyze(context).await {
                Ok(findings) => {
                    info!(
                        plugin = plugin.name(),
                        findings = findings.len(),
                        "Plugin analysis complete"
                    );
                    all_findings.extend(findings);
                }
                Err(e) => {
                    warn!(
                        plugin = plugin.name(),
                        error = %e,
                        "Plugin analysis failed"
                    );
                }
            }
        }
        all_findings
    }

    /// Run all generator plugins on a finding.
    pub async fn run_generators(
        &self,
        finding: &Finding,
        context: &RepoContext,
    ) -> Vec<Contribution> {
        let mut contribs = Vec::new();
        for plugin in &self.generators {
            match plugin.generate(finding, context).await {
                Ok(Some(c)) => contribs.push(c),
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        plugin = plugin.name(),
                        error = %e,
                        "Plugin generation failed"
                    );
                }
            }
        }
        contribs
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::*;
    use std::collections::HashMap;

    struct MockAnalyzer;
    #[async_trait]
    impl AnalyzerPlugin for MockAnalyzer {
        fn name(&self) -> &str { "mock-analyzer" }
        fn version(&self) -> &str { "1.0.0" }
        async fn analyze(&self, _ctx: &RepoContext) -> Result<Vec<Finding>> {
            Ok(vec![Finding {
                id: "mock-1".into(),
                finding_type: ContributionType::CodeQuality,
                severity: Severity::Low,
                title: "Mock finding".into(),
                description: "test".into(),
                file_path: "test.py".into(),
                line_start: None,
                line_end: None,
                suggestion: None,
                confidence: 0.9,
                priority_signals: vec![],
            }])
        }
    }

    struct FailAnalyzer;
    #[async_trait]
    impl AnalyzerPlugin for FailAnalyzer {
        fn name(&self) -> &str { "fail-analyzer" }
        async fn analyze(&self, _ctx: &RepoContext) -> Result<Vec<Finding>> {
            Err(crate::core::error::ContribError::GitHub("fail".into()))
        }
    }

    fn make_context() -> RepoContext {
        RepoContext {
            repo: Repository {
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
            file_tree: vec![],
            readme_content: None,
            contributing_guide: None,
            relevant_files: HashMap::new(),
            open_issues: vec![],
            coding_style: None,
            symbol_map: HashMap::new(),
            file_ranks: HashMap::new(),
        }
    }

    #[test]
    fn test_empty_registry() {
        let r = PluginRegistry::new();
        assert!(r.analyzer_names().is_empty());
        assert!(r.generator_names().is_empty());
    }

    #[test]
    fn test_register_analyzer() {
        let mut r = PluginRegistry::new();
        r.register_analyzer(Box::new(MockAnalyzer));
        assert_eq!(r.analyzer_names(), vec!["mock-analyzer"]);
    }

    #[tokio::test]
    async fn test_run_analyzers() {
        let mut r = PluginRegistry::new();
        r.register_analyzer(Box::new(MockAnalyzer));
        let ctx = make_context();
        let findings = r.run_analyzers(&ctx).await;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "mock-1");
    }

    #[tokio::test]
    async fn test_run_analyzers_with_failure() {
        let mut r = PluginRegistry::new();
        r.register_analyzer(Box::new(MockAnalyzer));
        r.register_analyzer(Box::new(FailAnalyzer));
        let ctx = make_context();
        // Should still get findings from MockAnalyzer despite FailAnalyzer error
        let findings = r.run_analyzers(&ctx).await;
        assert_eq!(findings.len(), 1);
    }
}
