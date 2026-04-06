//! Main code analysis orchestrator.
//!
//! Port from Python `analysis/analyzer.py`.
//! Runs multiple analyzers in parallel, now enhanced with
//! tree-sitter AST analysis, PageRank file prioritization,
//! and weighted triage scoring.

use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{debug, info, warn};

use super::ast_intel::AstIntel;
use super::compressor::ContextCompressor;
use super::repo_map;
use super::skills;
use super::triage::TriageEngine;
use crate::core::config::AnalysisConfig;
use crate::core::error::Result;
use crate::core::models::{
    AnalysisResult, ContributionType, Finding, Repository, Severity, Symbol,
};
use crate::github::client::GitHubClient;
use crate::llm::provider::LlmProvider;

/// File extensions we can meaningfully analyze.
const ANALYZABLE_EXTENSIONS: &[&str] = &[
    "py", "js", "ts", "jsx", "tsx", "java", "go", "rs", "rb", "php", "c", "cpp", "h", "hpp", "cs",
    "swift", "kt", "html", "css", "scss", "vue", "svelte",
];

/// Orchestrates multiple code analyzers using LLM + AST.
pub struct CodeAnalyzer<'a> {
    llm: &'a dyn LlmProvider,
    github: &'a GitHubClient,
    config: &'a AnalysisConfig,
    compressor: ContextCompressor,
}

impl<'a> CodeAnalyzer<'a> {
    pub fn new(
        llm: &'a dyn LlmProvider,
        github: &'a GitHubClient,
        config: &'a AnalysisConfig,
    ) -> Self {
        Self {
            llm,
            github,
            config,
            compressor: ContextCompressor::new(config.max_context_tokens),
        }
    }

    /// Run full analysis on a repository.
    ///
    /// Pipeline:
    /// 1. Fetch file tree
    /// 2. Select + prioritize files (PageRank)
    /// 3. Extract AST symbols (tree-sitter)
    /// 4. Run LLM analyzers in parallel
    /// 5. Triage and score findings
    pub async fn analyze(&self, repo: &Repository) -> Result<AnalysisResult> {
        let start = Instant::now();

        // 1. Fetch file tree
        let file_tree = self
            .github
            .get_file_tree(&repo.owner, &repo.name, None)
            .await?;
        let file_paths: Vec<String> = file_tree.iter().map(|f| f.path.clone()).collect();
        let analyzable = self.select_files(&file_paths);

        info!(
            repo = %repo.full_name,
            total = file_paths.len(),
            selected = analyzable.len(),
            "Files selected for analysis"
        );

        // 2. Fetch file contents for selected files
        let mut file_contents: HashMap<String, String> = HashMap::new();
        let total_files = analyzable.len();
        for (i, path) in analyzable.iter().enumerate() {
            if (i + 1) % 50 == 0 || i + 1 == total_files {
                info!(
                    repo = %repo.full_name,
                    progress = format!("{}/{}", i + 1, total_files),
                    "📥 Fetching file contents"
                );
            }
            match self
                .github
                .get_file_content(&repo.owner, &repo.name, path, None)
                .await
            {
                Ok(content) => {
                    file_contents.insert(path.clone(), content);
                }
                Err(e) => {
                    debug!(path = path, error = %e, "Skipping file");
                }
            }
        }

        // 3. AST analysis (tree-sitter) — NEW
        let mut all_symbols: Vec<Symbol> = Vec::new();
        let mut import_graph: HashMap<String, Vec<String>> = HashMap::new();

        for (path, content) in &file_contents {
            if let Ok(symbols) = AstIntel::extract_symbols(content, path) {
                all_symbols.extend(symbols);
            }
            let imports = AstIntel::count_imports(content, path);
            import_graph.insert(path.clone(), imports);
        }

        // 4. PageRank file importance — NEW
        let file_ranks = repo_map::rank_files(&import_graph);
        let top_files = repo_map::top_files(&file_ranks, 20);
        info!(top = top_files.len(), "PageRank: top files identified");

        // 5. Select relevant skills
        let language = repo.language.as_deref().unwrap_or("unknown");
        let frameworks: HashSet<String> = HashSet::new(); // TODO: detect from imports
        let active_skills = skills::select_skills(language, &frameworks);
        info!(
            skills = active_skills.len(),
            language = language,
            "Active analysis skills"
        );

        // 6. Build context and run LLM analysis
        let context = self.build_context(repo, &file_contents, &all_symbols, &top_files);
        let mut all_findings: Vec<Finding> = Vec::new();

        // Run all analyzers in parallel (~3-4x speedup vs sequential)
        let analyzer_futures: Vec<_> = self
            .config
            .enabled_analyzers
            .iter()
            .map(|name| self.run_analyzer(name, &context))
            .collect();

        info!(
            analyzers = self.config.enabled_analyzers.len(),
            "🚀 Running analyzers in parallel"
        );
        let results = futures::future::join_all(analyzer_futures).await;

        for (i, result) in results.into_iter().enumerate() {
            let name = &self.config.enabled_analyzers[i];
            match result {
                Ok(findings) => {
                    info!(
                        analyzer = name,
                        findings = findings.len(),
                        "Analyzer complete"
                    );
                    all_findings.extend(findings);
                }
                Err(e) => warn!(analyzer = name, error = %e, "Analyzer failed"),
            }
        }

        // 7. Triage and score — NEW
        let specs = TriageEngine::triage(all_findings.clone());
        info!(
            findings = all_findings.len(),
            triaged = specs.len(),
            "Triage complete"
        );

        // 8. Deduplicate
        let findings = self.deduplicate(all_findings);

        let duration = start.elapsed();
        Ok(AnalysisResult {
            repo: repo.clone(),
            findings,
            analyzed_files: file_contents.len(),
            skipped_files: file_paths.len() - file_contents.len(),
            analysis_duration_sec: duration.as_secs_f64(),
        })
    }

