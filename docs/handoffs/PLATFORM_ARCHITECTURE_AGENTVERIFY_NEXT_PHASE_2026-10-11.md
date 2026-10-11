# AgentVerify next-phase integration/readiness handoff (2026-10-11)

**Author role:** Platform-Architecture execution agent
**Scope:** docs-only readiness audit of dispatch, idempotency persistence,
fail-closed behavior, and test strategy at the current `main` boundary;
accounting for the open PR stack #27 → #29 without modifying or duplicating
them; ranked next-phase tasks with acceptance gates. **No source code is
changed by this handoff.**
**Revision 2 (2026-10-11, commit `a7dd069`):** same evidence boundary as
Rev. 1 — at that commit the branch changed only this document. Adds §1.1
(independently verified spec finding: the shipped executor cannot dispatch,
and Rev. 1 scheduled no task for it), re-ranks §5 around an executable
utility proof chain (real sandboxed dispatch adapter → honest
simulation-vs-real benchmark → Platform-Architecture lifecycle/receipt
integration → live multi-turn end-to-end acceptance), demotes idempotency
polish behind that proof, and pins every owner decision to an owner role, a
bounded date, and a fallback (§6). §3's audit findings are unchanged except
citation corrections recorded in §7.
**Revision 3 (2026-10-11, commit `49f71fb`):** same evidence boundary
(`f55e171`) and same §1–§6 substance; the Rev. 2 utility proof chain is
retained unchanged. Three corrections/registrations, no re-ranking: (1) §2's
crate inventory is corrected from "13 crates" to **14 workspace crates**
(`agentverify-policy` was already in the per-crate test inventory; the prose
count alone was wrong) — re-counted with `cargo metadata --no-deps` at the
boundary, receipt in §7; (2) this handoff is now registered in the task
ledger (`docs/internal/TASKS.md` Progress Timeline, per that ledger's
"update the rows here" rule) and `docs/README.md`'s navigation note now
points to `handoffs/` — the branch therefore changes three documentation
files, still no source; (3) every line citation carried into Rev. 3 was
re-verified against `f55e171` in this clone — all confirmed, no further
corrections. *(Rev. 4 supersedes item (3): two load-bearing claims escaped
that pass — see the Rev. 4 paragraph and the §7 citation-corrections
bullet.)*
**Revision 4 (2026-10-11, this commit):** same evidence boundary (`f55e171`)
and same §1–§6 substance and ranking; three corrections from an independent
exact-head review, docs-only again (this revision changes only this file):
(1) §3.3 and P2-7 cite the Redis `release` warn as `crates/agentverify-runtime/src/idempotency_store.rs:
450`; the warn is at **`:449`** — Rev. 2's recorded `:449 → :450`
"correction" was itself wrong (verified in this clone, whose `crates/` and
`.github/` trees are byte-identical to `f55e171`; §7's `:449-451` anchor
range already contained the true line); (2) the PR #27 CI snapshot is now
stated once, identically, in §1/§2/§4/§7 — all 10 substantive GitHub
Actions checks green at head `dc956d089ae82ba0193a4839045b713266d9229d`
(run 38106630891, conclusion `success`; the 11th rollup entry, CodeRabbit,
reports "Review skipped: draft pull request") — replacing Rev. 2's stale
"10 green checks + 1 pending" and labeled a point-in-time observation, not
a standing guarantee; (3) P1-5 no longer says `Receipt` has "no replay
fields": the optional **action-scoped** `idempotency_key` exists
(`receipt.rs:93-95`) but is not by itself persisted request-level
idempotency/replay evidence, and P1-5 now explicitly preserves-or-extends
that field instead of denying it.
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
| PR #27 opened (draft) | NOT on `main` | Closes the *store-layer* fail-open claim semantics for `RedisIdempotencyStore` (adds `ClaimResult::Unavailable` + `ExecutorError::IdempotencyUnavailable`, refuses dispatch without a proven claim). All 10 substantive GitHub Actions checks pass on its head `dc956d089ae82ba0193a4839045b713266d9229d` (run 38106630891, conclusion `success`; the 11th rollup entry, CodeRabbit, is skipped while the PR is a draft). Point-in-time snapshot read 2026-10-11 — an observation, not a standing guarantee (§4). |
| PR #29 opened (draft) | NOT on `main` | Closes the same gap for `FileIdempotencyStore` (claim-persist failure → `Unavailable`, cache untouched). Stacked on #27 (`base` = #27's head branch; merge #27 first). No CI checks have run on it yet. |

Nothing in this document duplicates #27/#29 scope. Section 4 states exactly
what each covers; sections 3 and 5 cover what remains **after** both land.

### 1.1 Driver for this revision: independently verified spec finding

An independent verification pass over Rev. 1 of this handoff found a spec
gap that Rev. 1 documented but never scheduled work for. Both halves are
verified at the same boundary (`f55e171`):

