"""FastAPI REST API server for ContribAI dashboard.

Provides endpoints for monitoring stats, runs, PRs,
webhook receiver, and triggering pipeline executions.
"""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager

from fastapi import BackgroundTasks, Depends, FastAPI
from fastapi.responses import HTMLResponse

from contribai.core.config import ContribAIConfig, load_config
from contribai.orchestrator.memory import Memory
from contribai.orchestrator.pipeline import ContribPipeline
from contribai.web.auth import configure_auth, verify_api_key
from contribai.web.dashboard import render_dashboard
from contribai.web.webhooks import configure_webhooks
from contribai.web.webhooks import router as webhook_router

logger = logging.getLogger(__name__)

_config: ContribAIConfig | None = None
_memory: Memory | None = None


async def _webhook_event_handler(
    event_type: str,
    action: str,
    repo_url: str,
):
    """Handle webhook events by running pipeline."""
    config = load_config()
    pipeline = ContribPipeline(config)
    try:
        result = await pipeline.run_single(repo_url, dry_run=False)
        logger.info(
            "Webhook-triggered run: %d PRs for %s",
            result.prs_created,
            repo_url,
        )
    except Exception:
        logger.exception(
            "Webhook pipeline run failed for %s",
            repo_url,
        )


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Initialize and cleanup shared resources."""
    global _config, _memory
    _config = load_config()
    _memory = Memory(_config.storage.resolved_db_path)
    await _memory.init()

    # Configure auth
    configure_auth(_config.web.api_keys)

    # Configure webhooks
    configure_webhooks(
        secret=_config.web.webhook_secret,
        on_event=_webhook_event_handler,
    )

    logger.info("Dashboard API started")
    yield
    if _memory:
        await _memory.close()


app = FastAPI(
    title="ContribAI Dashboard",
    version="0.5.0",
    lifespan=lifespan,
)

# Mount webhook router
app.include_router(webhook_router)


# ── Public endpoints ─────────────────────────────────


@app.get("/", response_class=HTMLResponse)
async def dashboard():
    """Serve the HTML dashboard."""
    stats = await _memory.get_stats()
    repos = await _memory.get_analyzed_repos(limit=20)
    prs = await _memory.get_prs(limit=20)
    return render_dashboard(stats, repos, prs)


@app.get("/api/health")
async def health():
    """Health check endpoint."""
    return {"status": "ok", "version": "0.5.0"}


@app.get("/api/stats")
async def get_stats():
    """Get overall statistics."""
    return await _memory.get_stats()


@app.get("/api/repos")
async def get_repos(limit: int = 50):
    """Get analyzed repositories."""
    return await _memory.get_analyzed_repos(limit=limit)


@app.get("/api/prs")
async def get_prs(status: str | None = None, limit: int = 50):
    """Get submitted PRs, optionally filtered."""
    return await _memory.get_prs(status=status, limit=limit)


@app.get("/api/runs")
async def get_runs(limit: int = 20):
    """Get run history."""
    return await _memory.get_run_history(limit=limit)


# ── Protected endpoints (require API key) ────────────


async def _background_run(repo_url: str | None, dry_run: bool):
    """Execute pipeline in background."""
    config = load_config()
    pipeline = ContribPipeline(config)
    try:
        if repo_url:
            result = await pipeline.run_single(repo_url, dry_run=dry_run)
        else:
            result = await pipeline.run(dry_run=dry_run)
        logger.info(
            "Background run: %d repos, %d PRs",
            result.repos_analyzed,
            result.prs_created,
        )
    except Exception:
        logger.exception("Background run failed")


@app.post("/api/run")
async def trigger_run(
    background_tasks: BackgroundTasks,
    dry_run: bool = False,
    _key: str | None = Depends(verify_api_key),
):
    """Trigger a pipeline run (auth required)."""
    background_tasks.add_task(_background_run, None, dry_run)
    return {"status": "started", "dry_run": dry_run}


@app.post("/api/run/target")
async def trigger_target(
    background_tasks: BackgroundTasks,
    repo_url: str = "",
    dry_run: bool = False,
    _key: str | None = Depends(verify_api_key),
):
    """Target a specific repo (auth required)."""
    if not repo_url:
        return {"error": "repo_url is required"}, 400
    background_tasks.add_task(_background_run, repo_url, dry_run)
    return {
        "status": "started",
        "repo_url": repo_url,
        "dry_run": dry_run,
    }


def run_server(
    config: ContribAIConfig | None = None,
):
    """Start the uvicorn server."""
    import uvicorn

    cfg = config or load_config()
    uvicorn.run(
        "contribai.web.server:app",
        host=cfg.web.host,
        port=cfg.web.port,
        log_level="info",
    )
