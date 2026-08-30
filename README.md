# AgentVerify

AgentVerify is an open-source outcome verification library for action-taking AI agents. It checks that high-risk actions reached the required final state in your systems of record and preserves the evidence in signed receipts.

## Core Principle

> **"Given an intended action and its expected postconditions, determine whether the external state satisfies those postconditions."**

**UNKNOWN is a first-class state.** A timeout does NOT equal failure.

## Architecture

```
                    +---------------------+
                    |        AGENT        |
                    +---------+-----------+
                              |
                              v
                    +---------------------+
                    |       AMORTYX       |
                    | Cognition / Middleware|
                    +---------+-----------+
                              |
            +-----------------+-----------------+
            |                                   |
            v                                   v
    +------------------+              +------------------+
    |    AGENTVERIFY   |              |      ATHEON      |
    |                  |              |                  |
    | Can we prove     |              | Is something     |
    | the outcome?     |              | wrong/suspicious?|
    +------------------+              +------------------+
```

AgentVerify must remain independently useful - deployable without Amortyx or Atheon.

## Verification Lifecycle

```
PROPOSED -> VALIDATING -> AUTHORIZED -> EXECUTING
                                       |
                         (FAILED / TIMEOUT / UNKNOWN)
                                       |
                                   OBSERVING
                                       |
                                   VERIFYING
                                       |
                         (VERIFIED / FAILED)
                                       |
                                   COMMITTED
```

## Verification Results

| Result | Meaning | Terminal? |
|--------|---------|-----------|
| `VERIFIED` | All postconditions satisfied | Yes (success) |
| `FAILED` | Postconditions not met | Yes (failure) |
| `UNKNOWN` | Cannot determine (timeout, partial, consistency issues) | No |
| `PARTIAL` | Some postconditions met, others not | Yes (failure) |
| `DUPLICATE` | Action already executed | Yes (success) |

## Key Design Principles

1. **Core must be deterministic** - no LLM, database, HTTP, or Amortyx dependencies
2. **Verify-before-retry** - never retry without verifying first
3. **Zero-trust annotations** - MCP tool annotations are hints, not proof
4. **UNKNOWN is first-class** - timeout != failure
5. **Receipts for evidence** - every operation produces a structured, signable receipt

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `agentverify-core` | Core types: Action, Contract, Predicate, Receipt, StateMachine |
| `agentverify-contract` | JSON/YAML contract parsing |
| `agentverify-engine` | Deterministic predicate evaluation engine |
| `agentverify-runtime` | VerifiedExecutor implementation |
| `agentverify-observe` | Observation adapters (PostgreSQL, REST, Redis) |
| `agentverify-recovery` | Recovery strategies |
| `agentverify-receipt` | Receipt signing (Ed25519) |
| `agentverify-policy` | Policy engine |
| `agentverify-storage` | Storage adapters |
| `agentverify-mcp` | MCP client integration |
| `agentverify-otel` | OpenTelemetry export |
| `agentverify-http` | HTTP gateway (REST observers) |
| `agentverify-cli` | CLI tool |
| `agentverify-testkit` | Testing utilities |

## Installation

```bash
# Add to Cargo.toml
[dependencies]
agentverify-core = { path = "crates/agentverify-core" }
agentverify-engine = { path = "crates/agentverify-engine" }
agentverify-runtime = { path = "crates/agentverify-runtime" }
```

## Quick Start

### 1. Define a Contract

```json
{
  "version": "1.0",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "action_name": "create_customer",
  "preconditions": [
    {
      "description": "Customer email must be unique",
      "predicate": {
        "type": "not_exists",
        "path": "customers.email"
      }
    }
  ],
  "postconditions": [
    {
      "description": "Customer must be created with correct email",
      "predicate": {
        "type": "equals",
        "path": "customer.email",
        "value": "user@example.com"
      }
    },
    {
      "description": "Customer status must be active",
      "predicate": {
        "type": "equals",
        "path": "customer.status",
        "value": "active"
      }
    }
  ],
  "recovery": {
    "strategy": "retry",
    "max_attempts": 3,
    "backoff": {
      "type": "exponential",
      "initial_delay_ms": 1000,
      "max_delay_ms": 30000
    }
  }
}
```

### 2. Execute Verification

```rust
use agentverify_core::{Action, Contract, IdempotencyKey};
use agentverify_runtime::{Executor, RestObserver, RestObserverConfig};
use agentverify_engine::PredicateEngine;

async fn verify_action() -> Result<(), Box<dyn std::error::Error>> {
    // Load contract
    let contract: Contract = serde_json::from_str(contract_json)?;

    // Create action with idempotency key
    let idempotency_key = IdempotencyKey::new("create_customer_user@example.com");
    let action = Action::with_idempotency(
        "create_customer",
        serde_json::json!({"email": "user@example.com"}),
        idempotency_key,
    );

    // Setup observer
    let observer = RestObserver::new(RestObserverConfig::new("http://localhost:8080"))?;

    // Execute verification
    let executor = Executor::new();
    let (result, receipt) = executor
        .execute_with_executor(action, contract, action_executor, Some(Arc::new(observer)))
        .await?;

    println!("Verification result: {:?}", result);
    println!("Receipt: {}", receipt.id);

    Ok(())
}
```

## CLI Usage

```bash
# Validate a contract
agentverify contract validate contract.json

# Run verification
agentverify verify --contract contract.json --args '{"email":"user@example.com"}'

# Start HTTP server
agentverify serve --port 8080
```

## Receipts

Every verification produces a signed receipt containing:

- Action and contract identifiers
- Verification result and attempts
- Observations collected during verification
- Postcondition evaluation results
- SHA-256 digest for tamper evidence
- Ed25519 signature (when signed)

## OpenTelemetry Integration

Export verification traces via OTLP:

```rust
use agentverify_otel::{OtlpExporter, OtlpExporterConfig};

let config = OtlpExporterConfig::default()
    .with_endpoint("http://localhost:4317")
    .with_service_name("my-agent");
let exporter = OtlpExporter::new(config)?;
```

## Documentation

- [CLAUDE.md](CLAUDE.md) - Agent/AI guidance
- [docs/](docs/) - Detailed documentation

## License

Licensed under the [MIT License](LICENSE).
