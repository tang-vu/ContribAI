//! LLM-powered contribution generator.
//!
//! Port from Python `generator/engine.py`.
//! Takes findings from analysis and generates actual code changes,
//! tests, and commit messages that follow the target repo's conventions.

use chrono::Utc;
use regex::Regex;
use std::sync::LazyLock;
use tracing::{info, warn};

use crate::core::safe_truncate;

static RE_SLUG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

use crate::core::config::ContributionConfig;
use crate::core::error::Result;
use crate::core::models::{
    Contribution, ContributionType, FileChange, Finding, RepoContext,
};
use crate::github::guidelines::{adapt_pr_title, extract_scope_from_path, RepoGuidelines};
use crate::llm::provider::LlmProvider;

// ── Generator struct ─────────────────────────────────────────────────────────

/// Generate code contributions from analysis findings.
pub struct ContributionGenerator<'a> {
    pub(crate) llm: &'a dyn LlmProvider,
    pub(crate) config: &'a ContributionConfig,
    /// Enable LLM self-review gate after generation (default: true).
    pub(crate) self_review_enabled: bool,
}

impl<'a> ContributionGenerator<'a> {
    pub fn new(llm: &'a dyn LlmProvider, config: &'a ContributionConfig) -> Self {
        Self {
            llm,
            config,
            self_review_enabled: true,
        }
    }

    /// Disable self-review (useful for batch pipelines where latency matters).
    pub fn without_self_review(mut self) -> Self {
        self.self_review_enabled = false;
        self
    }

    /// Generate a contribution for a single finding.
    ///
    /// Pipeline:
    /// 1. Build context-aware prompt
    /// 2. Get LLM to generate the fix
    /// 3. Parse structured output into FileChanges (with search/replace)
    /// 4. Generate commit message
    /// 5. Optional self-review LLM gate
    pub async fn generate(
        &self,
        finding: &Finding,
        context: &RepoContext,
    ) -> Result<Option<Contribution>> {
        self.generate_with_guidelines(finding, context, None).await
    }

    /// Generate a contribution, optionally adapting PR title to repo guidelines.
    pub async fn generate_with_guidelines(
        &self,
        finding: &Finding,
        context: &RepoContext,
        guidelines: Option<&RepoGuidelines>,
    ) -> Result<Option<Contribution>> {
        // 1. Build prompts
        let system = self.build_system_prompt(context);
        let prompt = self.build_generation_prompt(finding, context);

        // 2. Generate with retry (max 1 retry = 2 attempts)
        let mut changes: Option<Vec<FileChange>> = None;
        let mut last_error = String::new();

        for attempt in 0..2 {
            let actual_prompt = if attempt > 0 {
                format!(
                    "{}\n\n## IMPORTANT: Your previous attempt failed.\n\
                     Error: {}\n\
                     Please fix the issue and return ONLY valid JSON \
                     with no markdown fences or extra text.",
                    prompt, last_error
                )
            } else {
                prompt.clone()
            };

            let response = self
                .llm
                .complete(&actual_prompt, Some(&system), Some(0.2), None)
                .await?;

            // 3. Parse changes (search/replace or full-content format)
            match self.parse_changes(&response, context) {
                Some(c) if !c.is_empty() => {
                    if self.validate_changes(&c) {
                        changes = Some(c);
                        break;
                    } else {
                        last_error = "Generated code failed syntax validation \
                                     (unbalanced brackets or empty edits)"
                            .into();
                    }
                }
                _ => {
                    last_error = "No valid changes could be parsed from JSON output".into();
                }
            }
        }

        let changes = match changes {
            Some(c) => c,
            None => {
                warn!(title = %finding.title, "No valid changes after retries");
                return Ok(None);
            }
        };

        // 4. Generate commit message
        let commit_msg = self.generate_commit_message(finding, &changes);

        // 5. Generate branch name
        let branch_name = Self::generate_branch_name(finding);

        // 6. Generate PR title (adapted to guidelines if available)
        let pr_title = Self::generate_pr_title_with_guidelines(finding, guidelines);

        let contribution = Contribution {
            finding: finding.clone(),
            contribution_type: finding.finding_type.clone(),
            title: pr_title,
            description: finding.description.clone(),
            changes,
            commit_message: commit_msg,
            tests_added: vec![],
            branch_name,
            generated_at: Utc::now(),
        };

        // 7. Optional self-review LLM gate
        if self.self_review_enabled {
            let approved = self.self_review(&contribution, context).await;
            if !approved {
                warn!(title = %finding.title, "Self-review rejected contribution");
                return Ok(None);
            }
        }

        info!(
            title = %contribution.title,
            files = contribution.total_files_changed(),
            "Generated contribution"
        );

        Ok(Some(contribution))
    }

