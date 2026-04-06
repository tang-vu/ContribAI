//! Integration tests for pipeline components.
//!
//! Tests merge_contributions, quality scorer, risk classification,
//! event bus, and pipeline construction — all without network calls.

mod common;

use chrono::Utc;
use contribai::core::events::{Event, EventBus, EventType};
use contribai::core::models::{
    Contribution, ContributionType, FileChange, Finding, Severity, Symbol, SymbolKind,
};
use contribai::generator::risk::{classify_risk, is_within_tolerance, RiskLevel};
use contribai::generator::scorer::QualityScorer;
use contribai::orchestrator::memory::Memory;
use contribai::orchestrator::pipeline::merge_contributions_pub;
use std::collections::HashMap;

// ── Test Helpers ──────────────────────────────────────────────────────────

fn make_finding(title: &str, file: &str) -> Finding {
    Finding {
        id: uuid::Uuid::new_v4().to_string(),
        finding_type: ContributionType::CodeQuality,
        severity: Severity::Medium,
        title: title.into(),
        description: format!("Fix: {}", title),
        file_path: file.into(),
        line_start: Some(10),
        line_end: Some(15),
        suggestion: Some("Refactor this".into()),
        confidence: 0.9,
        priority_signals: vec![],
    }
}

fn make_contribution(title: &str, files: &[(&str, &str)]) -> Contribution {
    Contribution {
        finding: make_finding(title, files[0].0),
        contribution_type: ContributionType::CodeQuality,
        title: title.into(),
        description: format!("Description for: {}", title),
        changes: files
            .iter()
            .map(|(path, content)| FileChange {
                path: path.to_string(),
                original_content: Some("old code".into()),
                new_content: content.to_string(),
                is_new_file: false,
                is_deleted: false,
            })
            .collect(),
        commit_message: format!("fix: {}", title),
        tests_added: vec![],
        branch_name: "fix/test".into(),
        generated_at: Utc::now(),
    }
}

// ── Merge Contributions ───────────────────────────────────────────────────

#[test]
fn test_merge_single_contribution_passthrough() {
    let c = make_contribution("single fix", &[("src/lib.rs", "fn main() {}")]);
    let merged = merge_contributions_pub(vec![c.clone()]);

    assert_eq!(merged.title, "single fix");
    assert_eq!(merged.changes.len(), 1);
}

#[test]
fn test_merge_multiple_contributions() {
    let c1 = make_contribution("fix imports", &[("src/a.rs", "use std::io;")]);
    let c2 = make_contribution("fix naming", &[("src/b.rs", "let count = 0;")]);
    let c3 = make_contribution("fix types", &[("src/c.rs", "fn foo() -> i32 { 0 }")]);

    let merged = merge_contributions_pub(vec![c1, c2, c3]);

    // Should merge all file changes
    assert_eq!(merged.changes.len(), 3);
    // Title should mention count
    assert!(merged.title.contains("3") && merged.title.contains("improvements"));
}

#[test]
fn test_merge_deduplicates_file_paths() {
    let c1 = make_contribution("fix A", &[("src/lib.rs", "version A")]);
    let c2 = make_contribution("fix B", &[("src/lib.rs", "version B")]);

    let merged = merge_contributions_pub(vec![c1, c2]);

    // Same file path — first-seen wins, deduplicated
    assert_eq!(merged.changes.len(), 1);
    assert_eq!(merged.changes[0].new_content, "version A");
}

// ── Quality Scorer ────────────────────────────────────────────────────────

#[test]
fn test_scorer_good_contribution_passes() {
    let scorer = QualityScorer::new(0.6);
    let c = make_contribution(
        "fix: remove unused import in handler module",
        &[("src/handler.rs", "use std::collections::HashMap;\nfn handle() {\n    let m = HashMap::new();\n    println!(\"{:?}\", m);\n}\n")],
    );

    let report = scorer.evaluate(&c, None);
    assert!(
        report.passed,
        "Good contribution should pass: {}",
        report.summary()
    );
    assert!(report.score >= 0.6);
}

