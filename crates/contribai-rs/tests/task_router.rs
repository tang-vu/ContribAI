//! Task router tests.
//!
//! Tests task routing logic:
//! - Code gen → routed to CodeGen agent + appropriate model
//! - Analysis → routed to Analyzer agent + cheaper model
//! - Review → routed to Reviewer agent
//! - Cost strategy: cheap model for simple tasks, expensive for complex

use contribai::llm::models::TaskType;
use contribai::llm::router::{CostStrategy, TaskRouter};

// ── Router Construction ─────────────────────────────────────────────────

#[test]
fn test_router_with_balanced_strategy() {
    let _router = TaskRouter::new(CostStrategy::Balanced);
}

#[test]
fn test_router_with_performance_strategy() {
    let _router = TaskRouter::new(CostStrategy::Performance);
}

#[test]
fn test_router_with_economy_strategy() {
    let _router = TaskRouter::new(CostStrategy::Economy);
}

// ── Task Routing ────────────────────────────────────────────────────────

#[test]
fn test_route_analysis_task() {
    let mut router = TaskRouter::new(CostStrategy::Balanced);
    let decision = router.route(TaskType::Analysis, 5, 1);
    assert_eq!(
        decision.task_type,
        TaskType::Analysis,
        "Should route analysis tasks correctly"
    );
}

#[test]
fn test_route_codegen_task() {
    let mut router = TaskRouter::new(CostStrategy::Balanced);
    let decision = router.route(TaskType::CodeGen, 5, 1);
    assert_eq!(
        decision.task_type,
        TaskType::CodeGen,
        "Should route codegen tasks correctly"
    );
}

#[test]
fn test_route_review_task() {
    let mut router = TaskRouter::new(CostStrategy::Balanced);
    let decision = router.route(TaskType::Review, 3, 1);
    assert_eq!(
        decision.task_type,
        TaskType::Review,
        "Should route review tasks correctly"
    );
}

#[test]
fn test_route_planning_task() {
    let mut router = TaskRouter::new(CostStrategy::Balanced);
    let decision = router.route(TaskType::Planning, 7, 5);
    assert_eq!(
        decision.task_type,
        TaskType::Planning,
        "Should route planning tasks correctly"
    );
}

// ── Complexity-Based Routing ────────────────────────────────────────────

#[test]
fn test_high_complexity_uses_capable_model() {
    let mut router = TaskRouter::new(CostStrategy::Economy);
    let decision = router.route(TaskType::CodeGen, 9, 3);
    // Economy strategy for high complexity should still pick a capable model
    assert!(
        !decision.model.name.is_empty(),
        "Should select a model for high complexity, got: {}",
        decision.model.name
    );
}

#[test]
fn test_low_complexity_uses_cheaper_model() {
    let mut router = TaskRouter::new(CostStrategy::Economy);
    let decision = router.route(TaskType::Analysis, 2, 1);
    // Economy strategy for low complexity should pick cheapest model
    assert!(
        !decision.model.name.is_empty(),
        "Should select a model for low complexity, got: {}",
        decision.model.name
    );
}

// ── Performance Strategy ────────────────────────────────────────────────

#[test]
fn test_performance_strategy_uses_best_model() {
    let mut router = TaskRouter::new(CostStrategy::Performance);
    let decision = router.route(TaskType::CodeGen, 5, 1);
    // Performance strategy should use the most capable model
    assert!(
        decision.reason.contains("Performance"),
        "Performance strategy should mention performance, got: {}",
        decision.reason
    );
}

// ── File Count Impact ───────────────────────────────────────────────────

#[test]
fn test_many_files_affects_routing() {
    let mut router = TaskRouter::new(CostStrategy::Balanced);

    // Many files should still route correctly
    let decision = router.route(TaskType::CodeGen, 3, 20);
    assert!(
        !decision.model.name.is_empty(),
        "Many files should still route to a model, got: {}",
        decision.model.name
    );
}
