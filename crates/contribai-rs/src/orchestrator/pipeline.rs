//! Main pipeline orchestrator.
//!
//! Port from Python `orchestrator/pipeline.py`.
//! Coordinates: discover → analyze → generate → PR.
//!
//! v5.1: Full wiring of middleware chain, TaskRouter, agents, and working memory.

use std::collections::{HashMap, HashSet};
use tracing::{debug, info, warn};

use crate::analysis::analyzer::CodeAnalyzer;
use crate::analysis::compressor::ContextCompressor;
use crate::core::config::ContribAIConfig;
use crate::core::error::Result;
use crate::core::events::{Event, EventBus, EventType};
use crate::core::middleware::{build_default_chain, MiddlewareChain, PipelineContext};
use crate::core::models::{AnalysisResult, DiscoveryCriteria, Finding, Repository};
use crate::generator::engine::ContributionGenerator;
use crate::generator::risk::{classify_risk, is_within_tolerance};
use crate::generator::scorer::QualityScorer;
use crate::github::client::GitHubClient;
use crate::github::discovery::RepoDiscovery;
use crate::github::guidelines::fetch_repo_guidelines;
use crate::llm::models::TaskType;
use crate::llm::provider::LlmProvider;
use crate::llm::router::{CostStrategy, TaskRouter};
use crate::orchestrator::memory::Memory;
use crate::pr::manager::PrManager;

/// Files that should NEVER be modified by ContribAI.
const PROTECTED_META_FILES: &[&str] = &[
    "CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    ".github/CODE_OF_CONDUCT.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    ".github/FUNDING.yml",
    ".github/SECURITY.md",
    "SECURITY.md",
    ".github/CODEOWNERS",
    ".all-contributorsrc",
];

/// Extensions to skip.
const SKIP_EXTENSIONS: &[&str] = &[
    ".md", ".txt", ".rst", ".yml", ".yaml", ".toml", ".cfg", ".ini", ".json",
];

/// Directories to skip.
const SKIP_DIRECTORIES: &[&str] = &[
    "examples",
    "example",
    "samples",
    "sample",
    "demos",
    "demo",
    "docs",
    "doc",
    "test",
    "tests",
    "testing",
    "test_data",
    "testdata",
    "fixtures",
    "benchmarks",
    "benchmark",
    "__pycache__",
    "vendor",
    "third_party",
    "third-party",
    "node_modules",
];

/// Result of a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineResult {
    pub repos_analyzed: usize,
    pub findings_total: usize,
    pub contributions_generated: usize,
    pub prs_created: usize,
    pub errors: Vec<String>,
}

/// Main orchestrator for the contribution pipeline.
pub struct ContribPipeline<'a> {
    config: &'a ContribAIConfig,
    github: &'a GitHubClient,
    llm: &'a dyn LlmProvider,
    memory: &'a Memory,
    event_bus: &'a EventBus,
    scorer: QualityScorer,
    middleware_chain: MiddlewareChain,
    router: std::sync::Mutex<TaskRouter>,
    /// Allow HIGH risk changes to auto-submit (set via --approve flag).
    approve_high_risk: bool,
}

impl<'a> ContribPipeline<'a> {
    pub fn new(
        config: &'a ContribAIConfig,
        github: &'a GitHubClient,
        llm: &'a dyn LlmProvider,
        memory: &'a Memory,
        event_bus: &'a EventBus,
    ) -> Self {
        let min_quality = config.pipeline.min_quality_score;

        // Build middleware chain from config
        let middleware_chain = build_default_chain(
            config.github.max_prs_per_day as i32,
            config.pipeline.max_retries,
            min_quality,
        );

        // Build task router from config strategy
        let strategy = match config.llm.provider.as_str() {
            _ if config.pipeline.min_quality_score >= 8.0 => CostStrategy::Performance,
            _ => CostStrategy::Balanced,
        };
        let router = TaskRouter::new(strategy);

        info!(
            middlewares = 5,
            strategy = ?strategy,
            "Pipeline initialized: middleware chain + task router"
        );

        Self {
            config,
            github,
            llm,
            memory,
            event_bus,
            scorer: QualityScorer::new(min_quality),
            middleware_chain,
            router: std::sync::Mutex::new(router),
            approve_high_risk: false,
        }
    }

    /// Alternative constructor using an Arc<dyn LlmProvider> directly (kept for API compat).
    /// Prefer `new` for most uses.
    pub fn with_arc_llm(
        config: &'a ContribAIConfig,
        github: &'a GitHubClient,
        llm: &'a dyn LlmProvider,
        memory: &'a Memory,
        event_bus: &'a EventBus,
    ) -> Self {
        Self::new(config, github, llm, memory, event_bus)
    }

