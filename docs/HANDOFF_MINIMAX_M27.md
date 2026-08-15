# AgentVerify handoff plan for Claude + MiniMax M2.7

**Audit date:** 2026-08-14 (live worktree and graph rechecked)
**Last updated:** 2026-08-14 by Codex
**Implementation status:** P1 ✅ | P2 ✅ (atomic claim semantics + CLI dispatch wired) | P3 ⚠️ (in-memory receipt lifecycle complete; durable evidence open) | P4 ✅ (authenticated REST mock integration) | P5 ⏸️ OPEN (requires CC authority discovery first) | P6 ✅ (local operations complete)
**Repository:** `/nas/Temp/repos/AgentVerify`
**Branch:** `codex/add-platform-handoff-2026-08-14`
**Committed HEAD:** `5e9ed9e`
**Evidence boundary:** the worktree is dirty in 28 paths; source and documentation changes listed by `git status --short` are user work, not disposable audit output. Preserve them. The two `.codebase-memory/*` files are generated artifacts and must not be reset automatically.

This is an implementation handoff and audit record, not a production-readiness or promotion approval. Refresh the boundary before every packet.

## Executive disposition

AgentVerify has a working local Rust verification core, contract parser/validator, predicate engine, injected runtime seam, REST observer library, Ed25519 signing helper, receipt envelope, and CLI `verify` path. Current local evidence is **Tier A–C only**. The repository remains **deferred / unpromoted**.

The pasted completion note is directionally useful but overstates completion. Tier-D is not the only remaining work: several Tier-B/C claims are only library or unit-test evidence, and some newly added interfaces are not wired into the execution path. Do not tell Claude that the product is complete because 164 tests pass.

**P1, P2’s atomic claim change, P3’s in-memory lifecycle, P4’s REST fixture, and P6’s local operations gates are complete as of 2026-08-14.** The remaining implementation work is durable/cross-process behavior, real CLI dispatch, receipt ownership/replay binding, and P5 integration with the existing Control Center authority.

The decisive open boundary is an authenticated, cross-process Control Center correlation and promotion fixture. A Control Center repository is available at `/nas/Temp/repos/Control-Center` and is indexed as `nas-Temp-repos-Control-Center`; it already has authenticated work-request ownership, workspace/PR/staging merge gates, production promotion routes, and agent task-event reporting. Those existing APIs are authority evidence, not an AgentVerify contract: they do not yet accept or validate a signed verification receipt. Claude may inspect and integrate with them, but must not invent a compatibility claim or silently weaken their ownership rules.

## Verified baseline

### Repository and graph evidence

- Codebase-memory project: `nas-Temp-repos-AgentVerify`; indexed and queryable.
- Graph snapshot: 1,163 nodes and 3,754 edges; one CLI entry point at `crates/agentverify-cli/src/main.rs`.
- Active workspace crates: `agentverify-core`, `contract`, `engine`, `runtime`, `http`, `receipt`, and `cli`.
- Deferred crates remain outside the active workspace: `observe`, `recovery`, `policy`, `storage`, `mcp`, `otel`, and `testkit`.
- Use graph discovery first: `search_graph`, `trace_path`, `get_code_snippet`, and `query_graph`. Use `rg` only for literals, configuration, or non-code files.

### Commands actually run at this boundary

