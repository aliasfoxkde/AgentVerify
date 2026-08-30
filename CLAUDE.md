# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

AgentVerify is an open-source outcome verification library for action-taking AI agents. It checks that high-risk actions reached the required final state in systems of record and preserves evidence in signed receipts.

**Core principle:** "Given an intended action and its expected postconditions, determine whether the external state satisfies those postconditions."

**UNKNOWN is a first-class state.** A timeout does NOT equal failure.

## Architecture

```
                    ┌──────────────────────┐
                    │        AGENT         │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │       AMORTYX        │
                    │ Cognition / Middleware│
                    └──────────┬───────────┘
                               │
                 ┌─────────────┴─────────────┐
                 │                           │
                 ▼                           ▼
       ┌──────────────────┐        ┌──────────────────┐
       │   AGENTVERIFY    │        │      ATHEON      │
       │                  │        │                  │
       │ Can we prove     │        │ Is something     │
       │ the outcome?    │        │ wrong/suspicious?│
       └──────────────────┘        └──────────────────┘
```

AgentVerify must remain independently useful—deployable without Amortyx or Atheon.

### Verification Lifecycle

```
PROPOSED → VALIDATING → AUTHORIZED → EXECUTING
                                        ↓
                              (FAILED / TIMEOUT / UNKNOWN)
                                        ↓
                                    OBSERVING
                                        ↓
                                    VERIFYING
                                        ↓
                              (VERIFIED / FAILED)
                                        ↓
                                   COMMITTED
```

### Verification Results

- `VERIFIED` — all postconditions satisfied
- `FAILED` — postconditions not met
- `UNKNOWN` — cannot determine (timeout, partial, consistency issues)
- `PARTIAL` — some postconditions met, others not
- `DUPLICATE` — action already executed

## Crate Structure

```
crates/
├── agentverify-core/       # Core types: Action, Contract, Predicate, Receipt, StateMachine
├── agentverify-contract/   # JSON/YAML contract parsing
├── agentverify-engine/     # Predicate evaluation engine
├── agentverify-runtime/    # VerifiedExecutor implementation
├── agentverify-observe/    # Observation adapters (PostgreSQL, Redis)
├── agentverify-recovery/   # Recovery strategies
├── agentverify-receipt/    # Receipt signing (Ed25519)
├── agentverify-policy/     # Policy engine with rate limiting
├── agentverify-storage/    # Storage adapters
├── agentverify-mcp/        # MCP client integration
├── agentverify-otel/       # OpenTelemetry OTLP export
├── agentverify-http/       # HTTP gateway and REST observer
├── agentverify-cli/        # CLI tool
└── agentverify-testkit/    # Testing utilities (mocks, helpers)
```

Core types live in `agentverify-core/src/`:
- `action.rs` — Action and ActionId
- `contract.rs` — Contract, Precondition, Postcondition, RecoveryConfig
- `predicate.rs` — Predicate enum and operators
- `observation.rs` — Observation, Evidence, SourceId
- `receipt.rs` — Receipt, PostconditionResult
- `state_machine.rs` — StateMachine and State enum
- `verification_result.rs` — VerificationResult enum

## MVP Scope (v0.1)

Only:
- Rust core with Action, Contract, Preconditions, Postconditions
- Observation, Verification, UNKNOWN state
- Idempotency, verify-before-retry
- PostgreSQL + REST observers
- CLI, JSON/YAML contracts
- Basic receipts, OpenTelemetry

## Key Design Principles

1. **Core must be deterministic** — no LLM, database, HTTP, or Amortyx dependencies
2. **Verify-before-retry** — never retry without verifying first
3. **Zero-trust annotations** — MCP tool annotations are hints, not proof
4. **UNKNOWN is first-class** — timeout ≠ failure
5. **Receipts for evidence** — every operation produces a structured, signable receipt

## Build Commands

```bash
cargo build                  # Build workspace
cargo build --release       # Release build
cargo test                  # Run all tests
cargo test --package agentverify-core  # Test specific crate
cargo clippy --workspace --all-targets -- -D warnings  # Lint
cargo fmt --all             # Format
cargo check                 # Fast check without building
```

## Testing Strategy

- Unit tests: core, contracts, predicates, state machines, receipts
- Integration tests: Postgres, REST, Redis, MCP, HTTP
- Property testing with `proptest`
- Failure-injection testing (timeouts, partial failures, duplicates, network failures)
- Conformance test suite for adapters

## Non-Goals (explicitly rejected)

- Replacing agent frameworks or LLMs
- Another tracing platform or generic guardrail system
- LLM judges or autonomous contract generation in v1
- Requiring users to rewrite workflows
- Becoming an orchestration monolith

## Important Files

- `docs/PLANNING.md` — Full architectural specification and 32-phase roadmap
- `README.md` — Project overview