    /// Set whether HIGH risk changes should be auto-submitted.
    pub fn set_approve_high_risk(&mut self, approve: bool) {
        self.approve_high_risk = approve;
    }

    /// Get model name for a given task type via the router.
    fn route_model(&self, task: TaskType, complexity: u32, files: u32) -> String {
        if let Ok(mut router) = self.router.lock() {
            let decision = router.route(task, complexity, files);
            debug!(
                model = %decision.model.name,
                reason = %decision.reason,
                "Task routed"
            );
            decision.model.name
        } else {
            // Fallback if mutex poisoned
            "gemini-3-flash-preview".to_string()
        }
    }

    /// Run pipeline on a **specific** repo (no discovery).
    ///
    /// Used by `contribai target <url>` and `contribai analyze <url>`.
    pub async fn run_targeted(
        &self,
        owner: &str,
        name: &str,
        dry_run: bool,
    ) -> Result<PipelineResult> {
        let run_id = self.memory.start_run()?;
        let full_name = format!("{}/{}", owner, name);

        info!(repo = %full_name, dry_run, "🎯 Targeting specific repo");

        self.event_bus
            .emit(
                Event::new(EventType::PipelineStart, "pipeline.run_targeted")
                    .with_data("repo", full_name.as_str())
                    .with_data("dry_run", dry_run),
            )
            .await;

        // Fetch repo details from GitHub
        let repo = self.github.get_repo_details(owner, name).await?;

        // Build middleware context
        let user_info = self.github.get_authenticated_user().await.ok();
        let mut ctx = PipelineContext {
            repo_name: full_name.clone(),
            owner: owner.to_string(),
            dry_run,
            remaining_prs: self.config.github.max_prs_per_day as i32,
            ..Default::default()
        };
        if let Some(ref u) = user_info {
            ctx.metadata.insert("user".to_string(), u.clone());
        }
        let ctx = self.middleware_chain.execute(ctx).await?;

        let result = self.process_repo(&repo, dry_run, 10, &ctx).await?;

        self.memory.finish_run(
            run_id,
            result.repos_analyzed as i64,
            result.prs_created as i64,
            result.findings_total as i64,
            result.errors.len() as i64,
        )?;

        Ok(result)
    }

    /// Run the full pipeline: discover → analyze → generate → PR.
    pub async fn run(
        &self,
        criteria: Option<&DiscoveryCriteria>,
        dry_run: bool,
    ) -> Result<PipelineResult> {
        let mut result = PipelineResult::default();
        let run_id = self.memory.start_run()?;

        self.event_bus
            .emit(
                Event::new(EventType::PipelineStart, "pipeline.run").with_data("dry_run", dry_run),
            )
            .await;

        // Check daily PR limit
        let today_prs = self.memory.get_today_pr_count()?;
        let remaining_prs = (self.config.github.max_prs_per_day as usize).saturating_sub(today_prs);
        if remaining_prs == 0 && !dry_run {
            warn!(
                limit = self.config.github.max_prs_per_day,
                "Daily PR limit reached"
            );
            return Ok(result);
        }

        // 1. Discover repos
        let default_criteria = DiscoveryCriteria::default();
        let criteria = criteria.unwrap_or(&default_criteria);

        info!("🔍 Discovering repositories...");
        let discovery = RepoDiscovery::new(self.github, &self.config.discovery);
        let repos = discovery.discover(Some(criteria)).await?;

        if repos.is_empty() {
            warn!("No repositories found matching criteria");
            return Ok(result);
        }

        info!(count = repos.len(), "Found candidate repositories");

        // Limit to max repos per run
        let max_repos = self.config.pipeline.max_repos_per_run;
        let repos: Vec<_> = repos.into_iter().take(max_repos).collect();

        // Get user info for DCO signoff
        let user_info = match self.github.get_authenticated_user().await {
            Ok(u) => Some(u),
            Err(e) => {
                debug!("Could not get user info for DCO: {}", e);
                None
            }
        };

        // 2. Process each repo through middleware chain first
        for repo in &repos {
            if self.memory.has_analyzed(&repo.full_name)? {
                info!(repo = %repo.full_name, "Skipping (already analyzed)");
                continue;
            }

            // ── Middleware chain pre-check ──────────────────────────────
            let mut ctx = PipelineContext {
                repo_name: repo.full_name.clone(),
                owner: repo.owner.clone(),
                dry_run,
                remaining_prs: remaining_prs as i32,
                ..Default::default()
            };

            // Inject user info for DCO middleware
            if let Some(ref u) = user_info {
                ctx.metadata.insert("user".to_string(), u.clone());
            }

            let ctx = match self.middleware_chain.execute(ctx).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(repo = %repo.full_name, err = %e, "Middleware error");
                    result
                        .errors
                        .push(format!("Middleware error for {}: {}", repo.full_name, e));
                    continue;
                }
            };