1. **The shipped executor cannot dispatch.** The only shipped
   `ActionExecutor` implementation is `SimulatedActionExecutor`
   (`crates/agentverify-runtime/src/action_executor.rs:104-127`), whose
   `execute` returns `Ok(DispatchOutcome::Completed)` unconditionally, before
   any external system is contacted — its own doc comment states this
   ("immediately completes actions successfully without actually
   dispatching"). The CLI `verify` command wires it
   (`crates/agentverify-cli/src/main.rs:386-388`). This is §3.1 finding 1,
   unchanged since Rev. 1. **[confirmed]**
2. **Rev. 1's task list never scheduled a dispatch/outcome task.** All nine
   of its P0–P2 items govern idempotency claims, store persistence, WASM
   taxonomies, CI, and dependency sequencing. A real dispatch adapter existed
   only as open question Q4 and as the prior handoff's unstarted packet
   P-DIS-01 (`docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_2026-10-10.md`
   §7; ledger row in `docs/internal/TASKS.md`, "Handoff packets 2026-10-10").
   Platform-Architecture integration likewise appeared only as open
   questions, not as executable tasks. **[confirmed]**

Consequence: Rev. 1 optimized correctness of the *claim/persistence layer*
while the library's headline capability — proving an external action reached
its required final state — remained simulation-only, with no scheduled,
gated path to executable proof. This revision re-ranks §5 to close that gap
first: the P1 chain (P1-3 … P1-6) is a dependency-ordered utility proof —
real sandboxed dispatch → honest benchmark → platform contract integration →
live multi-turn acceptance — and the idempotency-polish items move behind it
(P2), with the justification stated at each item. The P2 items are demoted,
not dropped: they still land before any promotion claim.

## 2. Boundary snapshot

- `main` @ `f55e171`; **14 workspace crates** (`cargo metadata --no-deps`:
  cli, contract, core, engine, http, mcp, observe, otel, policy, receipt,
  recovery, runtime, storage, testkit); static inventory of
  `#[test]`/`#[tokio::test]`
  functions: core 131, observe 111, engine 108, runtime 97, policy 66, mcp 63,
  recovery 55, http 51, storage 22, contract 21, testkit 8, cli 7, receipt 5,
  otel 3 — **748 declared test functions** (grep count, not a run count)
  **[confirmed]**. (Rev. 1–2 said "13 crates" here; the per-crate list below
  already named all 14 — only the prose count was wrong, corrected in Rev. 3.)
- Open PRs: #27 (draft; 10/10 substantive Actions checks green at head
  `dc956d0`, snapshot 2026-10-11 — §1/§4), #29 (draft, stacked, no CI runs),
  #25 + #8–#17 (11 dependabot/action bumps, including redis 0.24.1 → 1.6.0 in
  #13 and axum 0.7.9 → 0.8.9 in #11) **[confirmed]**.
- GitForge: the GitForge repo inventory (21 repos, read 2026-10-11) contains
  **no AgentVerify repository** **[confirmed]**. AgentVerify's merge gates are
  GitHub Actions only; the `AGENTS.md` guidance to route expensive validation
  "through GitForge" has no repo-level seam to route through today.

## 3. Audit findings at `f55e171`

### 3.1 Dispatch

