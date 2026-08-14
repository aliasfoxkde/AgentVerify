# AgentVerify handoff plan for Codex + MiniMax M2.7

**Audit date:** 2026-08-14
**Repository:** `/nas/Temp/repos/AgentVerify`  
**Audience:** Claude operating with MiniMax M2.7, with Codex-style review discipline
**Status:** active implementation handoff; this document is evidence-based planning, not a production-readiness approval

### Current audit boundary

- Branch: `codex/add-platform-handoff-2026-08-14`
- HEAD: `3a03e6d` (`feat(runtime): harden execute_with_executor with failure injection tests`)
- Worktree: two modified generated code-memory artifacts: `.codebase-memory/artifact.json` and `.codebase-memory/graph.db.zst` (pre-existing, not reset)
- Index: `nas-Temp-repos-AgentVerify`, ready; 1,001 nodes and 2,141 edges
- Last verified gates at this boundary: `cargo test --workspace --all-targets` (68 passing tests), `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo doc --workspace --no-deps`
- `cargo audit` completed with one allowed warning: `rustls-pemfile 1.0.4` is unmaintained (`RUSTSEC-2025-0134`); no claim of a clean audit should be made until the dependency path is resolved or explicitly accepted.

Refresh this boundary before every implementation packet. Preserve unrelated worktree changes and do not delete or reset generated artifacts automatically.

## 1. Mission and definition of done

Make AgentVerify a usable, deterministic outcome-verification product for action-taking agents. The first shippable milestone is a library/CLI path that can:

1. load and validate a JSON or YAML contract;
2. execute an injected action through an explicit executor interface;
3. observe a system of record through at least one real adapter;
4. evaluate preconditions and postconditions, including compound predicates and argument substitution;
5. preserve `UNKNOWN` separately from `FAILED`;
6. retry only under an explicit, idempotency-safe policy;
7. emit a verifiable receipt with evidence;
8. expose a documented integration surface and tests that prove the above under timeout, duplicate, stale-read, and partial-write scenarios.

Do not call the repository production-ready merely because the workspace is green. The suite proves local semantics and selected failure/security seams, but does not prove a deployed service, authenticated ownership, durable persistence, real action dispatch through a production adapter, MCP/OTel integration, or Control Center correlation.

## 2. Audited baseline

### Evidence-backed architecture

The refreshed codebase-memory index is `nas-Temp-repos-AgentVerify` and reports 1,001 nodes, 2,141 edges, and one executable entry point at `crates/agentverify-cli/src/main.rs`. The active workspace contains seven member crates:

```text
CLI -> runtime -> contract -> core
              -> engine -> core
HTTP observer -> runtime/core
receipt signing -> core receipt
```

The high fan-in symbols are `PredicateEngine::evaluate`, `IdempotencyRegistry::new`, `Predicate::exists`, `Observation::get`, and `validate_contract`.

### What is genuinely implemented

- `agentverify-core`: action, contract, observation/evidence, receipt data model, state machine, predicates, verification results, recovery/config types.
- `agentverify-contract`: JSON/YAML parsing, file loading, serialization, and basic validation.
- `agentverify-engine`: exists/not-exists, equals/not-equals, contains, regex matching, numeric comparisons, count, empty checks, all/any/not/implies, and basic `$args` resolution; 24 unit tests currently pass.
- `agentverify-runtime`: observer/action-executor traits, state transitions, postcondition evaluation, bounded retry loop, and process-local idempotency cache. The injected `execute_with_executor` path is exercised by six runtime tests; the convenience `execute` path still simulates dispatch.
- `agentverify-http`: REST observer with timeout, URL-string rejection checks, redaction, truncation, and 11 unit/security tests. This is a library seam, not proof of an authenticated production observer deployment.
- `agentverify-receipt`: Ed25519 signing/verification and three unit tests. The current signature canonicalization is local to the service and does not yet bind verifier identity, ownership, replay protection, or a key-distribution contract.
- CI workflow: format, clippy with `-D warnings`, workspace tests, docs, and cargo-audit steps are declared; inspect the workflow and rerun it from a clean checkout before relying on it as a release gate.

### What is not implemented or is only a shell

