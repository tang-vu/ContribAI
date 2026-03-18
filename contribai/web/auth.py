"""API key authentication for the web dashboard.

Simple API key verification via X-API-Key header
or api_key query parameter.
"""

from __future__ import annotations

import hashlib
import hmac
import logging
import secrets

from fastapi import HTTPException, Security
from fastapi.security import APIKeyHeader, APIKeyQuery

logger = logging.getLogger(__name__)

_api_key_header = APIKeyHeader(name="X-API-Key", auto_error=False)
_api_key_query = APIKeyQuery(name="api_key", auto_error=False)

# Will be set during app lifespan
_valid_keys: list[str] = []
_auth_enabled: bool = False


def configure_auth(api_keys: list[str]):
    """Configure valid API keys at startup."""
    global _valid_keys, _auth_enabled
    _valid_keys = api_keys
    _auth_enabled = len(api_keys) > 0
    if _auth_enabled:
        logger.info(
            "API key auth enabled with %d key(s)",
            len(api_keys),
        )
    else:
        logger.info("API key auth disabled (no keys configured)")


async def verify_api_key(
    header_key: str | None = Security(_api_key_header),
    query_key: str | None = Security(_api_key_query),
) -> str | None:
    """Verify API key from header or query param.

    If no keys are configured, auth is disabled and
    all requests pass through.
    """
    if not _auth_enabled:
        return None

    key = header_key or query_key
    if not key:
        raise HTTPException(
            status_code=401,
            detail="API key required. Use X-API-Key header or api_key param.",
        )

    if key not in _valid_keys:
        raise HTTPException(
            status_code=403,
            detail="Invalid API key.",
        )

    return key


def verify_webhook_signature(
    payload: bytes,
    signature: str,
    secret: str,
) -> bool:
    """Verify GitHub webhook HMAC-SHA256 signature."""
    if not signature.startswith("sha256="):
        return False
    expected = hmac.new(secret.encode(), payload, hashlib.sha256).hexdigest()
    return hmac.compare_digest(signature[7:], expected)


def generate_api_key() -> str:
    """Generate a new random API key."""
    return f"cai_{secrets.token_urlsafe(32)}"
