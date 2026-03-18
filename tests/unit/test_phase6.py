"""Tests for Phase 6: leaderboard, language rules, notifications, TUI."""

from __future__ import annotations

import hashlib
import hmac

import pytest

from contribai.analysis.language_rules import (
    GO_RULES,
    JS_TS_RULES,
    RUST_RULES,
    get_analysis_prompt,
    get_rules_for_language,
    get_supported_languages,
)
from contribai.core.config import (
    ContribAIConfig,
    NotificationConfig,
)
from contribai.core.leaderboard import (
    Leaderboard,
    LeaderboardEntry,
    TypeStats,
)
from contribai.notifications.notifier import (
    NotificationEvent,
    Notifier,
    _get_color,
    _get_emoji,
)


# ── Config Tests ───────────────────────────────────


class TestPhase6Config:
    def test_notification_config_defaults(self):
        cfg = NotificationConfig()
        assert cfg.slack_webhook == ""
        assert cfg.discord_webhook == ""
        assert cfg.telegram_token == ""
        assert cfg.on_merge is True
        assert cfg.on_close is True

    def test_full_config_has_notifications(self):
        cfg = ContribAIConfig()
        assert hasattr(cfg, "notifications")
        assert isinstance(cfg.notifications, NotificationConfig)


# ── Language Rules Tests ───────────────────────────


class TestLanguageRules:
    def test_js_rules_exist(self):
        assert len(JS_TS_RULES) >= 7

    def test_go_rules_exist(self):
        assert len(GO_RULES) >= 6

    def test_rust_rules_exist(self):
        assert len(RUST_RULES) >= 6

    def test_get_rules_javascript(self):
        rules = get_rules_for_language("javascript")
        assert len(rules) >= 6
        names = [r.name for r in rules]
        assert "eval-usage" in names

    def test_get_rules_typescript_inherits_js(self):
        rules = get_rules_for_language("typescript")
        # Should include TS-specific + JS rules
        names = [r.name for r in rules]
        assert "no-any-type" in names
        assert "eval-usage" in names

    def test_get_rules_go(self):
        rules = get_rules_for_language("go")
        names = [r.name for r in rules]
        assert "sql-injection" in names
        assert "unchecked-error" in names

    def test_get_rules_rust(self):
        rules = get_rules_for_language("rust")
        names = [r.name for r in rules]
        assert "unsafe-block" in names
        assert "unwrap-panic" in names

    def test_get_rules_unknown_language(self):
        rules = get_rules_for_language("cobol")
        assert rules == []

    def test_supported_languages(self):
        langs = get_supported_languages()
        assert "go" in langs
        assert "rust" in langs
        assert "javascript" in langs

    def test_analysis_prompt_generation(self):
        prompt = get_analysis_prompt(
            "javascript",
            "const x = eval('code')",
            "src/app.js",
        )
        assert "javascript" in prompt
        assert "eval-usage" in prompt
        assert "src/app.js" in prompt

    def test_analysis_prompt_empty_for_unknown(self):
        prompt = get_analysis_prompt("cobol", "code", "f.cob")
        assert prompt == ""

    def test_rule_severities(self):
        critical = [r for r in JS_TS_RULES if r.severity == "critical"]
        assert len(critical) >= 2
        for r in critical:
            assert r.fix_hint  # All critical rules should have fix hints


# ── Leaderboard Tests ──────────────────────────────


class TestLeaderboard:
    def test_leaderboard_entry_merge_rate(self):
        entry = LeaderboardEntry(
            repo="owner/repo",
            total_prs=10,
            merged=7,
            closed=3,
            open=0,
        )
        assert entry.merge_rate == 70.0
        assert entry.status == "excellent"

    def test_leaderboard_entry_zero(self):
        entry = LeaderboardEntry()
        assert entry.merge_rate == 0.0
        assert entry.status == "pending"

    def test_leaderboard_entry_good(self):
        entry = LeaderboardEntry(merged=4, closed=6)
        assert entry.merge_rate == 40.0
        assert entry.status == "good"

    def test_leaderboard_entry_needs_improvement(self):
        entry = LeaderboardEntry(merged=1, closed=9)
        assert entry.merge_rate == 10.0
        assert entry.status == "needs_improvement"

    def test_type_stats_merge_rate(self):
        stats = TypeStats(
            type="security_fix",
            total=5,
            merged=3,
            closed=2,
        )
        assert stats.merge_rate == 60.0

    def test_type_stats_zero(self):
        stats = TypeStats(type="docs", total=0, merged=0, closed=0)
        assert stats.merge_rate == 0.0


# ── Notification Tests ─────────────────────────────


class TestNotifications:
    def test_notifier_not_configured(self):
        n = Notifier()
        assert n.is_configured is False

    def test_notifier_slack_configured(self):
        n = Notifier(slack_webhook="https://hooks.slack.com/test")
        assert n.is_configured is True

    def test_notifier_discord_configured(self):
        n = Notifier(discord_webhook="https://discord.com/api/webhooks/test")
        assert n.is_configured is True

    def test_notifier_telegram_configured(self):
        n = Notifier(
            telegram_token="123:ABC",
            telegram_chat_id="-100123",
        )
        assert n.is_configured is True

    def test_notification_event(self):
        event = NotificationEvent(
            event_type="pr_merged",
            title="PR Merged",
            message="Fix security issue",
            url="https://github.com/owner/repo/pull/1",
        )
        assert event.event_type == "pr_merged"
        assert event.extra == {}

    def test_emoji_mapping(self):
        assert _get_emoji("pr_merged") == "🎉"
        assert _get_emoji("pr_closed") == "❌"
        assert _get_emoji("run_complete") == "✅"
        assert _get_emoji("unknown") == "📢"

    def test_color_mapping(self):
        assert _get_color("pr_merged") == 0x22C55E
        assert _get_color("pr_closed") == 0xEF4444
        assert _get_color("unknown") == 0x94A3B8


# ── TUI Tests ──────────────────────────────────────


class TestTUI:
    def test_interactive_mode_creation(self):
        from contribai.cli.tui import InteractiveMode

        config = ContribAIConfig()
        tui = InteractiveMode(config)
        assert tui._selected_repos == []

    def test_run_interactive_import(self):
        from contribai.cli.tui import run_interactive

        assert callable(run_interactive)


# ── Web Server Version ─────────────────────────────


class TestWebVersion:
    def test_health_version(self):
        from contribai.web.server import app

        assert app.version == "0.6.0"