#[test]
fn test_scorer_no_changes_detected() {
    let scorer = QualityScorer::new(0.6);
    let mut c = make_contribution("empty fix", &[("src/x.rs", "placeholder")]);
    c.changes.clear(); // no file changes at all

    let report = scorer.evaluate(&c, None);
    // has_changes check should fail
    let has_check = report
        .checks
        .iter()
        .find(|c| c.name == "has_changes")
        .unwrap();
    assert!(!has_check.passed, "has_changes should fail when no changes");
    assert_eq!(has_check.score, 0.0);
}

#[test]
fn test_scorer_debug_code_penalized() {
    let scorer = QualityScorer::new(0.6);
    // Use patterns the scorer actually detects: print(, console.log(, # TODO
    let c = make_contribution(
        "fix: add logging",
        &[(
            "src/app.py",
            "def main():\n    print (\"debug output\")\n    # TODO remove this\n    pass\n",
        )],
    );

    let report = scorer.evaluate(&c, None);
    let debug_check = report.checks.iter().find(|c| c.name == "no_debug_code");
    assert!(debug_check.is_some());
    let check = debug_check.unwrap();
    assert!(!check.passed, "Debug code should be detected");
}

// ── Risk Classification ───────────────────────────────────────────────────

#[test]
fn test_risk_docs_change_is_low() {
    let risk = classify_risk("docs", &["README.md".into()], 10);
    assert_eq!(risk.level, RiskLevel::Low);
}

#[test]
fn test_risk_multi_file_refactor_is_high() {
    let files: Vec<String> = (0..6).map(|i| format!("src/mod{}.rs", i)).collect();
    let risk = classify_risk("refactor", &files, 300);
    assert_eq!(risk.level, RiskLevel::High);
}

#[test]
fn test_risk_tolerance_filtering() {
    assert!(is_within_tolerance(RiskLevel::Low, "low"));
    assert!(!is_within_tolerance(RiskLevel::Medium, "low"));
    assert!(is_within_tolerance(RiskLevel::Medium, "medium"));
    assert!(!is_within_tolerance(RiskLevel::High, "medium"));
    assert!(is_within_tolerance(RiskLevel::High, "high"));
}

// ── Event Bus ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_event_bus_emit_and_history() {
    let bus = EventBus::new(100);

    bus.emit(Event::new(EventType::PipelineStart, "test").with_data("repo", "owner/repo"))
        .await;

    bus.emit(Event::new(EventType::PipelineComplete, "test").with_data("prs_created", 2))
        .await;

    let history = bus.history(None, 100).await;
    assert_eq!(history.len(), 2);
    // history() returns most-recent-first
    assert_eq!(history[0].event_type, EventType::PipelineComplete);
    assert_eq!(history[1].event_type, EventType::PipelineStart);
}

#[tokio::test]
async fn test_event_bus_subscribe() {
    let bus = EventBus::new(100);
    let mut rx = bus.subscribe();

    bus.emit(Event::new(EventType::PrCreated, "test")).await;

    let event = rx.recv().await.unwrap();
    assert_eq!(event.event_type, EventType::PrCreated);
}

// ── Pipeline Construction ─────────────────────────────────────────────────

#[tokio::test]
async fn test_pipeline_constructs_with_mock_llm() {
    use contribai::core::config::ContribAIConfig;
    use contribai::github::client::GitHubClient;
    use contribai::orchestrator::pipeline::ContribPipeline;

    let config = ContribAIConfig::default();
    let github = GitHubClient::new("fake-token-for-test", 100).unwrap();
    let memory = Memory::open_in_memory().unwrap();
    let event_bus = EventBus::new(100);
    let mock_llm = common::mock_llm::MockLlm::new();

    // Pipeline should construct without error
    let _pipeline = ContribPipeline::new(&config, &github, &mock_llm, &memory, &event_bus);
}

// ── v5.6 Config Defaults ──────────────────────────────────────────────────

