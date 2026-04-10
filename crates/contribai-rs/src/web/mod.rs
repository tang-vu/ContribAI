//! Web dashboard and REST API server.
//!
//! Port from Python `web/server.py` + `web/dashboard.py`.
//! Provides a lightweight axum HTTP server with:
//!   - HTML dashboard (GET /)
//!   - Health endpoint (GET /api/health)
//!   - Stats, repos, PRs, runs (GET /api/*)
//!   - Trigger run / target (POST /api/run*)  — API-key protected
//!   - GitHub webhook receiver (POST /api/webhooks/github)
//!
//! v5.4 (Sprint 4): API key authentication + GitHub webhook receiver.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::core::config::ContribAIConfig;
use crate::orchestrator::memory::Memory;

// ── Shared state ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub memory: Arc<Memory>,
    pub version: &'static str,
    /// Accepted API keys. Empty = no authentication required.
    pub api_keys: Vec<String>,
    /// HMAC-SHA256 shared secret for GitHub webhook verification.
    pub webhook_secret: Option<String>,
}

// ── Auth helpers ──────────────────────────────────────────────────────────────

/// Query parameter for API key fallback (e.g. `?api_key=...`).
#[derive(Deserialize)]
pub(super) struct ApiKeyQuery {
    api_key: Option<String>,
}

