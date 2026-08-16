# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-16

### Added

- **WASM Support**: Core libraries now compile for `wasm32-wasip1` target:
  - `agentverify-core`: Core types, verification state machine, receipt types
  - `agentverify-engine`: Predicate evaluation engine with 71 unit tests
  - `agentverify-contract`: JSON/YAML contract parsing and validation
  - `agentverify-runtime`: VerifiedExecutor implementation
  - `agentverify-http`: WASM-native HTTP client using web-sys fetch API

- **WasmHttpClient**: WASM-native HTTP client using JavaScript's fetch API via web-sys.

- **WasmRestObserver**: WASM-native REST observer for observation during verification.

- **WasmReceiptStore**: WASM-native receipt store using browser localStorage.

- **WasmIdempotencyStore**: WASM-native idempotency store using browser localStorage.

- **Property-Based Tests**: Added proptest-based property tests for predicate engine, verifying determinism across all predicate types.

- **CI WASM Job**: Added dedicated WASM build job in CI workflow to verify wasm32-wasip1 compilation.

- **Release WASM Target**: Added wasm32-wasip1 target to release workflow for distributing WASM libraries.

- **WASM Architecture Guide**: `docs/WASM_ARCHITECTURE.md` documenting WASM support, limitations, and usage.

### Changed

- **tokio Dependency**: Split tokio dependency by platform:
  - Native targets: Full features
  - wasm32 targets: Minimal features (sync, rt, time, macros)

### Fixed

- RUSTSEC-2025-0134: Updated reqwest from 0.11 to 0.12 to address rustls-pemfile vulnerability.

### Known Limitations

- **WASM**: Standard `Observer` trait cannot be implemented due to `JsFuture` not being `Send`. Use `WasmRestObserver.observe()` directly instead.
- **WASM**: FileReceiptStore/FileIdempotencyStore not available (use `WasmReceiptStore`/`WasmIdempotencyStore` with localStorage or distributed stores instead)
- **WASM**: CLI binary is platform-specific and cannot be built for WASM.

## [0.0.1] - 2026-08-11

### Added

- Initial project structure with Cargo workspace
- Core types: Action, Contract, Predicate, Observation, Receipt
- State machine with VERIFIED, FAILED, UNKNOWN, PARTIAL, DUPLICATE states
- Predicate engine with multiple predicate types
- VerifiedExecutor with verify-before-retry logic
- HTTP REST observer
- GitHub Actions CI workflow
