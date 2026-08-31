//! Benchmark tests for `AgentVerify` predicate engine

use agentverify_core::Predicate;
use agentverify_engine::PredicateEngine;
use criterion::{criterion_main, Criterion, Throughput};

/// Benchmark `Equals` predicate evaluation against a flat state document.
pub fn predicate_equals_benchmark(c: &mut Criterion) {
    let engine = PredicateEngine::default();
    let state = serde_json::json!({
        "value": 42,
        "name": "test",
        "count": 100
    });
    let predicate = Predicate::Equals {
        path: "value".into(),
        value: serde_json::json!(42),
    };

    let mut group = c.benchmark_group("predicate_evaluation");
    group.throughput(Throughput::Elements(1));
    group.bench_function("equals_matching", |b| {
        b.iter(|| {
            engine.evaluate(
                std::hint::black_box(&predicate),
                std::hint::black_box(&state),
                std::hint::black_box(&serde_json::json!({})),
            )
        });
    });
}

/// Benchmark `Exists` predicate evaluation against a nested state document.
pub fn predicate_exists_benchmark(c: &mut Criterion) {
    let engine = PredicateEngine::default();
    let state = serde_json::json!({
        "value": 42,
        "nested": {
            "key": "exists"
        }
    });
    let predicate = Predicate::Exists {
        path: "nested.key".into(),
    };

    let mut group = c.benchmark_group("predicate_exists");
    group.throughput(Throughput::Elements(1));
    group.bench_function("exists_found", |b| {
        b.iter(|| {
            engine.evaluate(
                std::hint::black_box(&predicate),
                std::hint::black_box(&state),
                std::hint::black_box(&serde_json::json!({})),
            )
        });
    });
}

/// Benchmark `Matches` predicate evaluation using a realistic email regex.
pub fn predicate_regex_benchmark(c: &mut Criterion) {
    let engine = PredicateEngine::default();
    let state = serde_json::json!({
        "email": "user@example.com"
    });
    let predicate = Predicate::Matches {
        path: "email".into(),
        pattern: r"^[\w.-]+@[\w.-]+\.\w+$".into(),
    };

    let mut group = c.benchmark_group("predicate_regex");
    group.throughput(Throughput::Elements(1));
    group.bench_function("regex_matching", |b| {
        b.iter(|| {
            engine.evaluate(
                std::hint::black_box(&predicate),
                std::hint::black_box(&state),
                std::hint::black_box(&serde_json::json!({})),
            )
        });
    });
}

// The `criterion_group!` macro expands to a `pub fn` without a doc comment,
// which the workspace-wide `missing_docs` lint would otherwise reject. The
// generated group is therefore isolated in a module that opts out of it.
#[allow(missing_docs)]
mod bench_groups {
    use super::{
        predicate_equals_benchmark, predicate_exists_benchmark, predicate_regex_benchmark,
    };
    use criterion::criterion_group;

    criterion_group!(
        benches,
        predicate_equals_benchmark,
        predicate_exists_benchmark,
        predicate_regex_benchmark
    );
}

criterion_main!(bench_groups::benches);
