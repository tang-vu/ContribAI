"""Contribution leaderboard and success rate tracking.

Tracks PR merge/close rates by repository, contribution type,
and time period. Provides rankings and statistics.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass

logger = logging.getLogger(__name__)


@dataclass
class LeaderboardEntry:
    """A single leaderboard entry."""

    repo: str = ""
    total_prs: int = 0
    merged: int = 0
    closed: int = 0
    open: int = 0

    @property
    def merge_rate(self) -> float:
        decided = self.merged + self.closed
        if decided == 0:
            return 0.0
        return self.merged / decided * 100

    @property
    def status(self) -> str:
        if self.merge_rate >= 70:
            return "excellent"
        if self.merge_rate >= 40:
            return "good"
        if self.merge_rate > 0:
            return "needs_improvement"
        return "pending"


@dataclass
class TypeStats:
    """Stats per contribution type."""

    type: str = ""
    total: int = 0
    merged: int = 0
    closed: int = 0

    @property
    def merge_rate(self) -> float:
        decided = self.merged + self.closed
        return (self.merged / decided * 100) if decided else 0.0


class Leaderboard:
    """Contribution leaderboard with success tracking.

    Reads from the existing submitted_prs table.
    """

    def __init__(self, db):
        """Initialize with aiosqlite connection."""
        self._db = db

    async def get_overall_stats(self) -> dict:
        """Get overall contribution statistics."""
        cursor = await self._db.execute(
            "SELECT status, COUNT(*) FROM submitted_prs GROUP BY status"
        )
        rows = await cursor.fetchall()

        stats = {"total": 0, "merged": 0, "closed": 0, "open": 0}
        for status, count in rows:
            stats["total"] += count
            if status in stats:
                stats[status] = count

        decided = stats["merged"] + stats["closed"]
        stats["merge_rate"] = round(stats["merged"] / decided * 100, 1) if decided else 0.0
        return stats

    async def get_repo_rankings(self, limit: int = 20) -> list[LeaderboardEntry]:
        """Get repo rankings by merge rate."""
        cursor = await self._db.execute(
            """
            SELECT
                repo,
                COUNT(*) as total,
                SUM(CASE WHEN status='merged' THEN 1 ELSE 0 END) as merged,
                SUM(CASE WHEN status='closed' THEN 1 ELSE 0 END) as closed,
                SUM(CASE WHEN status='open' THEN 1 ELSE 0 END) as open_count
            FROM submitted_prs
            GROUP BY repo
            ORDER BY merged DESC, total DESC
            LIMIT ?
            """,
            (limit,),
        )
        rows = await cursor.fetchall()
        return [
            LeaderboardEntry(
                repo=r[0],
                total_prs=r[1],
                merged=r[2],
                closed=r[3],
                open=r[4],
            )
            for r in rows
        ]

    async def get_type_stats(self) -> list[TypeStats]:
        """Get success stats by contribution type."""
        cursor = await self._db.execute(
            """
            SELECT
                type,
                COUNT(*) as total,
                SUM(CASE WHEN status='merged' THEN 1 ELSE 0 END) as merged,
                SUM(CASE WHEN status='closed' THEN 1 ELSE 0 END) as closed
            FROM submitted_prs
            GROUP BY type
            ORDER BY merged DESC
            """
        )
        rows = await cursor.fetchall()
        return [TypeStats(type=r[0], total=r[1], merged=r[2], closed=r[3]) for r in rows]

    async def get_recent_merges(self, limit: int = 10) -> list[dict]:
        """Get recently merged PRs."""
        cursor = await self._db.execute(
            """
            SELECT repo, pr_number, pr_url, title, type, updated_at
            FROM submitted_prs
            WHERE status = 'merged'
            ORDER BY updated_at DESC
            LIMIT ?
            """,
            (limit,),
        )
        rows = await cursor.fetchall()
        return [
            {
                "repo": r[0],
                "pr_number": r[1],
                "pr_url": r[2],
                "title": r[3],
                "type": r[4],
                "merged_at": r[5],
            }
            for r in rows
        ]