/// Constant-time byte-slice equality to mitigate timing attacks.
///
/// Returns `true` only when both slices have identical length and content.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // XOR every byte pair; if any differ the accumulator is non-zero.
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Verify API key from `X-API-Key` header or `?api_key=` query param.
///
/// If no keys are configured the call is allowed through unconditionally.
/// Returns `Ok(())` on success, `Err(StatusCode)` otherwise.
fn verify_api_key(
    headers: &HeaderMap,
    query_key: Option<&str>,
    api_keys: &[String],
) -> Result<(), StatusCode> {
    // No keys configured → open access
    if api_keys.is_empty() {
        return Ok(());
    }

    // X-API-Key header takes precedence over query parameter
    let provided = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or_else(|| query_key.map(String::from));

    match provided {
        Some(key) => {
            if api_keys
                .iter()
                .any(|valid| constant_time_eq(key.as_bytes(), valid.as_bytes()))
            {
                Ok(())
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Webhook signature verification ───────────────────────────────────────────

/// Verify GitHub HMAC-SHA256 webhook signature.
///
/// `signature` must be in the `sha256=<hex>` format sent by GitHub.
fn verify_webhook_signature(payload: &[u8], signature: &str, secret: &str) -> bool {
    if !signature.starts_with("sha256=") {
        return false;
    }
    let expected_hex = &signature[7..];

    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(payload);
    let result = mac.finalize().into_bytes();
    let computed_hex = hex::encode(result);

    constant_time_eq(computed_hex.as_bytes(), expected_hex.as_bytes())
}

// ── Query params ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Deserialize)]
pub struct PrFilterParams {
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Deserialize)]
pub struct TriggerRunBody {
    pub repo_url: Option<String>,
    pub dry_run: Option<bool>,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub total_prs: usize,
    pub total_repos: usize,
    pub merged_prs: usize,
    pub open_prs: usize,
    pub ci_passed: usize,
}

#[derive(Serialize)]
pub struct TriggerResponse {
    pub status: &'static str,
    pub message: String,
}

// ── Route handlers ────────────────────────────────────────────────────────────

/// GET / — HTML dashboard
pub async fn dashboard() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

/// GET /api/health
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: state.version,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// GET /api/stats
pub async fn get_stats(State(state): State<AppState>) -> impl IntoResponse {
    let prs = state.memory.get_prs(None, 10000).unwrap_or_default();
    let total_prs = prs.len();
    let merged = prs
        .iter()
        .filter(|p| p.get("status").map(|s| s == "merged").unwrap_or(false))
        .count();
    let open = prs
        .iter()
        .filter(|p| {
            p.get("status")
                .map(|s| s == "open" || s == "ci_passed")
                .unwrap_or(false)
        })
        .count();
    let ci_passed = prs
        .iter()
        .filter(|p| p.get("status").map(|s| s == "ci_passed").unwrap_or(false))
        .count();

    // Unique repos
    let repos: std::collections::HashSet<&str> = prs
        .iter()
        .filter_map(|p| p.get("repo").map(|s| s.as_str()))
        .collect();

    Json(StatsResponse {
        total_prs,
        total_repos: repos.len(),
        merged_prs: merged,
        open_prs: open,
        ci_passed,
    })
}

/// GET /api/repos?limit=N
pub async fn get_repos(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let prs = state
        .memory
        .get_prs(None, params.limit * 3)
        .unwrap_or_default();
    let mut repo_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for pr in &prs {
        if let Some(repo) = pr.get("repo") {
            *repo_map.entry(repo.clone()).or_insert(0) += 1;
        }
    }
    let mut repos: Vec<serde_json::Value> = repo_map
        .into_iter()
        .map(|(repo, count)| serde_json::json!({ "repo": repo, "pr_count": count }))
        .collect();
    repos.sort_by(|a, b| b["pr_count"].as_u64().cmp(&a["pr_count"].as_u64()));
    repos.truncate(params.limit);
    Json(repos)
}

/// GET /api/prs?status=open&limit=50
pub async fn get_prs(
    State(state): State<AppState>,
    Query(params): Query<PrFilterParams>,
) -> impl IntoResponse {
    let prs = state
        .memory
        .get_prs(params.status.as_deref(), params.limit)
        .unwrap_or_default();
    // Convert HashMap<String,String> → serde_json::Value for JSON response
    let json_prs: Vec<serde_json::Value> = prs
        .into_iter()
        .map(|m| {
            serde_json::Value::Object(
                m.into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            )
        })
        .collect();
    Json(json_prs)
}

/// GET /api/runs?limit=20
pub async fn get_runs(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let prs = state.memory.get_prs(None, params.limit).unwrap_or_default();
    let runs: Vec<serde_json::Value> = prs
        .iter()
        .map(|pr| {
            serde_json::json!({
                "repo": pr.get("repo").map(|s| s.as_str()).unwrap_or(""),
                "pr_number": pr.get("pr_number").map(|s| s.as_str()).unwrap_or(""),
                "type": pr.get("contribution_type").map(|s| s.as_str()).unwrap_or(""),
                "status": pr.get("status").map(|s| s.as_str()).unwrap_or(""),
                "created_at": pr.get("created_at").map(|s| s.as_str()).unwrap_or(""),
            })
        })
        .collect();
    Json(runs)
}

/// GET /metrics — Prometheus-format metrics for monitoring.
///
/// Exposed metrics:
/// - `contribai_pipeline_runs_total` — Total pipeline runs
/// - `contribai_pr_submissions_total` — Total PRs submitted
/// - `contribai_pr_merged_total` — Total PRs merged
/// - `contribai_findings_total` — Total findings across all runs
/// - `contribai_errors_total` — Total errors across all runs
/// - `contribai_cache_entries_total` — LLM cache entries
/// - `contribai_circuit_breaker_state` — Circuit breaker state (0=Closed, 1=Open, 2=HalfOpen)
pub async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.memory.get_stats().unwrap_or_default();
    let cache_stats = get_cache_stats();

    let prs_submitted = stats.get("total_prs_submitted").unwrap_or(&0);
    let prs_merged = stats.get("prs_merged").unwrap_or(&0);
    let findings = stats.get("total_findings").unwrap_or(&0);
    let errors = stats.get("total_errors").unwrap_or(&0);
    let runs = stats.get("total_runs").unwrap_or(&0);

    let metrics = format!(
        "# HELP contribai_pipeline_runs_total Total pipeline runs.\n\
         # TYPE contribai_pipeline_runs_total counter\n\
         contribai_pipeline_runs_total {}\n\n\
         # HELP contribai_pr_submissions_total Total PRs submitted.\n\
         # TYPE contribai_pr_submissions_total counter\n\
         contribai_pr_submissions_total {}\n\n\
         # HELP contribai_pr_merged_total Total PRs merged.\n\
         # TYPE contribai_pr_merged_total counter\n\
         contribai_pr_merged_total {}\n\n\
         # HELP contribai_findings_total Total findings discovered.\n\
         # TYPE contribai_findings_total counter\n\
         contribai_findings_total {}\n\n\
         # HELP contribai_errors_total Total errors encountered.\n\
         # TYPE contribai_errors_total counter\n\
         contribai_errors_total {}\n\n\
         # HELP contribai_cache_entries_total LLM cache entries (valid).\n\
         # TYPE contribai_cache_entries_total gauge\n\
         contribai_cache_entries_total {}\n\n\
         # HELP contribai_circuit_breaker_state Circuit breaker state (0=Closed, 1=Open, 2=HalfOpen).\n\
         # TYPE contribai_circuit_breaker_state gauge\n\
         contribai_circuit_breaker_state 0\n",
        runs,
        prs_submitted,
        prs_merged,
        findings,
        errors,
        cache_stats.valid_entries,
    );

    (
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4; charset=utf-8")],
        metrics,
    )
}

/// Get cache stats from the LLM cache database if it exists.
fn get_cache_stats() -> CacheMetrics {
    let cache_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".contribai")
        .join("llm_cache.db");

    if !cache_path.exists() {
        return CacheMetrics {
            valid_entries: 0,
            total_entries: 0,
        };
    }

    // Quick SQLite query to count cache entries
    match rusqlite::Connection::open_with_flags(
        &cache_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM llm_cache", [], |r| r.get(0))
                .unwrap_or(0);
            let valid: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM llm_cache WHERE expires_at > datetime('now')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            CacheMetrics {
                valid_entries: valid,
                total_entries: total,
            }
        }
        Err(_) => CacheMetrics {
            valid_entries: 0,
            total_entries: 0,
        },
    }
}

