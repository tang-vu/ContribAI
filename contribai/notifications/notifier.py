"""Notification system for Slack, Discord, and Telegram.

Sends webhooks when PRs are merged/closed or pipeline runs complete.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass

import httpx

logger = logging.getLogger(__name__)


@dataclass
class NotificationEvent:
    """An event to notify about."""

    event_type: str  # pr_merged | pr_closed | run_complete | error
    title: str
    message: str
    url: str = ""
    repo: str = ""
    extra: dict = None

    def __post_init__(self):
        if self.extra is None:
            self.extra = {}


class Notifier:
    """Multi-channel notification dispatcher."""

    def __init__(
        self,
        slack_webhook: str = "",
        discord_webhook: str = "",
        telegram_token: str = "",
        telegram_chat_id: str = "",
    ):
        self._slack = slack_webhook
        self._discord = discord_webhook
        self._telegram_token = telegram_token
        self._telegram_chat = telegram_chat_id
        self._client = httpx.AsyncClient(timeout=10.0)

    async def close(self):
        await self._client.aclose()

    @property
    def is_configured(self) -> bool:
        """Check if any notification channel is configured."""
        return bool(self._slack or self._discord or (self._telegram_token and self._telegram_chat))

    async def notify(self, event: NotificationEvent):
        """Send notification to all configured channels."""
        if not self.is_configured:
            return

        if self._slack:
            await self._send_slack(event)
        if self._discord:
            await self._send_discord(event)
        if self._telegram_token and self._telegram_chat:
            await self._send_telegram(event)

    # ── Slack ─────────────────────────────────────────

    async def _send_slack(self, event: NotificationEvent):
        """Send to Slack via incoming webhook."""
        emoji = _get_emoji(event.event_type)
        payload = {
            "text": f"{emoji} *{event.title}*\n{event.message}",
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": (f"{emoji} *{event.title}*\n{event.message}"),
                    },
                }
            ],
        }
        if event.url:
            payload["blocks"].append(
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "text": {
                                "type": "plain_text",
                                "text": "View on GitHub",
                            },
                            "url": event.url,
                        }
                    ],
                }
            )

        try:
            resp = await self._client.post(self._slack, json=payload)
            if resp.status_code != 200:
                logger.warning(
                    "Slack webhook failed: %s",
                    resp.text,
                )
        except Exception:
            logger.warning(
                "Slack notification failed",
                exc_info=True,
            )

    # ── Discord ───────────────────────────────────────

    async def _send_discord(self, event: NotificationEvent):
        """Send to Discord via webhook."""
        emoji = _get_emoji(event.event_type)
        color = _get_color(event.event_type)

        payload = {
            "embeds": [
                {
                    "title": f"{emoji} {event.title}",
                    "description": event.message,
                    "color": color,
                    "url": event.url or None,
                    "footer": {"text": "ContribAI"},
                }
            ]
        }

        try:
            resp = await self._client.post(self._discord, json=payload)
            if resp.status_code not in (200, 204):
                logger.warning(
                    "Discord webhook failed: %s",
                    resp.text,
                )
        except Exception:
            logger.warning(
                "Discord notification failed",
                exc_info=True,
            )

    # ── Telegram ──────────────────────────────────────

    async def _send_telegram(self, event: NotificationEvent):
        """Send to Telegram via Bot API."""
        emoji = _get_emoji(event.event_type)
        text = f"{emoji} <b>{event.title}</b>\n{event.message}"
        if event.url:
            text += f'\n<a href="{event.url}">View on GitHub</a>'

        url = f"https://api.telegram.org/bot{self._telegram_token}/sendMessage"
        payload = {
            "chat_id": self._telegram_chat,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": True,
        }

        try:
            resp = await self._client.post(url, json=payload)
            data = resp.json()
            if not data.get("ok"):
                logger.warning(
                    "Telegram failed: %s",
                    data.get("description"),
                )
        except Exception:
            logger.warning(
                "Telegram notification failed",
                exc_info=True,
            )

    # ── Convenience methods ──────────────────────────

    async def notify_pr_merged(
        self,
        repo: str,
        pr_number: int,
        title: str,
        pr_url: str,
    ):
        """Notify that a PR was merged."""
        await self.notify(
            NotificationEvent(
                event_type="pr_merged",
                title=f"PR Merged: {repo}#{pr_number}",
                message=title,
                url=pr_url,
                repo=repo,
            )
        )

    async def notify_pr_closed(
        self,
        repo: str,
        pr_number: int,
        title: str,
        pr_url: str,
    ):
        """Notify that a PR was closed without merge."""
        await self.notify(
            NotificationEvent(
                event_type="pr_closed",
                title=f"PR Closed: {repo}#{pr_number}",
                message=title,
                url=pr_url,
                repo=repo,
            )
        )

    async def notify_run_complete(
        self,
        repos_analyzed: int,
        prs_created: int,
        errors: int = 0,
    ):
        """Notify that a pipeline run completed."""
        await self.notify(
            NotificationEvent(
                event_type="run_complete",
                title="Pipeline Run Complete",
                message=(f"Repos: {repos_analyzed} | PRs: {prs_created} | Errors: {errors}"),
            )
        )


def _get_emoji(event_type: str) -> str:
    return {
        "pr_merged": "🎉",
        "pr_closed": "❌",
        "run_complete": "✅",
        "error": "🚨",
    }.get(event_type, "📢")


def _get_color(event_type: str) -> int:
    return {
        "pr_merged": 0x22C55E,  # green
        "pr_closed": 0xEF4444,  # red
        "run_complete": 0x38BDF8,  # blue
        "error": 0xF59E0B,  # amber
    }.get(event_type, 0x94A3B8)
