//! Middleware chain for pipeline processing.
//!
//! Port from Python `core/middleware.py`.
//! Each middleware handles a specific cross-cutting concern,
//! executing in strict order via a chain-of-responsibility pattern.

use std::fmt;
use std::sync::Arc;
use tracing::{info, warn};

use crate::core::error::Result;

/// Context passed through the middleware chain.
#[derive(Debug, Clone)]
pub struct PipelineContext {
    pub repo_name: String,
    pub owner: String,
    pub dry_run: bool,

    // Decisions
    pub should_skip: bool,
    pub skip_reason: String,

    // Rate limiting
    pub remaining_prs: i32,
    pub rate_limited: bool,

    // Compliance
    pub cla_required: bool,
    pub cla_signed: bool,
    pub dco_required: bool,
    pub signoff: Option<String>,

    // Quality
    pub quality_score: f64,
    pub quality_passed: bool,

    // Data
    pub errors: Vec<String>,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl Default for PipelineContext {
    fn default() -> Self {
        Self {
            repo_name: String::new(),
            owner: String::new(),
            dry_run: false,
            should_skip: false,
            skip_reason: String::new(),
            remaining_prs: 10,
            rate_limited: false,
            cla_required: false,
            cla_signed: false,
            dco_required: false,
            signoff: None,
            quality_score: 0.0,
            quality_passed: true,
            errors: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }
}

/// Trait for pipeline middlewares.
#[async_trait::async_trait]
pub trait Middleware: Send + Sync + fmt::Debug {
    async fn process(
        &self,
        ctx: PipelineContext,
        next: &MiddlewareChain,
        index: usize,
    ) -> Result<PipelineContext>;
}

/// Executes middlewares in order, passing context through each.
#[derive(Clone)]
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new(middlewares: Vec<Arc<dyn Middleware>>) -> Self {
        Self { middlewares }
    }

    pub async fn execute(&self, ctx: PipelineContext) -> Result<PipelineContext> {
        self.execute_from(ctx, 0).await
    }

    pub async fn execute_from(
        &self,
        ctx: PipelineContext,
        index: usize,
    ) -> Result<PipelineContext> {
        if index >= self.middlewares.len() {
            return Ok(ctx);
        }
        let mw = &self.middlewares[index];
        mw.process(ctx, self, index + 1).await
    }
}

// ── Built-in Middlewares ──────────────────────────────

/// Check GitHub API and daily PR limits before processing.
#[derive(Debug)]
pub struct RateLimitMiddleware {
    max_prs_per_day: i32,
}

impl RateLimitMiddleware {
    pub fn new(max_prs_per_day: i32) -> Self {
        Self { max_prs_per_day }
    }
}

#[async_trait::async_trait]
impl Middleware for RateLimitMiddleware {
    async fn process(
        &self,
        mut ctx: PipelineContext,
        next: &MiddlewareChain,
        index: usize,
    ) -> Result<PipelineContext> {
        if ctx.remaining_prs <= 0 && !ctx.dry_run {
            ctx.should_skip = true;
            ctx.skip_reason = format!("Daily PR limit reached ({})", self.max_prs_per_day);
            ctx.rate_limited = true;
            warn!(repo = %ctx.repo_name, reason = %ctx.skip_reason, "Rate limited");
            return Ok(ctx);
        }
        next.execute_from(ctx, index).await
    }
}

/// Validate repo is suitable for contribution.
#[derive(Debug)]
pub struct ValidationMiddleware;

#[async_trait::async_trait]
impl Middleware for ValidationMiddleware {
    async fn process(
        &self,
        mut ctx: PipelineContext,
        next: &MiddlewareChain,
        index: usize,
    ) -> Result<PipelineContext> {
        if ctx.repo_name.is_empty() {
            ctx.should_skip = true;
            ctx.skip_reason = "No repo data".into();
            return Ok(ctx);
        }
        next.execute_from(ctx, index).await
    }
}

/// Wrap downstream processing with retry logic.
#[derive(Debug)]
pub struct RetryMiddleware {
    max_retries: u32,
    base_delay_secs: f64,
}

impl RetryMiddleware {
    pub fn new(max_retries: u32, base_delay_secs: f64) -> Self {
        Self {
            max_retries,
            base_delay_secs,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RetryMiddleware {
    async fn process(
        &self,
        ctx: PipelineContext,
        next: &MiddlewareChain,
        index: usize,
    ) -> Result<PipelineContext> {
        let mut last_error = None;
        for attempt in 0..self.max_retries {
            match next.execute_from(ctx.clone(), index).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < self.max_retries {
                        let delay = self.base_delay_secs * 2.0_f64.powi(attempt as i32);
                        warn!(
                            repo = %ctx.repo_name,
                            attempt = attempt + 1,
                            delay_sec = delay,
                            "Retry"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs_f64(delay)).await;
                    }
                }
            }
        }
        let mut ctx = ctx;
        let err_msg = format!(
            "All {} attempts failed: {}",
            self.max_retries,
            last_error.as_ref().map(|e| e.to_string()).unwrap_or_default()
        );
        ctx.errors.push(err_msg);
        Ok(ctx)
    }
}

/// Auto-compute DCO signoff from authenticated user.
#[derive(Debug)]
pub struct DCOMiddleware;

#[async_trait::async_trait]
impl Middleware for DCOMiddleware {
    async fn process(
        &self,
        mut ctx: PipelineContext,
        next: &MiddlewareChain,
        index: usize,
    ) -> Result<PipelineContext> {
        if let Some(user) = ctx.metadata.get("user") {
            let name = user
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| user.get("login").and_then(|v| v.as_str()))
                .unwrap_or("");
            let email = user
                .get("email")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| {
                    let uid = user.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let login = user.get("login").and_then(|v| v.as_str()).unwrap_or("");
                    format!("{uid}+{login}@users.noreply.github.com")
                });
            if !name.is_empty() {
                ctx.signoff = Some(format!("{name} <{email}>"));
                ctx.dco_required = true;
            }
        }
        next.execute_from(ctx, index).await
    }
}

