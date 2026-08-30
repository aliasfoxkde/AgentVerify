//! Benchmark tests for AgentVerify predicate engine

use agentverify_core::Predicate;
use agentverify_engine::PredicateEngine;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

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
                black_box(&predicate),
                black_box(&state),
                black_box(&serde_json::json!({})),
            )
        });
    });
}

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
                black_box(&predicate),
                black_box(&state),
                black_box(&serde_json::json!({})),
            )
        });
    });
}

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
                black_box(&predicate),
                black_box(&state),
                black_box(&serde_json::json!({})),
            )
        });
    });
}

criterion_group!(
    benches,
    predicate_equals_benchmark,
    predicate_exists_benchmark,
    predicate_regex_benchmark
);
criterion_main!(benches);
