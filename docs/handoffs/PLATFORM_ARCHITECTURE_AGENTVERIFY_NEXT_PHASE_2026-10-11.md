# AgentVerify next-phase integration/readiness handoff (2026-10-11)

**Author role:** Platform-Architecture execution agent
**Scope:** docs-only readiness audit of dispatch, idempotency persistence,
fail-closed behavior, and test strategy at the current `main` boundary;
accounting for the open PR stack #27 → #29 without modifying or duplicating
them; ranked next-phase tasks with acceptance gates. **No source code is
changed by this handoff.**
**Evidence boundary:** branch `main` @ `f55e17149b47c04eb53faa21c8b56b6de50a53d9`
(origin/main, fast-forwarded local clone, working tree clean except this
document). This boundary already contains PR #26 (post-claim error-exit claim
resolution) and PR #28 (Redis concurrency invariant test fix).
**Method:** codebase-memory index `nas-Temp-repos-AgentVerify` used for symbol
discovery only; every claim below was verified against the live source at
`f55e171` (line numbers refer to that tree). PR/CI state via `gh` (GitHub API)
on 2026-10-11. GitForge repo inventory via harness MCP on 2026-10-11. No tests
were run and no builds were executed: this is a source-read audit of a
docs-only branch.
**Claim tags:** **[confirmed]** = read from source, CI, or API at the SHAs and
URLs named. **[inferred]** = source-grounded reasoning about conditions not
directly exercised.
**Related authority (not superseded by this file):** `AGENTS.md`,
`docs/planning/HANDOFF.md`, `docs/internal/HANDOFF.md` (Tier A–D model),
`docs/internal/TASKS.md`,
`docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_2026-10-10.md` (prior-day
assessment; §1 of this file states what changed since it).

---

## 1. What changed since the 2026-10-10 handoff

The 2026-10-10 handoff assessed `main` @ `2b6d021` and predated the merge of
#26. This handoff is the next-phase continuation, not a re-statement. Delta
since that boundary **[confirmed]**:

| Change | State | Consequence for readiness |
|---|---|---|
| PR #26 merged (`25571b7`) | on `main` | Every post-claim error exit in the executor now resolves the idempotency claim (`Release` or `ConsumeUnknown`); `TransportError` is `UNKNOWN` in both `DispatchError` and `DispatchOutcome` shapes. The "abandoned in-flight key → false `Duplicate`" defect class is closed at the executor layer. |
| PR #28 merged (`f55e171`) | on `main` | Redis observer concurrency invariant test repaired. |
| PR #27 opened (draft) | NOT on `main` | Closes the *store-layer* fail-open claim semantics for `RedisIdempotencyStore` (adds `ClaimResult::Unavailable` + `ExecutorError::IdempotencyUnavailable`, refuses dispatch without a proven claim). All 10 GitHub Actions checks pass on its head. |
| PR #29 opened (draft) | NOT on `main` | Closes the same gap for `FileIdempotencyStore` (claim-persist failure → `Unavailable`, cache untouched). Stacked on #27 (`base` = #27's head branch; merge #27 first). No CI checks have run on it yet. |

Nothing in this document duplicates #27/#29 scope. Section 4 states exactly
what each covers; sections 3 and 5 cover what remains **after** both land.

## 2. Boundary snapshot

- `main` @ `f55e171`; 13 crates; static inventory of `#[test]`/`#[tokio::test]`
  functions: core 131, observe 111, engine 108, runtime 97, policy 66, mcp 63,
  recovery 55, http 51, storage 22, contract 21, testkit 8, cli 7, receipt 5,
  otel 3 — **748 declared test functions** (grep count, not a run count)
  **[confirmed]**.
- Open PRs: #27 (draft, CI green), #29 (draft, stacked, no CI runs),
  #25 + #8–#17 (11 dependabot/action bumps, including redis 0.24.1 → 1.6.0 in
  #13 and axum 0.7.9 → 0.8.9 in #11) **[confirmed]**.
- GitForge: the GitForge repo inventory (21 repos, read 2026-10-11) contains
  **no AgentVerify repository** **[confirmed]**. AgentVerify's merge gates are
  GitHub Actions only; the `AGENTS.md` guidance to route expensive validation
  "through GitForge" has no repo-level seam to route through today.