    /// Select analyzable files from file tree.
    fn select_files(&self, file_tree: &[String]) -> Vec<String> {
        file_tree
            .iter()
            .filter(|path| {
                // Check extension
                let ext = path.rsplit('.').next().unwrap_or("");
                if !ANALYZABLE_EXTENSIONS.contains(&ext) {
                    return false;
                }

                // Check skip patterns
                for pattern in &self.config.skip_patterns {
                    if path.contains(pattern.as_str()) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect()
    }

    /// Build compressed context string for LLM prompt.
    fn build_context(
        &self,
        repo: &Repository,
        files: &HashMap<String, String>,
        symbols: &[Symbol],
        top_files: &[(&String, f64)],
    ) -> String {
        let mut parts = Vec::new();

        // Repo overview
        parts.push(format!(
            "# Repository: {}\nLanguage: {}\nStars: {}\nOpen Issues: {}\n",
            repo.full_name,
            repo.language.as_deref().unwrap_or("unknown"),
            repo.stars,
            repo.open_issues
        ));

        // PageRank top files
        if !top_files.is_empty() {
            parts.push("## Most Important Files (PageRank)".to_string());
            for (path, score) in top_files.iter().take(10) {
                parts.push(format!("  - {} (importance: {:.2})", path, score));
            }
            parts.push(String::new());
        }

        // AST symbols summary
        if !symbols.is_empty() {
            parts.push(format!(
                "## Code Structure ({} symbols)\n{}",
                symbols.len(),
                AstIntel::symbols_summary(symbols)
            ));
            parts.push(String::new());
        }

        // File contents (compressed)
        let file_pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let compressed = self.compressor.compress_files(&file_pairs, 3000);

        for (path, content) in &compressed {
            parts.push(format!("## File: {}\n```\n{}\n```\n", path, content));
        }

        parts.join("\n")
    }

    /// Run a single analyzer using LLM with retry + exponential backoff.
    ///
    /// Retries up to 3 times on transient errors (timeout, 429, 5xx).
    /// Does NOT retry on 400/401 (bad request, auth failure).
    async fn run_analyzer(&self, name: &str, context: &str) -> Result<Vec<Finding>> {
        let system_prompt = format!(
            "You are an expert {} code analyzer for open source contributions. \
             Analyze the code and report findings as JSON array. \
             Each finding must have: title, description, severity (critical/high/medium/low), \
             file_path, line_start, line_end, suggestion, confidence (0-1).",
            name
        );

        let prompt = format!(
            "Analyze this repository for {} issues:\n\n{}\n\n\
             Respond with a JSON array of findings. Be specific and actionable.",
            name, context
        );

        // Retry with exponential backoff: 2s → 4s → 8s (max 3 attempts)
        let mut last_error = None;
        let delays = [2u64, 4, 8];

        for attempt in 0..=delays.len() {
            match self
                .llm
                .complete(&prompt, Some(&system_prompt), None, None)
                .await
            {
                Ok(response) => {
                    if attempt > 0 {
                        info!(
                            analyzer = name,
                            attempt = attempt + 1,
                            "Analyzer succeeded after retry"
                        );
                    }
                    // Parse findings from LLM response
                    let findings = self.parse_findings(&response, name);
                    return Ok(findings);
                }
                Err(e) => {
                    let is_transient = is_transient_llm_error(&e);
                    warn!(
                        analyzer = name,
                        attempt = attempt + 1,
                        error = %e,
                        transient = is_transient,
                        "Analyzer attempt failed"
                    );

                    if !is_transient {
                        // Non-transient error (auth, bad request) — don't retry
                        return Err(e);
                    }

                    last_error = Some(e);

                    if attempt < delays.len() {
                        let delay = delays[attempt];
                        info!(
                            analyzer = name,
                            delay_secs = delay,
                            "Waiting before retry..."
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    }
                }
            }
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| {
            crate::core::error::ContribError::Llm("Analyzer exhausted after retries".into())
        }))
    }

    /// Parse LLM response into Finding objects.
    fn parse_findings(&self, response: &str, analyzer: &str) -> Vec<Finding> {
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
            Err(e) => {
                warn!(analyzer = analyzer, error = %e, "Failed to parse LLM findings");
                vec![]
            }
        }
    }

    /// Deduplicate findings by file + title.
    fn deduplicate(&self, findings: Vec<Finding>) -> Vec<Finding> {
        let mut seen: HashSet<String> = HashSet::new();
        findings
            .into_iter()
            .filter(|f| {
                let key = format!("{}:{}", f.file_path, f.title);
                seen.insert(key)
            })
            .collect()
    }
}

/// Check if an LLM error is transient (retryable).
///
/// Transient errors: timeout, 429 (rate limit), 5xx (server error), HTTP errors.
/// Non-transient: 400 (bad request), 401 (auth failure), JSON parse errors.
fn is_transient_llm_error(e: &crate::core::error::ContribError) -> bool {
    match e {
        crate::core::error::ContribError::Llm(msg) => {
            // Check for known transient patterns
            msg.contains("429")
                || msg.contains("rate limit")
                || msg.contains("timeout")
                || msg.contains("500")
                || msg.contains("502")
                || msg.contains("503")
                || msg.contains("504")
                || msg.contains("HTTP error")
                || msg.contains("response read")
        }
        crate::core::error::ContribError::Http(_) => true, // HTTP errors are transient
        _ => false, // Config, JSON, DB errors are NOT transient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_files() {
        let tree = vec![
            "src/main.py".to_string(),
            "src/utils.js".to_string(),
            "README.md".to_string(),
            "LICENSE".to_string(),
            "data/test.csv".to_string(),
            "src/config.rs".to_string(),
        ];

        let _config = AnalysisConfig::default();
        // Can't create real CodeAnalyzer without LlmProvider, test logic directly
        let analyzable: Vec<_> = tree
            .iter()
            .filter(|path| {
                let ext = path.rsplit('.').next().unwrap_or("");
                ANALYZABLE_EXTENSIONS.contains(&ext)
            })
            .collect();

        assert_eq!(analyzable.len(), 3);
        assert!(analyzable.iter().any(|p| p.as_str() == "src/main.py"));
        assert!(analyzable.iter().any(|p| p.as_str() == "src/utils.js"));
        assert!(analyzable.iter().any(|p| p.as_str() == "src/config.rs"));
    }

    #[test]
    fn test_deduplicate() {
        let findings = vec![
            Finding {
                id: "1".into(),
                finding_type: ContributionType::SecurityFix,
                severity: Severity::High,
                title: "SQL injection".into(),
                description: "Found in query".into(),
                file_path: "db.py".into(),
                line_start: Some(10),
                line_end: Some(15),
                suggestion: None,
                confidence: 0.9,
                priority_signals: vec![],
            },
            Finding {
                id: "2".into(),
                finding_type: ContributionType::SecurityFix,
                severity: Severity::High,
                title: "SQL injection".into(), // duplicate
                description: "Same issue".into(),
                file_path: "db.py".into(),
                line_start: Some(10),
                line_end: Some(15),
                suggestion: None,
                confidence: 0.8,
                priority_signals: vec![],
            },
        ];

        let mut seen: HashSet<String> = HashSet::new();
        let deduped: Vec<_> = findings
            .into_iter()
            .filter(|f| {
                let key = format!("{}:{}", f.file_path, f.title);
                seen.insert(key)
            })
            .collect();

        assert_eq!(deduped.len(), 1);
    }

    // ── Transient error detection ─────────────────────────────────────────

    #[test]
    fn test_transient_error_timeout() {
        let e = crate::core::error::ContribError::Llm("Gemini API error: timeout".into());
        assert!(is_transient_llm_error(&e));
    }

    #[test]
    fn test_transient_error_rate_limit() {
        let e = crate::core::error::ContribError::Llm(
            "Gemini rate limit: 429 Too Many Requests".into(),
        );
        assert!(is_transient_llm_error(&e));
    }

    #[test]
    fn test_transient_error_500() {
        let e = crate::core::error::ContribError::Llm(
            "Gemini API error 500: Internal Server Error".into(),
        );
        assert!(is_transient_llm_error(&e));
    }

    #[test]
    fn test_transient_error_http() {
        let e =
            crate::core::error::ContribError::Llm("Gemini HTTP error: connection refused".into());
        assert!(is_transient_llm_error(&e));
    }

    #[test]
    fn test_non_transient_error_400() {
        let e = crate::core::error::ContribError::Llm("Gemini API error 400: Bad request".into());
        assert!(!is_transient_llm_error(&e));
    }

    #[test]
    fn test_non_transient_error_auth() {
        let e = crate::core::error::ContribError::Llm("GEMINI_API_KEY not set".into());
        assert!(!is_transient_llm_error(&e));
    }

    #[test]
    fn test_non_transient_json_parse() {
        let e = crate::core::error::ContribError::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "test",
        )));
        assert!(!is_transient_llm_error(&e));
    }
}