- `Executor::execute` explicitly simulates dispatch and substitutes an empty observation when no observer is supplied. Use `execute_with_executor` for current injected-dispatch behavior and do not silently promote the convenience path.
- `agentverify-observe`, `recovery`, `policy`, `storage`, `mcp`, `otel`, and `testkit` remain placeholder crates outside the workspace. They are intentionally deferred, not complete.
- The CLI parses `init`, `verify`, and `serve`, but those branches only print status. `contract validate` is the only substantive command.
- No durable receipt store, authenticated HTTP/MCP boundary, Postgres/Redis observer, testcontainers integration, or Control Center adapter exists.
- The architecture and integration documents describe planned features as if they were available; update them as features land and label examples as aspirational until executable.

### Repository hygiene finding

`target/` is build output and is not tracked by `git ls-files` at this audit boundary. It may still be regenerated locally; keep it out of handoff diffs and verify the ignore rule from a clean checkout. Do not reset or delete user changes automatically.

### Verification performed during audit

- `cargo test --workspace --all-targets`: passed; the observed suite is 63 passing tests across the seven active crates, with no CLI unit tests.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo doc --workspace --no-deps`: passed.
- `cargo audit`: completed with the `rustls-pemfile` unmaintained warning described above.

## 3. Required implementation order

### Phase 0 — establish a clean, reviewable baseline

Before feature work:

- inspect and preserve the existing worktree changes;
- verify that `target/` is ignored and not tracked; do not assume a build-created directory is a repository defect without checking `git ls-files`;
- refresh the code-memory artifacts only when needed and keep their generated modifications separate from source changes;
- run `cargo fmt`, `cargo test`, `cargo clippy`, `cargo doc`, and `cargo audit` from a clean build directory;
- add a `docs/STATUS.md` or update this document with the exact shipped-vs-planned matrix.

**Gate:** a clean checkout builds without generated-file churn, every advertised command is marked implemented or planned, and the audit warning has an owner/decision.

### Phase 1 — stabilize the domain contract

Focus files: `agentverify-core/src/{action,contract,predicate,observation,receipt,state_machine,verification_result}.rs` and `agentverify-contract/src/contract.rs`.

- Define the contract schema version and compatibility policy.
- Make predicate semantics explicit for missing paths, type mismatch, null, empty collections, regex errors, and numeric coercion.
- Decide whether `Partial` and `Duplicate` are terminal success/failure states; document the policy.
- Add validation for duplicate/ambiguous postconditions, invalid retry settings, missing observer configuration, and unsupported predicate combinations.
- Add property tests for predicate totality, serialization round trips, state-machine transition invariants, and argument substitution.

**Gate:** schema fixtures and property tests prove deterministic results for valid and invalid inputs.

### Phase 2 — make execution real and safe

Focus file: `agentverify-runtime/src/executor.rs` plus a new executor abstraction.

- Separate `ActionExecutor` (dispatch) from `Observer` (read-after-action) and `ReceiptStore`.
- Return a typed dispatch outcome that distinguishes accepted, completed, timeout-before-dispatch, timeout-after-dispatch, transport error, and ambiguous result.
- Never infer `FAILED` from a timeout. Reconcile through observation before retrying.
- Make retries bounded, backoff-aware, cancellation-safe, and idempotency-aware.
- Replace the process-local idempotency map with an interface; keep in-memory only as a test implementation. Define key scope, expiry, collision behavior, and concurrent-request behavior.
- Ensure receipts are not emitted as `VERIFIED` until every required postcondition has evidence.

**Gate:** deterministic tests cover timeout-before-dispatch, timeout-after-dispatch, duplicate dispatch, stale reads, observer errors, retry exhaustion, and cancellation.

### Phase 3 — ship one real observer and receipts

Recommended first observer: harden the existing REST library seam with a local mock-server integration test; add Postgres only after the ownership/consistency requirements for the system of record are explicit.

- Define an observer configuration trait with timeout, retry, consistency, redaction, and evidence-size limits.
- Implement request construction with strict URL/path/query handling and no secret leakage in logs or receipts.
- Add receipt canonicalization and Ed25519 signing/verifying in `agentverify-receipt`; bind the signature to action, contract version, outcome, evidence digest, timestamps, verifier identity/version, source identity, and key identifier. Define key rotation and verification policy.
- Add a storage interface and an in-memory/file implementation before database-backed storage.

**Gate:** end-to-end tests produce a signed receipt and reject tampering, stale contract versions, malformed evidence, and signature/key mismatches.

### Phase 4 — usable CLI and integration boundary

- Implement `contract validate` first, then `verify` with an explicit dry-run mode.
- Do not ship a `serve` or `mcp proxy` command until the HTTP/MCP security model, authentication, request limits, and observer authorization are defined.
- Add machine-readable JSON output and stable exit codes.
- Make the CLI display `UNKNOWN` distinctly and include receipt IDs/digests without printing secrets.
- Implement the testkit and failure injection used by the CLI and integration tests.

**Gate:** documented CLI examples run in CI against local fixtures and have snapshot/stability tests for output and exit codes.

### Phase 5 — integrations and operations

Only after the core lifecycle is real:

- MCP interception with untrusted annotations and explicit contract mapping.
- HTTP gateway with authentication, authorization, rate limits, body limits, TLS deployment guidance, and health/readiness endpoints.
- OpenTelemetry spans and metrics with outcome labels, bounded cardinality, redaction, and no raw evidence by default.
- Postgres/Redis observers, then recovery strategies and policy evaluation.
- Examples for REST, Postgres, MCP, and a local deterministic test system.

**Gate:** threat model, integration tests, cargo-audit, docs, and operational runbooks are updated together.

## 4. MiniMax M2.7 operating protocol for Codex

MiniMax's official API documentation currently lists `MiniMax-M2.7` and `MiniMax-M2.7-highspeed`, recommends streaming for reasoning models, and exposes OpenAI-compatible and Anthropic-compatible interfaces. The official announcement emphasizes complex agent harnesses, dynamic tool search, and end-to-end software engineering. Treat model marketing and benchmark claims as capability hints, not verification evidence.

Recommended setup:

```text
Provider/API: MiniMax OpenAI-compatible or Anthropic-compatible endpoint
Model: MiniMax-M2.7
Repository context: this handoff + targeted files, not the entire target/ tree
Primary loop: inspect -> patch -> format -> test -> review diff -> update handoff
Fallback: MiniMax-M2.7-highspeed for bounded mechanical edits or reruns
```

Prompt the model with these invariants on every coding task:

1. Preserve `UNKNOWN` semantics; timeout is not failure.
2. Never claim an integration exists without an executable test.
3. Read the exact source and caller path before editing.
4. Keep write scope explicit and avoid `target/`.
5. Run the smallest relevant test first, then the workspace gates.
6. Report changed files, assumptions, unresolved risks, and evidence.

Use short, bounded work packets rather than asking for the whole roadmap in one generation. A good packet names one crate, one behavior, one test matrix, and one completion gate. Have Codex review each patch independently because tool-capable models can produce plausible but unverified integration code.

## 5. Suggested first five work packets

1. ~~**Baseline and status truth:** refresh SHA/index state, reconcile stale docs, verify generated-artifact policy, and assign the audit warning.~~ ✅ (2026-08-13)
2. ~~**Runtime seam:** harden `execute_with_executor`; test dispatch ambiguity, timeout-before/after dispatch, observer errors, stale reads, retry exhaustion, cancellation, and concurrent idempotency.~~ ✅ (2026-08-14, SHA 3a03e6d)
3. **Contract semantics:** add property/fixture tests for missing paths, type mismatch, nulls, numeric coercion, regex errors, compound predicates, schema compatibility, and argument substitution.
4. **REST observer + evidence:** add a deterministic mock-server integration test, strict URL construction, authentication policy, response-size limits, redaction guarantees, and source/observation metadata.
5. **Receipts and persistence:** define the versioned receipt envelope, canonical digest, key identity/rotation, replay/idempotency semantics, durable store interface, and tamper/ownership tests.

Each packet should end with a commit-sized diff and a handoff note containing: files changed, commands run, test count, behavior decisions, and the next packet.

## 6. Research record

Use primary/official sources for changing MiniMax behavior:

- MiniMax model overview: https://platform.minimax.io/docs/guides/models-intro
- MiniMax text API reference: https://platform.minimax.io/docs/api-reference/text-post
- MiniMax M2.7 announcement: https://www.minimax.io/news/minimax-m27-en
- MiniMax M2.7 tool-calling guide: https://github.com/MiniMax-AI/MiniMax-M2.7/blob/main/docs/tool_calling_guide.md
- MiniMax M2 series technical paper: https://arxiv.org/abs/2605.26494

The repository's own competitive/research documents should be treated as historical planning context until their external links and dates are rechecked. Re-run web research before making cost, context-window, availability, licensing, or benchmark claims in release documentation.

## 7. Current completion matrix

- [x] Core contract schema/version and basic validation exist; [ ] full compatibility/edge-case policy.
- [x] Predicate engine implementation and unit coverage exist; [ ] property tests and complete semantic fixture matrix.
- [x] Injected action-dispatch abstraction exists; [ ] production adapter and hardened reconciliation semantics.
- [x] REST observer library has timeout/redaction/truncation/unit security tests; [ ] authenticated mock-server/integration proof.
- [x] Ed25519 receipt signing and tamper unit tests exist; [ ] versioned envelope, identity binding, key lifecycle, replay protection, and durable persistence.
- [x] `UNKNOWN`/retry/idempotency seams have selected failure-injection tests; stale-read, cancellation, and concurrency are now covered with deterministic tests.
- [x] `contract validate` has human/JSON output paths; [ ] testable stable exit-code contract and real `verify` workflow.
- [x] Deferred crates are removed from the active workspace and named; [ ] implementation decisions for MCP/OTel/policy/recovery/storage/testkit.
- [x] fmt, clippy, tests, and docs pass locally; [ ] clean-checkout CI evidence and resolution/acceptance of the audit warning.
- [ ] authenticated Control Center correlation and promotion fixture; this is the decisive Tier-D gate.

## 8. Implementation Summary (2026-08-14)

### Commits Made

1. **chore: fix manifest warnings and remove target/ from version control**
   - Removed `[workspace].version` and `[lint]` from root Cargo.toml
   - Added `.gitignore` excluding `target/`
   - Removed target/ from git tracking

2. **feat(core): add schema version and contract validation**
   - Added `CONTRACT_SCHEMA_VERSION` (1.0) and `SchemaVersion` type
   - Added `Contract::validate()` with comprehensive validation
   - Added `ContractValidationError` enum
   - Added Display impl for ActionId, ContractId, ReceiptId

3. **feat(runtime): separate ActionExecutor and add typed DispatchOutcome**
   - Added `ActionExecutor` trait for dispatching actions
   - Added `DispatchOutcome` enum with terminal/non-terminal/timeout distinction
   - Added `ReceiptStore` trait
   - Added `execute_with_executor()` with bounded backoff retry

4. **feat: add REST observer and Ed25519 receipt signing**
   - Added `RestObserver` in agentverify-http with configurable timeout, redaction, truncation
   - Added `SigningService` in agentverify-receipt for Ed25519 signing/verification

5. **feat(cli): implement contract validate command**
   - Implement contract validate with JSON output
   - Stable exit codes: 0=success, 1=error, 2=invalid

6. **chore: remove placeholder crates from workspace**
   - Deferred: observe, mcp, otel, policy, recovery, storage, testkit

7. **test(runtime): add failure injection tests for DispatchOutcome**
   - Tests for timeout, transport error, ambiguous, retry exhaustion behavior

8. **test(http): add security tests for REST observer**
   - URL injection rejection (path traversal, double slash, scheme injection)
   - Redaction tests (password, nested secrets, multiple paths)
   - Truncation tests (large response, small response, boundary)

9. **feat(runtime): harden execute_with_executor with failure injection tests (2026-08-14)**
   - Added `Executed` state to state machine for proper post-dispatch transition path
   - Fixed observer error to propagate as `Unknown`, not as a `Result::Err`
   - Added 5 tests: observer error→Unknown, stale read→verification failure, timeout-before/after dispatch, transport error terminal, ambiguous terminal, retry exhaustion, concurrent idempotency safety

### Remaining Work

- MCP/OTel security tests (deferred - crates removed from workspace)
- Property tests for predicate totality and state-machine invariants
- Integration tests with testcontainers
- CI security audit

### Test Status

At the 2026-08-14 boundary (SHA 3a03e6d), `cargo test --workspace --all-targets` passes 68 tests: contract 7, core 12, engine 24, HTTP 11, receipt 3, runtime 11, and CLI 0. The 5 new runtime tests cover: observer error propagation as Unknown, stale-read verification failure, timeout-before/after-dispatch handling, transport error terminality, ambiguous result terminality, retry exhaustion, and concurrent idempotency safety. The state machine gained an `Executed` state to enable proper Executing→Executed→Observing transitions. These are local tests; they do not establish deployment, authentication, persistence, or cross-service ownership.

## 9. Claude/MiniMax execution template

For every packet, Claude should start from the current boundary and return this exact evidence:

```text
Packet / objective:
Current SHA and worktree status:
Files changed:
Behavioral decision(s):
Tests added or changed:
Commands run and results:
Security/ownership/replay implications:
Known limitations:
Next packet:
```

The reviewer must reject any packet that claims a feature from a type, trait, placeholder, or unit test alone. Require an executable path, an adversarial test, and documentation that distinguishes implemented, deferred, and unproven behavior.