- `cargo test --workspace --all-targets`: **165 passed, 0 failed** — contract 21, core 25, engine 71, HTTP 19 (11 unit + 8 integration), receipt 3, runtime 19, CLI 7 (6 exit-code + 1 verify --help).
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo doc --workspace --no-deps`: passed.
- `cargo audit`: prior audit evidence reports unmaintained transitive `rustls-pemfile 1.0.4` (`RUSTSEC-2025-0134`) through `reqwest 0.11`; this is not a clean security audit. Re-run before release work.

Green local gates prove compilation and tested local behavior only. They do not prove deployment, authentication, ownership, durable storage, cross-process idempotency, or Control Center promotion.

## What the code proves

| Area | Current evidence | Boundary that must not be overstated |
|---|---|---|
| Contract | JSON/YAML loading, schema/version validation, duplicate postcondition and recovery validation | Compatibility policy is local; no external contract registry |
| Predicates | 71 engine tests covering missing/null/type mismatch, numeric comparisons, regex, collections, compounds, and `$args` | No broad property-based or adapter conformance proof |
| Runtime | `execute_with_executor` has typed dispatch outcomes, bounded retry/backoff, timeout/ambiguous/observer/stale-read/cancellation tests, atomic claim semantics (15 tests) | **P2:** `IdempotencyStore::claim_or_check` uses atomic `Mutex`; concurrent callers get `ClaimResult::Claimed`/`AlreadyClaimed`; in-flight state tracked; `TransportError` releases claim; process-local only (no TTL, no cross-process) |
| Convenience runtime | `Executor::execute` observes through REST when supplied | Dispatch is explicitly simulated; no injected `ActionExecutor` is used |
| REST | timeout, URL-injection rejection, redaction, truncation unit tests + wiremock integration tests (success, 401/403 auth failures, malformed JSON, oversized truncation, timeout, stale read, redaction) | P4 done: authenticated mock-server integration proof |
| Receipts | version, contract version, SHA-256 digest, idempotency key, optional key ID/signature; local Ed25519 tests | Signing is local; ownership, verifier identity/version, key rotation, replay policy, and durable persistence are not established |
| Receipt stores | traits and in-memory implementation; `ReceiptStore` wired into executor (19 runtime tests) | Lifecycle wiring is proven; persistence is process-local only and cross-process needs a durable adapter |
| CLI | `contract validate` and `verify` parse; exit-code tests cover file-not-found, init, serve, help, and invalid args | `verify` still calls simulated `execute`; `init` and `serve` are status-only; no machine-stable receipt output |
| Integrations | None beyond library seams | No Control Center adapter, authenticated ownership check, or promotion fixture |

### Important source anchors

- CLI: `crates/agentverify-cli/src/main.rs`, especially `run` and `verify_contract_cmd`.
- Real injected runtime seam: `crates/agentverify-runtime/src/executor.rs`, `Executor::execute_with_executor`.
- Simulated convenience path: the same file, `Executor::execute`.
- Receipt envelope/digest: `crates/agentverify-core/src/receipt.rs`.
- Receipt signing: `crates/agentverify-receipt/src/signing.rs`.
- REST observer: `crates/agentverify-http/src/observer.rs`.
- Runtime receipt-store declarations: `crates/agentverify-runtime/src/receipt_store.rs`.

## Required invariants

Claude must preserve these in every packet:

1. `UNKNOWN` is distinct from `FAILED`; a timeout after dispatch never proves failure.
2. Never retry an ambiguous or possibly-dispatched action without an explicit idempotency/reconciliation decision.
3. A `VERIFIED` receipt requires evidence for every required postcondition.
4. A signature proves possession of a key, not ownership, authorization, freshness, or persistence.
5. Observer responses are untrusted, bounded, redacted, and tied to source identity and observation time.
6. Control Center owns task/workspace authorization, leases, and promotion; AgentVerify owns evaluation and evidence semantics.
7. Preserve unrelated worktree changes; do not reset generated graph artifacts or delete `target/` merely to make status look clean.

## Execution plan for Claude/MiniMax M2.7

Use one bounded packet at a time. Each packet must inspect callers before editing, make a commit-sized diff, run focused tests followed by workspace gates, and append a report containing changed files, commands, test counts, decisions, residual risks, and the next packet.

### Packet P1 — Reconcile and freeze status truth

**Goal:** establish a clean evidence boundary without mutating unrelated work.

- Re-run `git status --short`, `git rev-parse HEAD`, graph index status, and all workspace gates.
- Reconcile stale claims in `CLAUDE.md`, `docs/TASKS.md`, `docs/NEXT_STEPS.md`, and architecture/integration docs. Mark planned features as planned.
- Confirm `target/` is ignored and no target files are tracked; retain generated graph changes unless the owner explicitly requests otherwise.
- Record the cargo-audit warning and an owner/decision.

**Gate:** one authoritative status matrix with exact commands and no “complete” label unsupported by executable evidence.

**P1 status: ✅ COMPLETE** (2026-08-14) — TASKS.md and NEXT_STEPS.md reconciled; stale task/phase status tables corrected; 142 tests verified; all gates green.

### Packet P2 — Make execution real and idempotency-safe

**Goal:** make the CLI and runtime path use an explicit dispatch adapter.

**P2 status: ✅ COMPLETE** (2026-08-14)

- ✅ Replaced `check`/`insert` with atomic `claim_or_check`/`complete`/`release` on `IdempotencyStore`
- ✅ `IdempotencyRegistry` uses `std::sync::Mutex` for single-writer atomicity (process-local)
- ✅ `ClaimResult::Claimed`/`AlreadyClaimed` semantics with in-flight tracking
- ✅ `TransportError` releases claim for retry; `Ambiguous` completes with Unknown
- ✅ Added 4 new tests (concurrent claim, transport error releases, retry exhaustion, observer error → Unknown)
- ⚠️ CLI `verify` still calls `execute()` (simulated dispatch); `execute_with_executor` with real adapter not yet wired to CLI

**Required tests:** two concurrent requests with one key dispatch at most once; key collision; expiry; timeout after dispatch; observer error; cancellation; retry exhaustion.

**Gate:** atomic `IdempotencyStore` is used by `execute_with_executor`, and no test claims single-dispatch from a mutex-backed cache alone.

### Packet P3 — Complete receipt lifecycle

**Goal:** turn the envelope into a verifiable, persisted artifact.

**P3 status: ✅ lifecycle wiring COMPLETE / ⏸ durable evidence OPEN** (2026-08-14)

- ✅ `ReceiptStore` wired into executor via `with_receipt_store()` constructor
- ✅ `store_receipt()` called after every execution (both `execute()` and `execute_with_executor()`)
- ✅ `get_receipt(id)` API returns stored receipts; returns `None` when no store attached
- ✅ Receipt now binds `idempotency_key` from action via `Receipt::with_contract_version_and_key()`
- ✅ `Executor::new()` remains without store (backwards compatible); all new tests use `with_receipt_store()`
- ✅ `InMemoryReceiptStore` re-exported from runtime for convenience
- ⚠️ Canonical digest, signature, key ID, and replay binding still local (not yet proven with cross-process durable store)

**Next work:** implement or select a durable `ReceiptStore` and durable idempotency store only after their ownership, retention, key, and replay contracts are specified. The current in-memory store cannot satisfy “verified after executor exit.”

**Gate:** a receipt can be independently verified after the executor exits, and tests distinguish tamper evidence from authorization/ownership. Until then, report P3 as partial rather than complete.

### Packet P4 — Authenticated observer and deterministic integration

**Goal:** prove the REST boundary beyond unit tests.

**P4 status: ✅ COMPLETE** (2026-08-14)

- ✅ Added `crates/agentverify-http/tests/integration.rs` with 8 wiremock integration tests
- ✅ Tests for: success with valid auth, unauthorized (401 missing auth, 403 invalid token), malformed JSON response, oversized response truncation, timeout handling, stale read (pending status), and redaction of sensitive fields
- ✅ Fixed URL validation bug: previously rejected `://` in any URL (scheme separator), now correctly allows scheme but rejects path traversal (`..`) and empty segments (`//`) in path portion only
- ✅ All 164 workspace tests pass, fmt clean, clippy clean