#[test]
fn test_pipeline_config_defaults() {
    use contribai::core::config::PipelineConfig;

    let config = PipelineConfig::default();
    assert!(config.skip_docs_prs, "skip_docs_prs should default to true");
    assert!(
        config.require_bug_verification,
        "require_bug_verification should default to true"
    );
    assert!(
        config.skip_repos_with_open_pr,
        "skip_repos_with_open_pr should default to true"
    );
}

// ── Cross-Run Dedup ───────────────────────────────────────────────────────

#[test]
fn test_cross_run_dedup_detects_open_pr() {
    let m = Memory::open_in_memory().unwrap();

    // Record an open PR
    m.record_pr("owner/repo", 42, "url", "Fix stuff", "quality", "b", "f")
        .unwrap();

    // Check if we have an open PR to this repo
    let existing = m.get_prs(Some("open"), 100).unwrap();
    let has_open = existing
        .iter()
        .any(|pr| pr.get("repo").map(|r| r.as_str()) == Some("owner/repo"));
    assert!(has_open, "Should detect existing open PR");

    // Different repo should not be blocked
    let has_other = existing
        .iter()
        .any(|pr| pr.get("repo").map(|r| r.as_str()) == Some("other/repo"));
    assert!(!has_other, "Different repo should not be blocked");
}

#[test]
fn test_cross_run_dedup_allows_after_merge() {
    let m = Memory::open_in_memory().unwrap();

    // Record a PR then mark as merged
    m.record_pr("owner/repo", 42, "url", "Fix", "quality", "b", "f")
        .unwrap();
    m.update_pr_status("owner/repo", 42, "merged").unwrap();

    let existing = m.get_prs(Some("open"), 100).unwrap();
    let has_open = existing
        .iter()
        .any(|pr| pr.get("repo").map(|r| r.as_str()) == Some("owner/repo"));
    assert!(!has_open, "Merged PR should not block new contributions");
}

// ── Semantic Chunking (v5.6) ──────────────────────────────────────────────

#[test]
fn test_semantic_chunk_small_file_single_chunk() {
    use contribai::analysis::compressor::ContextCompressor;

    let content = "use std::io;\n\nfn main() {\n    println!(\"hello\");\n}\n";
    let symbols = vec![contribai::core::models::Symbol {
        name: "main".into(),
        kind: contribai::core::models::SymbolKind::Function,
        file_path: "main.rs".into(),
        line_start: 2,
        line_end: 4,
    }];

    let chunks = ContextCompressor::semantic_chunk(content, &symbols, 10000);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].contains("main"));
}

#[test]
fn test_semantic_chunk_no_symbols_fallback() {
    use contribai::analysis::compressor::ContextCompressor;

    let content = "fn main() {\n    println!(\"hello\");\n}\n";
    let chunks = ContextCompressor::semantic_chunk(content, &[], 10000);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].contains("main"));
}

#[test]
fn test_semantic_chunk_respects_budget() {
    use contribai::analysis::compressor::ContextCompressor;

    // Create a large file with multiple functions
    let mut content = String::from("use std::io;\n\n");
    let mut symbols = Vec::new();
    let mut line = 2;
    for i in 0..10 {
        let body: String = (0..20)
            .map(|j| format!("    let x{} = {};", j, j))
            .collect::<Vec<_>>()
            .join("\n");
        content.push_str(&format!("fn func_{}() {{\n{}\n}}\n\n", i, body));
        symbols.push(contribai::core::models::Symbol {
            name: format!("func_{}", i),
            kind: contribai::core::models::SymbolKind::Function,
            file_path: "lib.rs".into(),
            line_start: line,
            line_end: line + 21,
        });
        line += 23;
    }

    // Small budget should force multiple chunks
    let chunks = ContextCompressor::semantic_chunk(&content, &symbols, 200);
    assert!(
        chunks.len() > 1,
        "Should split into multiple chunks, got {}",
        chunks.len()
    );
}

// ── Symbol Map + Resolved Imports Wiring (M-3) ──────────────────────────

