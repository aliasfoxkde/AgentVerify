# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-30

First open-source release of AgentVerify: outcome verification for
action-taking AI agents, under the MIT license.

### Added

- **Core verification model** (`agentverify-core`): `Action`, `Contract`
  with pre/postconditions, `Predicate`, `Observation`/`Evidence`, `Receipt`,
  lifecycle `StateMachine` (PROPOSED → VALIDATING → AUTHORIZED → EXECUTING →
  OBSERVING → VERIFYING → COMMITTED), and `VerificationResult` with
  first-class `UNKNOWN` (timeout ≠ failure).
- **Contract DSL** (`agentverify-contract`): JSON/YAML contract parsing and
  validation.
- **Predicate engine** (`agentverify-engine`): deterministic evaluation of
  `exists`, `equals`, `contains`, `matches`, comparisons, `count`,
  compound `all`/`any`/`not`/`implies`, with criterion benchmarks
  (`crates/agentverify-engine/benches/predicate.rs`) and property tests.
- **Verified executor** (`agentverify-runtime`): verify-before-retry loop,
  atomic idempotency claim/complete/release with `IdempotencyRegistry`,
  in-memory and file-backed receipt stores, and receipts carrying
  per-postcondition evidence with SHA-256 digest binding.
- **Observers** (`agentverify-observe`): PostgreSQL and Redis observers;
  REST observer and HTTP gateway (`agentverify-http`).
- **Receipt signing** (`agentverify-receipt`): Ed25519 signing support.
- **Policy engine** (`agentverify-policy`): action allow/block lists,
  pattern matching, access levels, per-action and per-idempotency-key rate
  limiting.
- **Recovery strategies** (`agentverify-recovery`): retry with backoff,
  verify-before-retry semantics.
- **OpenTelemetry export** (`agentverify-otel`): OTLP/gRPC spans for the
  verification lifecycle (upgraded to opentelemetry 0.32).
- **CLI** (`agentverify-cli`): `contract validate`, `verify`, `serve`,
  `init`, with stable exit codes.
- **Testkit** (`agentverify-testkit`): mock idempotency stores, observers,
  and action executors for downstream testing.
- **WASM support**: `agentverify-core`, `-contract`, `-engine`, `-receipt`,
  and `-policy` compile for `wasm32-wasip1` (CI-enforced).
- **Cross-platform release automation**: tag-triggered builds for Linux
  (x86_64 GNU, aarch64, x86_64 musl), macOS (Intel + Apple Silicon),
  Windows (x86_64 MSVC), WASI libraries, with SHA-256 checksums, plus
  crates.io publishing in dependency order.
- **Project governance**: MIT `LICENSE`, `CONTRIBUTING.md`, `SECURITY.md`
  (private vulnerability reporting), `SUPPORT.md`, Contributor Covenant
  `CODE_OF_CONDUCT.md`, GitHub issue templates (including a dedicated
  verification-bug form), PR template, `CODEOWNERS`, dependabot, and
  `codecov.yml`.

### Changed

- Workspace-wide strict lint policy: `clippy::pedantic` enforced with
  `panic`/`unwrap`/`expect`/`todo`/`unsafe`/`print_stdout` denied via
  `[workspace.lints]`; CI gates with `-D warnings` for both default and
  all-features builds.
- `ReceiptStore::store` is now fallible (`Result<(), ReceiptStoreError>`);
  receipt persistence failures are logged and observable instead of silent.
- Policy engine rate limiters use `Arc<Mutex<…>>` interior mutability
  (replacing `unsafe` pointer casts); poisoned locks fall back to the guard
  data instead of panicking.
- MSRV set to Rust 1.88, matching the true floor of the dependency tree
  (tonic 0.14 / icu 2.x) and verified in CI with `cargo hack`.

### Fixed

- Receipt digest integrity: adding an observation or postcondition result
  mutated receipt content after the digest was computed, making
  `verify_digest()` fail on evidence-bearing receipts. Builders now
  recompute the digest (regression-tested).
- Receipt evidence completeness: receipts now record the outcome of every
  evaluated postcondition (predicate, description, pass/fail, and any
  indeterminate outcome), not just the aggregate result.
- RUSTSEC-2026-0258 (h2 unbounded empty DATA frames) via the
  opentelemetry/tonic 0.32 upgrade.
- Flaky `serve` CLI test replaced with a poll-with-deadline harness that
  fails fast if the server exits early.
- CI: Codecov upload no longer fails the build when no token is configured;
  documentation job enforces `-D warnings` with `--all-features`.

[0.1.0]: https://github.com/aliasfoxkde/AgentVerify/releases/tag/v0.1.0
