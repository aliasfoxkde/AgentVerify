//! AgentVerify benchmarks
//!
//! This module exists to satisfy cargo bench's requirement that
//! benches/bench.rs exists at the workspace root.

mod bench_predicate {
    criterion::criterion_group!(benches, super::predicate_equals_benchmark);
    criterion::criterion_main!(benches);
}

pub use criterion;
