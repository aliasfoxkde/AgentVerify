# AgentVerify Task Tracking

**Version:** 2.0
**Created:** 2026-08-11
**Status:** Active

---

## Overview

This document tracks all tasks for AgentVerify development. Organized by priority for MVP delivery.

---

## Progress Summary

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 0: Research & Planning | ✅ Complete | Documentation, planning, core types |
| Phase 1: Repository Structure | ✅ Complete | Cargo workspace, all crates created |
| Phase 2: Core Verification Model | ✅ Complete | Core types, state machine, verification result |
| Phase 3: Contract DSL | ✅ Complete | JSON/YAML parsing, schema validation, duplicate detection |
| Phase 4: Predicate Engine | ✅ Complete | All predicates (Exists, Equals, Contains, Matches, GreaterThan, LessThan, compound) |
| Phase 5: Runtime | ✅ Complete | Executor with verify-before-retry, bounded retry/backoff |
| Phase 6: HTTP Observer | ✅ Complete | REST observer with auth, redaction, truncation |
| Phase 7: Receipts | ✅ Complete | Ed25519 signing, SHA-256 digest, idempotency |
| Phase 8: CLI | ⚠️ Partial | `contract validate` correct exit codes; `verify` observes really but **dispatch is simulated** (`SimulatedActionExecutor`, main.rs:386) and `--max-retries` is accepted but ignored (`_max_retries`, main.rs:345) |
| Phase 9: Observers (Postgres, Redis) | ✅ Implemented | `agentverify-observe` with live-service integration tests (CI: PG 5433 / Redis 7) |
| Phase 10: Policy, recovery, storage, testkit | ✅ Implemented | crates + unit tests present; storage adapters are in-memory/file-backed, not cross-process |
| Phase 11: MCP, OTel | ✅ Implemented | `agentverify-mcp` (stdio transport, protocol), `agentverify-otel` (OTLP/gRPC); deployment boundaries unestablished |
| Phase 12: HTTP gateway | ⚠️ Partial | `ControlCenterClient` (submission-only, wiremock-tested); CLI `serve` is health/shutdown only |

**Current Focus (2026-10-10):** deterministic core evidenced through Tier C;
open work is real dispatch (P-DIS-01), receipt ingestion contract with
Control Center (P-CC-01), evidence-shape semantics (P-ENG-01), durability
(P-DUR-01) — specified in
`docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_2026-10-10.md` and tracked
below under "Handoff packets 2026-10-10". Evidence: CI run 33482496746 all
10 jobs green at `main@2b6d021` (2026-09-01); ~748 test functions counted
statically on this branch.

---

## P0 - Critical for MVP

### Complete Predicate Engine

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P0-001 | Implement `Contains` predicate | ✅ | Done — 71 engine tests |
| P0-002 | Implement `Matches` (regex) predicate | ✅ | Done |
| P0-003 | Implement `GreaterThan`, `LessThan` predicates | ✅ | Done |
| P0-004 | Implement collection predicates (`Count`, `IsEmpty`) | ✅ | Done |
| P0-005 | Implement compound predicates (`All`, `Any`, `Not`, `Implies`) | ✅ | Done |
| P0-006 | Add JSONPath support | ✅ | Done |
| P0-007 | Add `$args.` resolution in values | ✅ | Done |

### Implement Contract Parsing

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P0-008 | JSON contract loader | ✅ | Done |
| P0-009 | YAML contract loader | ✅ | Done |
| P0-010 | Contract validation | ✅ | Done |

### Implement VerifiedExecutor

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P0-011 | Precondition validation | ✅ | Done |
| P0-012 | Action execution wrapper | ✅ | Done (injectable ActionExecutor trait) |
| P0-013 | Observation collection | ✅ | Done (Observer trait) |
| P0-014 | Postcondition verification loop | ✅ | Done |
| P0-015 | Verify-before-retry logic | ✅ | Done |
| P0-016 | Idempotency key handling | ✅ | Done (process-local; atomic version P2) |

---

## P1 - Important for MVP

