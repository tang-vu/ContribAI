"""Tests for Phase 5: plugins, quotas, auth, webhooks."""

from __future__ import annotations

import hashlib
import hmac

import pytest

from contribai.core.config import (
    ContribAIConfig,
    QuotaConfig,
    WebConfig,
)
from contribai.core.quotas import UsageTracker
from contribai.plugins.base import (
    AnalyzerPlugin,
    GeneratorPlugin,
    PluginRegistry,
)
from contribai.web.auth import (
    configure_auth,
    generate_api_key,
    verify_webhook_signature,
)


# ── Config Tests ───────────────────────────────────


class TestPhase5Config:
    def test_quota_config_defaults(self):
        cfg = QuotaConfig()
        assert cfg.github_daily_limit == 5000
        assert cfg.llm_daily_limit == 1000
        assert cfg.llm_daily_tokens == 1_000_000

    def test_web_config_has_auth(self):
        cfg = WebConfig()
        assert cfg.api_keys == []
        assert cfg.webhook_secret == ""

    def test_full_config_has_quota(self):
        cfg = ContribAIConfig()
        assert hasattr(cfg, "quota")
        assert isinstance(cfg.quota, QuotaConfig)


# ── Plugin Tests ───────────────────────────────────


class TestPluginSystem:
    def test_registry_creation(self):
        registry = PluginRegistry()
        assert registry.analyzers == []
        assert registry.generators == []

    def test_register_analyzer(self):
        class TestAnalyzer(AnalyzerPlugin):
            @property
            def name(self):
                return "test"

            async def analyze(self, context):
                return []

        registry = PluginRegistry()
        plugin = TestAnalyzer()
        registry.register_analyzer(plugin)
        assert len(registry.analyzers) == 1
        assert registry.analyzers[0].name == "test"

    def test_register_generator(self):
        class TestGenerator(GeneratorPlugin):
            @property
            def name(self):
                return "test-gen"

            async def generate(self, finding, context):
                return None

        registry = PluginRegistry()
        plugin = TestGenerator()
        registry.register_generator(plugin)
        assert len(registry.generators) == 1
        assert registry.generators[0].name == "test-gen"

    def test_plugin_version_default(self):
        class TestPlugin(AnalyzerPlugin):
            @property
            def name(self):
                return "test"

            async def analyze(self, context):
                return []

        p = TestPlugin()
        assert p.version == "0.1.0"

    def test_discover_no_crash(self):
        """Discover should work even with no plugins."""
        registry = PluginRegistry()
        registry.discover()
        assert registry._loaded is True


# ── Quota Tests ────────────────────────────────────


class TestUsageTracker:
    def test_initial_state(self):
        tracker = UsageTracker()
        assert tracker.github_remaining == 5000
        assert tracker.llm_remaining == 1000

    def test_record_github_calls(self):
        tracker = UsageTracker(github_daily_limit=100)
        tracker.record_github_call(10)
        assert tracker.github_remaining == 90
        assert tracker.check_github_quota() is True

    def test_github_quota_exhausted(self):
        tracker = UsageTracker(github_daily_limit=5)
        for _ in range(5):
            tracker.record_github_call()
        assert tracker.check_github_quota() is False
        assert tracker.github_remaining == 0

    def test_record_llm_calls(self):
        tracker = UsageTracker(llm_daily_limit=10)
        tracker.record_llm_call(tokens_used=500)
        assert tracker.llm_remaining == 9
        assert tracker.llm_tokens_remaining == 999_500

    def test_llm_quota_exhausted(self):
        tracker = UsageTracker(llm_daily_limit=2)
        tracker.record_llm_call()
        tracker.record_llm_call()
        assert tracker.check_llm_quota() is False

    def test_get_usage(self):
        tracker = UsageTracker(
            github_daily_limit=100,
            llm_daily_limit=50,
        )
        tracker.record_github_call(5)
        tracker.record_llm_call(tokens_used=1000)

        usage = tracker.get_usage()
        assert usage["github"]["calls"] == 5
        assert usage["github"]["remaining"] == 95
        assert usage["llm"]["calls"] == 1
        assert usage["llm"]["tokens_used"] == 1000

    def test_custom_limits(self):
        tracker = UsageTracker(
            github_daily_limit=10,
            llm_daily_limit=5,
            llm_daily_tokens=10000,
        )
        assert tracker.github_remaining == 10
        assert tracker.llm_remaining == 5
        assert tracker.llm_tokens_remaining == 10000


# ── Auth Tests ─────────────────────────────────────


class TestAuth:
    def test_generate_api_key(self):
        key = generate_api_key()
        assert key.startswith("cai_")
        assert len(key) > 10

    def test_generate_unique_keys(self):
        keys = {generate_api_key() for _ in range(10)}
        assert len(keys) == 10  # all unique

    def test_webhook_signature_valid(self):
        secret = "test-secret"
        payload = b'{"action": "opened"}'
        sig = "sha256=" + hmac.new(secret.encode(), payload, hashlib.sha256).hexdigest()
        assert verify_webhook_signature(payload, sig, secret) is True

    def test_webhook_signature_invalid(self):
        assert verify_webhook_signature(b"payload", "sha256=invalid", "secret") is False

    def test_webhook_signature_bad_prefix(self):
        assert verify_webhook_signature(b"payload", "md5=abc", "secret") is False

    def test_configure_auth(self):
        configure_auth(["key1", "key2"])
        # No exception
        configure_auth([])
        # No exception


# ── Webhook Tests ──────────────────────────────────


class TestWebhooks:
    def test_webhook_router_exists(self):
        from contribai.web.webhooks import router

        assert router.prefix == "/api/webhooks"

    def test_server_app_includes_webhook(self):
        from contribai.web.server import app

        routes = [r.path for r in app.routes]
        assert "/api/webhooks/github" in routes

    def test_health_endpoint_version(self):
        from fastapi.testclient import TestClient

        from contribai.web.server import app

        client = TestClient(app)
        resp = client.get("/api/health")
        assert resp.status_code == 200
        assert resp.json()["version"] == "0.6.0"