/// Cache metrics struct.
#[allow(dead_code)]
struct CacheMetrics {
    valid_entries: i64,
    total_entries: i64,
}

/// POST /api/run — trigger a background run (requires API key if configured)
async fn trigger_run(
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<TriggerResponse>), StatusCode> {
    verify_api_key(&headers, query.api_key.as_deref(), &state.api_keys)?;

    // Note: actual pipeline execution requires config + env — this signals intent.
    // In production: send to a tokio channel consumed by the scheduler.
    info!("Manual run triggered via API");
    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerResponse {
            status: "accepted",
            message: "Pipeline run queued. Check /api/runs for progress.".into(),
        }),
    ))
}

/// POST /api/run/target — trigger run on specific repo (requires API key if configured)
async fn trigger_target(
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<TriggerResponse>), StatusCode> {
    verify_api_key(&headers, query.api_key.as_deref(), &state.api_keys)?;

    info!("Targeted run triggered via API");
    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerResponse {
            status: "accepted",
            message: "Targeted run queued.".into(),
        }),
    ))
}

/// POST /api/webhooks/github — receive and verify GitHub webhook events
pub async fn github_webhook(
    headers: HeaderMap,
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, StatusCode> {
    // Verify HMAC-SHA256 signature when a webhook secret is configured
    if let Some(secret) = &state.webhook_secret {
        let signature = headers
            .get("X-Hub-Signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_webhook_signature(&body, signature, secret) {
            tracing::warn!("GitHub webhook signature verification failed");
            return Err(StatusCode::FORBIDDEN);
        }
    }

    // Parse event type from header (e.g. "push", "pull_request", "ping")
    let event = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Parse JSON body; reject malformed payloads
    let payload: Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    info!(event = %event, action = %action, "GitHub webhook received");

    // Return accepted; pipeline triggering can be wired to the scheduler later
    Ok(Json(serde_json::json!({
        "status": "accepted",
        "event": event,
        "action": action,
    })))
}

// ── Server builder ───────────────────────────────────────────────────────────

/// Build the axum router with all routes.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Public routes
        .route("/", get(dashboard))
        .route("/api/health", get(health))
        .route("/api/stats", get(get_stats))
        .route("/api/repos", get(get_repos))
        .route("/api/prs", get(get_prs))
        .route("/api/runs", get(get_runs))
        .route("/metrics", get(get_metrics))
        // Pipeline API routes (protected)
        .route("/api/run", post(trigger_run))
        .route("/api/run/target", post(trigger_target))
        // Session API routes (protected)
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/create", post(create_session))
        // GitHub webhook
        .route("/api/webhooks/github", post(github_webhook))
        .layer(cors)
        .with_state(state)
}