    // ── Prompt builders ──────────────────────────────────────────────────────

    /// Build system prompt with repo context and style guidance.
    fn build_system_prompt(&self, context: &RepoContext) -> String {
        let mut prompt = String::from(
            "You are a senior open-source contributor who writes production-ready \
             code. You understand that PRs are judged by maintainers who value \
             minimal, focused, and convention-matching changes.\n\n\
             RULES FOR GENERATING CHANGES:\n\
             1. Match existing code style EXACTLY (indentation, naming, patterns)\n\
             2. Make the SMALLEST change that correctly fixes the issue\n\
             3. Include proper error handling consistent with the codebase\n\
             4. Do NOT break existing functionality\n\
             5. Do NOT add unnecessary dependencies or imports\n\
             6. Do NOT refactor adjacent code — fix only the reported issue\n\
             7. Do NOT add comments explaining what the code does\n\
             8. Do NOT modify files unrelated to the finding\n\n\
             OUTPUT FORMAT RULES (CRITICAL):\n\
             - Return ONLY raw JSON — no markdown fences, no ```json blocks\n\
             - No explanatory text before or after the JSON\n\
             - The response must be valid, parseable JSON and nothing else\n\n\
             ACCEPTANCE CRITERIA:\n\
             - Would a busy maintainer merge this in under 30 seconds?\n\
             - Is the change obviously correct with no side effects?\n",
        );

        if let Some(style) = &context.coding_style {
            prompt.push_str(&format!(
                "\nCODEBASE STYLE:\n{}\n\
                 You MUST match these conventions exactly.\n",
                style
            ));
        }

        prompt.push_str(&format!(
            "\nREPOSITORY: {}\nLanguage: {}\n",
            context.repo.full_name,
            context.repo.language.as_deref().unwrap_or("unknown")
        ));

        prompt
    }

    /// Build the generation prompt based on finding.
    ///
    /// Uses search/replace format for existing files (matching Python engine).
    fn build_generation_prompt(&self, finding: &Finding, context: &RepoContext) -> String {
        let current_content = context
            .relevant_files
            .get(&finding.file_path)
            .map(|s| s.as_str())
            .unwrap_or("");

        let suggestion_line = finding
            .suggestion
            .as_deref()
            .map(|s| format!("- **Suggestion**: {}\n", s))
            .unwrap_or_default();

        let mut prompt = format!(
            "## Task\nFix this issue.\n\n\
             ## Finding\n\
             - **Title**: {}\n\
             - **Severity**: {}\n\
             - **File**: {}\n\
             - **Description**: {}\n\
             {}",
            finding.title,
            finding.severity,
            finding.file_path,
            finding.description,
            suggestion_line
        );

        // Cross-file: find other files with the same issue pattern
        let cross_files = self.find_cross_file_instances(finding, context);
        if !cross_files.is_empty() {
            prompt.push_str(&format!(
                "\n## IMPORTANT: Same issue in {} OTHER file(s)\n\
                 Fix ALL instances across ALL files in a single contribution.\n\n",
                cross_files.len()
            ));
            for (fpath, fcontent) in &cross_files {
                let snippet = safe_truncate(fcontent, 3000);
                prompt.push_str(&format!("### {}\n```\n{}\n```\n\n", fpath, snippet));
            }
        }

        prompt.push_str("\n## Output Format\nReturn your changes as a JSON object.\n\n");

        if !current_content.is_empty() {
            let snippet = safe_truncate(current_content, 6000);
            prompt.push_str(&format!(
                "## Current File Content ({})\n```\n{}\n```\n\n",
                finding.file_path, snippet
            ));
            prompt.push_str(
                "Since this is an EXISTING file, use SEARCH/REPLACE blocks \
                 to make targeted edits. DO NOT rewrite the entire file.\n\n\
                 ```json\n\
                 {{\n  \"changes\": [\n    {{\n\
                       \"path\": \"path/to/file\",\n\
                       \"is_new_file\": false,\n\
                       \"edits\": [\n        {{\n\
                           \"search\": \"exact text to find in the file\",\n\
                           \"replace\": \"replacement text\"\n\
                       }}\n      ]\n    }}\n  ]\n}}\n\
                 ```\n\n\
                 RULES for search/replace:\n\
                 - `search` must be an EXACT substring from the current file\n\
                 - `replace` is what replaces it (can be longer/shorter)\n\
                 - To DELETE content, set `replace` to empty string\n\
                 - Keep each edit small and focused\n",
            );
        } else {
            prompt.push_str(
                "Since this is a NEW file, provide the full content:\n\n\
                 ```json\n\
                 {{\n  \"changes\": [\n    {{\n\
                       \"path\": \"path/to/file\",\n\
                       \"content\": \"full content of the new file\",\n\
                       \"is_new_file\": true\n\
                   }}\n  ]\n}}\n\
                 ```\n",
            );
        }

        prompt
    }

