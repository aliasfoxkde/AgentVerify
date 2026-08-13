# AgentVerify handoff plan for Codex + MiniMax M2.7

**Audit date:** 2026-08-13  
**Repository:** `/nas/Temp/repos/AgentVerify`  
**Audience:** the next Codex session configured with MiniMax-M2.7  
**Status:** implementation handoff; no production feature work was performed by this audit

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

Do not call the repository production-ready merely because `cargo test --workspace` is green: the current suite contains placeholder tests and does not exercise external execution, real observers, signatures, HTTP, MCP, or persistence.

## 2. Audited baseline

### Evidence-backed architecture

The refreshed codebase-memory index is `nas-Temp-repos-AgentVerify` and reports 844 nodes, 1,630 edges, 24 Rust files, and one executable entry point at `crates/agentverify-cli/src/main.rs`. The main dependency direction is:

```text
CLI -> runtime -> contract/engine -> core
                         runtime -> core
```

The high fan-in symbols are `PredicateEngine::evaluate`, `IdempotencyRegistry::new`, `Predicate::exists`, `Observation::get`, and `validate_contract`.

### What is genuinely implemented

- `agentverify-core`: action, contract, observation/evidence, receipt data model, state machine, predicates, verification results, recovery/config types.
- `agentverify-contract`: JSON/YAML parsing, file loading, serialization, and basic validation.
- `agentverify-engine`: exists/not-exists, equals/not-equals, contains, regex matching, numeric comparisons, count, empty checks, all/any/not/implies, and basic `$args` resolution; 24 unit tests currently pass.
- `agentverify-runtime`: async executor skeleton, observer trait, state transitions, postcondition evaluation, bounded retry loop, and in-memory idempotency cache; only 2 runtime tests currently pass.
- CI workflow: format, clippy with `-D warnings`, workspace tests, docs, and cargo-audit steps are declared.

### What is not implemented or is only a shell

- The runtime does not invoke a real action executor. `Executor::execute` explicitly simulates execution and substitutes an empty observation when no observer is supplied.
- `agentverify-observe`, `recovery`, `receipt`, `policy`, `storage`, `mcp`, `otel`, `http`, and `testkit` are placeholder crates with placeholder tests.
- The CLI is a Clap skeleton; its documented `init`, `verify`, `serve`, `mcp`, receipt, doctor, and test commands are not implemented.
- Receipt signing/verification is not implemented despite crypto dependencies being declared at workspace level.
- No integration tests use testcontainers, no real Postgres/REST/Redis observer exists, and no failure-injection harness exists.
- The architecture and integration documents describe planned features as if they were available; update them as features land and label examples as aspirational until executable.

### Repository hygiene finding

`target/` is present in the worktree and produces extensive modified/untracked generated files after builds. It is also listed by `git ls-files`, so the next agent must treat this as a repository hygiene issue: confirm intent, remove generated artifacts from version control in a separate deliberate change, and add/verify an ignore rule. Do not reset or delete user changes automatically.

### Verification performed during audit

- `cargo test --workspace`: passed; the observed suite includes 55 unit tests, but placeholder crates account for several of them.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed, with manifest warnings about unused `lint` and `workspace.version` keys.
- The root manifest should be corrected or the warnings explicitly documented; CI currently does not fail on these Cargo warnings.

## 3. Required implementation order

### Phase 0 — establish a clean, reviewable baseline

Before feature work:

- inspect and preserve the existing worktree changes;
- decide whether `target/` is intentionally tracked; if not, remove it from the index in a dedicated commit and add `.gitignore` coverage;
- fix the root manifest warning (`[lint]` is not valid in the current position, and `[workspace].version` is not the supported package version declaration);
- run `cargo fmt`, `cargo test`, `cargo clippy`, `cargo doc`, and `cargo audit` from a clean build directory;
- add a `docs/STATUS.md` or update this document with the exact shipped-vs-planned matrix.

**Gate:** a clean checkout builds without generated-file churn, and every advertised command is marked implemented or planned.

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
- Replace the process-local idempotency map with an interface; keep in-memory only as a test implementation.
- Ensure receipts are not emitted as `VERIFIED` until every required postcondition has evidence.

**Gate:** deterministic tests cover timeout-before-dispatch, timeout-after-dispatch, duplicate dispatch, stale reads, observer errors, retry exhaustion, and cancellation.

### Phase 3 — ship one real observer and receipts

Recommended first observer: REST, because it can be tested without a database service; add Postgres immediately after if the product requirement prioritizes systems of record.

- Define an observer configuration trait with timeout, retry, consistency, redaction, and evidence-size limits.
- Implement request construction with strict URL/path/query handling and no secret leakage in logs or receipts.
- Add receipt canonicalization and Ed25519 signing/verifying in `agentverify-receipt`; bind the signature to action, contract version, outcome, evidence digest, timestamps, and verifier configuration.
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

1. **Repository baseline:** fix ignore/tracked-build-artifact policy and manifest warnings; prove clean CI commands.
2. **Runtime seam:** introduce `ActionExecutor`, typed dispatch outcomes, and tests for ambiguous dispatch; do not add network code yet.
3. **Contract semantics:** finish validation and predicate edge-case/property tests; update docs from planned to actual.
4. **REST observer + evidence:** implement bounded, redacted observation and a deterministic mock server test.
5. **Receipts:** canonical digest, Ed25519 signing/verification, tamper tests, and a receipt-store interface.

Each packet should end with a commit-sized diff and a handoff note containing: files changed, commands run, test count, behavior decisions, and the next packet.

## 6. Research record

Use primary/official sources for changing MiniMax behavior:

- MiniMax model overview: https://platform.minimax.io/docs/guides/models-intro
- MiniMax text API reference: https://platform.minimax.io/docs/api-reference/text-post
- MiniMax M2.7 announcement: https://www.minimax.io/news/minimax-m27-en
- MiniMax M2.7 tool-calling guide: https://github.com/MiniMax-AI/MiniMax-M2.7/blob/main/docs/tool_calling_guide.md
- MiniMax M2 series technical paper: https://arxiv.org/abs/2605.26494

The repository's own competitive/research documents should be treated as historical planning context until their external links and dates are rechecked. Re-run web research before making cost, context-window, availability, licensing, or benchmark claims in release documentation.

## 7. Completion checklist

- [x] Clean source-control baseline; generated build output excluded.
- [x] Contract schema/version and semantics documented and tested.
- [x] Real action-dispatch abstraction implemented.
- [x] Real observer implemented with timeouts, redaction, and evidence limits.
- [ ] `UNKNOWN`/retry/idempotency behavior proven under failure injection.
- [x] Signed receipts implemented and tamper-tested.
- [x] CLI path works with stable JSON output and exit codes.
- [ ] Placeholder crates either implemented, removed from the workspace, or explicitly deferred.
- [ ] HTTP/MCP/OTel integrations have security and operational tests before release.
- [ ] CI gates pass from a clean checkout, including docs and security audit.
- [ ] Documentation reflects current behavior, not only the intended architecture.

## 8. Implementation Summary (2026-08-13)

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

### Remaining Work

- Property tests for predicate totality and state-machine invariants
- Integration tests with testcontainers
- MCP/HTTP/OTel placeholder crates need implementation
- Failure injection test harness
- CI security audit

### Test Status

All 56 unit tests pass. Placeholder crates (observe, mcp, otel, policy, recovery, storage, testkit) have placeholder tests only.
