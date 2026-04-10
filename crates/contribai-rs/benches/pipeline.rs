//! Pipeline benchmarks for ContribAI.
//!
//! Run with: `cargo bench`
//!
//! These benchmarks measure performance of hot paths:
//! - AST symbol extraction
//! - Framework detection
//! - Risk classification
//! - Quality scoring

use contribai::analysis::ast_intel::AstIntel;
use contribai::generator::risk::classify_risk;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// ── AST Symbol Extraction Benchmarks ────────────────────────────────────────

fn bench_ast_extraction_python(c: &mut Criterion) {
    let code = include_str!("../tests/fixtures/sample.py");
    c.bench_function("ast_extract_python", |b| {
        b.iter(|| AstIntel::extract_symbols(black_box(code), black_box("sample.py")))
    });
}

fn bench_ast_extraction_rust(c: &mut Criterion) {
    let code = include_str!("../tests/fixtures/sample.rs");
    c.bench_function("ast_extract_rust", |b| {
        b.iter(|| AstIntel::extract_symbols(black_box(code), black_box("sample.rs")))
    });
}

fn bench_ast_extraction_javascript(c: &mut Criterion) {
    let code = include_str!("../tests/fixtures/sample.js");
    c.bench_function("ast_extract_javascript", |b| {
        b.iter(|| AstIntel::extract_symbols(black_box(code), black_box("sample.js")))
    });
}

// ── Framework Detection Benchmarks ──────────────────────────────────────────

fn bench_framework_detection(c: &mut Criterion) {
    use std::collections::HashMap;

    let imports = HashMap::from([
        (
            "views.py".to_string(),
            vec!["django.http".to_string(), "django.shortcuts".to_string()],
        ),
        ("models.py".to_string(), vec!["django.db".to_string()]),
        ("urls.py".to_string(), vec!["django.urls".to_string()]),
    ]);

    c.bench_function("framework_detection_django", |b| {
        b.iter(|| {
            // Simulate framework detection logic
            let all_imports: std::collections::HashSet<String> =
                imports.values().flat_map(|i| i.iter().cloned()).collect();
            let mut frameworks = std::collections::HashSet::new();
            if all_imports.iter().any(|i| i.contains("django")) {
                frameworks.insert("django".to_string());
            }
            black_box(frameworks)
        })
    });
}

// ── Risk Classification Benchmarks ──────────────────────────────────────────

fn bench_risk_classification(c: &mut Criterion) {
    c.bench_function("risk_docs_change", |b| {
        b.iter(|| {
            classify_risk(
                black_box("docs"),
                black_box(&["README.md".to_string()]),
                black_box(10),
            )
        })
    });

    c.bench_function("risk_security_fix", |b| {
        b.iter(|| {
            classify_risk(
                black_box("security"),
                black_box(&["auth.py".to_string()]),
                black_box(50),
            )
        })
    });

    c.bench_function("risk_refactor_high", |b| {
        b.iter(|| {
            classify_risk(
                black_box("refactor"),
                black_box(&[
                    "auth.py".to_string(),
                    "db.py".to_string(),
                    "api.py".to_string(),
                ]),
                black_box(200),
            )
        })
    });
}

// ── Benchmark Group Configuration ───────────────────────────────────────────

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(100);
    targets = bench_ast_extraction_python,
              bench_ast_extraction_rust,
              bench_ast_extraction_javascript,
              bench_framework_detection,
              bench_risk_classification
);

criterion_main!(benches);
