# AgentVerify - Gaps Analysis & Next Steps

**Version:** 1.0
**Created:** 2026-08-11
**Status:** Analysis Complete

---

## Current State Summary

| Crate | Lines | Status |
|-------|-------|--------|
| agentverify-core | 1118 | ✅ Complete (core types, state machine, predicates) |
| agentverify-contract | 10 | ❌ Placeholder (just re-exports) |
| agentverify-engine | 130 | ⚠️ Partial (basic predicates only) |
| agentverify-runtime | 35 | ❌ Placeholder (executor stub) |
| agentverify-cli | 70 | ⚠️ Basic (clap skeleton) |
| agentverify-observe | 0 | ❌ Empty |
| agentverify-recovery | 0 | ❌ Empty |
| agentverify-receipt | 0 | ❌ Empty |
| agentverify-policy | 0 | ❌ Empty |
| agentverify-storage | 0 | ❌ Empty |
| agentverify-mcp | 0 | ❌ Empty |
| agentverify-otel | 0 | ❌ Empty |
| agentverify-http | 0 | ❌ Empty |
| agentverify-testkit | 0 | ❌ Empty |

**Total Rust Code:** 1,462 lines
**Test Coverage:** 22 unit tests (core + engine)
**Workspace Status:** Compiles, tests pass

---

## Critical Gaps

### 1. Contract Parsing (agentverify-contract)
**Gap:** Only 10 lines, just re-exports. No JSON/YAML parsing.
**Impact:** Cannot load contracts from files.
**Priority:** P0
**Next Step:** Implement `serde` serialization for Contract types + YAML/JSON parsers.

### 2. Full Predicate Engine (agentverify-engine)
**Gap:** Only `Exists`, `NotExists`, `Equals` implemented. Missing:
- `Contains`, `Matches`, `GreaterThan`, `LessThan`
- Collection predicates: `Count`, `IsEmpty`, `IsNotEmpty`
- Compound predicates: `All`, `Any`, `Not`, `Implies`
- JSONPath resolution
**Priority:** P0
**Next Step:** Implement remaining predicates + JSONPath support.

### 3. VerifiedExecutor (agentverify-runtime)
**Gap:** Placeholder executor that returns `Verified` always.
**Impact:** Core runtime logic missing.
**Priority:** P0
**Next Step:** Implement actual:
- Precondition validation
- Action execution (mock)
- Observation collection
- Postcondition verification
- Verify-before-retry logic
- Idempotency handling

### 4. CI/CD Pipeline
**Gap:** No GitHub Actions, no automated checks.
**Impact:** No quality enforcement.
**Priority:** P1
**Next Step:** Create `.github/workflows/ci.yml` with:
- `cargo test`
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt -- --check`

### 5. Observer Implementations
**Gap:** All observer crates empty.
**Impact:** Cannot actually verify anything.
**Priority:** P1 (for MVP)
**Next Step:** Implement `PostgresObserver` first (most important per planning).

### 6. Property Tests
**Gap:** No `proptest` usage despite being in dependencies.
**Impact:** No deterministic testing of predicate evaluation.
**Priority:** P2
**Next Step:** Add property tests for predicate engine.

---

## Recommended Priority Order

### Phase 1: Make it Functional (MVP)

1. **Complete predicate engine** (all predicate types + JSONPath)
2. **Implement contract parsing** (JSON + YAML loaders)
3. **Implement VerifiedExecutor** (with verify-before-retry)
4. **Add PostgresObserver** (first real observer)

### Phase 2: Quality Enforcement

5. **Setup CI/CD** (clippy, fmt, test)
6. **Add property tests** (predicate engine)
7. **Add integration tests** (with testcontainers)

### Phase 3: Expand Capabilities

8. **REST observer**
9. **Redis observer**
10. **Receipt signing** (Ed25519)
11. **MCP proxy**
12. **OpenTelemetry export**

---

## Quick Wins (Low Effort, High Value)

| Task | Effort | Impact |
|------|--------|--------|
| Add `serde` derives to all public types | Low | Required for any serialization |
| Implement missing predicates | Medium | Core functionality |
| Add `cargo-dist` config | Low | Release automation |
| Create example contracts | Low | Documentation + testing |
| Add more unit tests | Medium | Coverage + confidence |

---

## Technical Debt

1. **`CompareOperator` enum unused** - Defined in `predicate.rs` but never used
2. **Unused variables** - `_args`, `_action`, `_contract` warnings
3. **`chrono::Duration` in public API** - Consider `std::time::Duration` for better ergonomics
4. **No error types in most crates** - Using `thiserror` would help

---

## Files Needing Attention

```
crates/agentverify-core/src/predicate.rs     # Missing predicate implementations
crates/agentverify-contract/src/             # Needs full implementation
crates/agentverify-runtime/src/executor.rs   # Placeholder
crates/agentverify-cli/src/main.rs          # CLI skeleton only
```