/// Check contribution quality before PR creation.
#[derive(Debug)]
pub struct QualityGateMiddleware {
    min_score: f64,
}

impl QualityGateMiddleware {
    pub fn new(min_score: f64) -> Self {
        Self { min_score }
    }
}

#[async_trait::async_trait]
impl Middleware for QualityGateMiddleware {
    async fn process(
        &self,
        ctx: PipelineContext,
        next: &MiddlewareChain,
        index: usize,
    ) -> Result<PipelineContext> {
        let mut result = next.execute_from(ctx, index).await?;
        if result.quality_score > 0.0 && result.quality_score < self.min_score {
            result.quality_passed = false;
            info!(
                repo = %result.repo_name,
                score = result.quality_score,
                threshold = self.min_score,
                "Quality gate failed"
            );
        }
        Ok(result)
    }
}

/// Build the default middleware chain.
pub fn build_default_chain(
    max_prs_per_day: i32,
    max_retries: u32,
    min_quality_score: f64,
) -> MiddlewareChain {
    let middlewares: Vec<Arc<dyn Middleware>> = vec![
        Arc::new(RateLimitMiddleware::new(max_prs_per_day)),
        Arc::new(ValidationMiddleware),
        Arc::new(RetryMiddleware::new(max_retries, 5.0)),
        Arc::new(DCOMiddleware),
        Arc::new(QualityGateMiddleware::new(min_quality_score)),
    ];
    MiddlewareChain::new(middlewares)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_chain() {
        let chain = MiddlewareChain::new(vec![]);
        let ctx = PipelineContext::default();
        let result = chain.execute(ctx).await.unwrap();
        assert!(!result.should_skip);
    }

    #[tokio::test]
    async fn test_rate_limit_blocks() {
        let chain = MiddlewareChain::new(vec![Arc::new(RateLimitMiddleware::new(10))]);
        let mut ctx = PipelineContext::default();
        ctx.remaining_prs = 0;
        let result = chain.execute(ctx).await.unwrap();
        assert!(result.should_skip);
        assert!(result.rate_limited);
    }

    #[tokio::test]
    async fn test_rate_limit_allows_dry_run() {
        let chain = MiddlewareChain::new(vec![Arc::new(RateLimitMiddleware::new(10))]);
        let mut ctx = PipelineContext::default();
        ctx.remaining_prs = 0;
        ctx.dry_run = true;
        let result = chain.execute(ctx).await.unwrap();
        assert!(!result.should_skip);
    }

    #[tokio::test]
    async fn test_validation_blocks_empty_repo() {
        let chain = MiddlewareChain::new(vec![Arc::new(ValidationMiddleware)]);
        let ctx = PipelineContext::default();
        let result = chain.execute(ctx).await.unwrap();
        assert!(result.should_skip);
        assert_eq!(result.skip_reason, "No repo data");
    }

    #[tokio::test]
    async fn test_validation_passes() {
        let chain = MiddlewareChain::new(vec![Arc::new(ValidationMiddleware)]);
        let mut ctx = PipelineContext::default();
        ctx.repo_name = "test/repo".into();
        let result = chain.execute(ctx).await.unwrap();
        assert!(!result.should_skip);
    }

    #[tokio::test]
    async fn test_dco_signoff() {
        let chain = MiddlewareChain::new(vec![Arc::new(DCOMiddleware)]);
        let mut ctx = PipelineContext::default();
        ctx.metadata.insert(
            "user".into(),
            serde_json::json!({"name": "Test", "email": "t@e.com", "login": "test", "id": 1}),
        );
        let result = chain.execute(ctx).await.unwrap();
        assert!(result.dco_required);
        assert_eq!(result.signoff.unwrap(), "Test <t@e.com>");
    }

    #[tokio::test]
    async fn test_dco_noreply() {
        let chain = MiddlewareChain::new(vec![Arc::new(DCOMiddleware)]);
        let mut ctx = PipelineContext::default();
        ctx.metadata.insert(
            "user".into(),
            serde_json::json!({"name": "Test", "login": "test", "id": 123}),
        );
        let result = chain.execute(ctx).await.unwrap();
        assert_eq!(
            result.signoff.unwrap(),
            "Test <123+test@users.noreply.github.com>"
        );
    }

    #[tokio::test]
    async fn test_quality_gate_blocks() {
        let chain = MiddlewareChain::new(vec![Arc::new(QualityGateMiddleware::new(5.0))]);
        let mut ctx = PipelineContext::default();
        ctx.quality_score = 3.0;
        let result = chain.execute(ctx).await.unwrap();
        assert!(!result.quality_passed);
    }

    #[tokio::test]
    async fn test_quality_gate_passes() {
        let chain = MiddlewareChain::new(vec![Arc::new(QualityGateMiddleware::new(5.0))]);
        let mut ctx = PipelineContext::default();
        ctx.quality_score = 8.0;
        let result = chain.execute(ctx).await.unwrap();
        assert!(result.quality_passed);
    }

    #[tokio::test]
    async fn test_default_chain() {
        let chain = build_default_chain(10, 2, 5.0);
        let mut ctx = PipelineContext::default();
        ctx.repo_name = "test/repo".into();
        let result = chain.execute(ctx).await.unwrap();
        assert!(!result.should_skip);
    }
}