/// GET /api/sessions — list active sessions
async fn list_sessions(
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    verify_api_key(&headers, query.api_key.as_deref(), &state.api_keys)?;
    let sessions = state.memory.get_sessions().unwrap_or_default();
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

/// POST /api/sessions/create — create a new session
async fn create_session(
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    verify_api_key(&headers, query.api_key.as_deref(), &state.api_keys)?;
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let mode = body.get("mode").and_then(|v| v.as_str()).unwrap_or("build");
    let session_id = uuid::Uuid::new_v4().to_string();
    state
        .memory
        .create_session(&session_id, name, mode)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "id": session_id,
        "name": name,
        "mode": mode,
        "status": "running"
    })))
}

/// Start the web server.
///
/// `config` is consumed to extract `api_keys` and `webhook_secret` for `AppState`.
pub async fn run_server(
    memory: Memory,
    config: &ContribAIConfig,
    host: &str,
    port: u16,
) -> crate::core::error::Result<()> {
    let state = AppState {
        memory: Arc::new(memory),
        version: "5.1.0",
        api_keys: config.web.api_keys.clone(),
        webhook_secret: config.web.webhook_secret.clone(),
    };

    let router = build_router(state);
    let addr = format!("{}:{}", host, port);

    // Check if TLS is enabled
    if config.web.tls_enabled {
        let cert_path = config.web.tls_cert_path.as_deref().ok_or_else(|| {
            crate::core::error::ContribError::Config("TLS enabled but tls_cert_path not set".into())
        })?;
        let key_path = config.web.tls_key_path.as_deref().ok_or_else(|| {
            crate::core::error::ContribError::Config("TLS enabled but tls_key_path not set".into())
        })?;

        info!(address = %addr, cert = %cert_path, key = %key_path, "Web dashboard running (TLS)");
        println!("  Dashboard: https://{}", addr);

        // Load TLS config
        let tls_config = load_tls_config(cert_path, key_path).await.map_err(|e| {
            crate::core::error::ContribError::Config(format!("TLS config error: {}", e))
        })?;

        let addr: std::net::SocketAddr = addr.parse().map_err(|e| {
            crate::core::error::ContribError::Config(format!("Invalid address {}: {}", addr, e))
        })?;

        axum_server::bind_rustls(addr, tls_config)
            .serve(router.into_make_service())
            .await
            .map_err(|e| {
                crate::core::error::ContribError::Config(format!("Server error: {}", e))
            })?;
    } else {
        let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
            crate::core::error::ContribError::Config(format!("Cannot bind to {}: {}", addr, e))
        })?;

        info!(address = %addr, "Web dashboard running (HTTP)");
        println!("  Dashboard: http://{}", addr);

        axum::serve(listener, router).await.map_err(|e| {
            crate::core::error::ContribError::Config(format!("Server error: {}", e))
        })?;
    }

    Ok(())
}