### CI/CD & Quality

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P1-001 | GitHub Actions CI workflow | ✅ | `.github/workflows/ci.yml` and `release.yml` exist |
| P1-002 | Clippy enforcement (`-D warnings`) | ✅ | Configured; passes in workspace |
| P1-003 | Format check in CI | ✅ | `cargo fmt --check` passes |
| P1-004 | WASM support | ⚠️ Deferred | Async Rust WASM ecosystem immaturity: tokio, async-std, and smol all depend on `polling` crate which doesn't support WASM. True WASM support requires either: (1) synchronous-only subset, (2) custom executor, or (3) wait for WASM-native async I/O |

### First Observer

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P1-005 | REST observer with auth/redaction | ✅ | Done (HTTP crate) |
| P1-006 | Observer trait definition | ✅ | Done (runtime Observer trait) |
| P1-007 | PostgresObserver implementation | ❌ | Deferred |

### Tests

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P1-008 | Property tests for predicate engine | ❌ | Deferred (proptest available) |
| P1-009 | Integration tests with testcontainers | ❌ | Deferred |
| P1-010 | Example contracts (PostgreSQL, REST) | ❌ | Deferred |

---

## P2 - Nice to Have

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P2-001 | REST Observer | ✅ | Done (HTTP crate) |
| P2-002 | Redis Observer | ❌ | Deferred |
| P2-003 | Receipt Ed25519 signing | ✅ | Done (receipt crate) |
| P2-004 | MCP proxy | ❌ | Deferred |
| P2-005 | OpenTelemetry export | ❌ | Deferred |
| P2-006 | HTTP Gateway | ❌ | Deferred |

---

## Quick Fixes (Low Effort)

| ID | Task | Status | Notes |
|----|------|--------|-------|
| QF-001 | GitHub Actions CI workflow | ✅ | Already created |
| QF-002 | Cargo-dist configuration | ❌ | Needed for WASM builds |
| QF-003 | Property tests for predicate engine | ❌ | Deferred |
| QF-004 | CLI `--max-retries` accepted but ignored | ❌ Open | Fold into P-DIS-01 (`ExecutorConfig` wiring in `agentverify-cli/src/main.rs`) |
| QF-005 | Idempotency claim leak on error paths | ✅ | 2026-10-10: every post-claim error exit resolves the claim (`resolve_claim_on_error` + `ClaimPolicy`, 10 regression tests) and `DispatchOutcome::TransportError` aligns with `DispatchError::TransportError` (Unknown verdict, key consumed) (`agentverify-runtime/src/executor.rs`); PR #26 runs 38096454733/38096769946 failed (new-test qualification + clippy-1.99 drift on main, both fixed) — post-amendment PR run is the record |

---

## Handoff packets 2026-10-10

Full specification with file-level ownership and acceptance criteria:
`docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_2026-10-10.md`. Update the
rows here; do not fork a new ledger.

| ID | Packet | Status | Notes |
|----|--------|--------|-------|
| P-DIS-01 | Real dispatch adapter (`HttpActionExecutor`) + CLI executor selection | ❌ Open | Tier-D blocker; also closes QF-004 and the "(real executor path)" comment (main.rs:391) |
| P-ENG-01 | Evidence-shape semantics decision (missing path / type mismatch / truncation → `Failed` vs unevaluable/`Unknown`; `Implies` vacuous-`Verified`) | ❌ Open | Owner decision required; changes predicate-engine semantics |
| P-UNK-01 | Observe-before-re-dispatch on `Unknown` retry arm | ❌ Open | AGENTS.md verify-before-retry invariant |
| P-CC-01 | Versioned receipt ingestion contract + rejection fixtures (orphan/stale/tampered/unauthorized/cross-project/replayed) | ❌ Open | Tier-D blocker; co-owner: Control Center |
| P-DUR-01 | Cross-process `IdempotencyStore` + durable `ReceiptStore` (Postgres), restart/replay tests | ❌ Open | Model on `agentverify-observe/src/postgres.rs` |
| P-OPS-01 | crates.io publication of v0.1.0 | ❌ Blocked (user) | `CARGO_REGISTRY_TOKEN` missing — release run 33324170554 skipped publish and exited 1; GitHub Release v0.1.0 assets are published |

---

## Completed Tasks

### Phase 0: Research & Planning ✅