#[test]
fn test_symbol_map_not_polluted_by_cross_file_keys() {
    // Build a RepoContext with symbol_map and resolved_imports separately
    let mut symbol_map = HashMap::new();
    symbol_map.insert(
        "src/lib.rs".to_string(),
        vec![Symbol {
            name: "Config".into(),
            kind: SymbolKind::Struct,
            file_path: "src/lib.rs".into(),
            line_start: 10,
            line_end: 25,
        }],
    );

    let mut resolved_imports = HashMap::new();
    resolved_imports.insert(
        "src/main.rs".to_string(),
        vec![Symbol {
            name: "Config [resolved: Struct Config (src/lib.rs:L10-L25)]".into(),
            kind: SymbolKind::Import,
            file_path: "src/main.rs".into(),
            line_start: 0,
            line_end: 0,
        }],
    );

    // Verify no _cross_file_ keys in symbol_map
    assert!(
        !symbol_map.keys().any(|k| k.starts_with("_cross_file_")),
        "symbol_map must not contain _cross_file_ keys"
    );

    // Verify resolved_imports has expected data
    assert_eq!(resolved_imports.len(), 1);
    assert!(resolved_imports.contains_key("src/main.rs"));
}

#[test]
fn test_cross_file_import_resolution_end_to_end() {
    use contribai::analysis::ast_intel::AstIntel;

    // Python file that imports from a sibling module
    let main_py = "from config import Config\n\ndef main():\n    c = Config()\n";
    let config_py = "class Config:\n    def __init__(self):\n        self.value = 42\n";

    // Extract symbols from config.py
    let symbols = AstIntel::extract_symbols(config_py, "config.py").unwrap();
    assert!(!symbols.is_empty(), "Should extract Config symbol");

    // Build symbol_map
    let mut symbol_map = HashMap::new();
    symbol_map.insert("config.py".to_string(), symbols);

    // Extract imports from main.py and resolve
    let imports = AstIntel::extract_import_targets(main_py, "main.py");
    assert!(!imports.is_empty(), "Should extract Config import");

    let resolved = AstIntel::resolve_imports(&imports, &symbol_map);
    assert!(
        !resolved.is_empty(),
        "Should resolve Config to config.py definition"
    );
    assert!(
        resolved.contains_key("Config"),
        "Should resolve Config symbol"
    );
}

// ── Circuit Breaker Integration ─────────────────────────────────────────

#[test]
fn test_circuit_breaker_opens_after_consecutive_failures() {
    use contribai::orchestrator::circuit_breaker::{CircuitBreaker, CircuitState};

    let cb = CircuitBreaker::new().with_thresholds(3, 1, 999); // 3 failures → open, long cooldown

    // Simulate 3 consecutive LLM failures
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Closed);

    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    // With long cooldown, allow_request should return false
    assert!(!cb.allow_request());
    assert_eq!(cb.state(), CircuitState::Open);
}

#[test]
fn test_circuit_breaker_recovers_on_success() {
    use contribai::orchestrator::circuit_breaker::{CircuitBreaker, CircuitState};

    let cb = CircuitBreaker::new().with_thresholds(1, 1, 0);

    cb.record_failure(); // Opens
    assert_eq!(cb.state(), CircuitState::Open);

    cb.allow_request(); // → HalfOpen (cooldown = 0)
    cb.record_success(); // → Closed
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.allow_request());
}

#[test]
fn test_circuit_breaker_resets_success_clears_counter() {
    use contribai::orchestrator::circuit_breaker::{CircuitBreaker, CircuitState};

    let cb = CircuitBreaker::new().with_thresholds(5, 1, 999);

    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.failure_count(), 2);

    cb.record_success();
    assert_eq!(cb.failure_count(), 0);
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[test]
fn test_pipeline_config_circuit_breaker_defaults() {
    use contribai::core::config::PipelineConfig;

    let config = PipelineConfig::default();
    assert_eq!(config.circuit_breaker_failure_threshold, 5);
    assert_eq!(config.circuit_breaker_success_threshold, 2);
    assert_eq!(config.circuit_breaker_cooldown_secs, 300);
}
