"""GitHub webhook receiver for auto-triggering pipeline.

Supports events: issues.opened, issues.labeled, push.
Verifies HMAC-SHA256 signatures for security.
"""

from __future__ import annotations

import logging

from fastapi import APIRouter, Request

from contribai.web.auth import verify_webhook_signature

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/webhooks", tags=["webhooks"])

_webhook_secret: str = ""
_on_event_callback = None


def configure_webhooks(secret: str, on_event=None):
    """Configure webhook secret and event handler."""
    global _webhook_secret, _on_event_callback
    _webhook_secret = secret
    _on_event_callback = on_event


@router.post("/github")
async def github_webhook(request: Request):
    """Receive GitHub webhook events."""
    # Verify signature
    if _webhook_secret:
        signature = request.headers.get("X-Hub-Signature-256", "")
        body = await request.body()
        if not verify_webhook_signature(body, signature, _webhook_secret):
            logger.warning("Invalid webhook signature")
            return {"error": "Invalid signature"}, 403

    # Parse event
    event_type = request.headers.get("X-GitHub-Event", "")
    payload = await request.json()

    action = payload.get("action", "")
    repo_name = payload.get("repository", {}).get("full_name", "")

    logger.info(
        "Webhook: %s.%s from %s",
        event_type,
        action,
        repo_name,
    )

    # Handle events
    trigger = False
    repo_url = ""

    if event_type == "issues" and action in (
        "opened",
        "labeled",
    ):
        repo_url = payload.get("repository", {}).get("html_url", "")
        trigger = True
        logger.info(
            "Issue event: #%s %s",
            payload.get("issue", {}).get("number"),
            payload.get("issue", {}).get("title"),
        )

    elif event_type == "push":
        repo_url = payload.get("repository", {}).get("html_url", "")
        ref = payload.get("ref", "")
        if ref.endswith("/main") or ref.endswith("/master"):
            trigger = True
            logger.info("Push to default branch: %s", ref)

    if trigger and _on_event_callback and repo_url:
        try:
            await _on_event_callback(event_type, action, repo_url)
        except Exception:
            logger.exception("Webhook event handler failed")

    return {
        "status": "received",
        "event": event_type,
        "action": action,
        "triggered": trigger,
    }