- The only shipped action adapter is `SimulatedActionExecutor`
  (`crates/agentverify-runtime/src/action_executor.rs:104-127`): its
  `execute` returns `Ok(DispatchOutcome::Completed)` unconditionally, before
  any external system is contacted — its own doc comment states this
  ("immediately completes actions successfully without actually
  dispatching"). The CLI `verify` command wires it
  (`crates/agentverify-cli/src/main.rs:386-388`) — the `verify`
  command still verifies nothing that was externally dispatched, exactly as
  `AGENTS.md` warns. **[confirmed]** §1.1 records this as the driver for
  Rev. 2's replan; the P1 chain in §5 schedules the executable proof.
- Retry discipline is sound: `DispatchOutcome::should_retry` is true only for
  the two timeout variants (`action_executor.rs:85-91`), i.e. re-dispatch is
  advised only where the external state must be observed first
  (verify-before-retry). Terminal classification is `Completed` |
  `TransportError` | `Ambiguous` (`action_executor.rs:139-147` tests)
  **[confirmed]**.
- `RecoveryStrategy` retries on `UNKNOWN` and treats `DUPLICATE` as a
  successful stop (`crates/agentverify-recovery/src/lib.rs:999-1023`,
  `1485-1493`) — consistent with UNKNOWN-is-not-FAILED. **[confirmed]**
- `McpClient` exposes an outbound tool-call surface (`call_tool`,
  `crates/agentverify-mcp/src/client.rs:441-466`), but no `ActionExecutor`
  adapter implements it, and the type's private `dispatch` method
  (`crates/agentverify-mcp/src/client.rs:352-373`) is JSON-RPC message routing, not action dispatch —
  neither is an outcome-bearing dispatch adapter. **[confirmed]** (Rev. 1
  cited this bullet as `crates/agentverify-mcp/src/client.rs:380-412`; corrected in Rev. 2.)

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
| `crates/agentverify-runtime/src/idempotency_store.rs:321-324` | Redis pool get fails | `(Claimed, None)` | Dispatch with **no claim at all** |
| `crates/agentverify-runtime/src/idempotency_store.rs:331-334` | entry serialization fails | `(Claimed, None)` | Dispatch with no durable claim |
| `crates/agentverify-runtime/src/idempotency_store.rs:394-397` | `SET NX EX` command error | `(AlreadyClaimed, None)` | Fabricated occupancy; caller denied a dispatch that was actually never claimable |
| `crates/agentverify-runtime/src/idempotency_store.rs:387-390` | expiry-race retry `SET NX` error | `(AlreadyClaimed, None)` | Same fabricated occupancy |
| `crates/agentverify-runtime/src/idempotency_store.rs:181-185` | file claim persist fails | `(Claimed, None)` + cache insert | **Phantom claim**: dispatch with no durable claim, and later in-process calls see `AlreadyClaimed` from cache |

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
  (`crates/agentverify-runtime/src/idempotency_store.rs:428-435`: `let _: Result<(), _> = ...`); unlike
  `release` (which warns at `:449`) a failed completion write is not even
  logged. A lost completion leaves the key `InFlight` until TTL, so later
  callers get `(AlreadyClaimed, None)` — the unresolved-claim shape that reads
  as a false `Duplicate` downstream. **[inferred from the store state
  machine; the write-loss itself is confirmed by the discarded result]**
- **File-store completion divergence after restart.**
  `FileIdempotencyStore::complete` inserts the completed entry into the
  in-process cache regardless of whether `save_entry` succeeded
  (`crates/agentverify-runtime/src/idempotency_store.rs:190-205`). If the persist failed, the disk keeps
  `InFlight`; after a process restart the key reads as an unresolved claim,
  not the completed verdict. **[inferred, same basis]**
- **`complete`/`release` on the file store log persist failures but proceed**
  (`crates/agentverify-runtime/src/idempotency_store.rs:197-199`, `214-217`) — pre-existing, acknowledged in
  #29's limitations.
- **`WasmIdempotencyStore` is a separate claim taxonomy**
  (`crates/agentverify-http/src/wasm_storage.rs:140-147`:
  `ClaimResult::{Claimed, InFlight, Completed}` — a different enum from the
  runtime's), localStorage-backed, and untouched by #27/#29. It is also **not
  compiled by the WASM CI job**, which checks only core, contract, engine,
  receipt, and policy (`.github/workflows/ci.yml:212-219`). **[confirmed]**

### 3.4 UNKNOWN semantics vs. store TTL (interaction defect candidate)

`RedisIdempotencyStore::complete` writes consumed-as-`UNKNOWN` verdicts with
the **same** TTL as every other entry (`crates/agentverify-runtime/src/idempotency_store.rs:400-437`; default
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
  (`executor.rs` test module — seam paired with test line: timeout before
  dispatch `:1461`, transport error → Unknown `:1499`, ambiguity terminal
  `Unknown` `:1536`, retry exhaustion `:1572`, observer error → Unknown
  `:1607`, stale read `:1654`, cancellation → Unknown `:1714`, transport
  error consumes claim `:1856`, idempotency double-dispatch prevention
  `:2353`) **[confirmed — pairings re-verified in Rev. 2; Rev. 1 paired four
  of these lines with the wrong seam names]**. Property tests
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
  (draft, base `main`, head `dc956d089ae82ba0193a4839045b713266d9229d`; all
  10 substantive Actions checks green in run 38106630891 — completed,
  conclusion `success` — with CodeRabbit skipped as draft; snapshot
  2026-10-11, an observation at a point in time, not a permanent guarantee).
  Scope:
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
  ranked tasks below; none of them re-open decided scope in #27/#29. Rev. 2
  ranks the executable utility proof (P1-3 … P1-6) ahead of them: the
  idempotency items are now P2-7 … P2-9 (WASM taxonomy/CI items P2-11/P2-12)
  — demoted behind utility proof, not dropped (§5 legend).

## 5. Ranked next-phase tasks

Priority legend (Rev. 2): **P0** = blocks correct landing of in-flight work;
**P1** = the executable utility proof chain — real dispatch, honest
measurement, platform contract integration, live end-to-end acceptance —
which unblocks Platform-Architecture integration; **P2** = idempotency
persistence polish and readiness debt, deliberately ranked behind the
utility proof.

**Why the Rev. 1 idempotency items are demoted (justified, not dropped):**
once P0-1 lands, every store error is refused (`Unavailable`) instead of
answered with a fabricated claim, so the P2 items are robustness and
observability polish on already fail-closed stores — none of them gates
executable utility. The P1 chain also avoids the high-contention
`crates/agentverify-runtime/src/idempotency_store.rs` surface entirely, so it proceeds without rebasing
against #27/#29. Real dispatch raises the stakes of the polish items (real
external effects make a lost completion write consequential), which is why
they remain ranked ahead of promotion and carry owner decisions in §6.

### P0-1 · Land the claim fail-closed stack in order

- **Action (owner):** review and merge #27, then mark #29 ready and merge it
  (rebase if `main` moved). Do not batch them into one merge.
- **Evidence:** PR URLs and CI run 38106630891 (§4).
- **Acceptance gates:** #27 merged with all Actions checks green including
  the new `--all-features` Redis test run; #29 undrafted, its checks green on
  the post-#27 base, stack merges without semantic conflicts in
  `crates/agentverify-runtime/src/idempotency_store.rs`; `.github/CHANGELOG.md` Unreleased entries for both.
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

### P1-3 · Real sandboxed dispatch adapter: one concrete action, durable receipt, verified outcome

Executes the first slice of the prior handoff's P-DIS-01 packet
(`docs/handoffs/PLATFORM_ARCHITECTURE_AGENTVERIFY_2026-10-10.md` §7; ledger
row in `docs/internal/TASKS.md`, "Handoff packets 2026-10-10").
**Owner role:** Repository owner (@aliasfoxkde). Adapter crate location per
Q4's fallback unless decided sooner.

- **Evidence:** the gap is §1.1/§3.1 (`action_executor.rs:104-127`). The
  outcome contract the adapter must honor is `DispatchOutcome`/`DispatchError`
  (`action_executor.rs:10-34`; timeout variants `:23-30`; `should_retry`
  true for timeouts only, `:85-91`); the prior handoff's mapping guidance is
  transport/timeout/2xx/4xx/5xx → outcomes exactly per that enum.
- **Action:** implement `HttpActionExecutor: ActionExecutor` in
  `crates/agentverify-http/src/action_executor.rs`, **loopback-only**: the
  constructor refuses any base URL whose host is not `127.0.0.1`/`localhost`
  (the sandbox gate, test-enforced). Dispatch ONE concrete action end to end:
  POST the contract action to a loopback reference service that persists the
  payload durably to a file; verify the postcondition by reading the written
  artifact back through the REST-observer path; record the receipt in
  `FileReceiptStore` and re-read it (surface per `receipt.rs:67`: `action_id`,
  `contract_id`, digest, optional Ed25519 signature). CLI: wire
  `--executor simulated|http` with `simulated` the labeled default, per
  P-DIS-01 (the simulated default stays; `AGENTS.md`'s invariant — the
  convenience path must not be described as real dispatch — is lifted only
  for the explicitly selected `http` path once these gates pass).
- **Acceptance gates (executable):**
  1. Integration test: one dispatch → artifact written by the loopback
     service → postcondition `Verified` → receipt stored, digest matches
     canonical receipt data, signature verifies via the signing path.
  2. Sandbox assertion: constructor and tests reject non-loopback targets;
     no test in the suite contacts a non-loopback host.
  3. Timeout after dispatch → verdict `Unknown`, never `Failed`
     (`executor.rs:2520` pattern); transport error → `Unknown` (`:1499`
     pattern); timeout before dispatch releases the claim (`:1461` pattern).
  4. Existing simulated-default tests unchanged; workspace gates green
     (fmt, clippy `-D warnings`, tests) in CI on the implementing PR — this
     is a code change, so CI is the evidence of record.
- **No-claim:** this proves executable utility inside a loopback sandbox. It
  is not a production-readiness claim and not Tier D (no authenticated
  cross-process service yet — that is P1-5/P1-6).
- **CI/GitForge relevance:** GitHub Actions only (§2).

### P1-4 · Honest utility benchmark: real execution vs simulation, with failure/timeout/cancel behavior

Depends on P1-3. Runs after it; no other blocker.

- **Evidence:** no bench target exists for the runtime
  (`crates/agentverify-runtime/benches/` absent at `f55e171`);
  `criterion = "0.5"` is already a workspace dependency (`Cargo.toml:112`);
  the behavior matrix to measure exists as Tier-B seams (§3.5).
- **Action:** add `crates/agentverify-runtime/benches/utility_bench.rs`
  (criterion) comparing `SimulatedActionExecutor` vs the P1-3 adapter through
  `Executor::execute_with_executor` across: (a) happy path (dispatch →
  observe → verify → receipt), (b) transport error → `Unknown`, (c) timeout
  before and after dispatch, (d) cancellation (`executor.rs:1714` seam).
  Report p50/p95/p99 per the quality rules.
- **Acceptance gates:** the bench compiles and runs at the recorded SHA;
  numbers are committed to a dated report under `docs/` with the run command
  and SHA; every matrix scenario appears in the report, not only the happy
  path; the report carries this line verbatim: "Utility demonstration only —
  loopback sandbox measurement, not a production-readiness or latency
  claim."
- **No-claim:** simulation-vs-real deltas describe harness overhead, not
  production behavior.
- **CI/GitForge relevance:** benches stay out of the CI hot path; the dated
  report is the artifact of record.

### P1-5 · Platform-Architecture task-lifecycle/receipt-contract integration (co-owner: Control Center)

Executes the receipt-identity half of P-CC-01 (prior handoff §7;
`docs/internal/TASKS.md:144`). Depends on P1-3; parallel with P1-4.

- **Evidence:** the integration surface already exists one-directional at
  `f55e171`: `ControlCenterClient` POSTs receipts to
  `{base}/api/v1/verification-receipts`
  (`crates/agentverify-http/src/client.rs:168`) with the receipt digest as
  `X-Idempotency-Key` (`:193`) and a `submit_receipt_with_unknown_on_timeout`
  wrapper (`:232`), covered by wiremock tests (`:470`). What is missing is
  identity binding: `Receipt` (`receipt.rs:67`) carries `action_id`/
  `contract_id`/digest/signature **and** an optional *action-scoped*
  `idempotency_key` (`receipt.rs:93-95`, doc comment "Idempotency key for
  replay protection") — populated by the executor from the Action's own
  `idempotency_key` (`executor.rs:1356` via `with_contract_version_and_key`,
  `receipt.rs:132-158`; covered by the canonical digest, `receipt.rs:187`).
  That field is **not by itself persisted request-level
  idempotency/replay evidence**: it is `None` unless the action carried a
  key, the submission path keys `X-Idempotency-Key` on the receipt digest
  instead (`crates/agentverify-http/src/client.rs:193`), and nothing client- or server-side deduplicates
  on it. What `Receipt` has no equivalent of is project/task/work-request/
  job/agent/commit/verifier identity fields, and no request-scoped replay
  evidence. Whether any Control Center deployment implements the endpoint
  remains **[unverified]** (prior handoff §5).
- **Action:** add the P-CC-01 identity fields to `Receipt` (digest computed
  over the extended canonical form, preserving the existing
  `idempotency_key` and its canonical-digest coverage — `receipt.rs:187`),
  bind them at execution time from the platform task lifecycle
  (task/work-request/job/agent-run/commit), and pin the versioned schema in
  `docs/INTEGRATIONS.md`, including an explicit decision for the "replayed"
  rejection class: bind it to the existing action-scoped `idempotency_key`,
  or extend that field into a persisted request-scoped replay-evidence
  field — reuse-as-is is not replay evidence (see Evidence), so either way
  the choice is recorded in the schema, not implicit; add wiremock rejection
  fixtures (orphan, stale, tampered, unauthorized, cross-project, replayed)
  proving the client surfaces rejection reasons. Submit-only: no
  promotion-state mutation, per the client's existing design.
- **Boundary preservation:** AgentVerify stays independently deployable —
  `AGENTS.md`: it does not own Control Center authorization, GitForge
  execution, Amortyx routing, or VIVERE experimentation; `docs/internal/
  HANDOFF.md` assigns task/workspace ownership and lease state to Control
  Center. No Amortyx or VIVERE imports; `ControlCenterClient` remains the
  only platform seam.
- **Acceptance gates:** versioned schema doc merged; fixtures green for all
  six rejection classes; a receipt without bound identities is refused by
  the submission path (test); boundary check: zero new dependencies outside
  the workspace.
- **No-claim:** wiremock fixtures prove the contract, not a live deployment;
  live acceptance is P1-6 and stays stub-backed until Q6 resolves.
- **Co-owner role:** Control Center owner (schema sign-off per Q6).

### P1-6 · Live multi-turn end-to-end acceptance in an isolated repo, observable CI

The acceptance run for the whole chain. Mocks are not the sole proof: the
dispatched effects are real (files written by a real loopback service inside
the job); only the Control Center endpoint may be stubbed, and only while Q6
is undecided.

- **Evidence:** GitHub Actions is the only merge gate and GitForge has no
  AgentVerify repo (§2); internal Tier D remains open (§6; prior handoff §5).
- **Action:** create the isolated scratch repo chosen under Q7 with a
  workflow that builds AgentVerify at a pinned SHA of this repo and runs a
  ≥3-turn multi-turn loop through `execute_with_executor` using the P1-3
  adapter: turn 1 happy path; turn 2 injected transport error (final verdict
  must be `Unknown`, never `Failed`); turn 3 timeout-after-dispatch resolved
  by observation. Receipts are written, signature-verified in-job, and
  uploaded as workflow artifacts; the run URL is recorded in the
  `docs/internal/TASKS.md` ledger.
- **Acceptance gates:** a green Actions run URL on the scratch repo; ≥3
  receipts in the artifact, digest + signature verified in-job; turn 2's
  final verdict logged as `Unknown`; no non-loopback egress in the job
  (workflow-level network guard); the ledger row references the run URL
  rather than describing it.
- **No-claim:** passing this does not promote Tier D — the Control Center
  hop is stubbed-or-absent unless Q6 resolved to a real deployment. It
  proves the dispatch → observe → verify → receipt loop runs live and
  multi-turn, in CI, on a real (sandboxed) effect.
- **CI/GitForge relevance:** GitHub Actions is the record; if Q5 lands a
  GitForge mirror the run may additionally route there, with GitHub
  authoritative until then.

### P2-7 · Make Redis completion-write failure observable (demoted from Rev. 1 P1-3)

- **Demotion justification:** a one-line observability fix on a store whose
  claim path is fail-closed after P0-1; the P1 chain exercises file/in-memory
  stores, so this gates nothing in the utility proof.

- **Evidence:** `crates/agentverify-runtime/src/idempotency_store.rs:428-435` — `complete`'s `SET` result is
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

### P2-8 · Decide and pin the UNKNOWN-vs-TTL policy (demoted from Rev. 1 P1-4; decision pinned at §6 Q1)

- **Demotion justification:** the executor boundary already consumes
  `UNKNOWN`-verdict keys at every error exit (§3.2), so the residual hazard —
  re-claim after TTL expiry — is guarded by caller verify-before-retry until
  the pinned decision; Q1's fail-safe fallback (longer retention) is decided
  on a bounded date.

- **Evidence:** §3.4 — consumed-as-`UNKNOWN` keys expire with the shared 24 h
  TTL (`crates/agentverify-runtime/src/idempotency_store.rs:400-437`, `:257-259`), after which the key is
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

### P2-9 · File-store completion divergence after restart (demoted from Rev. 1 P1-5)

- **Demotion justification:** #29 closes the claim direction; the completion
  direction is an evidence-freshness defect observable after restart, not a
  dispatch-correctness gate for the utility proof.

- **Evidence:** `crates/agentverify-runtime/src/idempotency_store.rs:190-205` — `complete` caches the
  completed entry even when `save_entry` failed; disk keeps `InFlight`
  (§3.3). #29 fixes the claim direction only.
- **Action (after #29):** same decision shape as P1-3 (observability at
  minimum); add a restart-semantics test (write-completed-with-failed-persist
  → new store instance → observed state matches the decided policy).
- **Acceptance gates:** restart test pinning the decided behavior; no change
  to the #29 claim-path contract.
- **CI/GitForge relevance:** deterministic; runs in default-feature CI.

### P2-10 · Durable cross-process idempotency store (Tier D durability enabler; was Rev. 1 P2-6)

- **Ranking note:** an enabler for *durable* cross-process claims, but
  P1-6's acceptance loop is a single-job loopback run, so durability does
  not gate the utility proof; no durable/cross-process claim may be made in
  any handoff until this lands (Q3's fallback records exactly that).

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

### P2-11 · Unify or map the two `ClaimResult` taxonomies (was Rev. 1 P2-7)

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

### P2-12 · Add `agentverify-http` (wasm target) to the WASM CI job and enforce coverage once baselined (was Rev. 1 P2-8)

- **Evidence:** `ci.yml:212-219` checks only core/contract/engine/receipt/
  policy — `WasmIdempotencyStore` never compiles under CI's wasm target;
  `codecov.yml` 90 % targets are `informational: true`.
- **Action:** extend the wasm job's crate list; after the coverage baseline
  lands (`codecov.yml` comment references `docs/ROADMAP.md`), flip
  `informational` to enforcing for core crates per the quality rules.
- **Acceptance gates:** wasm job green with `agentverify-http` included;
  codecov status flips from informational with a recorded baseline.

### P2-13 · Sequence the dependency-debt PRs behind the stack (was Rev. 1 P2-9)

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

**Open questions (each pinned to an owner role, a bounded decision date, and
a real fallback; dates from 2026-10-11)**

| # | Decision | Owner role | Decide by | Fallback if undecided |
|---|---|---|---|---|
| Q1 | Is `UNKNOWN`-verdict expiry under the shared 24 h Redis TTL intended semantics, or should `UNKNOWN` persist longer (P2-8)? | Repository owner (@aliasfoxkde) | 2026-10-18 | Adopt option (a) — retain `UNKNOWN`-consumed entries longer — as the recorded default with a pinning test; it fails safe, unlike expiry |
| Q2 | Do `complete`/`release` persist failures stay non-fatal (logged-only) after the stack lands, or become verdict-affecting (P2-7, P2-9)? | Repository owner (@aliasfoxkde) | 2026-10-18 | Land the minimum: warn-level observability on both paths, matching #29's recorded limitation |
| Q3 | Postgres durable idempotency store in v0.x, or re-scope v0.x durability to Redis (P2-10)? | Repository owner (@aliasfoxkde) | 2026-10-25 | P2-10 stays open and every handoff records "no durable cross-process claim" until decided — no silent re-scoping |
| Q4 | Where does the production dispatch adapter live (new crate vs. `agentverify-runtime`), given `AGENTS.md` forbids describing the convenience path as real dispatch until one is wired and tested? | Repository owner (@aliasfoxkde) | 2026-10-14 | `crates/agentverify-http` per P-DIS-01's file list; P1-3 proceeds on that path |
| Q5 | Should AgentVerify gain a GitForge mirror so `AGENTS.md`'s "expensive validation through GitForge" guidance is routable, or does GitHub Actions remain the sole gate? | Platform-Architecture owner | 2026-10-25 | GitHub Actions remains the sole gate; P1-6 runs there and is the record |
| Q6 | Which Control Center deployment implements `/api/v1/verification-receipts` for live acceptance (P1-5/P1-6)? Endpoint existence is **[unverified]** (prior handoff §5) | Control Center owner (co-owner per P-CC-01) | 2026-10-18 | Loopback CC stub in P1-6, explicitly labeled stub-backed; Tier D stays open |
| Q7 | Location and retention of the isolated E2E scratch repo (P1-6) | Repository owner (@aliasfoxkde) | 2026-10-14 | Private dated scratch repo `agentverify-e2e-scratch-20261018`, GC-eligible after acceptance per the scratch-space rules |

## 7. Receipt (AGENTS.md change workflow item 4)

- **Branch / revision:** `docs/next-phase-handoff-20261011`, based on `main`
  @ `f55e17149b47c04eb53faa21c8b56b6de50a53d9` (origin/main,
  fast-forwarded). Rev. 1 (`45aabc8`) added this document; Rev. 2
  (`a7dd069`) updated only this file; Rev. 3 (`49f71fb`) corrected the
  §2 crate count in this file and registered the handoff in
  `docs/internal/TASKS.md` (Progress Timeline row) and `docs/README.md`
  (navigation pointer to `handoffs/`), changing exactly three documentation
  files; no source, test, CI, or workflow file is touched. **Rev. 4 (this
  commit)** corrects the §3.3/P2-7 `release`-warn citation and the P1-5
  `Receipt` replay-field claim in this file and re-states the #27 CI
  snapshot identically across §1/§2/§4/§7; it changes only this file.
- **Crate count (corrected in Rev. 3):** command
  `cargo metadata --no-deps --format-version 1`, run in this clone on
  2026-10-11: **14 workspace packages** — agentverify-{cli, contract, core,
  engine, http, mcp, observe, otel, policy, receipt, recovery, runtime,
  storage, testkit}. Boundary equivalence: `git diff f55e171..HEAD` at the
  pre-edit head `a7dd069` touched only `docs/` files (no `Cargo.toml`,
  `Cargo.lock`, or crate membership change), so the count at `f55e171` is
  identical. Rev. 1–2 said "13 crates" — wrong prose count; the §2 per-crate
  test inventory had always listed all 14 crates and summed correctly to
  748. Cross-checks agreeing on 14: `docs/ROADMAP.md` ("all 14 crates"),
  `docs/internal/TASKS.md` ("Create all 14 crates"), and the 2026-10-10
  handoff ("source read of all 14 crates").
- **Commands run for Rev. 2 (read-only):** `gh pr view 27/29` and
  `gh run view 38106630891` (PR/CI state re-read 2026-10-11: #27 draft with
  10 green checks + 1 pending — Rev. 4 re-read: that pending entry has
  since resolved to CodeRabbit "Review skipped: draft pull request"; see
  the Rev. 4 snapshot below — run success; #29 draft, zero completed
  checks, base = #27's head branch); per-crate recount
  `grep -rE "#\[(tokio::)?test\]"` across `src/`, `tests/`, `benches/`
  (748, matching §2); targeted `sed`/`awk`/`grep` re-verification of every
  line citation carried into Rev. 2.
- **Commands run for Rev. 3 (read-only):** the `cargo metadata` count above;
  independent re-run of the per-crate test-function grep (748; all 14
  per-crate numbers match §2 exactly); `sed`-based re-verification of every
  load-bearing line citation against this clone (which differs from
  `f55e171` only by docs commits): §3.1 `action_executor.rs:104-127`,
  `:85-91`, `:10-34`, `main.rs:386-388`, `crates/agentverify-mcp/src/client.rs:441-466`, `:352-373`;
  §3.2 `executor.rs:90-95`, `:99-102`, `:244-259`, `:316-330`, `:396-405`,
  `receipt.rs:67`, `:406-407`; §3.3 table anchors `crates/agentverify-runtime/src/idempotency_store.rs:
  321-324`, `:331-334`, `:387-390`, `:394-397`, `:181-185`, `:190-205`,
  `:428-435`, `:449-451`; §3.5 all ten seam↔test pairings (`:1461`, `:1499`,
  `:1536`, `:1572`, `:1607`, `:1654`, `:1714`, `:1856`, `:2353`, `:2520`)
  and `ci.yml:118-121`, `:212-219`, `:243-244`; `wasm_storage.rs:140-147`;
  `crates/agentverify-http/src/client.rs:168`, `:193`, `:232`; `recovery/src/lib.rs:999-1023`,
  `:1485-1493`. **All confirmed; no further citation corrections.**
  *(Superseded by Rev. 4: two load-bearing claims escaped that pass — see
  the citation-corrections bullet below.)*
- **Citation corrections made in Rev. 2:** §3.1 `McpClient` bullet — the
  tool-call surface is `call_tool` (`crates/agentverify-mcp/src/client.rs:441-466`) and the private
  `dispatch` (`crates/agentverify-mcp/src/client.rs:352-373`) is JSON-RPC routing; Rev. 1 cited
  `crates/agentverify-mcp/src/client.rs:380-412`. §3.1 `action_executor.rs:104-128` → `:104-127`.
  §3.3 `release` warn `:449` → `:450`. §3.5 seam list re-paired to the
  correct test lines (4 of 8 pairings were drifted) and the
  double-dispatch test line added (`:2353`). All other §2–§4 citations were
  re-verified against `f55e171` and unchanged.
- **Commands run for Rev. 4 (read-only):** `gh pr view 27 --json
  headRefOid,statusCheckRollup`, `gh pr checks 27`, and
  `gh run view 38106630891` (re-read 2026-10-11): #27 is a draft at head
  `dc956d089ae82ba0193a4839045b713266d9229d`; run 38106630891 (created
  2026-10-11T02:53:53Z) is completed with conclusion `success`; all 10
  substantive Actions checks pass on that head (Clippy all-features,
  Clippy default, Coverage, Deny advisories, Deny bans/licenses/sources,
  Documentation, Formatting, MSRV, Test, WASM wasm32-wasip1); the 11th
  check-rollup entry, CodeRabbit, reports "Review skipped: draft pull
  request" — skipped, not pending. This is a point-in-time observation,
  not a permanent guarantee; re-read before merge decisions. Source
  re-verification in this clone: `release`'s warn at
  `crates/agentverify-runtime/src/idempotency_store.rs:449` (inside `release`, fn at `:439`) and
  `complete`'s discarded `SET` at `:428-435`; `Receipt` struct
  `receipt.rs:67`, `idempotency_key` field `:93-95`, constructor
  `:132-158`, canonical-digest coverage `:187`; executor key flow
  `executor.rs:1356`; header source `crates/agentverify-http/src/client.rs:193`; `Action.idempotency_key`
  `action.rs:20`. `git diff f55e171..HEAD` shows only `docs/` files, so
  source line numbers at this head equal the `f55e171` boundary.
- **Citation corrections made in Rev. 4:** §3.3 and P2-7 — the Redis
  `release` warn is `crates/agentverify-runtime/src/idempotency_store.rs:449`, not `:450`; Rev. 2's
  recorded `:449 → :450` correction (above) was itself wrong. §7's
  `:449-451` anchor range was verified to contain the warn line and stands
  as written. P1-5 — the Evidence and Action bullets no longer claim
  `Receipt` has "no replay fields": the optional action-scoped
  `idempotency_key` (`receipt.rs:93-95`) exists and is covered by the
  canonical digest; the corrected claim is that it is not by itself
  persisted request-level replay evidence, and the plan now
  preserves-or-extends it explicitly. No §1–§6 ranking, boundary, or
  evidence-boundary change.
- **Tests run:** none — docs-only branch, no code touched (Rev. 3 included;
  `cargo metadata` resolves manifests only, no build or test execution);
  per `AGENTS.md` the expensive validation gates (fmt/clippy/tests/coverage)
  apply to code changes and run in CI on the PR. The P1-3 … P1-6 acceptance
  gates are stated as future criteria, not results: nothing in this
  document claims any of them is met, and no utility, benchmark,
  integration, or E2E acceptance is claimed done. §5's P1-3 sandbox is
  explicitly loopback-only; all Rev. 2 wording distinguishing loopback/test
  doubles from production behavior is retained unchanged.
- **Docs validation (Rev. 3):** all referenced repo files verified to
  exist, including the newly registered `docs/internal/TASKS.md` row and
  the `docs/README.md` → `handoffs/` pointer (single pointer, no duplicate
  index rows — the README table still lists only the public doc set);
  markdown re-reviewed for broken relative links and claim tags on every
  finding; `git diff --check` clean.
- **Integration limitations:** unchanged from Rev. 1 — GitForge inventory
  and PR/CI state are point-in-time observations (2026-10-11), not standing
  guarantees: re-read `gh pr checks` at the current head before any merge
  decision; `main` moves when #27/#29 land; Q6's endpoint deployment
  remains unverified, so P1-6's Control Center hop is stub-backed until Q6
  resolves.