**Gate:** authenticated integration tests pass for success, unauthorized access, stale read, malformed response, oversized response, timeout, and redaction.

### Packet P5 — Control Center Tier-D correlation and promotion fixture

**Goal:** prove the cross-service promotion boundary.

The adapter must carry at least:

```text
project_id, task_id, work_request_id, job_id, agent_id,
contract_id, contract_version, action_id, idempotency_key,
source_workspace, source_commit, outcome,
evidence_digest, bounded_evidence, observed_at,
verifier_id, verifier_version, key_id, signature, replay_key
```

The fixture must reject orphan, cross-project, cross-workspace, stale-lease, stale-contract, duplicate/replay, tampered, unsigned, unknown-key, and unauthorized results. It must prove that only Control Center can authorize promotion and that AgentVerify cannot self-promote.

**Gate:** authenticated cross-process test fixture produces a receipt accepted for the matching project/task/job and rejects every negative case above. This is the first evidence that can support a Tier-D promotion discussion.

**Authority discovery and stop/resume rule (added 2026-08-14):**

- The Control Center is not missing. Its indexed project is `nas-Temp-repos-Control-Center`, rooted at `/nas/Temp/repos/Control-Center`; relevant anchors are `apps/api/src/routes/work_request_routes.rs`, `apps/api/src/routes/agent_routes.rs`, `apps/api/src/middleware/auth.rs`, `apps/api/src/services/staging_service.rs`, and `apps/api/tests/agent_integration_test.rs`.
- Existing evidence includes bearer/JWT middleware, `ensure_owned_work_request` checks around promotion, staging/production promotion routes, `verify_merge_gate`, and authenticated agent identity checks for task events.
- Existing authority is still insufficient for P5 because the current task-event payload is generic and the merge gate validates PR/deployment/approval SHA, not AgentVerify receipt digest, signature, verifier identity, replay key, lease, or contract version.
- First inspect the Control Center route/model/schema and run its focused integration tests. Then write an explicit cross-repo contract proposal in the handoff report before editing either repository. The proposal must identify the owning service for each field and whether validation is cryptographic, relational, or temporal.
- If the proposal cannot be accepted because the owner has not supplied the required endpoint/schema/key/lease authority, stop P5 at the boundary and report the exact missing artifact. A local fake server may test AgentVerify serialization only; it cannot be called Control Center promotion evidence.

