# AgentVerify - Gaps Analysis & Next Steps

**Version:** 1.0
**Created:** 2026-08-11
**Status:** Analysis Complete

---

## Current State Summary

| Crate | Status | Notes |
|-------|--------|-------|
| agentverify-core | ✅ Complete | Core types, state machine, predicates, verification result |
| agentverify-contract | ✅ Complete | JSON/YAML parsing, schema validation |
| agentverify-engine | ✅ Complete | Full predicate engine with compound predicates |
| agentverify-runtime | ✅ Complete | Executor with verify-before-retry, idempotency |
| agentverify-http | ✅ Complete | REST observer with auth, redaction |
| agentverify-receipt | ✅ Complete | Ed25519 signing, SHA-256 digest |
| agentverify-cli | ⚠️ Partial | validate/verify work; dispatch simulated (see P2) |
| agentverify-observe | ❌ Deferred | Placeholder crate |
| agentverify-recovery | ❌ Deferred | Placeholder crate |
| agentverify-policy | ❌ Deferred | Placeholder crate |
| agentverify-storage | ❌ Deferred | Placeholder crate |
| agentverify-mcp | ❌ Deferred | Placeholder crate |
| agentverify-otel | ❌ Deferred | Placeholder crate |
| agentverify-testkit | ❌ Deferred | Placeholder crate |

**Evidence boundary:** 142 tests passing (core 25, contract 21, engine 71, HTTP 11, receipt 3, runtime 11)
**Workspace status:** Compiles, tests pass, fmt clean, clippy clean, doc builds
**Tier A–C coverage:** Local deterministic semantics, executor behavior, observer/signature behavior
**Tier D:** Control Center integration — open (requires P5)

---

## Critical Gaps

### 1. Real Dispatch in CLI (agentverify-cli)
**Gap:** CLI `verify` command calls `Executor::execute()` which simulates dispatch.
**Impact:** Cannot prove actual action execution through documented CLI path.
**Priority:** P0 (Packet P2)
**Next Step:** Inject a real `ActionExecutor` adapter; add dry-run mode.

### 2. Atomic Idempotency
**Gap:** Current idempotency is check-then-insert (race-prone) and process-local.
**Impact:** Concurrent requests with same key may double-dispatch.
**Priority:** P0 (Packet P2)
**Next Step:** Implement atomic claim/complete abstraction with TTL and cross-process semantics.

### 3. Durable Receipt Persistence
**Gap:** `ReceiptStore` trait exists but is not wired into executor lifecycle.
**Impact:** Receipts are lost when process exits.
**Priority:** P0 (Packet P3)
**Next Step:** Wire `ReceiptStore` into executor; implement a durable adapter.

### 4. Control Center Integration
**Gap:** No authenticated cross-process correlation and promotion fixture.
**Impact:** Cannot prove Tier-D promotion boundary.
**Priority:** P1 (Packet P5)
**Next Step:** Implement authenticated fixture that rejects orphan, stale, tampered results.

### 5. CI/CD Pipeline
**Gap:** No GitHub Actions workflow.
**Impact:** No automated quality gates on PRs.
**Priority:** P1
**Next Step:** Create `.github/workflows/ci.yml` with test/clippy/fmt/audit gates.

### 6. Property Tests
**Gap:** No `proptest` usage despite being in dependencies.
**Impact:** No deterministic testing of predicate evaluation across wide input range.
**Priority:** P2
**Next Step:** Add property tests for predicate engine.

---

## Recommended Priority Order

### Phase 1: Make it Functional (MVP) — ✅ COMPLETE

1. ✅ **Complete predicate engine** (all predicate types + compound predicates)
2. ✅ **Implement contract parsing** (JSON + YAML loaders, schema validation)
3. ✅ **Implement VerifiedExecutor** (with verify-before-retry, bounded retry/backoff)
4. ✅ **REST observer** (with auth, redaction, truncation)

### Phase 2: Production Hardening

5. **Real CLI dispatch** (injectable ActionExecutor, dry-run mode) — Packet P2
6. **Atomic idempotency** (claim/complete, TTL, cross-process) — Packet P2
7. **Durable receipt persistence** (wire ReceiptStore into executor) — Packet P3
8. **Setup CI/CD** (clippy, fmt, test, audit gates)
9. **Add property tests** (predicate engine via proptest)
10. **Add integration tests** (with testcontainers)

### Phase 3: Control Center Integration

11. **Authenticated mock server fixture** — Packet P4
12. **Control Center correlation and promotion fixture** — Packet P5
13. **PostgresObserver** (first durable observer)
14. **MCP proxy**
15. **OpenTelemetry export**

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

1. **CLI dispatch is simulated** - `verify` command does not use injected ActionExecutor
2. **Idempotency is process-local** - `IdempotencyRegistry` does not persist across restarts
3. **ReceiptStore not wired** - Trait exists but not integrated into executor lifecycle
4. **`chrono::Duration` in public API** - Consider `std::time::Duration` for better ergonomics
5. **cargo audit warning** - `rustls-pemfile 1.0.4` is unmaintained (RUSTSEC-2025-0134); update `reqwest` or add override

---

## Files Needing Attention

```
crates/agentverify-cli/src/main.rs           # P2: add real ActionExecutor injection
crates/agentverify-runtime/src/executor.rs   # P2/P3: wire ReceiptStore and idempotency hardening
crates/agentverify-runtime/src/receipt_store.rs  # P3: implement durable adapter
```