## 3. Audit findings at `f55e171`

### 3.1 Dispatch

- The only shipped action adapter is `SimulatedActionExecutor`
  (`crates/agentverify-runtime/src/action_executor.rs:104-128`): it returns
  `Ok(DispatchOutcome::Completed)` unconditionally. The CLI `verify` command
  wires it (`crates/agentverify-cli/src/main.rs:386-388`) — the `verify`
  command still verifies nothing that was externally dispatched, exactly as
  `AGENTS.md` warns. **[confirmed]**
- Retry discipline is sound: `DispatchOutcome::should_retry` is true only for
  the two timeout variants (`action_executor.rs:85-91`), i.e. re-dispatch is
  advised only where the external state must be observed first
  (verify-before-retry). Terminal classification is `Completed` |
  `TransportError` | `Ambiguous` (`action_executor.rs:139-147` tests)
  **[confirmed]**.
- `RecoveryStrategy` retries on `UNKNOWN` and treats `DUPLICATE` as a
  successful stop (`crates/agentverify-recovery/src/lib.rs:999-1023`,
  `1485-1493`) — consistent with UNKNOWN-is-not-FAILED. **[confirmed]**
- `McpClient::dispatch` exists (`crates/agentverify-mcp/src/client.rs:380-412`)
  but is an MCP tool-call client, not a production outcome-bearing dispatch
  adapter. **[confirmed]**

### 3.2 Idempotency persistence

