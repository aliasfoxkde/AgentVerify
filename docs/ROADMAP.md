# AgentVerify Roadmap

This document is the working, phased plan for AgentVerify releases. The full
architectural specification — including the 32-phase build-out it derives
from — lives in [`PLANNING.md`](PLANNING.md).

Status markers: ✅ shipped · 🚧 in progress · ⬜ planned · ⬛ explicitly
deferred (see [Non-goals](#non-goals)).

---

## Where we are

**v0.1.0 (current)** delivers the MVP boundary from `PLANNING.md`:

- ✅ Core verification model: `Action`, `Contract`, pre/postconditions,
  lifecycle state machine with first-class `UNKNOWN`
- ✅ JSON/YAML contract parsing (`agentverify-contract`)
- ✅ Deterministic predicate engine (`agentverify-engine`) with property
  tests and benchmarks
- ✅ `VerifiedExecutor` with verify-before-retry and atomic idempotency
  (`agentverify-runtime`)
- ✅ Postgres / Redis / REST observers (`agentverify-observe`,
  `agentverify-http`)
- ✅ Receipts with SHA-256 digest binding and Ed25519 signing path
- ✅ Policy engine with rate limiting (`agentverify-policy`)
- ✅ Recovery strategies (`agentverify-recovery`)
- ✅ OpenTelemetry OTLP export (`agentverify-otel`)
- ✅ CLI: contract validation, verification, HTTP gateway serving
- ✅ Cross-platform release binaries (Linux x86_64/aarch64/musl, macOS
  x86_64/aarch64, Windows x86_64)
- ✅ Strict workspace lint policy (`pedantic` + panic/unwrap/unsafe denials,
  CI-enforced)

## Milestone 0.1.x — hardening (current)

| Item | Status | Notes |
|------|--------|-------|
| Clippy `pedantic` clean, `-D warnings` in CI | ✅ | 439 warnings cleared workspace-wide (default + all-features) |
| Coverage baseline → 90%+ per crate | ✅ | Workspace ~97% line coverage; live Postgres/Redis in CI service containers |
| Docs build with `-D warnings` | ✅ | `missing_docs` enforced; 100% public-item doc coverage |
| Failure-injection tests (timeouts, partial failures, duplicates) | 🚧 | Duplicate/timeout/partial paths covered in runtime suite; fault-injection harness remains |
| `cargo-deny` advisories/bans/licenses/sources in CI | ✅ | `deny.toml` |
| MSRV policy | ✅ | `rust-version = 1.88` (floor of the dependency tree), CI-verified with `cargo hack` |

### Known limitations (accepted for 0.1, scheduled for 0.2)

Found during the coverage push and audit; each is documented here rather
than silently present:

1. **`FileIdempotencyStore` cache staleness** — an instance that cached an
   in-flight entry never re-validates it, so a completion written by another
   process is not observed until restart. Single-instance deployments are
   unaffected; multi-instance deployments should use the Redis store. 0.2:
   TTL/revalidation policy.
2. **Executor `Unknown` retry branches are unreachable** —
   `PredicateEngine::evaluate` can only return Verified/Failed today, so the
   executor's indeterminate-verdict handling is defensive. 0.2: plumb real
   consistency-mode results (timestamp/sequencing checks) into the verdict so
   `UNKNOWN` verdicts become reachable end-to-end.
3. **`OtlpExporter::new` requires a tokio runtime** (tonic channel worker);
   the doc example implies otherwise. 0.2: document or make runtime-agnostic.
4. **`OtlpExporter::shutdown` returns `Ok` when the collector is
   unreachable** — span loss is only logged by the SDK. Pinned by test.
5. **`ControlCenterClientBuilder` is not re-exported** from
   `agentverify-http` and `max_receipt_size` has no public setter, so the
   1 MiB receipt cap is not configurable by dependents. 0.2: API surface fix.
6. **MCP client error variants `NotInitialized`, `ServerError`,
   `CapabilityNotSupported` are never constructed** (no initialize guard
   before tool calls). 0.2: enforce the initialize handshake.
7. **`RecoveryOutcome::NotApplicable` has no constructor site** in
   `agentverify-recovery`. 0.2: either produce it from strategy dispatch or
   remove the variant.
8. **Git history carries ~400 MB of accidental build artifacts** — commit
   `7bc8232` (a protective snapshot taken during a concurrent-session
   incident) tracked `.target-msrv/`, `.target-wasm/`, and session logs;
   they are removed from the current tree and ignored, but remain in
   history, so clones are larger than they should be. 0.2: rewrite with
   `git filter-repo` in a coordinated window (requires a force push and a
   fresh clone for all contributors).
9. **Dependency upgrade wave is open** — dependabot has 9 major-bump PRs
   (axum 0.8, thiserror 2, redis 1.6, rand 0.10, sha2 0.11, jsonpath-rust
   1.0, criterion 0.8, tower-http 0.6, base64 0.23) that need code changes
   and were deliberately not merged into the 0.1.0 launch. 0.1.1/0.2: land
   them in small batches with full test runs.

## Milestone 0.2 — dependency and API modernization

A deliberate modernization pass, each step gated by the full test suite:

| Dependency | From → To | Why |
|------------|-----------|-----|
| `axum` | 0.7 → 0.8 | Path-parameter syntax change; current LTS line |
| `sqlx` | 0.7 → 0.9 | Runtime/driver updates, Postgres observer |
| `thiserror` | 1.x → 2.x | Current major across the ecosystem |
| `redis` | 0.24 → 1.x | Stable 1.0 line for the Redis observer/idempotency store |
| `serde_yaml` | 0.9 → `serde_norway` | `serde_yaml` is archived/unmaintained; contract YAML parsing must move off it |
| `tower-http` | 0.5 → current | Match axum upgrade |

Also in 0.2:

- Contract DSL v1.1: conditional postconditions (`Implies` exposure in the
  file format), multi-source consistency windows
- Receipt signing made first-class in the executor (currently an explicit
  opt-in via `ReceiptSigner`)
- Conformance test suite for adapters (`agentverify-testkit`), so third-party
  observers/stores can be validated against the contract in `PLANNING.md`
  Phase 23

## Milestone 0.3 — operations and integrations

- MCP integration maturation: annotation-vs-proof zero-trust checks as a
  middleware (`PLANNING.md` Phase 10)
- Amortyx integration as a showcase deployment (`Phase 13/28`), keeping the
  core standalone
- Atheon feedback loop (`Phase 29`) — verification outcomes feed anomaly
  detection without coupling the crates
- Performance engineering: `cargo bench` baselines for the full verify path
  (predicate evaluation is already benchmarked), p50/p95/p99 tracking
  (`Phase 21/26`)
- Formal invariants documented and property-tested (`Phase 19`:
  "UNKNOWN is never collapsed into FAILED" and friends)

## Milestone 1.0 — stability commitment

- SemVer-stable public API across all published crates
- 90%+ enforced (non-informational) coverage on core crates
- Reference implementations with real systems of record (`Phase 27`)
- Contract drift detection design (`Phase 31`) at least specified
- Release engineering fully automated (below)

## Release engineering

Current (this repo):

1. `CHANGELOG.md` updated under `## [Unreleased]`
2. PRs squash-merged to `main`; CI must be green
3. Tag `vX.Y.Z` on `main` → the Release workflow builds all six platform
   binaries, attaches them + SHA-256 checksums to a GitHub Release, and
   publishes all 14 crates to crates.io in dependency order
4. `CARGO_REGISTRY_TOKEN` repository secret authorizes publishing

Planned:

- **release-plz** adoption: PR-based version bumps + changelog generation on
  every merge to `main`, replacing manual changelog edits
- **crates.io trusted publishing** (OIDC): removes the long-lived
  `CARGO_REGISTRY_TOKEN`; note a crate's *first* version must still be
  published with a token, so trusted publishing activates from 0.1.x
  onward
- `cargo-vet` for dependency audit trails once the dependency set stabilizes
- Signed release binaries (Sigstore/cosign) alongside the SHA-256 checksums

## Non-goals

Unchanged from `PLANNING.md` §"What I would explicitly NOT do in v1" and the
README: no LLM judges, no autonomous contract generation in v1, no agent
framework replacement, no generic guardrail/tracing platform, no requirement
to rewrite existing workflows.
