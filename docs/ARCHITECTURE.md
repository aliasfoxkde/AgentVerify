# AgentVerify Architecture

**Version:** 1.0
**Created:** 2026-08-11
**Status:** Planning

---

## Overview

AgentVerify is an outcome verification library for action-taking AI agents. It verifies that high-risk actions reached their required final state in systems of record and preserves evidence in signed receipts.

**Core Question:** "Given an intended action and its expected postconditions, did the external state satisfy those postconditions?"

---

## Core Principle

> **UNKNOWN must be a first-class state.**
> A timeout does NOT equal failure.

This distinguishes AgentVerify from naive "query the database afterward" implementations and aligns with recent research on verified tool calls.

---

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│                        AGENT                                 │
│                    (Claude, GPT, Custom)                     │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                      AMORTYX                                 │
│              Cognition / Middleware Layer                    │
│           routing · policy · orchestration                    │
└─────────────────────────┬───────────────────────────────────┘
                          │
          ┌───────────────┴───────────────┐
          │                               │
          ▼                               ▼
┌─────────────────────┐     ┌─────────────────────┐
│    AGENTVERIFY      │     │      ATHEON         │
│                     │     │                     │
│ "Did the intended   │     │ "Does this behavior │
│  outcome happen?"    │     │  look wrong?"       │
│                     │     │                     │
│ postconditions      │     │ patterns            │
│ verification        │     │ anomalies           │
│ reconciliation      │     │ quality gates       │
│ receipts            │     │                     │
└─────────┬───────────┘     └──────────┬──────────┘
          │                              │
          └──────────────┬───────────────┘
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                    EXISTING WORLD                           │
│         PostgreSQL · REST APIs · Redis · MCP · S3           │
└─────────────────────────────────────────────────────────────┘
```

---

## Key Distinction: AgentVerify vs Atheon

| Aspect | AgentVerify | Atheon |
|--------|-------------|--------|
| Question | "Did it happen?" | "Does it look wrong?" |
| Focus | Postconditions, outcomes | Patterns, anomalies |
| Output | VERIFIED / FAILED / UNKNOWN | Violations, risk scores |
| Trigger | After action execution | Continuous monitoring |

---

## Verification Lifecycle

```
PROPOSED
    │
    ▼
VALIDATING
    │
    ├── REJECTED (contract invalid)
    │
    ▼
AUTHORIZED
    │
    ▼
EXECUTING
    │
    ├── FAILED (execution error)
    │
    ├── TIMEOUT (no response)
    │
    └── UNKNOWN (ambiguous result)
    │
    ▼
OBSERVING
    │
    ▼
VERIFYING
    │
    ├── VERIFIED (all postconditions met)
    │
    └── FAILED (postconditions not met)
    │
    ▼