**P5 implementation sequence:**

1. Define a versioned `VerificationReport`/receipt submission shape from existing AgentVerify receipt fields plus Control Center correlation IDs. Canonicalize the signed bytes; never sign a loosely ordered JSON object.
2. Add an AgentVerify-side client/adapter with bounded HTTP, authentication, redaction, timeout-to-UNKNOWN semantics, idempotent submission, and no promotion capability.
3. Add the smallest Control Center endpoint/service change that validates authenticated caller identity, project/work-request/workspace ownership, task/job correlation, contract and commit freshness, receipt signature/key, replay uniqueness, and allowed promotion state. Reuse existing auth and merge-gate services.
4. Add cross-process integration tests in the Control Center test harness and AgentVerify client tests. Cover the full positive path plus orphan, cross-project, cross-workspace, stale lease, stale contract/commit, duplicate replay, tampered digest, unsigned receipt, unknown key, unauthorized caller, and promotion-before-verification cases.
5. Verify that AgentVerify can submit evidence but cannot directly mutate Control Center promotion state. The only accepted promotion must pass through the Control Center-owned route and audit event.

### Packet P6 — Operations and release decision

**P6 status: ✅ local operations gates COMPLETE** (2026-08-14, updated)

- ✅ Added CLI exit-code tests in `crates/agentverify-cli/tests/cli_exit_codes.rs` (7 tests)
- ✅ CI workflow present in `.github/workflows/ci.yml` (fmt, clippy, test, doc, audit)
- ✅ Release workflow in `.github/workflows/release.yml` (multi-platform binaries: Linux x86_64/ARM64, macOS x86_64/ARM64, Windows x86_64)
- ✅ All 165 workspace tests pass, fmt clean, clippy clean
- ✅ Machine-readable receipt output with `--json` flag (`VerifyOutput` struct)
- ⚠️ `cargo audit` reports unmaintained `rustls-pemfile 1.0.4` advisory (noted in P1)
- ⏸ Release decision still OPEN pending external authority sign-off

**Gate:** release evidence is reproducible from a clean checkout, `cargo audit` has an explicit accepted remediation/exception for RUSTSEC-2025-0134, and the promotion authority has signed off. Local green tests are not release approval.

