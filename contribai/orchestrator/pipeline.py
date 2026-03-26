"""Main pipeline orchestrator.

Coordinates the full contribution flow:
discover → analyze → generate → PR.
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field

from contribai.agents.registry import create_default_registry
from contribai.analysis.analyzer import CodeAnalyzer
from contribai.core.config import ContribAIConfig
from contribai.core.middleware import build_default_chain
from contribai.core.models import (
    AnalysisResult,
    DiscoveryCriteria,
    PRResult,
    Repository,
)
from contribai.generator.engine import ContributionGenerator
from contribai.github.client import GitHubClient
from contribai.github.discovery import RepoDiscovery
from contribai.github.guidelines import fetch_repo_guidelines
from contribai.issues.solver import IssueSolver
from contribai.llm.provider import create_llm_provider
from contribai.orchestrator.memory import Memory
from contribai.pr.manager import PRManager
from contribai.tools.protocol import create_default_tools

logger = logging.getLogger(__name__)

# Files that should NOT be modified/created by ContribAI
# These are meta/governance files that projects manage themselves
PROTECTED_META_FILES = {
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
}

# File extensions to skip — doc/config-only changes are low-value
# Only code files should be modified
SKIP_EXTENSIONS = {
    ".md",
    ".txt",
    ".rst",
    ".yml",
    ".yaml",
    ".toml",
    ".cfg",
    ".ini",
    ".json",
}


def _titles_similar(title_a: str, title_b: str) -> bool:
    """Check if two finding/PR titles are similar enough to be duplicates.

    Uses keyword overlap: if >50% of significant words match, consider similar.
    """
    stop_words = {"a", "an", "the", "in", "on", "of", "for", "to", "and", "or", "is"}
    words_a = {w for w in title_a.lower().split() if w not in stop_words and len(w) > 2}
    words_b = {w for w in title_b.lower().split() if w not in stop_words and len(w) > 2}
    if not words_a or not words_b:
        return False
    overlap = len(words_a & words_b)
    smaller = min(len(words_a), len(words_b))
    return overlap / smaller > 0.5


@dataclass
class PipelineResult:
    """Result of a pipeline run."""

    repos_analyzed: int = 0
    findings_total: int = 0
    contributions_generated: int = 0
    prs_created: int = 0
    prs: list[PRResult] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)


class ContribPipeline:
    """Main orchestrator for the contribution pipeline."""

    def __init__(self, config: ContribAIConfig):
        self.config = config
        self._github: GitHubClient | None = None
        self._llm = None
        self._memory: Memory | None = None
        self._analyzer: CodeAnalyzer | None = None
        self._generator: ContributionGenerator | None = None
        self._pr_manager: PRManager | None = None
        self._discovery: RepoDiscovery | None = None
        self._middleware_chain: list = []
        self._agent_registry = None
        self._tool_registry = None

    async def _init_components(self):
        """Initialize all pipeline components."""
        # LLM — with optional multi-model routing
        mm = self.config.multi_model
        self._llm = create_llm_provider(
            self.config.llm,
            multi_model=mm.enabled,
            strategy=mm.strategy,
        )

        # GitHub
        self._github = GitHubClient(
            token=self.config.github.token,
            rate_limit_buffer=self.config.github.rate_limit_buffer,
        )

        # Memory
        self._memory = Memory(self.config.storage.resolved_db_path)
        await self._memory.init()

        # Analyzer
        self._analyzer = CodeAnalyzer(
            llm=self._llm,
            github=self._github,
            config=self.config.analysis,
        )

        # Generator — now with memory for repo_preferences
        self._generator = ContributionGenerator(
            llm=self._llm,
            config=self.config.contribution,
            memory=self._memory,
        )

        # PR Manager
        self._pr_manager = PRManager(github=self._github)

        # Discovery
        self._discovery = RepoDiscovery(
            client=self._github,
            config=self.config.discovery,
        )

        # Middleware chain (DeerFlow pattern)
        self._middleware_chain = build_default_chain(
            max_prs_per_day=self.config.github.max_prs_per_day,
            max_retries=self.config.pipeline.max_retries,
            min_quality_score=self.config.pipeline.min_quality_score,
        )
        logger.info("Middleware chain: %d middlewares loaded", len(self._middleware_chain))

        # Agent registry (DeerFlow pattern)
        self._agent_registry = create_default_registry()
        logger.info(
            "Agent registry: %d agents loaded",
            len(self._agent_registry.list_agents()),
        )

        # Tool registry (DeerFlow pattern)
        self._tool_registry = create_default_tools(
            github_client=self._github,
            llm_provider=self._llm,
        )
        logger.info(
            "Tool registry: %d tools loaded",
            len(self._tool_registry.list_tools()),
        )

    async def _cleanup(self):
        """Clean up resources."""
        if self._github:
            await self._github.close()
        if self._llm:
            await self._llm.close()
        if self._memory:
            await self._memory.close()

    # ── Public API ─────────────────────────────────────────────────────────

    async def run(
        self,
        criteria: DiscoveryCriteria | None = None,
        dry_run: bool = False,
    ) -> PipelineResult:
        """Run the full pipeline: discover -> analyze -> generate -> PR.

        Processes multiple repos in parallel using asyncio.Semaphore.

        Args:
            criteria: Optional custom discovery criteria
            dry_run: If True, analyze and generate but don't create PRs
        """
        await self._init_components()
        result = PipelineResult()
        run_id = await self._memory.start_run()

        try:
            # Check daily PR limit
            today_prs = await self._memory.get_today_pr_count()
            remaining_prs = self.config.github.max_prs_per_day - today_prs
            if remaining_prs <= 0 and not dry_run:
                logger.warning(
                    "Daily PR limit reached (%d)",
                    self.config.github.max_prs_per_day,
                )
                return result

            # 1. Discover repos
            logger.info("Discovering repositories...")
            repos = await self._discovery.discover(criteria)
            if not repos:
                logger.warning("No repositories found matching criteria")
                return result

            logger.info("Found %d candidate repositories", len(repos))

            # Limit to max repos per run
            repos = repos[: self.config.github.max_repos_per_run]

            # 2. Process repos in parallel with semaphore
            max_conc = self.config.pipeline.max_concurrent_repos
            sem = asyncio.Semaphore(max_conc)
            logger.info(
                "Processing %d repos (max %d concurrent)",
                len(repos),
                max_conc,
            )

            async def _guarded(
                repo: Repository,
            ) -> PipelineResult | None:
                async with sem:
                    if await self._memory.has_analyzed(repo.full_name):
                        logger.info(
                            "Skipping %s (already analyzed)",
                            repo.full_name,
                        )
                        return None
                    try:
                        return await self._process_repo(repo, dry_run, remaining_prs)
                    except Exception as e:
                        msg = f"Error processing {repo.full_name}: {e}"
                        logger.error(msg)
                        err = PipelineResult()
                        err.errors.append(msg)
                        return err

            repo_results = await asyncio.gather(*[_guarded(r) for r in repos])

            # Aggregate results
            for rr in repo_results:
                if rr is None:
                    continue
                result.repos_analyzed += 1
                result.findings_total += rr.findings_total
                result.contributions_generated += rr.contributions_generated
                result.prs_created += rr.prs_created
                result.prs.extend(rr.prs)
                result.errors.extend(rr.errors)

            # Log run
            await self._memory.finish_run(
                run_id,
                repos_analyzed=result.repos_analyzed,
                prs_created=result.prs_created,
                findings=result.findings_total,
                errors=len(result.errors),
            )

        finally:
            await self._cleanup()

        return result

    async def hunt(
        self,
        *,
        rounds: int = 5,
        delay_sec: int = 30,
        dry_run: bool = False,
        mode: str = "both",
    ) -> PipelineResult:
        """Hunt mode: aggressively discover and contribute to repos.

        Runs multiple discovery rounds with varied criteria.
        For each round:
        1. Discover repos (varied star range, shuffled languages)
        2. Filter to repos that actually merge external PRs
        3. Process each repo through the full pipeline
        4. Wait between rounds to avoid rate limits

        Args:
            rounds: Number of discovery rounds
            delay_sec: Delay between rounds
            dry_run: If True, don't create PRs
            mode: 'analysis' (code scan), 'issues' (issue solving), 'both'
        """
        import random

        await self._init_components()
        total = PipelineResult()

        cfg_min, cfg_max = self.config.discovery.stars_range
        star_tiers = [
            (cfg_min, cfg_max),
            (100, 1000),
            (1000, 5000),
            (5000, 20000),
            (500, 3000),
        ]
        langs = list(self.config.discovery.languages)

        try:
            for rnd in range(1, rounds + 1):
                today_prs = await self._memory.get_today_pr_count()
                remaining = self.config.github.max_prs_per_day - today_prs
                if remaining <= 0 and not dry_run:
                    logger.warning(
                        "🛑 Daily PR limit reached (%d). Stopping.",
                        self.config.github.max_prs_per_day,
                    )
                    break

                random.shuffle(langs)
                stars = star_tiers[(rnd - 1) % len(star_tiers)]
                criteria = DiscoveryCriteria(
                    languages=langs[:2],
                    stars_min=stars[0],
                    stars_max=stars[1],
                    min_last_activity_days=7,
                    max_results=10,
                )

                logger.info(
                    "🔥 Hunt round %d/%d — %s, ★ %d-%d",
                    rnd,
                    rounds,
                    "/".join(criteria.languages),
                    stars[0],
                    stars[1],
                )

                repos = await self._discovery.discover(criteria)
                if not repos:
                    logger.info("No repos found this round")
                    if rnd < rounds:
                        await asyncio.sleep(delay_sec)
                    continue

                # Filter to merge-friendly repos
                targets: list[Repository] = []
                for repo in repos[:5]:
                    if await self._memory.has_analyzed(repo.full_name):
                        continue
                    try:
                        prs = await self._github.list_pull_requests(
                            repo.owner,
                            repo.name,
                            state="closed",
                            per_page=10,
                        )
                        merged = [p for p in prs if p.get("merged_at")]
                        if merged:
                            logger.info(
                                "✅ %s — %d merged PRs, good target!",
                                repo.full_name,
                                len(merged),
                            )
                            targets.append(repo)
                    except Exception:
                        pass

                if not targets:
                    logger.info("No merge-friendly repos this round")
                    if rnd < rounds:
                        await asyncio.sleep(delay_sec)
                    continue

                max_targets = self.config.github.max_repos_per_run
                max_conc = self.config.pipeline.max_concurrent_repos
                sem = asyncio.Semaphore(max_conc)
                selected = targets[:max_targets]

                logger.info(
                    "Processing %d repos (max %d concurrent)",
                    len(selected),
                    max_conc,
                )

                # Process repos sequentially with inter-repo delay
                # to avoid RESOURCE_EXHAUSTED rate limits
                delay_between = self.config.pipeline.inter_repo_delay_sec
                for i, repo in enumerate(selected):
                    if remaining <= 0 and not dry_run:
                        logger.warning("PR limit reached mid-round")
                        break
                    rr = await self._hunt_process_repo(repo, mode, dry_run, remaining, sem)
                    total.repos_analyzed += rr.repos_analyzed
                    total.findings_total += rr.findings_total
                    total.contributions_generated += rr.contributions_generated
                    total.prs_created += rr.prs_created
                    total.prs.extend(rr.prs)
                    total.errors.extend(rr.errors)
                    remaining -= rr.prs_created

                    # Inter-repo delay (skip after last repo)
                    if i < len(selected) - 1 and delay_between > 0:
                        logger.debug("⏳ Inter-repo delay: %.1fs", delay_between)
                        await asyncio.sleep(delay_between)

                if rnd < rounds:
                    logger.info(
                        "⏳ Waiting %ds before next round...",
                        delay_sec,
                    )
                    await asyncio.sleep(delay_sec)

        finally:
            await self._cleanup()

        return total

    async def _hunt_process_repo(
        self,
        repo: Repository,
        mode: str,
        dry_run: bool,
        remaining: int,
        sem: asyncio.Semaphore,
    ) -> PipelineResult:
        """Process a single repo in hunt mode (used for parallel execution)."""
        async with sem:
            rr = PipelineResult()
            try:
                if mode in ("analysis", "both"):
                    analysis_rr = await self._process_repo(repo, dry_run, remaining)
                    rr.repos_analyzed += analysis_rr.repos_analyzed
                    rr.findings_total += analysis_rr.findings_total
                    rr.contributions_generated += analysis_rr.contributions_generated
                    rr.prs_created += analysis_rr.prs_created
                    rr.prs.extend(analysis_rr.prs)

                if mode in ("issues", "both"):
                    issue_rr = await self._process_repo_issues(
                        repo, dry_run, remaining - rr.prs_created
                    )
                    rr.repos_analyzed = max(rr.repos_analyzed, issue_rr.repos_analyzed)
                    rr.findings_total += issue_rr.findings_total
                    rr.contributions_generated += issue_rr.contributions_generated
                    rr.prs_created += issue_rr.prs_created
                    rr.prs.extend(issue_rr.prs)

                rr.repos_analyzed = max(rr.repos_analyzed, 1)
            except Exception as e:
                rr.errors.append(f"{repo.full_name}: {e}")
                logger.error("Error processing %s: %s", repo.full_name, e)
            return rr

    async def run_single(
        self,
        repo_url: str,
        dry_run: bool = False,
    ) -> PipelineResult:
        """Run the pipeline on a single specific repo.

        Args:
            repo_url: GitHub repository URL (e.g., https://github.com/owner/repo)
            dry_run: If True, analyze and generate but don't create PRs
        """
        # Parse URL
        parts = repo_url.rstrip("/").split("/")
        owner, name = parts[-2], parts[-1]

        await self._init_components()
        result = PipelineResult()

        try:
            repo = await self._github.get_repo_details(owner, name)
            repo_result = await self._process_repo(repo, dry_run)
            result.repos_analyzed = 1
            result.findings_total = repo_result.findings_total
            result.contributions_generated = repo_result.contributions_generated
            result.prs_created = repo_result.prs_created
            result.prs = repo_result.prs
        except Exception as e:
            result.errors.append(str(e))
            logger.error("Failed: %s", e)
        finally:
            await self._cleanup()

        return result

    async def analyze_only(self, repo_url: str) -> AnalysisResult | None:
        """Analyze a repo without generating contributions or PRs."""
        parts = repo_url.rstrip("/").split("/")
        owner, name = parts[-2], parts[-1]

        await self._init_components()
        try:
            repo = await self._github.get_repo_details(owner, name)
            return await self._analyzer.analyze(repo)
        finally:
            await self._cleanup()

    # ── Internal ───────────────────────────────────────────────────────────

    async def _process_repo(
        self, repo: Repository, dry_run: bool, max_prs: int = 5
    ) -> PipelineResult:
        """Process a single repository through the full pipeline."""
        result = PipelineResult()
        logger.info("=" * 60)
        logger.info("📦 Processing: %s", repo.full_name)

        # Check AI policy — skip repos that ban AI-generated PRs
        if await self._check_ai_policy(repo):
            logger.warning(
                "🚫 %s has an AI policy that bans AI PRs, skipping.",
                repo.full_name,
            )
            result.repos_analyzed = 1
            return result

        # Fetch repo guidelines (CONTRIBUTING.md, PR template)
        guidelines = await fetch_repo_guidelines(self._github, repo.owner, repo.name)
        if guidelines.has_guidelines:
            logger.info(
                "📋 Repo guidelines: commit=%s, %d template sections",
                guidelines.commit_format,
                len(guidelines.required_sections),
            )

        # Analyze — set task context for model routing
        logger.info("🔬 Analyzing code...")
        self._set_task("analysis")
        analysis = await self._analyzer.analyze(repo)
        result.findings_total = len(analysis.findings)

        await self._memory.record_analysis(
            repo.full_name,
            repo.language or "unknown",
            repo.stars,
            len(analysis.findings),
        )

        if not analysis.findings:
            logger.info("No findings for %s", repo.full_name)
            return result

        # --- Early finding filter (pre-generation) ---
        # Filter out findings that target non-code files (blocked by SKIP_EXTENSIONS)
        # or are irrelevant to the project type, BEFORE wasting LLM calls.
        pre_filter_count = len(analysis.findings)
        filtered = []
        for f in analysis.findings:
            fp = f.file_path or ""
            ext = "." + fp.rsplit(".", 1)[-1].lower() if "." in fp else ""

            # Skip findings on non-code files (would be blocked at commit anyway)
            if ext in SKIP_EXTENSIONS:
                logger.debug("⏭️ Pre-filter: skip non-code file %s", fp)
                continue

            # Skip findings on protected meta files
            basename = fp.rsplit("/", 1)[-1] if "/" in fp else fp
            if basename.upper() in PROTECTED_META_FILES:
                logger.debug("⏭️ Pre-filter: skip protected file %s", fp)
                continue

            filtered.append(f)

        if len(filtered) < pre_filter_count:
            logger.info(
                "🔍 Pre-filter: %d → %d findings (removed %d non-code targets)",
                pre_filter_count,
                len(filtered),
                pre_filter_count - len(filtered),
            )
            analysis.findings = filtered

        if not analysis.findings:
            logger.info("All findings filtered (non-code targets) for %s", repo.full_name)
            return result

        logger.info(
            "Found %d issues (analyzed %d files in %.1fs)",
            len(analysis.findings),
            analysis.analyzed_files,
            analysis.analysis_duration_sec,
        )

        # Build context for generation — fetch files for ALL findings we'll process
        file_tree = await self._github.get_file_tree(repo.owner, repo.name)
        relevant_files: dict[str, str] = {}
        # Deduplicate file paths across all findings we'll process
        file_paths_to_fetch = []
        for finding in analysis.top_findings[:max_prs]:
            if finding.file_path and finding.file_path not in relevant_files:
                file_paths_to_fetch.append(finding.file_path)

        for fpath in file_paths_to_fetch:
            try:
                content = await self._github.get_file_content(repo.owner, repo.name, fpath)
                relevant_files[fpath] = content
            except Exception:
                logger.debug("Could not fetch %s", fpath)

        logger.info(
            "Fetched %d/%d unique files for code gen",
            len(relevant_files),
            len(file_paths_to_fetch),
        )

        from contribai.core.models import RepoContext

        context = RepoContext(
            repo=repo,
            file_tree=file_tree,
            relevant_files=relevant_files,
        )

        # Filter out findings that overlap with previously submitted PRs
        # Check BOTH local memory AND GitHub API for existing PRs
        past_titles_lower: set[str] = set()
        past_file_paths: set[str] = set()

        # 1) Local memory
        past_prs = await self._memory.get_repo_prs(repo.full_name)
        for pr in past_prs:
            past_titles_lower.add(pr.get("title", "").lower())

        # 2) GitHub API — fetch recent PRs (all states) to catch external PRs too
        try:
            github_prs = await self._github.list_pull_requests(
                repo.owner, repo.name, state="all", per_page=50
            )
            for gpr in github_prs:
                past_titles_lower.add(gpr.get("title", "").lower())
                # Extract file paths from branch name (contribai branches encode the topic)
                head = gpr.get("head", {})
                branch_label = head.get("label", "")
                if "contribai/" in branch_label:
                    past_titles_lower.add(gpr.get("title", "").lower())
                # Track all recently-targeted file info from PR body
                body = gpr.get("body", "") or ""
                # Extract file paths mentioned in PR bodies (e.g. `src/foo/bar.ts`)
                import re

                for match in re.findall(r"`(src/[^\s`]+\.\w+)`", body):
                    past_file_paths.add(match)
        except Exception:
            logger.debug("Could not fetch GitHub PRs for dedup, using memory only")

        original_count = len(analysis.top_findings[:max_prs])
        filtered_findings = []
        for finding in analysis.top_findings[:max_prs]:
            title_lower = finding.title.lower()
            # Check title similarity
            is_title_dup = any(_titles_similar(title_lower, pt) for pt in past_titles_lower)
            # Check if same file was already targeted
            is_file_dup = finding.file_path in past_file_paths if finding.file_path else False

            if is_title_dup:
                logger.info(
                    "⏭️ Skipping duplicate finding: %s (similar PR exists)",
                    finding.title,
                )
                continue
            if is_file_dup:
                logger.info(
                    "⏭️ Skipping finding on already-targeted file: %s → %s",
                    finding.title,
                    finding.file_path,
                )
                continue
            filtered_findings.append(finding)

        if len(filtered_findings) < original_count:
            logger.info(
                "🔁 Filtered %d duplicate findings (%d remaining)",
                original_count - len(filtered_findings),
                len(filtered_findings),
            )

        if not filtered_findings:
            logger.info("No new findings after duplicate filter")
            result.repos_analyzed = 1
            return result

        # Validate findings against full file content to filter false positives
        validated_findings = await self._validate_findings(filtered_findings, relevant_files)

        # Limit to max 2 findings per repo to avoid spamming
        if len(validated_findings) > 2:
            logger.info(
                "📉 Limiting to 2 findings per repo (had %d)",
                len(validated_findings),
            )
            validated_findings = validated_findings[:2]

        logger.info(
            "🔎 Validated %d/%d findings (filtered %d false positives)",
            len(validated_findings),
            min(len(analysis.top_findings), max_prs),
            min(len(analysis.top_findings), max_prs) - len(validated_findings),
        )

        # Generate contributions for validated findings
        for finding in validated_findings:
            logger.info("🛠️ Generating fix for: %s", finding.title)
            self._set_task("code_gen")
            contribution = await self._generator.generate(finding, context, guidelines=guidelines)

            if not contribution:
                continue

            result.contributions_generated += 1

            if dry_run:
                logger.info("🏃 [DRY RUN] Would create PR: %s", contribution.title)
                continue

            # Create PR
            try:
                logger.info("📤 Creating PR...")
                pr_result = await self._pr_manager.create_pr(
                    contribution, repo, guidelines=guidelines
                )
                result.prs_created += 1
                result.prs.append(pr_result)

                # Record in memory
                await self._memory.record_pr(
                    repo=repo.full_name,
                    pr_number=pr_result.pr_number,
                    pr_url=pr_result.pr_url,
                    title=contribution.title,
                    pr_type=contribution.contribution_type.value,
                    branch=pr_result.branch_name,
                    fork=pr_result.fork_full_name,
                )

                # 5. Post-PR compliance check & auto-fix
                try:
                    logger.info("🔍 Checking PR compliance...")
                    await self._pr_manager.check_compliance_and_fix(
                        pr_result,
                        contribution,
                        guidelines=guidelines,
                    )
                except Exception as e:
                    logger.warning("Compliance check failed: %s", e)

                # 6. Wait for CI and auto-close if tests fail
                try:
                    await self._check_ci_and_close_if_failed(pr_result, repo)
                except Exception as e:
                    logger.warning("CI check failed: %s", e)
            except Exception as e:
                error = f"PR creation failed for {finding.title}: {e}"
                logger.error(error)
                result.errors.append(error)

        result.repos_analyzed = 1
        return result

    async def _process_repo_issues(
        self, repo: Repository, dry_run: bool, max_prs: int = 3
    ) -> PipelineResult:
        """Process a repo by solving its open Issues.

        v2.0.0: Issue-driven mode. Fetches solvable issues, uses
        solve_issue_deep() to plan multi-file changes, generates
        contributions, and creates PRs that close issues.
        """
        result = PipelineResult()
        logger.info("📋 Looking for solvable issues in %s...", repo.full_name)

        # Check AI policy first
        if await self._check_ai_policy(repo):
            logger.warning(
                "🚫 %s bans AI PRs, skipping issue solving.",
                repo.full_name,
            )
            return result

        # Initialize issue solver
        solver = IssueSolver(llm=self._llm, github=self._github)

        # Fetch solvable issues
        issues = await solver.fetch_solvable_issues(repo, max_issues=max_prs, max_complexity=3)

        if not issues:
            logger.info("No solvable issues found in %s", repo.full_name)
            return result

        # Fetch repo guidelines
        guidelines = await fetch_repo_guidelines(self._github, repo.owner, repo.name)

        # Build repo context with more files for deeper understanding
        file_tree = await self._github.get_file_tree(repo.owner, repo.name)
        relevant_files: dict[str, str] = {}

        # Fetch key files for context (README, main modules, etc.)
        key_files = self._identify_key_files(file_tree, repo)
        for fpath in key_files[:10]:
            try:
                content = await self._github.get_file_content(repo.owner, repo.name, fpath)
                relevant_files[fpath] = content
            except Exception:
                pass

        from contribai.core.models import RepoContext

        context = RepoContext(
            repo=repo,
            file_tree=file_tree,
            relevant_files=relevant_files,
        )

        # Process each issue
        for issue in issues:
            if result.prs_created >= max_prs:
                break

            logger.info(
                "🧠 Solving issue #%d: %s",
                issue.number,
                issue.title,
            )

            # Deep solve → multi-file findings
            self._set_task("analysis")
            findings = await solver.solve_issue_deep(issue, repo, context)

            # Filter out findings that only touch non-code files
            # (docs, configs, meta files — low-value changes)
            def _is_code_file(path: str | None) -> bool:
                if not path:
                    return True  # no path → keep it
                import os

                _, ext = os.path.splitext(path.lower())
                if ext in SKIP_EXTENSIONS:
                    return False
                return path.lower() not in {p.lower() for p in PROTECTED_META_FILES}

            findings = [f for f in findings if _is_code_file(f.file_path)]

            result.findings_total += len(findings)

            if not findings:
                logger.info("Could not solve issue #%d", issue.number)
                continue

            # Fetch file contents for each finding
            for finding in findings:
                if finding.file_path and finding.file_path not in relevant_files:
                    try:
                        content = await self._github.get_file_content(
                            repo.owner, repo.name, finding.file_path
                        )
                        relevant_files[finding.file_path] = content
                        context.relevant_files[finding.file_path] = content
                    except Exception:
                        pass

            # Generate contributions — first finding is the primary one
            # The generator already handles multi-file via cross-file matching
            primary = findings[0]
            logger.info(
                "🛠️ Generating fix for issue #%d (%d files)...",
                issue.number,
                len(findings),
            )

            self._set_task("code_gen")
            contribution = await self._generator.generate(primary, context, guidelines=guidelines)

            if not contribution:
                logger.warning(
                    "Failed to generate contribution for issue #%d",
                    issue.number,
                )
                continue

            result.contributions_generated += 1

            if dry_run:
                logger.info(
                    "🏃 [DRY RUN] Would create PR for issue #%d: %s",
                    issue.number,
                    contribution.title,
                )
                continue

            # Create PR with "Closes #N" in body
            try:
                logger.info("📤 Creating PR for issue #%d...", issue.number)
                pr_result = await self._pr_manager.create_pr(
                    contribution,
                    repo,
                    guidelines=guidelines,
                    closes_issue=issue.number,
                )
                result.prs_created += 1
                result.prs.append(pr_result)

                await self._memory.record_pr(
                    repo=repo.full_name,
                    pr_number=pr_result.pr_number,
                    pr_url=pr_result.pr_url,
                    title=contribution.title,
                    pr_type=contribution.contribution_type.value,
                    branch=pr_result.branch_name,
                    fork=pr_result.fork_full_name,
                )

                # Post-PR compliance
                try:
                    await self._pr_manager.check_compliance_and_fix(
                        pr_result, contribution, guidelines=guidelines
                    )
                except Exception as e:
                    logger.warning("Compliance check failed: %s", e)

                # CI check
                try:
                    await self._check_ci_and_close_if_failed(pr_result, repo)
                except Exception as e:
                    logger.warning("CI check failed: %s", e)

            except Exception as e:
                error = f"PR creation failed for issue #{issue.number}: {e}"
                logger.error(error)
                result.errors.append(error)

        result.repos_analyzed = 1
        return result

    def _identify_key_files(self, file_tree: list, repo: Repository) -> list[str]:
        """Identify key files in a repo for building context.

        Prioritizes: README, main entry points, config files, core modules.
        """
        priority_patterns = [
            "README.md",
            "CONTRIBUTING.md",
            "setup.py",
            "pyproject.toml",
            "package.json",
            "Cargo.toml",
            "go.mod",
        ]

        # Collect all blob paths
        all_files = [f.path for f in file_tree if f.type == "blob"]

        key_files: list[str] = []

        # Add priority files first
        for pattern in priority_patterns:
            for fpath in all_files:
                if fpath.endswith(pattern) and fpath not in key_files:
                    key_files.append(fpath)
                    break

        # Add main entry points based on language
        lang = (repo.language or "").lower()
        entry_patterns = {
            "python": ["__init__.py", "main.py", "app.py", "cli.py"],
            "javascript": ["index.js", "app.js", "server.js"],
            "typescript": ["index.ts", "app.ts", "main.ts"],
            "go": ["main.go", "cmd/main.go"],
            "rust": ["main.rs", "lib.rs"],
        }

        for pat in entry_patterns.get(lang, []):
            for fpath in all_files:
                if fpath.endswith(pat) and fpath not in key_files:
                    key_files.append(fpath)

        # Add source files from common directories
        src_dirs = ["src/", "lib/", "app/", "pkg/", "internal/"]
        for fpath in all_files:
            if len(key_files) >= 15:
                break
            if any(fpath.startswith(d) for d in src_dirs) and fpath not in key_files:
                key_files.append(fpath)

        return key_files[:15]

    async def _validate_findings(
        self,
        findings: list,
        relevant_files: dict[str, str],
    ) -> list:
        """Validate findings against full file content to filter false positives.

        For each finding, asks the LLM to re-examine whether the issue is
        genuinely valid given the complete file context. This catches issues
        like:
        - Code protected by circuit breakers / error boundaries
        - Maps bounded by static data sources
        - Functions called only from safe contexts
        """
        if not findings:
            return []

        self._set_task("validation")
        validated = []

        for finding in findings:
            file_content = relevant_files.get(finding.file_path, "")
            if not file_content:
                # Can't validate without file content — keep the finding
                validated.append(finding)
                continue

            prompt = (
                f"## Finding Validation\n\n"
                f"A code analyzer found this issue. Your job is to determine "
                f"if it is a GENUINE problem or a FALSE POSITIVE.\n\n"
                f"### Finding\n"
                f"- **Title**: {finding.title}\n"
                f"- **Severity**: {finding.severity.value}\n"
                f"- **File**: {finding.file_path}\n"
                f"- **Description**: {finding.description}\n"
                f"- **Suggestion**: {finding.suggestion}\n\n"
                f"### Full File Content\n"
                f"```\n{file_content[:12000]}\n```\n\n"
                f"### Validation Checklist\n"
                f"Check ALL of these before deciding:\n"
                f"1. Is the affected code already protected by try/catch, "
                f"circuit breakers, error boundaries, or fallback patterns?\n"
                f"2. If the finding is about unbounded growth — is the data source "
                f"actually bounded (static array, enum, hardcoded list, config)?\n"
                f"3. Is the function only called from contexts where the issue "
                f"cannot occur?\n"
                f"4. Would the suggested fix add unnecessary complexity without "
                f"real benefit?\n"
                f"5. Does the existing code already handle this edge case through "
                f"a different mechanism?\n\n"
                f"### Response\n"
                f"Respond with EXACTLY one line:\n"
                f"VALID: [brief reason why this is a real issue]\n"
                f"or\n"
                f"INVALID: [brief reason why this is a false positive]\n"
            )

            try:
                response = await self._llm.complete(
                    prompt,
                    system=(
                        "You are a senior code reviewer validating automated findings. "
                        "Be skeptical — reject findings that are false positives. "
                        "A finding is INVALID if the code is already protected or "
                        "the issue doesn't exist in practice."
                    ),
                    temperature=0.1,
                )

                response_text = response.strip().upper()
                if response_text.startswith("INVALID"):
                    logger.info(
                        "❌ Finding rejected: %s — %s",
                        finding.title,
                        response.strip(),
                    )
                    continue

                logger.info(
                    "✅ Finding validated: %s — %s",
                    finding.title,
                    response.strip()[:80],
                )
                validated.append(finding)

            except Exception as e:
                logger.warning("Validation failed for %s: %s, keeping", finding.title, e)
                validated.append(finding)

        return validated

    async def _check_ai_policy(self, repo: Repository) -> bool:
        """Check if a repo has an AI policy that bans AI-generated PRs.

        Checks:
        - AI_POLICY.md or .github/AI_POLICY.md
        - Keywords in CONTRIBUTING.md suggesting AI PRs are banned

        Returns True if the repo bans AI PRs.
        """
        ai_policy_paths = [
            "AI_POLICY.md",
            ".github/AI_POLICY.md",
            ".github/ai_policy.md",
        ]

        for path in ai_policy_paths:
            try:
                content = await self._github.get_file_content(repo.owner, repo.name, path)
                if content:
                    content_lower = content.lower()
                    # Check for ban keywords
                    ban_keywords = [
                        "do not accept ai",
                        "no ai-generated",
                        "ai contributions are not accepted",
                        "ban ai",
                        "prohibit ai",
                        "ai-generated pull requests will be closed",
                        "reject ai",
                    ]
                    if any(kw in content_lower for kw in ban_keywords):
                        return True
            except Exception:
                pass

        # Also check CONTRIBUTING.md for anti-AI language
        try:
            for contrib_path in ["CONTRIBUTING.md", ".github/CONTRIBUTING.md"]:
                try:
                    content = await self._github.get_file_content(
                        repo.owner, repo.name, contrib_path
                    )
                    if content:
                        content_lower = content.lower()
                        ban_phrases = [
                            "ai-generated contributions",
                            "no ai pull requests",
                            "ban on ai-generated",
                            "do not submit ai",
                            "see ai_policy",
                        ]
                        if any(phrase in content_lower for phrase in ban_phrases):
                            return True
                except Exception:
                    pass
        except Exception:
            pass

        return False

    async def _check_ci_and_close_if_failed(
        self,
        pr_result: PRResult,
        repo: Repository,
        *,
        max_wait_sec: int = 90,
        poll_interval: int = 15,
    ) -> None:
        """Wait for CI checks and auto-close the PR if they fail.

        Polls the PR's head commit for check run results. If required
        checks fail (e.g. lint, typecheck, unit tests), closes the PR
        with a comment explaining which checks failed.
        """
        import asyncio

        branch = pr_result.branch_name
        fork_parts = pr_result.fork_full_name.split("/")
        fork_owner = fork_parts[0]
        fork_name = fork_parts[1] if len(fork_parts) > 1 else repo.name

        # Get the head SHA of the PR branch
        try:
            branch_data = await self._github._get(
                f"/repos/{fork_owner}/{fork_name}/git/ref/heads/{branch}"
            )
            head_sha = branch_data["object"]["sha"]
        except Exception:
            logger.debug("Could not get head SHA for CI check, skipping")
            return

        logger.info("⏳ Waiting for CI checks on PR #%d...", pr_result.pr_number)

        elapsed = 0
        while elapsed < max_wait_sec:
            await asyncio.sleep(poll_interval)
            elapsed += poll_interval

            status = await self._github.get_combined_status(repo.owner, repo.name, head_sha)

            if status["state"] == "pending":
                logger.debug(
                    "CI still running (%ds/%ds): %s",
                    elapsed,
                    max_wait_sec,
                    ", ".join(status.get("in_progress", [])),
                )
                continue

            if status["state"] == "success":
                logger.info(
                    "✅ CI passed for PR #%d (%d checks)",
                    pr_result.pr_number,
                    status["total"],
                )
                return

            if status["state"] == "failure":
                failed_names = ", ".join(status["failed"])
                logger.warning(
                    "❌ CI failed for PR #%d: %s",
                    pr_result.pr_number,
                    failed_names,
                )

                # Auto-close with comment
                comment = (
                    "## Auto-closed: CI checks failed\n\n"
                    f"The following checks failed: **{failed_names}**\n\n"
                    "Closing this PR since required CI checks did not pass. "
                    "Sorry for the inconvenience."
                )
                await self._github.close_pull_request(
                    repo.owner,
                    repo.name,
                    pr_result.pr_number,
                    comment=comment,
                )
                # Record closed status in memory
                await self._memory.update_pr_status(
                    repo.full_name, pr_result.pr_number, "ci_failed"
                )
                return

        # Timeout — log but don't close (CI may still be running)
        logger.info(
            "⏰ CI check timed out after %ds for PR #%d, leaving open",
            max_wait_sec,
            pr_result.pr_number,
        )

    def _set_task(self, task_name: str) -> None:
        """Set the current task context for multi-model routing."""
        from contribai.llm.provider import MultiModelProvider

        if isinstance(self._llm, MultiModelProvider):
            import contextlib

            from contribai.llm.models import TaskType

            with contextlib.suppress(ValueError):
                self._llm.set_task(TaskType(task_name))
