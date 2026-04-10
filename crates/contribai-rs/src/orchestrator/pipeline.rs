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
use crate::core::models::{
    AnalysisResult, Contribution, ContributionType, DiscoveryCriteria, FileChange, Finding,
    Repository,
};
use crate::generator::engine::ContributionGenerator;
use crate::generator::risk::{classify_risk, is_within_tolerance};
use crate::generator::scorer::QualityScorer;
use crate::github::client::GitHubClient;
use crate::github::discovery::RepoDiscovery;
use crate::github::guidelines::fetch_repo_guidelines;
use crate::llm::models::TaskType;
use crate::llm::provider::LlmProvider;
use crate::llm::router::{CostStrategy, TaskRouter};
use crate::orchestrator::circuit_breaker::CircuitBreaker;
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
    /// Circuit breaker for LLM failures — stops pipeline after too many failures.
    circuit_breaker: CircuitBreaker,
    /// Allow HIGH risk changes to auto-submit (set via --approve flag).
    approve_high_risk: bool,
}

/// Merge multiple contributions into a single multi-file contribution.
///
/// Deduplicates file changes by path (first seen wins) and creates a combined
/// title/description. If only one contribution, returns it as-is.
/// Public so CLI commands can reuse it without a full pipeline.
pub fn merge_contributions_pub(contribs: Vec<Contribution>) -> Contribution {
    if contribs.len() == 1 {
        return contribs.into_iter().next().unwrap();
    }

    let first = &contribs[0];
    let mut all_changes: Vec<FileChange> = Vec::new();
    let mut all_tests: Vec<FileChange> = Vec::new();
    let mut descriptions = Vec::new();
    let mut seen_paths = HashSet::new();

    for c in &contribs {
        descriptions.push(format!("- {}", c.title));
        for change in &c.changes {
            if seen_paths.insert(change.path.clone()) {
                all_changes.push(change.clone());
            }
        }
        for test in &c.tests_added {
            if seen_paths.insert(test.path.clone()) {
                all_tests.push(test.clone());
            }
        }
    }

    let type_prefix = match first.contribution_type {
        ContributionType::SecurityFix => "fix(security)",
        ContributionType::CodeQuality => "fix",
        ContributionType::PerformanceOpt => "perf",
        ContributionType::DocsImprove => "docs",
        ContributionType::FeatureAdd => "feat",
        _ => "refactor",
    };
    let title = format!(
        "{}: {} improvements across {} files",
        type_prefix,
        contribs.len(),
        all_changes.len()
    );

    info!(
        contributions = contribs.len(),
        files = all_changes.len(),
        "📦 Merged contributions into single PR"
    );

    Contribution {
        finding: first.finding.clone(),
        contribution_type: first.contribution_type.clone(),
        title: title.clone(),
        description: format!(
            "Combined multi-file contribution:\n\n{}\n\nFiles changed: {}",
            descriptions.join("\n"),
            all_changes
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        changes: all_changes,
        commit_message: format!("{}\n\n{}", title, descriptions.join("\n")),
        tests_added: all_tests,
        branch_name: first.branch_name.clone(),
        generated_at: chrono::Utc::now(),
    }
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
            cb_threshold = config.pipeline.circuit_breaker_failure_threshold,
            "Pipeline initialized: middleware chain + task router + circuit breaker"
        );

        let circuit_breaker = CircuitBreaker::new().with_thresholds(
            config.pipeline.circuit_breaker_failure_threshold,
            config.pipeline.circuit_breaker_success_threshold,
            config.pipeline.circuit_breaker_cooldown_secs,
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
            circuit_breaker,
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

    /// Get the current circuit breaker state summary.
    pub fn circuit_breaker_summary(&self) -> String {
        self.circuit_breaker.summary()
    }

    /// Check if the circuit breaker is open. Returns true if requests should proceed.
    fn check_circuit(&self) -> bool {
        if !self.circuit_breaker.allow_request() {
            warn!(
                circuit_state = ?self.circuit_breaker.state(),
                failures = self.circuit_breaker.failure_count(),
                "🔴 Circuit breaker OPEN — failing fast to save API quota"
            );
            false
        } else {
            true
        }
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

        // v5.4: Auto-trigger dream consolidation if gates pass
        self.maybe_dream();

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
            // ── Circuit breaker check ───────────────────────────────────
            if !self.check_circuit() {
                warn!("Circuit breaker open — stopping pipeline to save API quota");
                result
                    .errors
                    .push("Circuit breaker open: too many consecutive LLM failures".to_string());
                break;
            }

            // Skip repos analyzed within the last 7 days
            if self.memory.has_analyzed_since(&repo.full_name, 7)? {
                info!(repo = %repo.full_name, "Skipping (analyzed within 7 days)");
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
                    result.errors.extend(repo_result.errors.clone());

                    // Record circuit breaker success/failure
                    if repo_result.errors.is_empty() {
                        self.circuit_breaker.record_success();
                    } else {
                        self.circuit_breaker.record_failure();
                    }
                }
                Err(e) => {
                    let msg = format!("Error processing {}: {}", repo.full_name, e);
                    warn!("{}", msg);
                    result.errors.push(msg);
                    self.circuit_breaker.record_failure();
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

        // Rotate sort orders across rounds for variety
        let sort_orders = ["stars", "updated", "help-wanted-issues", "stars", "updated"];
        let langs = self.config.discovery.languages.clone();
        let all_languages = langs.clone(); // Config now includes all supported languages

        info!(rounds, delay_sec, mode, "🔥 Hunt mode started");

        for rnd in 1..=rounds {
            // ── Circuit breaker check ───────────────────────────────────
            if !self.check_circuit() {
                warn!("Circuit breaker open — stopping hunt to save API quota");
                total
                    .errors
                    .push("Circuit breaker open: too many consecutive LLM failures".to_string());
                break;
            }

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
            let sort = sort_orders[((rnd - 1) as usize) % sort_orders.len()];
            // Each round uses a different page for variety
            let page = rnd;
            let criteria = DiscoveryCriteria {
                languages: hunt_langs.iter().take(2).cloned().collect(),
                stars_min: stars.0,
                stars_max: stars.1,
                min_last_activity_days: 7,
                max_results: 10,
                sort: Some(sort.to_string()),
                page: Some(page),
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
                if self
                    .memory
                    .has_analyzed_since(&repo.full_name, 7)
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Ok(prs) = self
                    .github
                    .list_pull_requests(&repo.owner, &repo.name, "closed", 10)
                    .await
                {
                    let merged = prs
                        .iter()
                        .filter(|p| p.get("merged_at").and_then(|v| v.as_str()).is_some())
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
    ///
    /// Uses a file-based advisory lock (`DreamLock`) to prevent concurrent
    /// dream consolidation — eliminates the TOCTOU race from the old
    /// string-based DB lock.
    pub fn maybe_dream(&self) {
        // Gate 1: Check time/session thresholds (fast path)
        match self.memory.should_dream() {
            Ok(true) => {
                info!("🌙 Dream gates passed — acquiring lock");
            }
            Ok(false) => {
                debug!("Dream gates not yet met");
                return;
            }
            Err(e) => {
                debug!("Dream gate check failed: {}", e);
                return;
            }
        }

        // Gate 2: Atomic file lock — only one instance can hold this
        let db_path = self.config.storage.resolved_db_path();
        match crate::orchestrator::dream_lock::DreamLock::new(&db_path) {
            Ok(lock) => {
                if lock.try_acquire().is_some() {
                    // We hold the lock — run dream consolidation
                    info!("🌙 Dream lock acquired — running memory consolidation");
                    match self.memory.run_dream() {
                        Ok(r) => info!(
                            repos = r.repos_profiled,
                            pruned = r.entries_pruned,
                            "🌙 Dream complete"
                        ),
                        Err(e) => warn!("Dream consolidation failed: {}", e),
                    }
                    // Lock released when lock drops
                } else {
                    info!("🌙 Another instance holds dream lock — skipping");
                }
            }
            Err(e) => {
                debug!("Failed to create dream lock: {}", e);
            }
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
                    rr.errors.extend(r.errors.clone());
                    // Record circuit breaker
                    if r.errors.is_empty() {
                        self.circuit_breaker.record_success();
                    } else {
                        self.circuit_breaker.record_failure();
                    }
                }
                Err(e) => {
                    rr.errors.push(format!("{}: {}", repo.full_name, e));
                    self.circuit_breaker.record_failure();
                }
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

                    if self
                        .memory
                        .has_analyzed_since(&full_name, 7)
                        .unwrap_or(false)
                    {
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

        // ── v5.4: Dream profile — skip rejected contribution types ──────
        let findings: Vec<_> =
            if let Ok(Some(profile)) = self.memory.get_repo_profile(&repo.full_name) {
                if !profile.rejected_types.is_empty() {
                    info!(
                        repo = %repo.full_name,
                        rejected = ?profile.rejected_types,
                        merge_rate = profile.merge_rate,
                        "🧠 Dream profile loaded — filtering rejected types"
                    );
                }
                findings
                    .into_iter()
                    .filter(|f| {
                        let ftype = f.finding_type.to_string();
                        !profile.rejected_types.iter().any(|r| r == &ftype)
                    })
                    .collect()
            } else {
                findings
            };

        info!(
            repo = %repo.full_name,
            raw = analysis.findings.len(),
            filtered = findings.len(),
            mode = self.config.pipeline.agent_mode,
            "Findings after filtering"
        );

        if findings.is_empty() {
            return Ok(result);
        }

        // ── Agent Mode: Plan (read-only) vs Build (full PR flow) ────────
        if self.config.pipeline.agent_mode == "plan" {
            info!(
                repo = %repo.full_name,
                findings = findings.len(),
                "📋 Plan mode — analysis only, no code generation or PRs"
            );
            result.contributions_generated = 0;
            result.prs_created = 0;

            // Print findings summary
            for f in &findings {
                info!(
                    "  {} [{}/{}] {}",
                    f.severity,
                    f.file_path,
                    f.line_start
                        .map(|l| l.to_string())
                        .unwrap_or_else(|| "?".into()),
                    f.title
                );
            }
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

        // v5.7.1: Build symbol map from relevant files (for type-aware generation)
        let mut symbol_map: HashMap<String, Vec<crate::core::models::Symbol>> = HashMap::new();
        for (file_path, content) in &relevant_files {
            if let Ok(symbols) =
                crate::analysis::ast_intel::AstIntel::extract_symbols(content, file_path)
            {
                if !symbols.is_empty() {
                    symbol_map.insert(file_path.clone(), symbols);
                }
            }
        }

        // v5.8.0: Cross-file import resolution — separate from symbol_map
        let mut resolved_imports: HashMap<String, Vec<crate::core::models::Symbol>> =
            HashMap::new();
        for (file_path, content) in &relevant_files {
            let import_targets =
                crate::analysis::ast_intel::AstIntel::extract_import_targets(content, file_path);
            if !import_targets.is_empty() {
                let resolved = crate::analysis::ast_intel::AstIntel::resolve_imports(
                    &import_targets,
                    &symbol_map,
                );
                if !resolved.is_empty() {
                    let cross_symbols: Vec<crate::core::models::Symbol> = resolved
                        .iter()
                        .map(|(name, sig)| crate::core::models::Symbol {
                            name: format!("{} [resolved: {}]", name, sig),
                            kind: crate::core::models::SymbolKind::Import,
                            file_path: file_path.clone(),
                            line_start: 0,
                            line_end: 0,
                        })
                        .collect();
                    resolved_imports
                        .entry(file_path.clone())
                        .or_default()
                        .extend(cross_symbols);
                }
            }
        }

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
            symbol_map,
            resolved_imports,
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

        // ── v5.5: Batch generation — collect valid contributions, merge into single PR ──
        let mut valid_contributions: Vec<Contribution> = Vec::new();

        for finding in &findings {
            self.event_bus
                .emit(
                    Event::new(EventType::GenerationStart, "pipeline.process_repo")
                        .with_data("title", finding.title.as_str()),
                )
                .await;

            match generator.generate(finding, &repo_context).await {
                Ok(Some(contribution)) => {
                    let repo_prefs = self
                        .memory
                        .get_repo_preferences(&repo.full_name)
                        .ok()
                        .flatten();
                    let report = self.scorer.evaluate(&contribution, repo_prefs.as_ref());
                    if !report.passed {
                        info!(
                            title = %contribution.title,
                            score = report.score,
                            "❌ Quality check failed"
                        );
                        continue;
                    }

                    // Risk classification gate
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

                    // v5.6: Docs-type suppression — skip docs PRs unless repo accepts them
                    if self.config.pipeline.skip_docs_prs
                        && contribution.contribution_type == ContributionType::DocsImprove
                    {
                        let accepts_docs = self
                            .memory
                            .get_repo_preferences(&repo.full_name)
                            .ok()
                            .flatten()
                            .map(|p| p.preferred_types.iter().any(|t| t.contains("docs")))
                            .unwrap_or(false);
                        if !accepts_docs {
                            info!(
                                title = %contribution.title,
                                "📄 Skipping docs-only PR (low merge rate for docs type)"
                            );
                            continue;
                        }
                    }

                    // v5.6: Bug verification — ask LLM if finding is actually real
                    if self.config.pipeline.require_bug_verification
                        && !generator.verify_finding(&contribution, &repo_context).await
                    {
                        info!(
                            title = %contribution.title,
                            "🔍 Bug verification failed — likely false positive"
                        );
                        continue;
                    }

                    info!(
                        title = %contribution.title,
                        risk = %risk.level,
                        reason = %risk.reason,
                        "🛡️ Risk: {}", risk.level
                    );

                    valid_contributions.push(contribution);
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

        if valid_contributions.is_empty() {
            return Ok(result);
        }

        // v5.6: Cross-run dedup — skip if we already have an open PR to this repo
        if self.config.pipeline.skip_repos_with_open_pr {
            if let Ok(existing) = self.memory.get_prs(Some("open"), 100) {
                let has_open = existing
                    .iter()
                    .any(|pr| pr.get("repo").map(|r| r.as_str()) == Some(repo.full_name.as_str()));
                if has_open {
                    info!(
                        repo = %repo.full_name,
                        "🔁 Skipping — already have open PR to this repo"
                    );
                    return Ok(result);
                }
            }
        }

        // Merge multiple contributions into a single multi-file PR
        let merged = Self::merge_contributions(valid_contributions);
        result.contributions_generated = merged.changes.len();

        if dry_run {
            info!(
                title = %merged.title,
                files = merged.changes.len(),
                "🏃 [DRY RUN] Would create multi-file PR"
            );
        } else {
            let mut pr_mgr = PrManager::new(self.github);
            match pr_mgr.create_pr(&merged, repo).await {
                Ok(pr_result) => {
                    result.prs_created += 1;

                    self.memory.record_pr(
                        &repo.full_name,
                        pr_result.pr_number,
                        &pr_result.pr_url,
                        &merged.title,
                        &merged.contribution_type.to_string(),
                        &pr_result.branch_name,
                        &pr_result.fork_full_name,
                    )?;

                    if let Err(e) = self.memory.record_outcome(
                        &repo.full_name,
                        pr_result.pr_number,
                        &pr_result.pr_url,
                        &merged.contribution_type.to_string(),
                        "open",
                        &pr_result.branch_name,
                        0.0,
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
                        files = merged.changes.len(),
                        "✅ Multi-file PR created"
                    );
                }
                Err(e) => {
                    let msg = format!("PR creation failed: {}", e);
                    warn!("{}", msg);
                    result.errors.push(msg);
                }
            }
        }

        Ok(result)
    }

    /// Merge multiple contributions into a single multi-file contribution.
    fn merge_contributions(contribs: Vec<Contribution>) -> Contribution {
        merge_contributions_pub(contribs)
    }

    /// Process a repo's issues (issue-solver mode).
    ///
    /// v5.5: Full end-to-end: fetch issues → solve → generate code → create PR with `Fixes #N`.
    async fn process_repo_issues(
        &self,
        repo: &Repository,
        dry_run: bool,
        max_prs: usize,
        _ctx: &PipelineContext,
    ) -> Result<PipelineResult> {
        use crate::issues::solver::IssueSolver;

        let solver = IssueSolver::new(self.llm, self.github);
        let issues = solver.fetch_solvable_issues(repo, max_prs, 5).await;

        let mut result = PipelineResult {
            repos_analyzed: 1,
            findings_total: issues.len(),
            ..PipelineResult::default()
        };

        if issues.is_empty() {
            return Ok(result);
        }

        // Build repo context (shared across all issues)
        let file_tree = self
            .github
            .get_file_tree(&repo.owner, &repo.name, None)
            .await
            .unwrap_or_default();

        let repo_context = crate::core::models::RepoContext {
            repo: repo.clone(),
            file_tree,
            readme_content: None,
            contributing_guide: None,
            relevant_files: HashMap::new(),
            open_issues: Vec::new(),
            coding_style: None,
            symbol_map: HashMap::new(),
            resolved_imports: HashMap::new(),
            file_ranks: HashMap::new(),
        };

        let generator = ContributionGenerator::new(self.llm, &self.config.contribution);

        for issue in &issues {
            // Solve: issue → finding(s)
            let findings = solver.solve_issue_deep(issue, repo, &repo_context).await;
            if findings.is_empty() {
                info!(issue = issue.number, "No actionable findings for issue");
                continue;
            }

            // Fetch file contents for the specific files identified
            let mut ctx_with_files = repo_context.clone();
            for finding in &findings {
                if !finding.file_path.is_empty()
                    && !ctx_with_files
                        .relevant_files
                        .contains_key(&finding.file_path)
                {
                    if let Ok(content) = self
                        .github
                        .get_file_content(&repo.owner, &repo.name, &finding.file_path, None)
                        .await
                    {
                        ctx_with_files
                            .relevant_files
                            .insert(finding.file_path.clone(), content);
                    }
                }
            }

            // Generate contributions for each finding, then merge
            let mut valid = Vec::new();
            for finding in &findings {
                match generator.generate(finding, &ctx_with_files).await {
                    Ok(Some(mut contribution)) => {
                        // Inject `Fixes #N` into description
                        contribution.description =
                            format!("Fixes #{}\n\n{}", issue.number, contribution.description);
                        valid.push(contribution);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        result
                            .errors
                            .push(format!("Issue #{} generation error: {}", issue.number, e));
                    }
                }
            }

            if valid.is_empty() {
                continue;
            }

            let mut merged = Self::merge_contributions(valid);
            // Ensure PR title references the issue
            merged.title = format!("fix: resolve #{} — {}", issue.number, issue.title);
            merged.commit_message = format!(
                "fix: resolve #{} — {}\n\nFixes #{}",
                issue.number, issue.title, issue.number
            );
            result.contributions_generated += 1;

            if dry_run {
                info!(
                    issue = issue.number,
                    title = %merged.title,
                    files = merged.changes.len(),
                    "🏃 [DRY RUN] Would create issue-solving PR"
                );
                continue;
            }

            let mut pr_mgr = PrManager::new(self.github);
            match pr_mgr.create_pr(&merged, repo).await {
                Ok(pr_result) => {
                    result.prs_created += 1;

                    self.memory.record_pr(
                        &repo.full_name,
                        pr_result.pr_number,
                        &pr_result.pr_url,
                        &merged.title,
                        &merged.contribution_type.to_string(),
                        &pr_result.branch_name,
                        &pr_result.fork_full_name,
                    )?;

                    info!(
                        issue = issue.number,
                        pr = pr_result.pr_number,
                        url = %pr_result.pr_url,
                        "✅ Issue-solving PR created (Fixes #{})",
                        issue.number
                    );
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Issue #{} PR failed: {}", issue.number, e));
                }
            }
        }

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