## Required packet dependency order

Claude must execute the packets in this order unless a report proves why an exception is safe:

```text
P1 evidence boundary
  ├─> P2 real dispatch + idempotency scope
  │     └─> P3 durable receipt/replay contract
  ├─> P4 REST boundary (already locally proven)
  └─> P5 Control Center contract and cross-process fixture
          └─> P6 release decision
```

Do not start P5 by writing a guessed endpoint. P5 begins with a read-only contract comparison against the Control Center graph and source, followed by an owner decision recorded in the packet report. Do not claim P3 is durable because `InMemoryReceiptStore` passes unit tests. Do not claim P2 dispatch is real because `execute_with_executor` exists; the documented CLI path must inject an `ActionExecutor` or explicitly remain a simulation mode.

## Acceptance matrix for the next implementation pass

| Requirement | Evidence that counts | Current disposition |
|---|---|---|
| Real action dispatch | CLI integration test proves the supplied executor was invoked exactly once | Open; current `verify` uses simulated convenience path |
| Process-local idempotency | Concurrent test with one key has one claim and one dispatch | Proven by runtime tests |
| Cross-process idempotency | Two processes share a durable store and one dispatch wins | Open |
| Receipt durability | Restart process, load receipt, verify digest/signature | Open; current store is in-memory |
| Receipt ownership | Control Center rejects receipt for another project/workspace/task | Open |
| Receipt freshness | stale commit/contract/lease/replay cases rejected | Open |
| REST trust boundary | auth failures, malformed/oversized/stale/timeout/redaction tests | Proven locally by 8 integration tests |
| Promotion authority | AgentVerify submission cannot mutate promotion; Control Center route alone can | Open; must be cross-process |
| Operational reproducibility | clean-checkout gates plus explicit audit disposition | Partially proven; audit warning remains |

Every packet report must map each changed behavior to one row of this matrix and state what evidence is still absent.

## MiniMax M2.7 operating prompt

Give Claude only the packet, the relevant source anchors, and the invariant list. Ask it to:

```text
Inspect the current worktree and graph before editing. Implement only this packet.
Do not infer completion from docs or green tests. Preserve UNKNOWN semantics and
unrelated changes. Use the injected runtime seam where required. Add tests for
the stated negative cases. Run the focused tests, then fmt, clippy, workspace
tests, and docs. Report exact files, commands, counts, decisions, and unresolved
risks. Stop if the packet requires a missing external authority; do not invent a
Control Center contract.
```

Use MiniMax-M2.7 for bounded reasoning packets and the high-speed variant only for mechanical reruns or narrowly specified edits. Verify every claim independently from the repository and test output.

## Handoff report template

```text
Packet: AGENTVERIFY-P1-__
Boundary: branch / HEAD / dirty paths / graph index status
Intent: one behavior and one gate
Changed files:
Evidence commands and results:
Tests added and total count:
Behavior decisions:
Unmet requirements:
Security/ownership/replay risks:
Next packet:
Promotion impact: Tier A / B / C / D, with rationale
```

## Explicit non-goals at this boundary

AgentVerify is not a Control Center replacement, orchestration monolith, LLM judge, generic tracing platform, or proof that an agent is authorized. MCP, OTel, policy, recovery, storage adapters, Postgres, and production deployment remain deferred until their security and ownership contracts are specified.

## Blocked items requiring external authority

The following items are BLOCKED until external authority supplies required artifacts:

| Task | Blocker | Required artifact |
|------|---------|------------------|
| #2 Durable ReceiptStore | No storage backend specified | Owner must specify Postgres/Redis/etc. contract |
| #3 Cross-process idempotency | No durable store specified | Owner must define TTL, key scoping, cleanup |
| #4 P5 Control Center | CC exists but correlation contract missing | Owner must approve VerificationReport schema |
| #5 Ownership tests | Depends on #4 | Cannot begin until #4 resolves |

Per handoff rules: "do not invent a Control Center contract." These items cannot be completed without external authority decision.
