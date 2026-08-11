# AgentVerify Task Tracking

**Version:** 1.0
**Created:** 2026-08-11
**Status:** Active

---

## Overview

This document tracks all tasks for AgentVerify development. Tasks are organized by phase.

---

## Phase 0 — Research & Specification (Current)

### Completed

- [x] Create initial CLAUDE.md
- [x] Audit Platform-Architecture for AgentVerify context
- [x] Create ARCHITECTURE.md
- [x] Create CONCEPTS.md
- [x] Create CLI.md
- [x] Create INTEGRATIONS.md
- [x] Create COMPETITIVE_ANALYSIS.md
- [x] Create TASKS.md (this document)
- [x] Create project structure scaffolding

### In Progress

- [ ] Create quality rules
- [ ] Create commit guidelines
- [ ] Setup Cargo workspace skeleton

### TODO

- [ ] Finalize contract DSL specification
- [ ] Define predicate engine API
- [ ] Research failure injection testing approach
- [ ] Document formal invariants
- [ ] Create example contracts (PostgreSQL, REST)

---

## Phase 1 — Repository & Rust Foundation

- [ ] Create Cargo workspace (`Cargo.toml`)
- [ ] Create `agentverify-core` crate
- [ ] Create `agentverify-contract` crate
- [ ] Create `agentverify-engine` crate
- [ ] Create `agentverify-runtime` crate
- [ ] Create `agentverify-cli` crate
- [ ] Setup CI/CD (GitHub Actions)
- [ ] Setup `cargo-dist` for releases

---

## Phase 2 — Core Verification Model

- [ ] Define `Action` struct
- [ ] Define `Contract` struct
- [ ] Define `Predicate` enum with basic predicates
- [ ] Implement state machine
- [ ] Implement `VerificationResult` enum
- [ ] Write unit tests for core

---

## Phase 3 — Contract DSL

- [ ] JSON contract parser
- [ ] YAML contract parser
- [ ] Rust API for contracts
- [ ] Contract validation
- [ ] Contract versioning

---

## Phase 4 — Predicate Engine

- [ ] Basic predicates (exists, equals, contains, matches)
- [ ] Collection predicates (count, isEmpty)
- [ ] Compound predicates (all, any, not)
- [ ] JSONPath support
- [ ] CEL-like expressions

---

## Phase 5 — Observers

- [ ] PostgreSQL observer
- [ ] REST observer
- [ ] Redis observer
- [ ] Observer trait for custom implementations

---

## Phase 6 — VerifiedExecutor

- [ ] Executor implementation
- [ ] Verify-before-retry logic
- [ ] Idempotency handling
- [ ] Timeout handling

---

## Phase 7 — Receipts

- [ ] Receipt structure
- [ ] Ed25519 signing
- [ ] Receipt verification
- [ ] Receipt storage

---

## Phase 8 — MCP Integration

- [ ] MCP proxy implementation
- [ ] Tool-to-contract mapping
- [ ] MCP server implementation
- [ ] Annotation handling

---

## Phase 9 — OpenTelemetry

- [ ] Trace emission
- [ ] Metric emission
- [ ] Span correlation
- [ ] GenAI conventions compliance

---

## Phase 10 — HTTP Gateway

- [ ] REST API
- [ ] WebSocket/SSE
- [ ] Health endpoints
- [ ] Metrics endpoint

---

## Phase 11 — Amortyx Integration

- [ ] Amortyx middleware component
- [ ] Context passing
- [ ] Routing integration

---

## Phase 12 — Recovery

- [ ] Retry strategy
- [ ] Compensate strategy
- [ ] Escalate strategy
- [ ] Backoff configuration

---

## Future Phases

| Phase | Focus | Status |
|-------|-------|--------|
| 13 | Eventual consistency | Planned |
| 14 | Partial success | Planned |
| 15 | Sagas/compensation | Planned |
| 16 | Concurrency control | Planned |
| 17 | Security hardening | Planned |
| 18 | Performance optimization | Planned |
| 19 | Failure injection testing | Planned |
| 20 | Conformance test suite | Planned |
| 21 | Python SDK | Planned |
| 22 | TypeScript SDK | Planned |
| 23 | Cross-platform binaries | Planned |
| 24 | Benchmark suite | Planned |

---

## Task Properties

Each task should track:

| Property | Description |
|----------|-------------|
| **ID** | Unique identifier (e.g., T-001) |
| **Title** | Brief description |
| **Status** | Not started, In progress, Blocked, Done |
| **Phase** | Which phase it belongs to |
| **Blocked by** | Dependencies |
| **Priority** | P0 (critical), P1 (important), P2 (nice to have) |
| **Estimate** | Relative effort (S, M, L, XL) |

---

## Key Decisions

| Decision | Rationale | Date |
|----------|-----------|------|
| UNKNOWN as first-class state | Timeout ≠ failure per research | 2026-08-11 |
| Rust-first | Deterministic core, no runtime deps | 2026-08-11 |
| MCP as first-class | Best interception point | 2026-08-11 |
| Zero-trust annotations | MCP hints unreliable | 2026-08-11 |

---

## Progress

| Date | Phase | Milestone | Status |
|------|-------|-----------|--------|
| 2026-08-11 | 0 | Documentation & planning | In Progress |

---

## Legend

- [x] = Done
- [ ] = Not started
- 🔄 = In progress
- 🚧 = Blocked
