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
| Phase 2: Core Verification Model | ⚠️ Partial | Core types done, need executor + predicates |
| Phase 3: Contract DSL | ❌ Not Started | Parsing not implemented |
| Phase 4: Predicate Engine | ⚠️ Partial | Basic predicates only |
| Phase 5-12 | ❌ Not Started | Observers, MCP, HTTP, etc. |

**Current Focus:** Making the MVP actually functional

---

## P0 - Critical for MVP

### Complete Predicate Engine

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P0-001 | Implement `Contains` predicate | ❌ | Missing |
| P0-002 | Implement `Matches` (regex) predicate | ❌ | Missing |
| P0-003 | Implement `GreaterThan`, `LessThan` predicates | ❌ | Missing |
| P0-004 | Implement collection predicates (`Count`, `IsEmpty`) | ❌ | Missing |
| P0-005 | Implement compound predicates (`All`, `Any`, `Not`, `Implies`) | ❌ | Missing |
| P0-006 | Add JSONPath support | ❌ | Missing |
| P0-007 | Add `$args.` resolution in values | ❌ | Missing |

### Implement Contract Parsing

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P0-008 | JSON contract loader | ❌ | Missing |
| P0-009 | YAML contract loader | ❌ | Missing |
| P0-010 | Contract validation | ❌ | Missing |

### Implement VerifiedExecutor

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P0-011 | Precondition validation | ❌ | Missing |
| P0-012 | Action execution wrapper | ❌ | Placeholder only |
| P0-013 | Observation collection | ❌ | Missing |
| P0-014 | Postcondition verification loop | ❌ | Missing |
| P0-015 | Verify-before-retry logic | ❌ | Missing |
| P0-016 | Idempotency key handling | ❌ | Missing |

---

## P1 - Important for MVP

### CI/CD & Quality

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P1-001 | GitHub Actions CI workflow | ❌ | Missing |
| P1-002 | Clippy enforcement (`-D warnings`) | ❌ | Not configured |
| P1-003 | Format check in CI | ❌ | Not configured |
| P1-004 | Cargo-dist configuration | ❌ | Missing |

### First Observer

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P1-005 | PostgresObserver implementation | ❌ | Missing |
| P1-006 | Observer trait definition | ❌ | Missing |

### Tests

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P1-007 | Property tests for predicate engine | ❌ | Missing (proptest available) |
| P1-008 | Integration tests with testcontainers | ❌ | Missing |
| P1-009 | Example contracts (PostgreSQL, REST) | ❌ | Missing |

---

## P2 - Nice to Have

| ID | Task | Status | Notes |
|----|------|--------|-------|
| P2-001 | REST Observer | ❌ | Missing |
| P2-002 | Redis Observer | ❌ | Missing |
| P2-003 | Receipt Ed25519 signing | ❌ | Missing |
| P2-004 | MCP proxy | ❌ | Missing |
| P2-005 | OpenTelemetry export | ❌ | Missing |
| P2-006 | HTTP Gateway | ❌ | Missing |

---

## Quick Fixes (Low Effort)

| ID | Task | Status | Notes |
|----|------|--------|-------|
| QF-001 | Remove unused `CompareOperator` enum | ❌ | Dead code |
| QF-002 | Fix unused variable warnings | ❌ | `_args`, `_action` |
| QF-003 | Add more unit tests | 🔄 | Could always use more |

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

### Phase 2: Core Types (Partial) ✅

- [x] Action struct + ActionId
- [x] Contract struct + ContractId
- [x] Predicate enum (basic only)
- [x] StateMachine + State enum
- [x] VerificationResult enum
- [x] Observation + Evidence + SourceId
- [x] Receipt + PostconditionResult
- [x] All types have serde derives
- [x] 12 unit tests for core

### Phase 4: Predicate Engine (Partial) ✅

- [x] Exists predicate
- [x] NotExists predicate
- [x] Equals predicate
- [x] 2 unit tests

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
| TBD | Full predicate engine | ❌ |
| TBD | Contract parsing | ❌ |
| TBD | VerifiedExecutor | ❌ |
| TBD | CI/CD | ❌ |
| TBD | PostgresObserver | ❌ |

---

## Legend

- [x] = Done
- [ ] = Not started
- 🔄 = In progress
- 🚧 = Blocked