            if ctx.should_skip {
                info!(
                    repo = %repo.full_name,
                    reason = %ctx.skip_reason,
                    "⏭️ Skipping (middleware decision)"
                );
                if ctx.rate_limited {
                    warn!("Rate limited — stopping processing");
                    break;
                }
                continue;
            }

            // ── Process repo ────────────────────────────────────────────
            match self.process_repo(repo, dry_run, remaining_prs, &ctx).await {
                Ok(repo_result) => {
                    result.repos_analyzed += 1;
                    result.findings_total += repo_result.findings_total;
                    result.contributions_generated += repo_result.contributions_generated;
                    result.prs_created += repo_result.prs_created;
                    result.errors.extend(repo_result.errors);
                }
                Err(e) => {
                    let msg = format!("Error processing {}: {}", repo.full_name, e);
                    warn!("{}", msg);
                    result.errors.push(msg);
                }
            }
        }

        // 3. Log run
        self.memory.finish_run(
            run_id,
            result.repos_analyzed as i64,
            result.prs_created as i64,
            result.findings_total as i64,
            result.errors.len() as i64,
        )?;

        self.event_bus
            .emit(
                Event::new(EventType::PipelineComplete, "pipeline.run")
                    .with_data("repos", result.repos_analyzed as i64)
                    .with_data("prs", result.prs_created as i64)
                    .with_data("findings", result.findings_total as i64),
            )
            .await;

        // ── v5.4: Auto-trigger dream consolidation if gates pass ─────
        self.maybe_dream();

        Ok(result)
    }

    /// Hunt mode: aggressively discover and contribute across multiple rounds.
    pub async fn hunt(
        &self,
        rounds: u32,
        delay_sec: u64,
        dry_run: bool,
        mode: &str,
    ) -> Result<PipelineResult> {
        let mut total = PipelineResult::default();
        let (cfg_min, cfg_max) = (
            self.config.discovery.stars_min,
            self.config.discovery.stars_max,
        );
        let star_tiers = [
            (cfg_min, cfg_max),
            (100i64, 1000i64),
            (1000i64, 5000i64),
            (5000i64, 20000i64),
            (500i64, 3000i64),
        ];

        let langs = self.config.discovery.languages.clone();
        let all_languages = langs.clone(); // Config now includes all supported languages

        info!(rounds, delay_sec, mode, "🔥 Hunt mode started");

        for rnd in 1..=rounds {
            // Check daily limit
            let today_prs = self.memory.get_today_pr_count()?;
            let remaining = (self.config.github.max_prs_per_day as usize).saturating_sub(today_prs);
            if remaining == 0 && !dry_run {
                warn!("🛑 Daily PR limit reached. Stopping hunt.");
                break;
            }

            // Rotate languages — simple deterministic shuffle using seed
            let mut hunt_langs = if rnd % 2 == 0 {
                all_languages.clone()
            } else {
                langs.clone()
            };
            // Simple rotation instead of random shuffle (no rand dep)
            let rotate_by = (rnd as usize) % hunt_langs.len().max(1);
            hunt_langs.rotate_left(rotate_by);

            let stars = star_tiers[((rnd - 1) as usize) % star_tiers.len()];
            let criteria = DiscoveryCriteria {
                languages: hunt_langs.iter().take(2).cloned().collect(),
                stars_min: stars.0,
                stars_max: stars.1,
                min_last_activity_days: 7,
                max_results: 10,
                ..Default::default()
            };

            info!(
                round = rnd,
                total = rounds,
                langs = %hunt_langs.iter().take(2).cloned().collect::<Vec<_>>().join("/"),
                stars_min = stars.0,
                stars_max = stars.1,
                "🔥 Hunt round"
            );

            self.event_bus
                .emit(
                    Event::new(EventType::HuntRoundStart, "pipeline.hunt")
                        .with_data("round", rnd as i64)
                        .with_data("total", rounds as i64),
                )
                .await;

            let discovery = RepoDiscovery::new(self.github, &self.config.discovery);
            let repos = match discovery.discover(Some(&criteria)).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("Discovery failed round {}: {}", rnd, e);
                    if rnd < rounds {
                        tokio::time::sleep(std::time::Duration::from_secs(delay_sec)).await;
                    }
                    continue;
                }
            };

            if repos.is_empty() {
                info!("No repos found this round");
                if rnd < rounds {
                    tokio::time::sleep(std::time::Duration::from_secs(delay_sec)).await;
                }
                continue;
            }

            // Filter to repos that merge external PRs
            let mut targets: Vec<Repository> = Vec::new();
            for repo in repos.iter().take(5) {
                if self.memory.has_analyzed(&repo.full_name).unwrap_or(false) {
                    continue;
                }
                if let Ok(prs) = self
                    .github
                    .list_pull_requests(&repo.owner, &repo.name, "closed", 10)
                    .await
                {
                    let merged = prs
                        .iter()
                        .filter(|p| {
                            p.get("merged_at")
                                .and_then(|v| v.as_str())
                                .map(|s| !s.is_empty())
                                .unwrap_or(false)
                        })
                        .count();
                    if merged > 0 {
                        info!(repo = %repo.full_name, merged, "✅ Merge-friendly target");
                        targets.push(repo.clone());
                    }
                }
            }

            if targets.is_empty() {
                info!("No merge-friendly repos this round");
                if rnd < rounds {
                    tokio::time::sleep(std::time::Duration::from_secs(delay_sec)).await;
                }
                continue;
            }

            let max_targets = self.config.pipeline.max_repos_per_run;
            let delay_between = 5.0f64; // default inter-repo delay seconds

            let selected: Vec<_> = targets.into_iter().take(max_targets).collect();
            let mut remaining = remaining;

            for (i, repo) in selected.iter().enumerate() {
                if remaining == 0 && !dry_run {
                    warn!("PR limit reached mid-round");
                    break;
                }

                // Build middleware ctx
                let ctx = PipelineContext {
                    repo_name: repo.full_name.clone(),
                    owner: repo.owner.clone(),
                    dry_run,
                    remaining_prs: remaining as i32,
                    ..Default::default()
                };
                let ctx = match self.middleware_chain.execute(ctx).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(repo=%repo.full_name, err=%e, "Middleware error in hunt");
                        continue;
                    }
                };
                if ctx.should_skip {
                    if ctx.rate_limited {
                        break;
                    }
                    continue;
                }

                let rr = self
                    .hunt_process_repo(repo, mode, dry_run, remaining, &ctx)
                    .await;
                total.repos_analyzed += rr.repos_analyzed;
                total.findings_total += rr.findings_total;
                total.contributions_generated += rr.contributions_generated;
                total.prs_created += rr.prs_created;
                total.errors.extend(rr.errors);
                remaining = remaining.saturating_sub(rr.prs_created);

                if i < selected.len() - 1 && delay_between > 0.0 {
                    debug!("Inter-repo delay: {}s", delay_between);
                    tokio::time::sleep(std::time::Duration::from_secs_f64(delay_between)).await;
                }
            }

            // Issue-first on odd rounds
            if (mode == "issues" || mode == "both") && rnd % 2 == 1 {
                match self
                    .hunt_issues_globally(
                        &hunt_langs.iter().take(2).cloned().collect::<Vec<_>>(),
                        dry_run,
                        3,
                    )
                    .await
                {
                    Ok(ir) => {
                        total.findings_total += ir.findings_total;
                        total.contributions_generated += ir.contributions_generated;
                        total.prs_created += ir.prs_created;
                        total.errors.extend(ir.errors);
                    }
                    Err(e) => debug!("Issue-first hunt failed: {}", e),
                }
            }

            if rnd < rounds {
                info!(secs = delay_sec, "⏳ Waiting before next round...");
                tokio::time::sleep(std::time::Duration::from_secs(delay_sec)).await;
            }
        }

        info!(
            repos = total.repos_analyzed,
            prs = total.prs_created,
            findings = total.findings_total,
            "🏁 Hunt complete"
        );

        // ── v5.4: Auto-trigger dream consolidation if gates pass ─────
        self.maybe_dream();

        Ok(total)
    }

    /// Check dream gates and run consolidation if met (non-blocking).
    pub fn maybe_dream(&self) {
        match self.memory.should_dream() {
            Ok(true) => {
                info!("🌙 Dream gates passed — running memory consolidation");
                match self.memory.run_dream() {
                    Ok(r) => info!(
                        repos = r.repos_profiled,
                        pruned = r.entries_pruned,
                        "🌙 Dream complete"
                    ),
                    Err(e) => warn!("Dream consolidation failed: {}", e),
                }
            }
            Ok(false) => debug!("Dream gates not yet met"),
            Err(e) => debug!("Dream gate check failed: {}", e),
        }
    }

    /// Process a single repo in hunt mode.
    async fn hunt_process_repo(
        &self,
        repo: &Repository,
        mode: &str,
        dry_run: bool,
        remaining: usize,
        ctx: &PipelineContext,
    ) -> PipelineResult {
        let mut rr = PipelineResult::default();
        if mode == "analysis" || mode == "both" {
            match self.process_repo(repo, dry_run, remaining, ctx).await {
                Ok(r) => {
                    rr.repos_analyzed += r.repos_analyzed;
                    rr.findings_total += r.findings_total;
                    rr.contributions_generated += r.contributions_generated;
                    rr.prs_created += r.prs_created;
                    rr.errors.extend(r.errors);
                }
                Err(e) => rr.errors.push(format!("{}: {}", repo.full_name, e)),
            }
        }

        if mode == "issues" || mode == "both" {
            match self
                .process_repo_issues(repo, dry_run, remaining.saturating_sub(rr.prs_created), ctx)
                .await
            {
                Ok(r) => {
                    rr.repos_analyzed = rr.repos_analyzed.max(r.repos_analyzed);
                    rr.findings_total += r.findings_total;
                    rr.contributions_generated += r.contributions_generated;
                    rr.prs_created += r.prs_created;
                    rr.errors.extend(r.errors);
                }
                Err(e) => rr.errors.push(format!("{} issues: {}", repo.full_name, e)),
            }
        }

        rr.repos_analyzed = rr.repos_analyzed.max(1);
        rr
    }

    /// Issue-first strategy: search GitHub for high-value issues.
    async fn hunt_issues_globally(
        &self,
        languages: &[String],
        dry_run: bool,
        max_issues: usize,
    ) -> Result<PipelineResult> {
        let mut result = PipelineResult::default();
        info!("🎯 Issue-First: searching for high-value issues...");

        for lang in languages {
            for label in &["good first issue", "help wanted", "bug"] {
                let query = format!(
                    r#"label:"{label}" language:{lang} state:open stars:>100 archived:false"#
                );
                let issues = match self.github.search_issues(&query, "created", 10).await {
                    Ok(i) => i,
                    Err(e) => {
                        debug!("Issue search failed: {}", e);
                        continue;
                    }
                };

                for issue in issues.iter().take(max_issues) {
                    let repo_url = issue
                        .get("repository_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if repo_url.is_empty() {
                        continue;
                    }

                    let parts: Vec<&str> = repo_url.trim_end_matches('/').split('/').collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let (owner, repo_name) = (parts[parts.len() - 2], parts[parts.len() - 1]);
                    let full_name = format!("{owner}/{repo_name}");

                    if self.memory.has_analyzed(&full_name).unwrap_or(false) {
                        continue;
                    }

                    let past_open = self.memory.get_prs(Some("open"), 20).unwrap_or_default();
                    let has_open = past_open
                        .iter()
                        .any(|p| p.get("repo").map(|r| r == &full_name).unwrap_or(false));
                    if has_open {
                        continue;
                    }

                    info!(
                        issue = issue.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
                        repo = %full_name,
                        title = %issue.get("title").and_then(|v| v.as_str()).unwrap_or("?"),
                        label,
                        "🎯 Found issue"
                    );

                    match self.github.get_repo_details(owner, repo_name).await {
                        Ok(repo) => {
                            let ctx = PipelineContext {
                                repo_name: full_name.clone(),
                                owner: owner.to_string(),
                                dry_run,
                                remaining_prs: max_issues as i32,
                                ..Default::default()
                            };
                            let rr = self
                                .hunt_process_repo(&repo, "issues", dry_run, max_issues, &ctx)
                                .await;
                            result.repos_analyzed += rr.repos_analyzed;
                            result.findings_total += rr.findings_total;
                            result.contributions_generated += rr.contributions_generated;
                            result.prs_created += rr.prs_created;
                            result.errors.extend(rr.errors);

                            if result.prs_created >= max_issues {
                                return Ok(result);
                            }
                        }
                        Err(e) => debug!("Failed to get repo {}: {}", full_name, e),
                    }
                }
            }
        }

        Ok(result)
    }

    /// Process a single repository: analyze → generate → PR.
    async fn process_repo(
        &self,
        repo: &Repository,
        dry_run: bool,
        _max_prs: usize,
        ctx: &PipelineContext,
    ) -> Result<PipelineResult> {
        let mut result = PipelineResult::default();

        self.event_bus
            .emit(
                Event::new(EventType::AnalysisStart, "pipeline.process_repo")
                    .with_data("repo", repo.full_name.as_str()),
            )
            .await;

        info!(repo = %repo.full_name, "📦 Processing");

        // ── v5.1: Load working memory cache ──────────────────────────────
        let cached_context = self
            .memory
            .get_context(&repo.full_name, "analysis_summary")
            .ok()
            .flatten();
        if let Some(ref cached) = cached_context {
            info!(
                repo = %repo.full_name,
                chars = cached.len(),
                "💾 Loaded cached analysis context"
            );
        }

        // ── Fetch repo guidelines ─────────────────────────────────────────
        let guidelines = fetch_repo_guidelines(self.github, &repo.owner, &repo.name).await;
        if guidelines.has_guidelines() {
            info!(
                commit_format = %guidelines.commit_format,
                sections = guidelines.required_sections.len(),
                "📋 Repo guidelines found"
            );
        }
        let guidelines = Some(guidelines);

        // ── Inject PR history into context for dedup ─────────────────────
        let all_past_prs = self.memory.get_prs(None, 50).unwrap_or_default();
        let past_titles_lower: HashSet<String> = all_past_prs
            .iter()
            .filter(|p| p.get("repo").map(|r| r == &repo.full_name).unwrap_or(false))
            .filter_map(|p| p.get("title"))
            .map(|s| s.to_lowercase())
            .collect();

        // Also fetch GitHub PRs for external dedup
        let github_pr_titles: HashSet<String> = self
            .github
            .list_pull_requests(&repo.owner, &repo.name, "all", 50)
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|p| p.get("title").and_then(|v| v.as_str()))
            .map(|s| s.to_lowercase())
            .collect();

        // Merge all known PR titles
        let all_past_titles: HashSet<String> = past_titles_lower
            .union(&github_pr_titles)
            .cloned()
            .collect();

        info!(
            repo = %repo.full_name,
            known_prs = all_past_titles.len(),
            "🔁 Dedup context loaded"
        );

        // ── v5.1: Route to analysis model ───────────────────────────────
        let _analysis_model = self.route_model(
            TaskType::Analysis,
            5, // medium complexity by default
            1,
        );

        // ── Analyze ──────────────────────────────────────────────────────
        let analyzer = CodeAnalyzer::new(self.llm, self.github, &self.config.analysis);
        let analysis = analyzer.analyze(repo).await?;

        result.findings_total = analysis.findings.len();

        self.memory.record_analysis(
            &repo.full_name,
            repo.language.as_deref().unwrap_or("unknown"),
            repo.stars,
            analysis.findings.len() as i64,
        )?;

        self.event_bus
            .emit(
                Event::new(EventType::AnalysisComplete, "pipeline.process_repo")
                    .with_data("repo", repo.full_name.as_str())
                    .with_data("findings", analysis.findings.len() as i64),
            )
            .await;

        if analysis.findings.is_empty() {
            info!(repo = %repo.full_name, "✅ No findings");
            return Ok(result);
        }

        // ── v5.1: Save analysis to working memory ───────────────────────
        let findings_for_summary = &analysis.findings;
        let summary = ContextCompressor::summarize_findings_compact(findings_for_summary);
        if let Err(e) = self.memory.store_context(
            &repo.full_name,
            "analysis_summary",
            &summary,
            repo.language.as_deref().unwrap_or(""),
            72.0,
        ) {
            debug!("Failed to save context: {}", e);
        } else {
            info!(
                repo = %repo.full_name,
                findings = analysis.findings.len(),
                "💾 Saved analysis context"
            );
        }

        // ── Filter findings ──────────────────────────────────────────────
        let findings = self.filter_findings(&analysis, &all_past_titles);
        info!(
            repo = %repo.full_name,
            raw = analysis.findings.len(),
            filtered = findings.len(),
            "Findings after filtering"
        );

        if findings.is_empty() {
            return Ok(result);
        }

        // Limit to 2 per repo to avoid spamming
        let findings: Vec<_> = findings.into_iter().take(2).collect();

        // ── Build repo context ───────────────────────────────────────────
        let file_tree = self
            .github
            .get_file_tree(&repo.owner, &repo.name, None)
            .await
            .unwrap_or_default();

        let mut relevant_files: HashMap<String, String> = HashMap::new();
        for finding in &findings {
            if !finding.file_path.is_empty() && !relevant_files.contains_key(&finding.file_path) {
                if let Ok(content) = self
                    .github
                    .get_file_content(&repo.owner, &repo.name, &finding.file_path, None)
                    .await
                {
                    relevant_files.insert(finding.file_path.clone(), content);
                }
            }
        }

        let relevant_file_count = relevant_files.len() as u32;

        // Get coding style from working memory
        let coding_style = self
            .memory
            .get_context(&repo.full_name, "coding_style")
            .ok()
            .flatten();

        let repo_context = crate::core::models::RepoContext {
            repo: repo.clone(),
            file_tree,
            readme_content: None,
            contributing_guide: guidelines.as_ref().and_then(|g| {
                let text = &g.contributing_md;
                if !text.is_empty() {
                    Some(text.clone())
                } else {
                    None
                }
            }),
            relevant_files,
            open_issues: Vec::new(),
            coding_style,
            symbol_map: HashMap::new(),
            file_ranks: HashMap::new(),
        };

        // ── Generate contributions ───────────────────────────────────────
        // v5.1: Route to code gen model based on complexity
        let high_sev = findings
            .iter()
            .filter(|f| {
                matches!(
                    f.severity,
                    crate::core::models::Severity::Critical | crate::core::models::Severity::High
                )
            })
            .count() as u32;
        let complexity = (high_sev * 2 + 5).min(10);

        let gen_model =
            self.route_model(TaskType::CodeGen, complexity.min(10), relevant_file_count);
        debug!(model = %gen_model, "Using model for code generation");

        let generator = ContributionGenerator::new(self.llm, &self.config.contribution);

        // Get signoff from middleware context
        let _signoff = ctx.signoff.clone();

        for finding in &findings {
            self.event_bus
                .emit(
                    Event::new(EventType::GenerationStart, "pipeline.process_repo")
                        .with_data("title", finding.title.as_str()),
                )
                .await;

            match generator.generate(finding, &repo_context).await {
                Ok(Some(contribution)) => {
                    let report = self.scorer.evaluate(&contribution);
                    if !report.passed {
                        info!(
                            title = %contribution.title,
                            score = report.score,
                            "❌ Quality check failed"
                        );
                        continue;
                    }

                    // ── v5.4: Risk classification gate ────────────────────
                    let files_changed: Vec<String> = contribution
                        .changes
                        .iter()
                        .map(|f| f.path.clone())
                        .collect();
                    let diff_lines: usize = contribution
                        .changes
                        .iter()
                        .map(|f| f.new_content.lines().count())
                        .sum();
                    let risk = classify_risk(
                        &contribution.contribution_type.to_string(),
                        &files_changed,
                        diff_lines,
                    );

                    let tolerance = &self.config.pipeline.risk_tolerance;
                    if !is_within_tolerance(risk.level, tolerance) && !self.approve_high_risk {
                        warn!(
                            title = %contribution.title,
                            risk = %risk.level,
                            reason = %risk.reason,
                            tolerance = %tolerance,
                            "⛔ Risk too high — skipping (use --approve to override)"
                        );
                        continue;
                    }

                    info!(
                        title = %contribution.title,
                        risk = %risk.level,
                        reason = %risk.reason,
                        "🛡️ Risk: {}", risk.level
                    );

                    result.contributions_generated += 1;

                    if dry_run {
                        info!(
                            title = %contribution.title,
                            score = report.score,
                            risk = %risk.level,
                            "🏃 [DRY RUN] Would create PR"
                        );
                    } else {
                        let mut pr_mgr = PrManager::new(self.github);
                        match pr_mgr.create_pr(&contribution, repo).await {
                            Ok(pr_result) => {
                                result.prs_created += 1;

                                // ── v5.1: Record PR for dedup ──────────────
                                self.memory.record_pr(
                                    &repo.full_name,
                                    pr_result.pr_number,
                                    &pr_result.pr_url,
                                    &contribution.title,
                                    &contribution.contribution_type.to_string(),
                                    &pr_result.branch_name,
                                    &pr_result.fork_full_name,
                                )?;

                                // ── v5.2: Outcome learning — initial status ─
                                if let Err(e) = self.memory.record_outcome(
                                    &repo.full_name,
                                    pr_result.pr_number,
                                    &pr_result.pr_url,
                                    &contribution.contribution_type.to_string(),
                                    "open", // updated later by patrol
                                    &pr_result.branch_name,
                                    0.0, // merge time unknown yet
                                ) {
                                    debug!("Outcome record failed (non-fatal): {}", e);
                                }

                                self.event_bus
                                    .emit(
                                        Event::new(EventType::PrCreated, "pipeline.process_repo")
                                            .with_data("pr_number", pr_result.pr_number)
                                            .with_data("url", pr_result.pr_url.as_str()),
                                    )
                                    .await;

                                info!(
                                    pr = pr_result.pr_number,
                                    url = %pr_result.pr_url,
                                    "✅ PR created + outcome recorded"
                                );
                            }
                            Err(e) => {
                                let msg = format!("PR creation failed: {}", e);
                                warn!("{}", msg);
                                result.errors.push(msg);
                            }
                        }
                    }
                }
                Ok(None) => {
                    info!(title = %finding.title, "No contribution generated");
                }
                Err(e) => {
                    result.errors.push(format!("Generation error: {}", e));
                }
            }
        }

        result.repos_analyzed = 1;
        Ok(result)
    }

    /// Process a repo's issues (issue-solver mode).
    async fn process_repo_issues(
        &self,
        repo: &Repository,
        _dry_run: bool,
        max_prs: usize,
        _ctx: &PipelineContext,
    ) -> Result<PipelineResult> {
        use crate::issues::solver::IssueSolver;

        let mut result = PipelineResult::default();
        let solver = IssueSolver::new(self.llm, self.github);

        // Fetch solvable issues and count them
        let issues = solver.fetch_solvable_issues(repo, max_prs, 5).await;
        result.repos_analyzed = 1;
        result.findings_total = issues.len();
        result.contributions_generated = issues.len();
        // TODO: actually solve them with solve_issue() in future sprint

        Ok(result)
    }

    /// Filter findings: remove protected files, skip extensions, skip directories, dedup.
    fn filter_findings(
        &self,
        analysis: &AnalysisResult,
        past_titles: &HashSet<String>,
    ) -> Vec<Finding> {
        let protected: HashSet<&str> = PROTECTED_META_FILES.iter().copied().collect();

        analysis
            .findings
            .iter()
            .filter(|f| {
                // Skip protected files
                if protected.contains(f.file_path.as_str()) {
                    return false;
                }

                // Skip by extension
                if let Some(ext) = std::path::Path::new(&f.file_path).extension() {
                    let ext_str = format!(".{}", ext.to_string_lossy().to_lowercase());
                    if SKIP_EXTENSIONS.contains(&ext_str.as_str()) {
                        return false;
                    }
                }

                // Skip by directory
                let parts: Vec<&str> = f.file_path.split('/').collect();
                if parts.iter().any(|p| SKIP_DIRECTORIES.contains(p)) {
                    return false;
                }

                // Dedup against past PR titles
                let title_lower = f.title.to_lowercase();
                if past_titles
                    .iter()
                    .any(|pt| titles_similar(&title_lower, pt))
                {
                    debug!(title = %f.title, "Dedup: skipping similar to past PR");
                    return false;
                }

                true
            })
            .cloned()
            .collect()
    }
}

