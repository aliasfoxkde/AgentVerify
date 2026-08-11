# AgentVerify Concepts

**Version:** 1.0
**Created:** 2026-08-11

---

## Core Vocabulary

### Action

An action is an operation requested by an agent that AgentVerify will verify.

```rust
pub struct Action {
    pub id: ActionId,           // Unique identifier
    pub name: String,           // e.g., "create_customer"
    pub arguments: Value,        // JSON arguments
    pub idempotency_key: Option<IdempotencyKey>,
    pub timestamp: DateTime<Utc>,
}
```

### Contract

A contract defines preconditions, postconditions, and recovery for an action.

```rust
pub struct Contract {
    pub id: ContractId,
    pub action_name: String,
    pub preconditions: Vec<Predicate>,
    pub postconditions: Vec<Predicate>,
    pub recovery: Option<RecoveryPlan>,
    pub verification: VerificationConfig,
}
```

### Precondition

A condition that must be true before the action executes.

```rust
pub struct Precondition {
    pub predicate: Predicate,
    pub description: String,
}
```

### Postcondition

A condition that must be true after the action completes.

```rust
pub struct Postcondition {
    pub predicate: Predicate,
    pub description: String,
    pub mandatory: bool,  // If false, PARTIAL is allowed
}
```

### Predicate

A predicate is a deterministic, evaluable condition.

```rust
pub enum Predicate {
    // Basic
    Exists { path: String },
    NotExists { path: String },
    Equals { path: String, value: Value },
    NotEquals { path: String, value: Value },
    Contains { path: String, value: Value },
    Matches { path: String, regex: String },
    GreaterThan { path: String, value: Value },
    LessThan { path: String, value: Value },

    // Collection
    Count { path: String, operator: Op, value: i64 },
    IsEmpty { path: String },
    IsNotEmpty { path: String },

    // Compound
    All { predicates: Vec<Predicate> },
    Any { predicates: Vec<Predicate> },
    Not { predicate: Box<Predicate> },
}
```

### Observation

An observation captures state from a system of record.

```rust
pub struct Observation {
    pub source: SourceId,           // e.g., "postgres", "rest"
    pub timestamp: DateTime<Utc>,
    pub state: Value,               // Observed JSON state
    pub evidence: Vec<Evidence>,     // Raw evidence items
}
```

### Evidence

Raw data supporting an observation.

```rust
pub struct Evidence {
    pub source: String,
    pub data: Value,
    pub timestamp: DateTime<Utc>,
}
```

### VerificationResult

```rust
pub enum VerificationResult {
    Verified,       // All postconditions met
    Failed,         // Postconditions not met
    Unknown,        // Cannot determine
    Partial,        // Some postconditions met
    Duplicate,      // Already executed (idempotent)
}
```

### RecoveryPlan

Defines what to do when verification fails or times out.

```rust
pub struct RecoveryPlan {
    pub strategy: RecoveryStrategy,
    pub max_attempts: u32,
    pub backoff: Option<BackoffConfig>,
    pub on_unknown: Vec<RecoveryAction>,
}

pub enum RecoveryStrategy {
    NoAction,
    Retry,
    VerifyThenRetry,   // RECOMMENDED: verify before retry
    Poll,
    Compensate,
    Rollback,
    Escalate,
    HumanApproval,
    Abort,
}
```

### Receipt

A signed record of verification outcome.

```rust
pub struct Receipt {
    pub id: ReceiptId,
    pub action_id: ActionId,
    pub contract_id: ContractId,
    pub result: VerificationResult,
    pub attempts: u32,
    pub observations: Vec<Observation>,
    pub postcondition_results: Vec<PostconditionResult>,
    pub signature: Option<Ed25519Signature>,
    pub timestamp: DateTime<Utc>,
}
```

---

## Key Concepts

### UNKNOWN is First-Class

A timeout does NOT equal failure. The external system may have:
- Received the request but not yet persisted
- Partially completed
- Responded but the response was lost

**AgentVerify verifies before retrying.**

### Verify-Before-Retry

```
execute
   │
   ├── success ──► verify
   │
   └── timeout ──► verify
                     │
                ┌────┴────┐
                ▼         ▼
             exists     absent
                │         │
                ▼         ▼
             success     retry
```

Never retry on timeout without verifying first.

### Idempotency

Every action has an idempotency key. When supported by the external system:

```http
Idempotency-Key: av_<action-id>
```

When not supported, AgentVerify maintains a deduplication registry.

### Observation Consistency

| Mode | Behavior |
|------|----------|
| `strong` | Read after write completes |
| `eventual` | Poll until consistent |
| `polling` | Wait interval, max attempts |
| `webhook` | Wait for callback |

### Zero-Trust Annotations

MCP tool annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`) are hints, not proof. AgentVerify:
1. Inspects annotations for classification
2. Ignores them for verification
3. Applies its own verification logic

---

## State Machine

```
PROPOSED ──► VALIDATING ──► AUTHORIZED ──► EXECUTING
                                          │
                              ┌───────────┼───────────┐
                              ▼           ▼           ▼
                            FAILED     TIMEOUT     UNKNOWN
                              │           │           │
                              └───────────┴───────────┘
                                          │
                                          ▼
                                      OBSERVING
                                          │
                                          ▼
                                      VERIFYING
                                          │
                              ┌───────────┴───────────┐
                              ▼                       ▼
                          VERIFIED                   FAILED
                              │                       │
                              │               ┌──────┴──────┐
                              │               ▼             ▼
                              │          RECOVERING      ESCALATED
                              │               │             │
                              │         ┌─────┴─────┐       │
                              │         ▼           ▼       │
                              │      RECOVERED   ESCALATED   │
                              │                               │
                              └───────────────┬───────────────┘
                                              │
                                              ▼
                                         COMMITTED
```
