"""GitHub webhook receiver for auto-triggering pipeline.

Supports events: issues.opened, issues.labeled, push.
Verifies HMAC-SHA256 signatures for security.
"""

from __future__ import annotations

import json
import logging

from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse

from contribai.web.auth import verify_webhook_signature

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/webhooks", tags=["webhooks"])

_webhook_secret: str = ""
_on_event_callback = None

# Max webhook payload: 10 MB (GitHub's own limit is 25 MB)
MAX_PAYLOAD_SIZE = 10 * 1024 * 1024


def configure_webhooks(secret: str, on_event=None):
    """Configure webhook secret and event handler."""
    global _webhook_secret, _on_event_callback
    _webhook_secret = secret
    _on_event_callback = on_event


@router.post("/github")
async def github_webhook(request: Request):
    """Receive GitHub webhook events."""
    # Reject oversized payloads
    content_length = request.headers.get("content-length")
    if content_length and int(content_length) > MAX_PAYLOAD_SIZE:
        return JSONResponse({"error": "Payload too large"}, status_code=413)

    # Read body once — used for signature check and payload size fallback
    body = await request.body()

    # Bug 4 fix: if content-length header is missing, check actual body size
    if not content_length and len(body) > MAX_PAYLOAD_SIZE:
        return JSONResponse({"error": "Payload too large"}, status_code=413)

    # Verify signature
    if not _webhook_secret:
        logger.error("Webhook secret not configured")
        return JSONResponse({"error": "Webhook secret not configured"}, status_code=500)

    signature = request.headers.get("X-Hub-Signature-256", "")
    if not verify_webhook_signature(body, signature, _webhook_secret):
        logger.warning("Invalid webhook signature")
        # Bug 1 fix: use JSONResponse so FastAPI returns HTTP 403, not 200
        return JSONResponse({"error": "Invalid signature"}, status_code=403)

    # Parse event
    event_type = request.headers.get("X-GitHub-Event", "")
    payload = json.loads(body)

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
