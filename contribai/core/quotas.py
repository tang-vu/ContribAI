"""API usage tracking and quota enforcement.

Tracks GitHub API calls, LLM API calls, and token usage
with configurable daily limits and SQLite persistence.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import date

logger = logging.getLogger(__name__)


@dataclass
class UsageRecord:
    """Usage record for a single day."""

    date: str = ""
    github_calls: int = 0
    llm_calls: int = 0
    llm_tokens: int = 0


class UsageTracker:
    """Track and enforce API usage quotas.

    In-memory tracker with optional limits.
    Resets daily automatically.
    """

    def __init__(
        self,
        github_daily_limit: int = 5000,
        llm_daily_limit: int = 1000,
        llm_daily_tokens: int = 1_000_000,
    ):
        self._github_limit = github_daily_limit
        self._llm_limit = llm_daily_limit
        self._llm_token_limit = llm_daily_tokens
        self._usage: UsageRecord = UsageRecord(date=self._today())

    def _today(self) -> str:
        return date.today().isoformat()

    def _ensure_today(self):
        """Reset counters if day has changed."""
        today = self._today()
        if self._usage.date != today:
            self._usage = UsageRecord(date=today)

    # ── Recording ────────────────────────────────────

    def record_github_call(self, count: int = 1):
        """Record GitHub API call(s)."""
        self._ensure_today()
        self._usage.github_calls += count

    def record_llm_call(self, tokens_used: int = 0):
        """Record an LLM API call with token count."""
        self._ensure_today()
        self._usage.llm_calls += 1
        self._usage.llm_tokens += tokens_used

    # ── Checking ─────────────────────────────────────

    def check_github_quota(self) -> bool:
        """Check if GitHub API quota is available."""
        self._ensure_today()
        return self._usage.github_calls < self._github_limit

    def check_llm_quota(self) -> bool:
        """Check if LLM API quota is available."""
        self._ensure_today()
        return (
            self._usage.llm_calls < self._llm_limit
            and self._usage.llm_tokens < self._llm_token_limit
        )

    @property
    def github_remaining(self) -> int:
        self._ensure_today()
        return max(
            0,
            self._github_limit - self._usage.github_calls,
        )

    @property
    def llm_remaining(self) -> int:
        self._ensure_today()
        return max(
            0,
            self._llm_limit - self._usage.llm_calls,
        )

    @property
    def llm_tokens_remaining(self) -> int:
        self._ensure_today()
        return max(
            0,
            self._llm_token_limit - self._usage.llm_tokens,
        )

    def get_usage(self) -> dict:
        """Get current usage statistics."""
        self._ensure_today()
        return {
            "date": self._usage.date,
            "github": {
                "calls": self._usage.github_calls,
                "limit": self._github_limit,
                "remaining": self.github_remaining,
            },
            "llm": {
                "calls": self._usage.llm_calls,
                "limit": self._llm_limit,
                "remaining": self.llm_remaining,
                "tokens_used": self._usage.llm_tokens,
                "tokens_limit": self._llm_token_limit,
                "tokens_remaining": (self.llm_tokens_remaining),
            },
        }