    // ── Commit / branch / PR title ───────────────────────────────────────────

    /// Generate a conventional commit message.
    fn generate_commit_message(&self, finding: &Finding, changes: &[FileChange]) -> String {
        let prefix = match finding.finding_type {
            ContributionType::SecurityFix => "fix(security)",
            ContributionType::CodeQuality => "refactor",
            ContributionType::DocsImprove => "docs",
            ContributionType::PerformanceOpt => "perf",
            ContributionType::FeatureAdd => "feat",
            ContributionType::Refactor => "refactor",
            ContributionType::UiUxFix => "fix(ui)",
        };

        // Extract scope from first changed file path (matching Python logic)
        let scope = changes.first().and_then(|c| {
            let parts: Vec<&str> = c.path.split('/').collect();
            if parts.len() >= 2
                && matches!(parts[0], "src" | "packages" | "apps" | "libs")
            {
                Some(parts[1].to_string())
            } else {
                None
            }
        });

        let title = finding.title.to_lowercase();
        let title = safe_truncate(&title, 50);
        let files: String = changes
            .iter()
            .take(3)
            .map(|c| c.path.split('/').next_back().unwrap_or(&c.path))
            .collect::<Vec<_>>()
            .join(", ");

        if let Some(s) = scope {
            format!(
                "{}({}): {}\n\n{}\n\nAffected files: {}",
                prefix, s, title, finding.description, files
            )
        } else {
            format!(
                "{}: {}\n\n{}\n\nAffected files: {}",
                prefix, title, finding.description, files
            )
        }
    }

    /// Generate a natural-looking branch name.
    pub fn generate_branch_name(finding: &Finding) -> String {
        let prefix = match finding.finding_type {
            ContributionType::SecurityFix => "fix/security",
            ContributionType::CodeQuality => "improve/quality",
            ContributionType::DocsImprove => "docs",
            ContributionType::PerformanceOpt => "perf",
            ContributionType::FeatureAdd => "feat",
            ContributionType::Refactor => "refactor",
            ContributionType::UiUxFix => "fix/ui",
        };

        let lower = finding.title.to_lowercase();
        let slug = RE_SLUG.replace_all(&lower, "-");
        let slug = slug.trim_matches('-');
        let slug = safe_truncate(slug, 40);

        format!("contribai/{}/{}", prefix, slug)
    }

    /// Generate a PR title using the default label format.
    pub fn generate_pr_title(finding: &Finding) -> String {
        Self::generate_pr_title_with_guidelines(finding, None)
    }