- [x] Create CLAUDE.md
- [x] Audit Platform-Architecture
- [x] Create ARCHITECTURE.md
- [x] Create CONCEPTS.md
- [x] Create CLI.md
- [x] Create INTEGRATIONS.md
- [x] Create COMPETITIVE_ANALYSIS.md
- [x] Create NEXT_STEPS.md (gap analysis)

### Phase 1: Repository Structure ✅

- [x] Create Cargo workspace
- [x] Create all 14 crates
- [x] Setup workspace dependencies
- [x] Create .claude/rules/quality.md
- [x] Create .claude/rules/commit.md
- [x] Create .claude/settings.json

### Phase 2: Core Types ✅

- [x] Action struct + ActionId
- [x] Contract struct + ContractId
- [x] Predicate enum (full implementation)
- [x] StateMachine + State enum
- [x] VerificationResult enum (Verified, Failed, Unknown, Partial, Duplicate)
- [x] Observation + Evidence + SourceId
- [x] Receipt + PostconditionResult
- [x] IdempotencyKey
- [x] All types have serde derives
- [x] 25 unit tests for core

### Phase 3: Contract DSL ✅

- [x] JSON contract loader
- [x] YAML contract loader
- [x] Schema validation
- [x] Duplicate postcondition detection
- [x] Recovery config validation
- [x] 21 unit tests for contract

### Phase 4: Predicate Engine ✅

- [x] Exists predicate
- [x] NotExists predicate
- [x] Equals predicate
- [x] Contains predicate
- [x] Matches (regex) predicate
- [x] GreaterThan, LessThan predicates
- [x] Collection predicates (Count, IsEmpty)
- [x] Compound predicates (All, Any, Not, Implies)
- [x] JSONPath support
- [x] `$args.` resolution
- [x] 71 unit tests

---

## Key Decisions

| Decision | Rationale | Date |
|----------|-----------|------|
| UNKNOWN as first-class state | Timeout ≠ failure per research | 2026-08-11 |
| Rust-first | Deterministic core, no runtime deps | 2026-08-11 |
| MCP as first-class | Best interception point | 2026-08-11 |
| Zero-trust annotations | MCP hints unreliable | 2026-08-11 |
| Verify-before-retry | Always verify state before retry | 2026-08-11 |

---

## Progress Timeline

| Date | Milestone | Status |
|------|-----------|--------|
| 2026-08-11 | Phase 0-1 complete | ✅ |
| 2026-08-11 | Core types + tests | ✅ |
| 2026-08-14 | Full predicate engine | ✅ |
| 2026-08-14 | Contract parsing | ✅ |
| 2026-08-14 | VerifiedExecutor | ✅ |
| 2026-08-15 | CI/CD GitHub Actions | ✅ | `.github/workflows/ci.yml` and `release.yml` |
| 2026-08-30 | v0.1.0 GitHub Release (6 targets + WASI + checksums) | ✅ | Release run 33324170554; crates.io publish skipped — no `CARGO_REGISTRY_TOKEN` |
| 2026-09-01 | CI green at `main@2b6d021` (10/10 jobs incl. Test with PG/Redis services) | ✅ | Run 33482496746 |
| 2026-10-10 | Platform handoff + idempotency claim-leak fix | 🔄 | `docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_2026-10-10.md`; PR #26 — also unblocks main's clippy-1.99/rustls drift (16 `assert_is_empty` sites, rustls 0.23.45). Review round 2 (2026-10-10): full post-claim error-exit sweep + `DispatchOutcome::TransportError` → Unknown/consumed, 10 regression tests |
| 2026-10-11 | Next-phase readiness handoff: utility proof ranked ahead of polish | 🔄 | `docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_NEXT_PHASE_2026-10-11.md`; PR #30 (draft) — audits dispatch/idempotency/fail-closed at `main@f55e171`, accounts for PR stack #27→#29, re-ranks next phase around the executable utility proof (P1-3 real sandboxed dispatch → P1-4 honest benchmark → P1-5 CC receipt/lifecycle integration → P1-6 live multi-turn E2E); idempotency polish demoted to P2. Crate inventory corrected to 14 (`cargo metadata --no-deps`) |
| TBD | PostgresObserver | ✅ | `agentverify-observe` implemented with live-service tests |

---

## Legend

- [x] = Done
- [ ] = Not started
- 🔄 = In progress
- 🚧 = Blocked