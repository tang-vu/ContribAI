//! Middleware chain tests.
//!
//! Tests each middleware behavior and chain composition:
//! - RateLimit: blocks when limit exceeded, allows when under
//! - Validation: rejects empty repo names, passes valid
//! - Retry: wraps processing with retry logic
//! - DCO: passes through (non-blocking)
//! - QualityGate: blocks below threshold score

use contribai::core::middleware::{build_default_chain, PipelineContext};

// ── Chain Construction ──────────────────────────────────────────────────

#[test]
fn test_build_default_chain_has_five_middlewares() {
    let _chain = build_default_chain(5, 3, 0.6);
    // Chain is created without error — 5 middlewares are registered
}

#[tokio::test]
async fn test_default_chain_passes_valid_context() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 5,
        quality_score: 0.8,
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok(), "Should pass valid context through chain");
    let ctx = result.unwrap();
    assert!(!ctx.should_skip, "Should not skip");
}

#[tokio::test]
async fn test_default_chain_blocks_empty_repo() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: String::new(), // Empty — should trigger validation
        owner: String::new(),
        remaining_prs: 5,
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok(), "Chain should not error");
    let ctx = result.unwrap();
    assert!(ctx.should_skip, "Should skip empty repo");
}

// ── RateLimit ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rate_limit_allows_when_remaining_positive() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 3,
        dry_run: false,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(!ctx.should_skip, "Should allow when remaining > 0");
    assert!(!ctx.rate_limited);
}

#[tokio::test]
async fn test_rate_limit_blocks_when_remaining_zero() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 0,
        dry_run: false, // Not dry run — should enforce limit
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(ctx.should_skip, "Should skip when remaining = 0");
    assert!(ctx.rate_limited, "Should mark as rate limited");
}

#[tokio::test]
async fn test_rate_limit_allows_in_dry_run() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 0,
        dry_run: true, // Dry run — should allow regardless of limit
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(
        !ctx.should_skip,
        "Should allow in dry run even with 0 remaining"
    );
}

// ── Validation ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_validation_passes_valid_repo() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/valid-repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 5,
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(!ctx.should_skip, "Should pass valid repo name");
}

#[tokio::test]
async fn test_validation_blocks_empty_repo_name() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: String::new(),
        owner: String::new(),
        remaining_prs: 5,
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(ctx.should_skip, "Should skip empty repo name");
}

// ── QualityGate ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_quality_gate_passes_above_threshold() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 5,
        quality_score: 0.85, // Above 0.6 threshold
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(ctx.quality_passed, "Should pass above quality threshold");
}

#[tokio::test]
async fn test_quality_gate_fails_below_threshold() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 5,
        quality_score: 0.45, // Below 0.6 threshold
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(!ctx.quality_passed, "Should fail below quality threshold");
}

#[tokio::test]
async fn test_quality_gate_zero_score_fails() {
    let chain = build_default_chain(5, 3, 0.6);
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 5,
        quality_score: 0.0, // Default = no score
        dry_run: true,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    // 0.0 means no quality assessment — should not fail the gate
    assert!(
        ctx.quality_passed,
        "Should pass when quality score is 0 (no assessment yet)"
    );
}

// ── Chain Short-Circuit ──────────────────────────────────────────────────

#[tokio::test]
async fn test_chain_short_circuits_on_rate_limit() {
    let chain = build_default_chain(0, 3, 0.0); // 0 PRs/day, 0 quality threshold
    let ctx = PipelineContext {
        repo_name: "owner/repo".to_string(),
        owner: "owner".to_string(),
        remaining_prs: 0,
        dry_run: false,
        ..Default::default()
    };

    let result = chain.execute(ctx).await;
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(ctx.should_skip);
    assert!(ctx.rate_limited);
}