- Executor-side claim machinery (post-#26) is coherent: explicit
  `ClaimPolicy::{Release, ConsumeUnknown}` chosen at every error exit
  (`executor.rs:244-259`), default-safe adapter-error classification that
  releases only `ActionNotSupported | TimeoutBeforeDispatch | Rejected` and
  consumes-as-`UNKNOWN` everything else including future variants
  (`executor.rs:396-405`), and resolution helpers that cannot be bypassed by
  `?` on the state-machine path (`executor.rs:332-384`). **[confirmed]**
- Store implementations on `main`:
  `IdempotencyRegistry` (in-memory), `FileIdempotencyStore`,
  `RedisIdempotencyStore` (`feature = "redis"`), plus `WasmIdempotencyStore`
  in `agentverify-http` and `MockIdempotencyStore` in `agentverify-testkit`.
  **No PostgreSQL idempotency store exists** — the trait doc names PostgreSQL
  as a suggested durable implementor (`executor.rs:99-102`), but nothing
  implements it. **[confirmed]**
- `FileReceiptStore` is single-process by its own doc ("not suitable for
  multi-process access without file locking", no TTL —
  `crates/agentverify-core/src/receipt.rs:406-407`). Receipt persistence
  failure is logged, non-fatal (`executor.rs:316-330`): the verdict stands,
  the evidence gap is observable. **[confirmed]**

### 3.3 Fail-closed behavior (what #27/#29 close, and what they do not)

On `main` (without #27/#29), every store error is converted into a **lying
claim answer** rather than a refusal **[confirmed]**:

| Site (at `f55e171`) | Error | Answer returned | Consequence |
|---|---|---|---|
| `idempotency_store.rs:321-324` | Redis pool get fails | `(Claimed, None)` | Dispatch with **no claim at all** |
| `idempotency_store.rs:331-334` | entry serialization fails | `(Claimed, None)` | Dispatch with no durable claim |
| `idempotency_store.rs:394-397` | `SET NX EX` command error | `(AlreadyClaimed, None)` | Fabricated occupancy; caller denied a dispatch that was actually never claimable |
| `idempotency_store.rs:387-390` | expiry-race retry `SET NX` error | `(AlreadyClaimed, None)` | Same fabricated occupancy |
| `idempotency_store.rs:181-185` | file claim persist fails | `(Claimed, None)` + cache insert | **Phantom claim**: dispatch with no durable claim, and later in-process calls see `AlreadyClaimed` from cache |

#27 replaces the four Redis rows with `ClaimResult::Unavailable` (new
variant) refused by the executor as retryable
`ExecutorError::IdempotencyUnavailable` **before** any dispatch;
#29 replaces the file-store row the same way and leaves the cache untouched.
Both keep the genuine expiry race (`GET` nil → retry `SET NX`) as the only
claim retry. **[confirmed — PR bodies and diffs; the `main` rows above are
source-verified at `f55e171`]**

Residual fail-closed gaps **not** covered by #27/#29 (source-verified on
`main`; #29's "Limitations" explicitly defers the first two):

- **Redis completion writes are silently discarded on error.**
  `RedisIdempotencyStore::complete` ignores the `SET` result entirely
  (`idempotency_store.rs:428-435`: `let _: Result<(), _> = ...`); unlike
  `release` (which warns at `:449`) a failed completion write is not even
  logged. A lost completion leaves the key `InFlight` until TTL, so later
  callers get `(AlreadyClaimed, None)` — the unresolved-claim shape that reads
  as a false `Duplicate` downstream. **[inferred from the store state
  machine; the write-loss itself is confirmed by the discarded result]**
- **File-store completion divergence after restart.**
  `FileIdempotencyStore::complete` inserts the completed entry into the
  in-process cache regardless of whether `save_entry` succeeded
  (`idempotency_store.rs:190-205`). If the persist failed, the disk keeps
  `InFlight`; after a process restart the key reads as an unresolved claim,
  not the completed verdict. **[inferred, same basis]**
- **`complete`/`release` on the file store log persist failures but proceed**
  (`idempotency_store.rs:197-199`, `214-217`) — pre-existing, acknowledged in
  #29's limitations.
- **`WasmIdempotencyStore` is a separate claim taxonomy**
  (`crates/agentverify-http/src/wasm_storage.rs:140-147`:
  `ClaimResult::{Claimed, InFlight, Completed}` — a different enum from the
  runtime's), localStorage-backed, and untouched by #27/#29. It is also **not
  compiled by the WASM CI job**, which checks only core, contract, engine,
  receipt, and policy (`.github/workflows/ci.yml:212-219`). **[confirmed]**

### 3.4 UNKNOWN semantics vs. store TTL (interaction defect candidate)

`RedisIdempotencyStore::complete` writes consumed-as-`UNKNOWN` verdicts with
the **same** TTL as every other entry (`idempotency_store.rs:400-437`; default
24 h per the doc comment at `:257-259`, which frames TTL as allowing retry of
"genuinely failed actions"). After expiry the key vanishes, `claim_or_check`
answers `Claimed` again, and the only thing standing between a blind
re-dispatch and an action whose external effect is `UNKNOWN` is the caller's
verify-before-retry discipline. The store layer silently erases the
`UNKNOWN` verdict. The file store has no TTL, so the same verdict survives
there. **[confirmed mechanics; whether this is intended product semantics is
an owner question — §6 Q1]**

### 3.5 Test strategy and CI

- Evidence tiers follow `docs/internal/HANDOFF.md` (A: deterministic unit;
  B: runtime failure-injection; C: HTTP/receipt; D: authenticated
  cross-process — still open). Tier B seams present on `main`: timeout before
  and after dispatch, transport error, ambiguity, retry exhaustion, stale
  read, cancellation, observer error, idempotency double-dispatch
  (`executor.rs` test module; e.g. `:1461`, `:1499`, `:1536`, `:1572`,
  `:1607`, `:1654`, `:1714`, `:1856`) **[confirmed]**. Property tests
  (proptest) exist in core predicates and the engine **[confirmed]**.
- **CI never executes the feature-gated store tests.** The `Test` job runs
  `cargo test --locked --workspace` with default features
  (`.github/workflows/ci.yml:118-121`). It provisions Postgres and Redis
  services and exports `AGENTVERIFY_TEST_*` URLs (`ci.yml:78-104`), but with
  default features no `redis`/`postgres`-gated store code is compiled into
  the test binaries, so the services are idle. The `Coverage` job does run
  `--all-features` (`ci.yml:243-244`) but defines no service containers and
  no `AGENTVERIFY_TEST_*` env, so server-gated integration tests skip there.
  Net effect: on `main`, the Redis/file idempotency-store behavior that
  #27/#29 govern is only executor-mock-tested; the real store paths run
  nowhere in CI. **#27's CI change (run `--all-features` tests against the
  provisioned Redis service) fixes the Redis half** **[confirmed]**.
- Coverage gates are **informational**, not enforcing: codecov `project` and
  `patch` targets are 90 % with `informational: true` (`codecov.yml`), so the
  quality rules' 90 % core-crate requirement is not currently a merge gate.
  **[confirmed]**
- `RUSTFLAGS: -D warnings` on the Test job and dual-feature clippy
  (`default` + `all-features`) are present (`ci.yml:41-68`, `:121`)
  **[confirmed]**.

## 4. Accounting for #27 and #29 (no modification, no duplication)

Both are draft PRs owned by their authors; this handoff neither edits nor
reimplements them.

- **#27 — "fix(runtime): refuse dispatch on unprovable idempotency claims"**
  (draft, base `main`, all 10 Actions checks green, run 38106630891). Scope:
  Redis claim fail-closed (`ClaimResult::Unavailable`,
  `ExecutorError::IdempotencyUnavailable`, scripted-connection command-sequence
  tests, executor zero-dispatch tests, CI `--all-features` test job). Public
  API break pre-1.0, accepted by the PR.
- **#29 — "fix(runtime): fail file-store claims closed on persist error"**
  (draft, **base = #27's head branch** — strictly stacked). Scope:
  `FileIdempotencyStore::claim_or_check` → `(Unavailable, None)` on persist
  failure, cache untouched, executor-level zero-dispatch test with the real
  file store, deterministic `ENOENT` failure seam. Explicitly leaves
  `complete`/`release` non-fatal and `WasmIdempotencyStore` untouched.
- **Merge order is forced by the stack: #27 before #29.** #29 currently has
  zero CI runs (drafts skip review/CI in this repo); its gates run only once
  it is marked ready (after #27 lands and the stack base merges).
- **What remains after both land** is exactly §3.3's residual list and §3.4:
  silent Redis completion loss, file-store completion divergence, the
  `complete`/`release` non-fatal policy, the WASM claim taxonomy and its CI
  blind spot, and the UNKNOWN/TTL interaction. These are the subjects of the
  ranked tasks below; none of them re-open decided scope in #27/#29.

## 5. Ranked next-phase tasks

Priority legend: **P0** = blocks correct landing of in-flight work;
**P1** = correctness/observability debt that should land before any
integration promotion; **P2** = readiness/quality debt.

### P0-1 · Land the claim fail-closed stack in order

- **Action (owner):** review and merge #27, then mark #29 ready and merge it
  (rebase if `main` moved). Do not batch them into one merge.
- **Evidence:** PR URLs and CI run 38106630891 (§4).
- **Acceptance gates:** #27 merged with all Actions checks green including
  the new `--all-features` Redis test run; #29 undrafted, its checks green on
  the post-#27 base, stack merges without semantic conflicts in
  `idempotency_store.rs`; `.github/CHANGELOG.md` Unreleased entries for both.
- **CI/GitForge relevance:** GitHub Actions is the only gate (§2). If
  expensive workspace validation is routed through GitForge per `AGENTS.md`,
  note that no AgentVerify GitForge mirror exists today (§6 Q5).

### P0-2 · Close the feature-gated test blind spot in CI (Postgres half + coverage services)

- **Evidence:** `ci.yml:118-121` (default-features test run),
  `ci.yml:78-104` (services provisioned but unused by default-feature
  binaries), `ci.yml:243-244` (coverage `--all-features`, no services/env).
  #27 fixes only the Redis half.
- **Action:** after the stack lands, add the `AGENTVERIFY_TEST_POSTGRES_URL`
  env plus an `--all-features` (or explicit
  `--features postgres,redis`) test invocation to the Test job, and decide
  whether the Coverage job should share the service containers so
  server-gated paths count toward coverage.
- **Acceptance gates:** a CI run where the redis- and postgres-gated store
  test modules demonstrably execute (assert on test counts in the job log);
  no skip-cleanly regressions masked as passes.
- **CI/GitForge relevance:** pure GitHub Actions change; keep runtime
  `RUSTFLAGS: -D warnings`.

### P1-3 · Make Redis completion-write failure observable

- **Evidence:** `idempotency_store.rs:428-435` — `complete`'s `SET` result is
  discarded (`let _: Result<(), _>`); contrast `release`'s warned `DEL`
  (`:449`). Lost completion ⇒ key stays `InFlight` ⇒ false-`Duplicate`
  shape for later callers (§3.3).
- **Action (post-stack, coordinate rebases — #27 and #29 both touch this
  file):** at minimum `warn!` on completion error, matching `release`; the
  stronger option (align with the stack's philosophy: an unverifiable claim
  answer must not be fabricated) is an owner decision, not a spec in this
  handoff.
- **Acceptance gates:** scripted-connection test (pattern introduced by #27)
  proving a failed completion write is observable; existing claim tests
  unchanged.
- **CI/GitForge relevance:** covered by the post-P0-2 `--all-features` test
  job.

### P1-4 · Decide and pin the UNKNOWN-vs-TTL policy

- **Evidence:** §3.4 — consumed-as-`UNKNOWN` keys expire with the shared 24 h
  TTL (`idempotency_store.rs:400-437`, `:257-259`), after which the key is
  claimable again.
- **Action:** owner decision between (a) longer/permanent retention for
  `UNKNOWN`-consumed entries, (b) explicit reconciliation sweep before
  re-claim, or (c) documented acceptance of caller-side verify-before-retry
  as the sole guard. Whichever is chosen, pin it with a test and document it
  next to the TTL doc comment.
- **Acceptance gates:** a test that fails if an `UNKNOWN`-consumed key becomes
  claimable contrary to the decided policy; doc-comment update.
- **CI/GitForge relevance:** deterministic tests; no live-service dependency
  for option (a)/(c).

### P1-5 · File-store completion divergence after restart

- **Evidence:** `idempotency_store.rs:190-205` — `complete` caches the
  completed entry even when `save_entry` failed; disk keeps `InFlight`
  (§3.3). #29 fixes the claim direction only.
- **Action (after #29):** same decision shape as P1-3 (observability at
  minimum); add a restart-semantics test (write-completed-with-failed-persist
  → new store instance → observed state matches the decided policy).
- **Acceptance gates:** restart test pinning the decided behavior; no change
  to the #29 claim-path contract.
- **CI/GitForge relevance:** deterministic; runs in default-feature CI.

### P2-6 · Durable cross-process idempotency store (Tier D prerequisite)

- **Evidence:** trait doc names PostgreSQL as the durable implementor path
  (`executor.rs:99-102`); no implementation exists (§3.2); `FileReceiptStore`
  is single-process (`receipt.rs:406-407`); `docs/internal/HANDOFF.md`
  promotion gate requires "replay/idempotency behavior with atomic claim
  semantics … cross-process still needed".
- **Action:** either implement a Postgres (or equivalent) idempotency store
  with an atomic claim (`INSERT … ON CONFLICT` semantics) conforming to the
  post-#27 contract — including `Unavailable` on unprovable claims — or
  explicitly re-scope v0.x durability to Redis and record that decision.
- **Acceptance gates:** conformance tests against the `IdempotencyStore`
  contract (claim, occupied, completed, expiry race, unavailable) run in CI
  against a live service (P0-2's job); no promotion claim without them.
- **CI/GitForge relevance:** depends on P0-2 service provisioning; the
  conformance suite should be reusable by third-party implementors
  (`agentverify-testkit`).

### P2-7 · Unify or map the two `ClaimResult` taxonomies

- **Evidence:** `agentverify-runtime` `ClaimResult::{Claimed,
  AlreadyClaimed(, Unavailable post-#27)}` (`executor.rs:90-95`) vs
  `agentverify-http::wasm_storage::ClaimResult::{Claimed, InFlight,
  Completed}` (`wasm_storage.rs:140-147`).
- **Action:** post-#29, either converge on one enum or document the mapping
  and why the WASM store is exempt (pre-1.0 breaking budget was accepted by
  #27/#29, so the cost is bounded now).
- **Acceptance gates:** single documented taxonomy or a doc-commented
  mapping; WASM store compiled in CI (see P2-8).
- **CI/GitForge relevance:** none beyond compile coverage.

### P2-8 · Add `agentverify-http` (wasm target) to the WASM CI job and enforce coverage once baselined

- **Evidence:** `ci.yml:212-219` checks only core/contract/engine/receipt/
  policy — `WasmIdempotencyStore` never compiles under CI's wasm target;
  `codecov.yml` 90 % targets are `informational: true`.
- **Action:** extend the wasm job's crate list; after the coverage baseline
  lands (`codecov.yml` comment references `docs/ROADMAP.md`), flip
  `informational` to enforcing for core crates per the quality rules.
- **Acceptance gates:** wasm job green with `agentverify-http` included;
  codecov status flips from informational with a recorded baseline.

### P2-9 · Sequence the dependency-debt PRs behind the stack

- **Evidence:** 11 open dependabot/action PRs (§2), notably redis
  0.24.1 → 1.6.0 (#13) and deadpool-postgres (#17) — both touch the surface
  #27's code exercises — and axum 0.8 (#11).
- **Action:** after P0-1, rebase/merge in risk order (redis stack first),
  letting CI arbitrate; do not merge redis bumps concurrently with #27.
- **Acceptance gates:** green Actions runs per merged bump; no long-lived
  drift between `Cargo.lock` and open bumps.

## 6. Scope boundaries and open questions

**Boundaries of this handoff**

- Docs-only: no source, test, CI, or workflow files are changed; no PRs are
  merged or edited; #27/#29 are referenced, not touched.
- Amortyx and VIVERE remain separate boundaries (`AGENTS.md`); Control Center
  owns authorization and promotion; GitForge owns execution. This document
  makes no promotion claim: internal Tier D (authenticated cross-process
  service, Control Center correlation) remains open, and the receipt
  signing/verification crypto surface was **not** re-audited here (covered by
  the 2026-10-10 assessment and the observer/signature bounds tests).
- Claims are bounded to SHAs named above; the store-layer rows in §3.3 are
  `main` facts that #27/#29 will invalidate as they land — re-read this file
  against the new boundary before acting on P1+ items.

**Open questions (owner decisions)**

1. Is `UNKNOWN`-verdict expiry under the shared Redis TTL intended product
   semantics, or should `UNKNOWN` persist longer (P1-4)?
2. Should `complete`/`release` persist failures stay non-fatal
   (logged-only) after the stack lands, or become verdict-affecting (P1-3,
   P1-5)?
3. Postgres durable idempotency store: implement in v0.x, or re-scope v0.x
   durability to Redis (P2-6)?
4. Where should the production dispatch adapter live (new crate vs. existing
   `agentverify-runtime`), given `AGENTS.md` forbids describing the
   convenience path as real dispatch until one is wired and tested?
5. Should AgentVerify gain a GitForge mirror so `AGENTS.md`'s
   "expensive validation through GitForge" guidance is routable, or does
   GitHub Actions remain the sole gate?

## 7. Receipt (AGENTS.md change workflow item 4)

- **Branch:** `docs/next-phase-handoff-20261011`, based on `main` @
  `f55e17149b47c04eb53faa21c8b56b6de50a53d9` (origin/main, fast-forwarded).
- **Files changed:** this document only.
- **Commands run (read-only evidence gathering):** `git clone` (isolated NVMe
  clone under `/home/mkinney/worktrees/`), `git merge --ff-only origin/main`,
  `gh issue view 27/29`, `gh pr list`, `gh pr checks 27/29` (GitHub API),
  codebase-memory `list_projects`/`search_graph` (symbol discovery),
  harness `gitforge_repos` (inventory), targeted `grep`/`sed` source reads.
- **Tests run:** none — docs-only branch, no code touched; per `AGENTS.md`
  the expensive validation gates (fmt/clippy/tests/coverage) apply to code
  changes and run in CI on the PR.
- **Docs validation:** markdown reviewed for broken relative links to repo
  files and for claim tags on every finding; line references verified against
  `f55e171` in this clone.
- **Integration limitations:** GitForge inventory read is point-in-time
  (2026-10-11); PR/CI state is point-in-time; `main` will move when
  #27/#29 land.