COMMITTED
```

---

## Verification Results

| Result | Meaning |
|--------|---------|
| `VERIFIED` | All postconditions satisfied |
| `FAILED` | Postconditions not met |
| `UNKNOWN` | Cannot determine (timeout, partial, consistency issues) |
| `PARTIAL` | Some postconditions met, others not |
| `DUPLICATE` | Action already executed (idempotency) |

---

## Crate Architecture

```
agentverify/
├── agentverify-core/          # Pure Rust, zero network deps
│   ├── Action                 # Action definition
│   ├── Contract               # Pre/postconditions
│   ├── Observation            # State capture
│   ├── VerificationResult     # Outcome enum
│   └── StateMachine           # Lifecycle management
│
├── agentverify-contract/      # Contract DSL
│   ├── JsonContract           # JSON format
│   ├── YamlContract           # YAML format
│   └── RustApi                # Programmatic API
│
├── agentverify-engine/        # Predicate engine
│   ├── Predicates             # exists, equals, contains, etc.
│   ├── CompoundPredicates     # all, any, not, if/then
│   └── ExpressionEngine       # CEL-like expressions
│
├── agentverify-runtime/       # VerifiedExecutor
│   ├── Executor               # Main execution wrapper
│   ├── RetryLogic             # Verify-before-retry
│   └── Idempotency            # Duplicate protection
│
├── agentverify-observe/       # Observation adapters
│   ├── PostgresObserver        # PostgreSQL adapter
│   ├── RestObserver           # REST API adapter
│   ├── RedisObserver          # Redis adapter
│   └── FilesystemObserver     # FS adapter
│
├── agentverify-recovery/      # Recovery strategies
│   ├── Retry                  # Standard retry
│   ├── Compensate            # Saga compensation
│   └── Escalate              # Human approval
│
├── agentverify-receipt/       # Evidence/receipts
│   ├── ReceiptStruct          # Structured evidence
│   ├── Signer                 # Ed25519 signatures
│   └── Verifier              # Receipt validation
│
├── agentverify-policy/        # Policy engine
│   ├── PolicyLoader          # Policy from files
│   └── PolicyEvaluator       # Policy decision
│
├── agentverify-storage/       # Storage adapters
│
├── agentverify-mcp/          # MCP integration
│   ├── McpProxy              # MCP intercepting proxy
│   └── McpContractMapper     # Tool→contract mapping
│
├── agentverify-otel/          # OpenTelemetry export
│   ├── Traces                # Span emission
│   └── Metrics               # Metric emission
│
├── agentverify-http/         # HTTP gateway
│   ├── Server                # REST gateway
│   └── Client                # Client library
│
└── agentverify-cli/          # CLI tool
    ├── init                  # Project initialization
    ├── verify                # Run verification
    ├── contract              # Contract management
    ├── serve                 # Start gateway
    └── mcp                   # MCP proxy
```

---

## Data Flow

```
Agent Action
    │
    ▼
┌─────────────────┐
│  Validate       │ ← Contract syntax, preconditions
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Execute        │ ← Call external system
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Capture Result │ ← Raw response, timing, errors
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Observe        │ ← Query system of record
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Verify         │ ← Evaluate postconditions
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Receipt        │ ← Sign and store evidence
└─────────────────┘
```

---

## Contract Format (YAML)

```yaml
action: create_customer

preconditions:
  - customer.email:
      exists: false

postconditions:
  - customer.exists: true
  - customer.email:
      equals: "$args.email"
  - customer.status:
      equals: "active"

verification:
  consistency: strong  # strong, eventual, polling
  timeout: 500ms

recovery:
  strategy: verify-before-retry
  max_attempts: 3
  backoff:
    type: exponential
    initial: 100ms
    max: 5s
```

---

## MCP Integration

AgentVerify acts as an MCP proxy that intercepts tool calls:

```
Agent
   │
   ▼
AgentVerify MCP Proxy
   │
   ├── Inspect tool definition
   ├── Inspect annotations (but don't trust them)
   ├── Classify risk
   ├── Map tool → contract
   ├── Execute
   ├── Observe
   ├── Verify
   │
   ▼
Actual MCP Server
```

**Critical:** MCP tool annotations (`readOnlyHint`, `destructiveHint`, etc.) are hints, not proof. AgentVerify verifies independently.

---

## Event Export

AgentVerify emits OpenTelemetry events:

```
agentverify.action          # Action started
agentverify.attempt         # Attempt made
agentverify.observation     # State observed
agentverify.verification    # Result determined
agentverify.recovery        # Recovery attempted
```

With correlation:
- `trace_id`, `span_id`
- `run_id`, `action_id`, `attempt_id`

---

## Performance Targets

| Operation | Target |
|-----------|--------|
| Local verification (pure predicates) | <1ms |
| In-process observation | ~1-5ms |
| Network verification | Measure p50/p95/p99 |

---

## Invariants

1. **VERIFIED** requires all mandatory postconditions satisfied
2. **UNKNOWN** must never automatically become VERIFIED without new evidence
3. **retry(non-idempotent)** requires verification failure OR explicit authorization
4. **Receipt** must correspond to immutable action + evidence
5. **Observation source** must be identified
6. **Stale observation** cannot verify current state