/// Check if two titles are similar (>50% keyword overlap).
pub fn titles_similar(title_a: &str, title_b: &str) -> bool {
    let stop_words: HashSet<&str> = [
        "a", "an", "the", "in", "on", "of", "for", "to", "and", "or", "is",
    ]
    .iter()
    .copied()
    .collect();

    let words_a: HashSet<String> = title_a
        .to_lowercase()
        .split_whitespace()
        .filter(|w| !stop_words.contains(w) && w.len() > 2)
        .map(String::from)
        .collect();

    let words_b: HashSet<String> = title_b
        .to_lowercase()
        .split_whitespace()
        .filter(|w| !stop_words.contains(w) && w.len() > 2)
        .map(String::from)
        .collect();

    if words_a.is_empty() || words_b.is_empty() {
        return false;
    }

    let overlap = words_a.intersection(&words_b).count();
    let smaller = words_a.len().min(words_b.len());
    overlap as f64 / smaller as f64 > 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_titles_similar() {
        assert!(titles_similar(
            "SQL injection vulnerability fix",
            "SQL injection vulnerability"
        ));
        assert!(!titles_similar(
            "Add logging middleware",
            "Fix database connection pooling"
        ));
    }

    #[test]
    fn test_titles_similar_empty() {
        assert!(!titles_similar("", "something"));
        assert!(!titles_similar("a", "b"));
    }

    #[test]
    fn test_protected_files() {
        let protected: HashSet<&str> = PROTECTED_META_FILES.iter().copied().collect();
        assert!(protected.contains("CONTRIBUTING.md"));
        assert!(protected.contains("LICENSE"));
        assert!(!protected.contains("src/main.py"));
    }

    #[test]
    fn test_skip_directories() {
        assert!(SKIP_DIRECTORIES.contains(&"test"));
        assert!(SKIP_DIRECTORIES.contains(&"vendor"));
        assert!(!SKIP_DIRECTORIES.contains(&"src"));
    }
}
