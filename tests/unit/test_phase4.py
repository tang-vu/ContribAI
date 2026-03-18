"""Tests for Phase 4: templates, profiles, scheduler, web, and parallel pipeline."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from contribai.core.config import (
    ContribAIConfig,
    PipelineConfig,
    SchedulerConfig,
    WebConfig,
)
from contribai.core.profiles import (
    BUILTIN_PROFILES,
    ContribProfile,
    apply_profile,
    get_profile,
    list_profiles,
)
from contribai.templates.registry import Template, TemplateRegistry


# ── Config Tests ───────────────────────────────────────────────────────


class TestNewConfigs:
    def test_scheduler_config_defaults(self):
        cfg = SchedulerConfig()
        assert cfg.enabled is False
        assert cfg.cron == "0 */6 * * *"
        assert cfg.timezone == "UTC"
        assert cfg.max_concurrent == 3

    def test_web_config_defaults(self):
        cfg = WebConfig()
        assert cfg.host == "127.0.0.1"
        assert cfg.port == 8787
        assert cfg.enabled is True

    def test_pipeline_config_defaults(self):
        cfg = PipelineConfig()
        assert cfg.max_concurrent_repos == 3
        assert cfg.timeout_per_repo_sec == 300

    def test_full_config_has_new_sections(self):
        cfg = ContribAIConfig()
        assert hasattr(cfg, "scheduler")
        assert hasattr(cfg, "web")
        assert hasattr(cfg, "pipeline")
        assert isinstance(cfg.scheduler, SchedulerConfig)
        assert isinstance(cfg.web, WebConfig)
        assert isinstance(cfg.pipeline, PipelineConfig)


# ── Template Tests ─────────────────────────────────────────────────────


class TestTemplateRegistry:
    def test_load_builtins(self):
        registry = TemplateRegistry()
        templates = registry.list_all()
        assert len(templates) >= 5  # 5 built-in templates

    def test_get_template_by_name(self):
        registry = TemplateRegistry()
        tpl = registry.get("add-gitignore")
        assert tpl is not None
        assert tpl.name == "add-gitignore"
        assert tpl.type == "code_quality"

    def test_get_nonexistent_template(self):
        registry = TemplateRegistry()
        assert registry.get("nonexistent") is None

    def test_filter_by_type(self):
        registry = TemplateRegistry()
        security = registry.filter_by_type("security_fix")
        assert len(security) >= 1
        assert all(t.type == "security_fix" for t in security)

    def test_filter_by_language(self):
        registry = TemplateRegistry()
        python = registry.filter_by_language("python")
        # All language-agnostic + python-specific
        assert len(python) >= 1

    def test_template_dataclass(self):
        tpl = Template(
            name="test",
            description="Test template",
            type="code_quality",
            pattern="test pattern",
            fix_template="test fix",
        )
        assert tpl.name == "test"
        assert tpl.severity == "medium"  # default
        assert tpl.languages == []  # default

    def test_load_custom_directory(self, tmp_path):
        # Create a custom template
        tpl_file = tmp_path / "custom.yaml"
        tpl_file.write_text(
            "name: custom\n"
            "description: Custom template\n"
            "type: code_quality\n"
            "pattern: custom pattern\n"
            "fix_template: custom fix\n"
        )
        registry = TemplateRegistry()
        registry.load_directory(tmp_path)
        tpl = registry.get("custom")
        assert tpl is not None
        assert tpl.name == "custom"


# ── Profile Tests ──────────────────────────────────────────────────────


class TestProfiles:
    def test_builtin_profiles_exist(self):
        assert "security-focused" in BUILTIN_PROFILES
        assert "docs-focused" in BUILTIN_PROFILES
        assert "full-scan" in BUILTIN_PROFILES
        assert "gentle" in BUILTIN_PROFILES

    def test_get_builtin_profile(self):
        profile = get_profile("security-focused")
        assert profile is not None
        assert profile.name == "security-focused"
        assert "security" in profile.analyzers
        assert profile.severity_threshold == "high"

    def test_get_nonexistent_profile(self):
        assert get_profile("nonexistent") is None

    def test_list_profiles(self):
        profiles = list_profiles()
        assert len(profiles) >= 4
        names = [p.name for p in profiles]
        assert "security-focused" in names
        assert "gentle" in names

    def test_gentle_profile_is_dry_run(self):
        profile = get_profile("gentle")
        assert profile is not None
        assert profile.dry_run is True
        assert profile.max_prs_per_day == 3

    def test_apply_profile(self):
        profile = get_profile("security-focused")
        config_data = {}
        result = apply_profile(config_data, profile)
        assert result["analysis"]["enabled_analyzers"] == ["security"]
        assert result["analysis"]["severity_threshold"] == "high"
        assert result["github"]["max_prs_per_day"] == 5

    def test_profile_model(self):
        p = ContribProfile(
            name="test",
            description="Test",
            analyzers=["security"],
        )
        assert p.name == "test"
        assert p.dry_run is False  # default


# ── Scheduler Tests ────────────────────────────────────────────────────


class TestScheduler:
    def test_parse_cron_valid(self):
        from contribai.scheduler.scheduler import (
            ContribScheduler,
        )

        config = ContribAIConfig()
        sched = ContribScheduler(config)
        result = sched._parse_cron("0 */6 * * *")
        assert result["minute"] == "0"
        assert result["hour"] == "*/6"
        assert result["day"] == "*"

    def test_parse_cron_invalid(self):
        from contribai.scheduler.scheduler import (
            ContribScheduler,
        )

        config = ContribAIConfig()
        sched = ContribScheduler(config)
        with pytest.raises(ValueError, match="Invalid cron"):
            sched._parse_cron("invalid")

    def test_start_disabled(self, capsys):
        from contribai.scheduler.scheduler import (
            ContribScheduler,
        )

        config = ContribAIConfig()
        config.scheduler.enabled = False
        sched = ContribScheduler(config)
        sched.start()  # Should return immediately


# ── Dashboard Tests ────────────────────────────────────────────────────


class TestDashboard:
    def test_render_empty_dashboard(self):
        from contribai.web.dashboard import render_dashboard

        html = render_dashboard(
            stats={
                "total_repos_analyzed": 0,
                "total_prs_submitted": 0,
                "prs_merged": 0,
                "total_runs": 0,
            },
            repos=[],
            prs=[],
        )
        assert "ContribAI Dashboard" in html
        assert "No PRs yet" in html
        assert "No repos analyzed yet" in html

    def test_render_with_data(self):
        from contribai.web.dashboard import render_dashboard

        html = render_dashboard(
            stats={
                "total_repos_analyzed": 5,
                "total_prs_submitted": 10,
                "prs_merged": 3,
                "total_runs": 2,
            },
            repos=[
                {
                    "full_name": "owner/repo",
                    "language": "Python",
                    "stars": 100,
                    "findings": 5,
                    "analyzed_at": "2026-01-01T00:00:00",
                }
            ],
            prs=[
                {
                    "repo": "owner/repo",
                    "pr_url": "https://github.com/owner/repo/pull/1",
                    "pr_number": 1,
                    "title": "Fix security issue",
                    "status": "open",
                    "type": "security_fix",
                }
            ],
        )
        assert "owner/repo" in html
        assert "Fix security issue" in html
        assert "5" in html  # total_repos_analyzed

    def test_html_escaping(self):
        from contribai.web.dashboard import _esc

        assert _esc("<script>") == "&lt;script&gt;"
        assert _esc('a"b') == "a&quot;b"
        assert _esc("a&b") == "a&amp;b"


# ── Web Server Tests ──────────────────────────────────────────────────


class TestWebServer:
    def test_app_exists(self):
        from contribai.web.server import app

        assert app.title == "ContribAI Dashboard"

    def test_health_endpoint(self):
        from fastapi.testclient import TestClient

        from contribai.web.server import app

        client = TestClient(app)
        resp = client.get("/api/health")
        assert resp.status_code == 200
        data = resp.json()
        assert data["status"] == "ok"
        assert data["version"] == "0.6.0"


# ── Parallel Pipeline Tests ──────────────────────────────────────────


class TestParallelPipeline:
    def test_pipeline_has_config(self):
        from contribai.orchestrator.pipeline import (
            ContribPipeline,
        )

        config = ContribAIConfig()
        pipeline = ContribPipeline(config)
        assert pipeline.config.pipeline.max_concurrent_repos == 3

    def test_pipeline_result_aggregation(self):
        from contribai.orchestrator.pipeline import (
            PipelineResult,
        )

        r1 = PipelineResult(repos_analyzed=1, findings_total=5)
        r2 = PipelineResult(repos_analyzed=1, findings_total=3)
        total = PipelineResult()
        for r in [r1, r2]:
            total.repos_analyzed += r.repos_analyzed
            total.findings_total += r.findings_total
        assert total.repos_analyzed == 2
        assert total.findings_total == 8
