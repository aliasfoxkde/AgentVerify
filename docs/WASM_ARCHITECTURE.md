# WASM Architecture Guide

This document describes WASM (WebAssembly) support in AgentVerify, including what's available, limitations, and how to use distributed stores for WASM deployments.

## Supported Targets

- `wasm32-wasip1` (WASI Preview 1)

## Building for WASM

```bash
# Build all WASM libraries
cargo build --target wasm32-wasip1 \
  -p agentverify-core \
  -p agentverify-engine \
  -p agentverify-contract \
  -p agentverify-runtime \
  -p agentverify-http
```

## What's Available on WASM

### Core Libraries (All compile for wasm32-wasip1)

| Crate | Status | Notes |
|-------|--------|-------|
| `agentverify-core` | ✅ Full | Core types, state machine, receipt types |
| `agentverify-engine` | ✅ Full | Predicate evaluation engine |
| `agentverify-contract` | ✅ Full | JSON/YAML contract parsing |
| `agentverify-runtime` | ✅ Full | VerifiedExecutor implementation |
| `agentverify-http` | ✅ Partial | HTTP client via web-sys |

### WASM-Native Components

#### WasmHttpClient (`agentverify_http::WasmHttpClient`)

HTTP client using JavaScript's `fetch` API via `web-sys`:

```rust
use agentverify_http::{WasmHttpClient, WasmRestObserverConfig};

let config = WasmRestObserverConfig::new("http://api.example.com");
let client = WasmHttpClient::new(&config.base_url)
    .with_headers(config.headers.clone())
    .with_timeout(config.timeout_ms);

let response: MyData = client.get_json("/api/data").await?;
```

#### WasmRestObserver (`agentverify_http::WasmRestObserver`)

REST observer for WASM using JavaScript fetch. Note: Cannot implement the standard `Observer` trait because `JsFuture` is not `Send`. Use the direct `observe()` method:

```rust
use agentverify_http::{WasmRestObserver, WasmRestObserverConfig};

let config = WasmRestObserverConfig::new("http://api.example.com");
let observer = WasmRestObserver::new(config);

let observation = observer.observe(&action, &contract).await?;
```

#### WasmReceiptStore (`agentverify_http::WasmReceiptStore`)

Receipt store using browser `localStorage`:

```rust
use agentverify_http::WasmReceiptStore;

let store = WasmReceiptStore::new("myapp")?;
store.store(&receipt).await?;
let receipt = store.get(&receipt_id).await?;
```

#### WasmIdempotencyStore (`agentverify_http::WasmIdempotencyStore`)

Idempotency store using browser `localStorage`:

```rust
use agentverify_http::{WasmIdempotencyStore, ClaimResult};

let store = WasmIdempotencyStore::new("myapp")?;
let (result, prev) = store.claim_or_check("key123").await?;
if result == ClaimResult::Claimed {
    // Process action
    store.complete("key123", VerificationResult::Verified).await?;
}
```

## What's NOT Available on WASM

### FileReceiptStore and FileIdempotencyStore

These require filesystem I/O (`tokio::fs`) which is not available on WASM:

```rust
// NOT available on WASM
#[cfg(not(target_arch = "wasm32"))]
use agentverify_core::FileReceiptStore;
```

**Workaround**: Use distributed stores for WASM deployments (see below).

### Standard RestObserver (reqwest-based)

The reqwest-based `RestObserver` requires `tokio` with full features and is not available on WASM.

## Using Distributed Stores for WASM

For production WASM deployments, use distributed stores:

### Redis

```rust
// Use Redis for receipt storage
// See agentverify-redis or implement RedisReceiptStore
```

### PostgreSQL

```rust
// Use PostgreSQL for receipt and idempotency storage
// See agentverify-sqlx or implement SqlxReceiptStore
```

### Custom WASM Store

Implement storage using browser APIs:

```rust
// Use WasmReceiptStore with localStorage
// Or implement IndexedDB for larger storage needs
```

## WASM Limitations

1. **No `Send` Futures**: JavaScript Promises (`JsFuture`) cannot be sent between threads. This means:
   - Standard `Observer` trait cannot be implemented for WASM
   - `async_trait` with `Send` bounds don't work
   - Use direct method calls instead of trait objects

2. **No Filesystem**: WASM has no native filesystem. Use:
   - Browser `localStorage` or `IndexedDB` (size limits apply)
   - Remote storage services via HTTP

3. **Single-threaded by Default**: WASM runs in a single thread unless using Web Workers

## Platform Feature Flags

```rust
// In your code
#[cfg(target_arch = "wasm32")]
use agentverify_http::WasmRestObserver;

#[cfg(not(target_arch = "wasm32"))]
use agentverify_http::RestObserver;
```

## CI/CD

WASM builds are tested in CI:

```yaml
# .github/workflows/ci.yml
wasm:
  name: WASM Build
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
      with:
        targets: wasm32-wasip1
    - run: cargo build --target wasm32-wasip1 -p ...
```

## Release Artifacts

Release workflow builds WASM libraries alongside native binaries:

```bash
# After tag push, release includes:
# - agentverify-wasm32-wasip1.tar.gz  (WASM rlibs)
```

## Example: Full WASM Verification Flow

```rust
use agentverify_http::{WasmRestObserver, WasmReceiptStore, WasmRestObserverConfig};
use agentverify_core::{Action, Contract};
use agentverify_runtime::Executor;

// Create observer
let observer_config = WasmRestObserverConfig::new("http://api.example.com");
let observer = WasmRestObserver::new(observer_config);

// Create receipt store
let receipt_store = WasmReceiptStore::new("myapp")?;

// Create executor
let executor = Executor::builder()
    .with_observer(observer)
    .with_receipt_store(receipt_store)
    .build();

// Note: Execution must be driven by JavaScript since
// JsFuture cannot cross thread boundaries
```
