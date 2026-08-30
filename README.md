# AgentVerify

[![CI](https://github.com/aliasfoxkde/AgentVerify/actions/workflows/ci.yml/badge.svg)](https://github.com/aliasfoxkde/AgentVerify/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![Code Coverage](https://img.shields.io/codecov/c/github/aliasfoxkde/AgentVerify/main)](https://app.codecov.io/gh/aliasfoxkde/AgentVerify)

Outcome verification for action-taking AI agents. AgentVerify checks that
high-risk actions actually reached the required final state in your systems
of record — and preserves the evidence in signed, tamper-evident receipts.

**The problem it solves:** an agent calls a tool, the tool returns "200 OK",
and the agent declares success. But the payment didn't settle, the customer
wasn't created, the refund didn't happen. AgentVerify closes that gap: given
an intended action and its expected postconditions, it determines whether the
external world actually satisfies them.

**UNKNOWN is a first-class state.** A timeout does not equal failure. When
AgentVerify cannot determine the outcome, it says `UNKNOWN` — never a
guessed `FAILED`.

## Core Principle

> **"Given an intended action and its expected postconditions, determine
> whether the external state satisfies those postconditions."**

## Quick Start

The following is a complete, runnable program (same API for production
adapters):

```rust
use std::sync::{Arc, Mutex};

use agentverify_core::{Action, Contract, IdempotencyKey, Observation, SourceId};
use agentverify_runtime::{
    ActionExecutor, DispatchError, DispatchOutcome, Executor, ExecutorConfig, ExecutorError,
    Observer,
};
use async_trait::async_trait;

// 1. The contract: what must be true after the action runs.
const REFUND_CONTRACT: &str = r#"
{
  "version": "1.0",
  "action_name": "refund_customer",
  "postconditions": [
    {
      "description": "The refund must exist",
      "predicate": { "type": "exists", "path": "refund.id" }
    },
    {
      "description": "The refund must be in the succeeded state",
      "predicate": {
        "type": "equals",
        "path": "refund.status",
        "value": "succeeded"
      }
    }
  ]
}
"#;

// 2. Dispatch: how the action reaches the system that performs it.
//    (In this demo the "payment system" is an in-process ledger so the
//    example runs standalone; swap in your HTTP/gRPC client.)
#[derive(Clone, Default)]
struct RefundClient {
    ledger: Arc<Mutex<Option<serde_json::Value>>>,
}

#[async_trait]
impl ActionExecutor for RefundClient {
    async fn execute(&self, action: &Action) -> Result<DispatchOutcome, DispatchError> {
        let refund_id = format!("re_{}", action.id);
        *self.ledger.lock().unwrap() = Some(serde_json::json!({
            "refund": { "id": refund_id, "status": "succeeded" }
        }));
        Ok(DispatchOutcome::Completed)
    }
}

// 3. Observe: read the resulting state from the system of record.
#[derive(Clone, Default)]
struct LedgerObserver {
    ledger: Arc<Mutex<Option<serde_json::Value>>>,
}

#[async_trait]
impl Observer for LedgerObserver {
    async fn observe(
        &self,
        _action: &Action,
        _contract: &Contract,
    ) -> Result<Observation, ExecutorError> {
        let state = self
            .ledger
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(serde_json::json!({}));
        Ok(Observation::new(SourceId("ledger".into()), state))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let contract: Contract = serde_json::from_str(REFUND_CONTRACT)?;

    let ledger = Arc::new(Mutex::new(None));
    let executor = Executor::with_config(ExecutorConfig::default());
    let client = RefundClient { ledger: ledger.clone() };
    let observer = LedgerObserver { ledger };

    let action = Action::with_idempotency(
        "refund_customer",
        serde_json::json!({ "customer_id": "cus_42", "amount": 1999 }),
        IdempotencyKey::new("refund_customer:cus_42:order_9001"),
    );

    let (result, receipt) = executor
        .execute_with_executor(action, contract, Arc::new(client), Some(Arc::new(observer)))
        .await?;

    println!("result: {result:?}");
    println!("receipt digest verified: {}", receipt.verify_digest());
    for pc in &receipt.postcondition_results {
        println!("  - {} => passed={}", pc.description, pc.passed);
    }
    Ok(())
}
```

Output:

```text
result: Verified
receipt digest verified: true
  - The refund must exist => passed=true
  - The refund must be in the succeeded state => passed=true
```

The receipt returned alongside the result carries the observations, the
per-postcondition pass/fail evidence, a SHA-256 digest binding the content,
and an Ed25519 signature slot.

### Production observers

For real systems of record, AgentVerify ships Postgres, Redis, and REST
observers instead of the in-process one above:

```rust
use agentverify_http::{RestObserver, RestObserverConfig};
use std::sync::Arc;

let observer = RestObserver::new(
    RestObserverConfig::new("https://payments.internal.example")
        .with_timeout(2_000)
        .with_redact_path("authorization"),
)?;

let (result, receipt) = executor
    .execute_with_executor(action, contract, Arc::new(client), Some(Arc::new(observer)))
    .await?;
```

## Verification Results

| Result | Meaning | Terminal? |
|--------|---------|-----------|
| `VERIFIED` | All postconditions satisfied | Yes (success) |
| `FAILED` | Postconditions not met | Yes (failure) |
| `UNKNOWN` | Cannot determine (timeout, unreachable system of record) | No |
| `PARTIAL` | Some postconditions met, others not | Yes (failure) |
| `DUPLICATE` | Action already executed (idempotency hit) | Yes (success) |

The executor enforces **verify-before-retry**: it never re-dispatches an
action without observing state first, so a timeout leads to observation of
the real outcome rather than a blind retry that could double-charge a
customer.

## Key Design Principles

1. **Core must be deterministic** — no LLM, database, HTTP, or framework
   dependencies in `agentverify-core`
2. **Verify-before-retry** — never retry without verifying first
3. **Zero-trust annotations** — MCP tool annotations are hints, not proof
4. **UNKNOWN is first-class** — timeout ≠ failure
5. **Receipts for evidence** — every operation produces a structured,
   signable receipt
6. **Independently useful** — deployable without any sibling framework

## Installation

```bash
cargo add agentverify-runtime
```

or in `Cargo.toml`:

```toml
[dependencies]
agentverify-runtime = "0.1"
```

Pre-publish, depend on the repository directly:

```toml
[dependencies]
agentverify-runtime = { git = "https://github.com/aliasfoxkde/AgentVerify" }
```

## CLI

```bash
# Validate a contract
agentverify contract validate contract.json

# Verify an action's outcome
agentverify verify --contract contract.json --args '{"email":"user@example.com"}'

# Start the HTTP gateway
agentverify serve --port 8080
```

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `agentverify-core` | Core types: Action, Contract, Predicate, Receipt, StateMachine |
| `agentverify-contract` | JSON/YAML contract parsing and validation |
| `agentverify-engine` | Deterministic predicate evaluation engine |
| `agentverify-runtime` | VerifiedExecutor: idempotency, verify-before-retry |
| `agentverify-observe` | Postgres, Redis observers |
| `agentverify-recovery` | Recovery strategies |
| `agentverify-receipt` | Receipt signing (Ed25519) |
| `agentverify-policy` | Policy engine with rate limiting |
| `agentverify-storage` | Storage adapters |
| `agentverify-mcp` | MCP client integration |
| `agentverify-otel` | OpenTelemetry OTLP export |
| `agentverify-http` | HTTP gateway and REST observer |
| `agentverify-cli` | CLI tool |
| `agentverify-testkit` | Mocks and testing utilities |

## WASM

`agentverify-core`, `-contract`, `-engine`, `-receipt`, and `-policy` compile
for `wasm32-wasip1` (CI-enforced). Capabilities and limitations of the WASM
story are documented in [`docs/WASM_ARCHITECTURE.md`](docs/WASM_ARCHITECTURE.md).

## Documentation

- [Getting started concepts](docs/CONCEPTS.md)
- [Architecture](docs/ARCHITECTURE.md)
- [CLI reference](docs/CLI.md)
- [Observer integrations](docs/INTEGRATIONS.md)
- [Roadmap](docs/ROADMAP.md) · [Full specification](docs/PLANNING.md)

## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for setup,
quality gates, and PR expectations. For wrong verification outcomes (a false
`VERIFIED` or a mishandled `UNKNOWN`), use the
[verification bug template](https://github.com/aliasfoxkde/AgentVerify/issues/new?template=verification-bug.yml)
— that class of report is treated as highest priority.

## Security

See [SECURITY.md](SECURITY.md). Please report vulnerabilities through
GitHub's private vulnerability reporting, not public issues.

## License

Licensed under the [MIT License](LICENSE).