    /// Generate a PR title adapted to repo guidelines if available.
    ///
    /// If `guidelines` is `Some` and `has_guidelines()` returns true, delegates to
    /// `adapt_pr_title` + `extract_scope_from_path` from `guidelines.rs`.
    /// Otherwise falls back to the default label-based format.
    pub fn generate_pr_title_with_guidelines(
        finding: &Finding,
        guidelines: Option<&RepoGuidelines>,
    ) -> String {
        if let Some(g) = guidelines {
            if g.has_guidelines() {
                let scope = extract_scope_from_path(&finding.file_path, g);
                let type_str = match finding.finding_type {
                    ContributionType::SecurityFix => "security_fix",
                    ContributionType::CodeQuality => "code_quality",
                    ContributionType::DocsImprove => "docs_improve",
                    ContributionType::UiUxFix => "ui_ux_fix",
                    ContributionType::PerformanceOpt => "performance_opt",
                    ContributionType::FeatureAdd => "feature_add",
                    ContributionType::Refactor => "refactor",
                };
                return adapt_pr_title(&finding.title, type_str, g, &scope);
            }
        }

        // Default: label-based format
        let label = match finding.finding_type {
            ContributionType::SecurityFix => "Security",
            ContributionType::CodeQuality => "Quality",
            ContributionType::DocsImprove => "Docs",
            ContributionType::UiUxFix => "UI/UX",
            ContributionType::PerformanceOpt => "Performance",
            ContributionType::FeatureAdd => "Feature",
            ContributionType::Refactor => "Refactor",
        };
        format!("{}: {}", label, finding.title)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::models::{ContributionType, Severity};
    use std::collections::HashMap;

    pub fn test_finding() -> Finding {
        Finding {
            id: "test".into(),
            finding_type: ContributionType::SecurityFix,
            severity: Severity::High,
            title: "SQL injection in user query".into(),
            description: "User input not sanitized".into(),
            file_path: "src/db/queries.py".into(),
            line_start: Some(42),
            line_end: Some(45),
            suggestion: Some("Use parameterized queries".into()),
            confidence: 0.9,
            priority_signals: vec![],
        }
    }

    /// Construct a minimal Repository without relying on Default.
    fn test_repo() -> crate::core::models::Repository {
        crate::core::models::Repository {
            owner: "owner".into(),
            name: "repo".into(),
            full_name: "owner/repo".into(),
            description: None,
            language: None,
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
        }
    }

    /// Construct a minimal RepoContext for tests.
    pub fn test_context(files: HashMap<String, String>) -> RepoContext {
        RepoContext {
            repo: test_repo(),
            relevant_files: files,
            file_tree: vec![],
            readme_content: None,
            contributing_guide: None,
            open_issues: vec![],
            coding_style: None,
            symbol_map: HashMap::new(),
            file_ranks: HashMap::new(),
        }
    }

    pub fn mock_gen() -> ContributionGenerator<'static> {
        use std::sync::OnceLock;
        static CONFIG: OnceLock<ContributionConfig> = OnceLock::new();
        let config = CONFIG.get_or_init(ContributionConfig::default);
        static MOCK: MockLlm = MockLlm;
        ContributionGenerator {
            llm: &MOCK,
            config,
            self_review_enabled: false,
        }
    }

    // ── Branch name ──────────────────────────────────────────────────────────

    #[test]
    fn test_generate_branch_name() {
        let f = test_finding();
        let branch = ContributionGenerator::generate_branch_name(&f);
        assert!(branch.starts_with("contribai/fix/security/"));
        assert!(branch.contains("sql-injection"));
    }

    // ── PR title ─────────────────────────────────────────────────────────────

    #[test]
    fn test_generate_pr_title() {
        let f = test_finding();
        let title = ContributionGenerator::generate_pr_title(&f);
        assert!(title.starts_with("Security: "));
    }

    #[test]
    fn test_generate_pr_title_with_conventional_guidelines() {
        let g = RepoGuidelines {
            uses_conventional_commits: true,
            contributing_md: "uses conventional commits".into(),
            pr_template: "## Description".into(),
            ..Default::default()
        };
        let f = test_finding();
        let title = ContributionGenerator::generate_pr_title_with_guidelines(&f, Some(&g));
        // Conventional commits format: "fix: sql injection in user query"
        assert!(title.starts_with("fix:") || title.contains("sql injection"));
    }

    // ── Commit message ───────────────────────────────────────────────────────

    #[test]
    fn test_generate_commit_message() {
        let gen = mock_gen();
        let f = test_finding();
        let changes = vec![FileChange {
            path: "src/db/queries.py".into(),
            original_content: None,
            new_content: "fixed".into(),
            is_new_file: false,
            is_deleted: false,
        }];
        let msg = gen.generate_commit_message(&f, &changes);
        // Should contain "fix(security)" and scope "(db)"
        assert!(msg.contains("fix(security)"));
        assert!(msg.contains("(db)"));
    }

    // ── Mock LLM ─────────────────────────────────────────────────────────────

    pub(crate) struct MockLlm;

    #[async_trait::async_trait]
    impl LlmProvider for MockLlm {
        async fn complete(
            &self,
            _prompt: &str,
            _system: Option<&str>,
            _temperature: Option<f64>,
            _max_tokens: Option<u32>,
        ) -> Result<String> {
            Ok("mock response".into())
        }

        async fn chat(
            &self,
            _messages: &[crate::llm::provider::ChatMessage],
            _system: Option<&str>,
            _temperature: Option<f64>,
            _max_tokens: Option<u32>,
        ) -> Result<String> {
            Ok("mock response".into())
        }
    }
}