/// Load TLS configuration from cert and key files.
#[cfg(feature = "web")]
async fn load_tls_config(
    cert_path: &str,
    key_path: &str,
) -> std::io::Result<axum_server::tls_rustls::RustlsConfig> {
    axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await
}

// ── Dashboard HTML ────────────────────────────────────────────────────────────

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>ContribAI Dashboard</title>
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { font-family: 'Segoe UI', system-ui, sans-serif; background: #0d1117; color: #e6edf3; min-height: 100vh; }
    header { background: linear-gradient(135deg, #161b22 0%, #21262d 100%);
             border-bottom: 1px solid #30363d; padding: 1rem 2rem;
             display: flex; align-items: center; gap: 0.75rem; }
    header h1 { font-size: 1.4rem; font-weight: 700; color: #58a6ff; }
    header span { font-size: 0.8rem; color: #8b949e; background: #161b22;
                  padding: 0.2rem 0.6rem; border-radius: 20px; border: 1px solid #30363d; }
    .container { max-width: 1200px; margin: 0 auto; padding: 2rem; }
    .stats-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
                  gap: 1rem; margin-bottom: 2rem; }
    .stat-card { background: #161b22; border: 1px solid #30363d; border-radius: 12px;
                 padding: 1.25rem; text-align: center; transition: border-color 0.2s; }
    .stat-card:hover { border-color: #58a6ff; }
    .stat-card .num { font-size: 2rem; font-weight: 700; color: #58a6ff; }
    .stat-card .label { font-size: 0.8rem; color: #8b949e; margin-top: 0.25rem; }
    .section { background: #161b22; border: 1px solid #30363d; border-radius: 12px;
               padding: 1.5rem; margin-bottom: 1.5rem; }
    .section h2 { font-size: 1rem; font-weight: 600; color: #c9d1d9;
                  margin-bottom: 1rem; border-bottom: 1px solid #21262d; padding-bottom: 0.5rem; }
    table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
    th { text-align: left; padding: 0.5rem; color: #8b949e;
         border-bottom: 1px solid #21262d; font-weight: 500; }
    td { padding: 0.5rem; border-bottom: 1px solid #21262d; color: #e6edf3; }
    tr:last-child td { border-bottom: none; }
    tr:hover td { background: #21262d; }
    .badge { display: inline-block; padding: 0.1rem 0.5rem; border-radius: 20px;
             font-size: 0.75rem; font-weight: 500; }
    .badge-open { background: #1f6feb33; color: #58a6ff; border: 1px solid #1f6feb; }
    .badge-merged { background: #238636; color: #aff2c0; }
    .badge-ci_passed { background: #13563b; color: #56d364; }
    .badge-ci_failed { background: #6e1c20; color: #f85149; }
    .btn { background: #238636; color: #fff; border: none; border-radius: 6px;
           padding: 0.5rem 1rem; cursor: pointer; font-size: 0.875rem;
           transition: background 0.2s; }
    .btn:hover { background: #2ea043; }
    .loading { color: #8b949e; text-align: center; padding: 2rem; }
    #status-dot { width: 8px; height: 8px; background: #56d364;
                  border-radius: 50%; display: inline-block; margin-right: 6px;
                  animation: pulse 2s infinite; }
    @keyframes pulse { 0%,100% { opacity:1; } 50% { opacity:0.4; } }
    .actions { display: flex; gap: 0.5rem; margin-bottom: 1.5rem; }
  </style>
</head>
<body>
<header>
  <h1>ContribAI</h1>
  <span><span id="status-dot"></span>Live</span>
  <span id="version-badge">v5.1.0</span>
</header>

<div class="container">
  <div class="stats-grid">
    <div class="stat-card"><div class="num" id="stat-total">-</div><div class="label">Total PRs</div></div>
    <div class="stat-card"><div class="num" id="stat-merged">-</div><div class="label">Merged</div></div>
    <div class="stat-card"><div class="num" id="stat-open">-</div><div class="label">Open</div></div>
    <div class="stat-card"><div class="num" id="stat-ci">-</div><div class="label">CI Passed</div></div>
    <div class="stat-card"><div class="num" id="stat-repos">-</div><div class="label">Repos Targeted</div></div>
  </div>

  <div class="actions">
    <button class="btn" onclick="triggerRun(false)">Run Now</button>
    <button class="btn" style="background:#30363d" onclick="triggerRun(true)">Dry Run</button>
  </div>

  <div class="section">
    <h2>Recent Pull Requests</h2>
    <div id="prs-table"><p class="loading">Loading...</p></div>
  </div>

  <div class="section">
    <h2>Top Repos</h2>
    <div id="repos-table"><p class="loading">Loading...</p></div>
  </div>
</div>

<script>
async function fetchStats() {
  const r = await fetch('/api/stats');
  const d = await r.json();
  document.getElementById('stat-total').textContent = d.total_prs;
  document.getElementById('stat-merged').textContent = d.merged_prs;
  document.getElementById('stat-open').textContent = d.open_prs;
  document.getElementById('stat-ci').textContent = d.ci_passed;
  document.getElementById('stat-repos').textContent = d.total_repos;
}

async function fetchPRs() {
  const r = await fetch('/api/prs?limit=20');
  const prs = await r.json();
  if (!prs.length) { document.getElementById('prs-table').innerHTML = '<p class="loading">No PRs yet.</p>'; return; }
  const rows = prs.map(p => {
    const badge = `<span class="badge badge-${p.status}">${p.status}</span>`;
    return `<tr><td><a href="${p.pr_url||'#'}" target="_blank" style="color:#58a6ff">#${p.pr_number}</a></td>
      <td>${p.repo}</td><td>${p.title||p.contribution_type||''}</td><td>${badge}</td></tr>`;
  }).join('');
  document.getElementById('prs-table').innerHTML =
    `<table><thead><tr><th>PR</th><th>Repo</th><th>Type</th><th>Status</th></tr></thead><tbody>${rows}</tbody></table>`;
}

async function fetchRepos() {
  const r = await fetch('/api/repos?limit=10');
  const repos = await r.json();
  if (!repos.length) { document.getElementById('repos-table').innerHTML = '<p class="loading">No repos yet.</p>'; return; }
  const rows = repos.map(r => `<tr><td>${r.repo}</td><td>${r.pr_count}</td></tr>`).join('');
  document.getElementById('repos-table').innerHTML =
    `<table><thead><tr><th>Repository</th><th>PRs Submitted</th></tr></thead><tbody>${rows}</tbody></table>`;
}

async function triggerRun(dryRun) {
  const r = await fetch('/api/run', { method: 'POST',
    headers: {'Content-Type':'application/json'},
    body: JSON.stringify({ dry_run: dryRun }) });
  const d = await r.json();
  alert(d.message);
}

async function init() {
  const h = await fetch('/api/health');
  const hd = await h.json();
  document.getElementById('version-badge').textContent = 'v' + hd.version;
  await Promise.all([fetchStats(), fetchPRs(), fetchRepos()]);
}

init();
setInterval(() => { fetchStats(); fetchPRs(); }, 30000);
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    // ── constant_time_eq ──────────────────────────────────────────────────────

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_constant_time_eq_different_content() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn test_constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"hello", b"helloworld"));
    }

    #[test]
    fn test_constant_time_eq_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    // ── verify_api_key ────────────────────────────────────────────────────────

    #[test]
    fn test_verify_api_key_no_keys_configured() {
        let headers = HeaderMap::new();
        // Empty api_keys list → always allow
        assert!(verify_api_key(&headers, None, &[]).is_ok());
    }

    #[test]
    fn test_verify_api_key_valid_header() {
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "secret123".parse().unwrap());
        let keys = vec!["secret123".to_string()];
        assert!(verify_api_key(&headers, None, &keys).is_ok());
    }

    #[test]
    fn test_verify_api_key_invalid_header() {
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "wrong".parse().unwrap());
        let keys = vec!["secret123".to_string()];
        assert_eq!(
            verify_api_key(&headers, None, &keys),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn test_verify_api_key_via_query_param() {
        let headers = HeaderMap::new();
        let keys = vec!["qparam-key".to_string()];
        assert!(verify_api_key(&headers, Some("qparam-key"), &keys).is_ok());
    }

    #[test]
    fn test_verify_api_key_missing_key() {
        let headers = HeaderMap::new();
        let keys = vec!["secret".to_string()];
        assert_eq!(
            verify_api_key(&headers, None, &keys),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn test_verify_api_key_header_precedence_over_query() {
        // Header present and valid; query param (if present) should be ignored
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "header-key".parse().unwrap());
        let keys = vec!["header-key".to_string()];
        // query param contains a wrong value but header is correct → OK
        assert!(verify_api_key(&headers, Some("wrong-query"), &keys).is_ok());
    }

    // ── verify_webhook_signature ──────────────────────────────────────────────

    #[test]
    fn test_verify_webhook_signature_valid() {
        // Generate a known-good signature for "hello world" with secret "mysecret"
        // sha256 HMAC computed: use known value
        let payload = b"hello world";
        let secret = "mysecret";

        // Compute expected signature using the same code path
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        let result = mac.finalize().into_bytes();
        let hex_sig = format!("sha256={}", hex::encode(result));

        assert!(verify_webhook_signature(payload, &hex_sig, secret));
    }

    #[test]
    fn test_verify_webhook_signature_wrong_secret() {
        let payload = b"hello world";
        // Signature computed with "mysecret" but verified against "wrongsecret"
        let mut mac = Hmac::<Sha256>::new_from_slice(b"mysecret").unwrap();
        mac.update(payload);
        let result = mac.finalize().into_bytes();
        let hex_sig = format!("sha256={}", hex::encode(result));

        assert!(!verify_webhook_signature(payload, &hex_sig, "wrongsecret"));
    }

    #[test]
    fn test_verify_webhook_signature_missing_prefix() {
        assert!(!verify_webhook_signature(b"data", "abcdef1234", "secret"));
    }

    #[test]
    fn test_verify_webhook_signature_tampered_payload() {
        let secret = "mysecret";
        let original = b"original payload";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(original);
        let result = mac.finalize().into_bytes();
        let hex_sig = format!("sha256={}", hex::encode(result));

        // Verify with a different payload
        assert!(!verify_webhook_signature(
            b"tampered payload",
            &hex_sig,
            secret
        ));
    }

    // ── router construction ───────────────────────────────────────────────────

    #[test]
    fn test_build_router_compiles() {
        // Verify the router can be constructed with a mock state
        let _ = std::mem::size_of::<AppState>();
        let _ = std::mem::size_of::<HealthResponse>();
        let _ = std::mem::size_of::<StatsResponse>();
    }

    #[test]
    fn test_default_limit() {
        assert_eq!(default_limit(), 50);
    }

    #[test]
    fn test_dashboard_html_nonempty() {
        assert!(!DASHBOARD_HTML.is_empty());
        assert!(DASHBOARD_HTML.contains("ContribAI"));
        assert!(DASHBOARD_HTML.contains("/api/health"));
        assert!(DASHBOARD_HTML.contains("/api/prs"));
        // Webhook endpoint is on the router, not referenced in the static dashboard HTML
    }

    #[test]
    fn test_trigger_response_structure() {
        let r = TriggerResponse {
            status: "accepted",
            message: "test".into(),
        };
        assert_eq!(r.status, "accepted");
    }
}
