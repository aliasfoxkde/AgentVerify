# AgentVerify Quality Rules

## Rust Standards

### Error Handling
- **No `unwrap()` in production code** — use `?` or proper error handling with `Result`
- **No `panic!()` in library code** — return `Result` instead
- **No `unsafe` code** without explicit review and safety documentation
- Use `thiserror` for error types
- Use `anyhow` for application errors

### Types
- Use strongly-typed IDs (`ActionId`, `ContractId`, `ReceiptId`) over raw strings
- Use `chrono.DateTime<Utc>` for timestamps
- Use `serde_json::Value` for unstructured JSON
- Prefer `Vec<u8>` over `String` for binary data

### Async
- All I/O operations are async (using `tokio`)
- Use `#[tokio::test]` for async tests
- Avoid blocking in async context

## Testing

### Test Structure
- Unit tests in `#[cfg(test)]` modules within each source file
- Integration tests in `tests/` directory
- Property tests using `proptest`

### Coverage
- Minimum 90% line coverage for core crates
- 100% coverage for predicate engine
- Critical paths require dedicated tests

### Test Naming
```
#[test]
fn verify_postcondition_equals_success() { ... }

#[test]
fn verify_postcondition_equals_failure() { ... }

#[test]
fn verify_unknown_timeout_handled_correctly() { ... }
```

## Linting

### Required Checks
```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test --workspace
cargo audit
```

### Clippy Rules
- `clippy::all` enabled
- `clippy::pedantic` for core crates
- Allow `unwrap_used` only in tests with `#![allow(clippy::unwrap_used)]`

## Anti-Patterns

### Forbidden
- **No versioned files:** `file_v1.rs`, `file_v2.rs`
- **No placeholder code:** `TODO`, `FIXME`, `HACK`
- **No duplicate implementations** — extract to shared function
- **No magic numbers** — use named constants

### Recommended
- Explicit over implicit
- Small functions with single responsibility
- Document public API with doc comments
- Log at appropriate levels (error, warn, info, debug, trace)

## Performance

### Targets
- Local verification (pure predicates): <1ms
- In-process observation: ~1-5ms
- Memory allocation in hot paths: avoid

### Benchmarks
- Run benchmarks with `cargo bench`
- Track p50, p95, p99 latencies
- Add benchmark for any optimization

## Documentation

### Required Documentation
- All public API items must have doc comments
- Complex algorithms require doc comments with complexity analysis
- Crate-level docs in `lib.rs`

### Format
```rust
/// Verifies that a postcondition is satisfied.
///
/// # Arguments
/// * `predicate` - The predicate to evaluate
/// * `state` - The observed state
///
/// # Returns
/// `Ok(true)` if satisfied, `Ok(false)` if not, `Err(...)` on error
///
/// # Complexity
/// O(n) where n is the number of predicates in compound expressions
pub fn verify(&self, predicate: &Predicate, state: &Value) -> Result<bool> {
    // ...
}
```
