# AgentVerify Platform Handoff — 2026-10-10

**Worker:** Z.AI GLM-5.3-Flash execution worker (Platform Architecture component)
**Checkout:** `/home/mkinney/worktrees/agentverify-platform-handoff-20261011` (NVMe, `/dev/nvme0n1p2`, ext4)
**Branch:** `codex/agentverify-platform-handoff-20261011`
**Baseline audited:** `2b6d021` (parent `a716342` — the previously recorded evidence
boundary; the only delta is a docs-only commit, so source code is identical to
`a716342`), 0 dirty status entries at session start.
**Canonical planning handoff:** `docs/planning/HANDOFF.md` (refreshed in this change)
**Historical handoffs:** `docs/internal/HANDOFF.md` (boundary stale: `a3b1eb2`,
2026-08-30 — kept as record, superseded by `docs/planning/HANDOFF.md`) and
`docs/internal/HANDOFF_MINIMAX_M27.md`

---

## 1. Purpose and boundaries

AgentVerify is an outcome-verification library for action-taking AI agents: given
an intended action and its expected postconditions, determine whether the system
of record actually satisfies them, and preserve the evidence in signed receipts.
`UNKNOWN` is a first-class result; a timeout is not a failure
(`CLAUDE.md`, `docs/CONCEPTS.md:121`).

Role separation (do not conflate):

- **AgentVerify** — deterministic postcondition evaluation, verify-before-retry
  semantics, signed evidence. It does not authorize, orchestrate, or scan.
- **Aegis** — pattern/security scanning of the implementation and receipt path.
  It is a scanner over AgentVerify's artifacts, not a verification component;
  its findings never constitute outcome proof.
- **Control Center** — owns authorization, task/workspace ownership, leases, and
  promotion (`docs/planning/HANDOFF.md`, "Integration boundary").
- **GitForge** — execution and artifact delivery; its CI is the shared source of
  truth for green status.

AgentVerify must remain deployable without any of the above (`CLAUDE.md`,
"Architecture").

## 2. Verified current state (facts, this session)

Facts below were verified by reading source/config and by running commands at
the baseline commit in this checkout. CI run status was **not** verifiable this
session (neither `gh` nor GitForge MCP was authorized here); the last merge to
`main` visible in `git log` is PR #24 → `a716342`.

- **Workspace:** 14 crates (`Cargo.toml:2-19`), version `0.1.0`, MSRV
  `rust-version = "1.88"`, workspace-wide lints deny `panic`/`todo`/
  `unimplemented`/`unwrap_used`/`expect_used`/`print_stdout`/`print_stderr`,
  pedantic warn, CI promotes with `-D warnings` (`Cargo.toml:26-46`).
- **CI design** (`.github/workflows/ci.yml`): fmt; clippy (default **and**
  all-features, locked, `-D warnings`); test with Postgres 17 (:5433) and Redis
  7 (:6379) services, gated on `AGENTVERIFY_TEST_POSTGRES_URL` /
  `AGENTVERIFY_TEST_REDIS_URL`, plus contract-example validation and benchmark
  compilation; doc build with `-D warnings`; MSRV via cargo-hack; cargo-deny
  (advisories + bans/licenses/sources); wasm32-wasip1 check of core, contract,
  engine, receipt, policy; llvm-cov → Codecov (`fail_ci_if_error: false`).
- **Static test census:** 741 `#[test]`/`#[tokio::test]`/proptest attributes
  across workspace sources (grep at baseline). Counts are static evidence, not
  pass claims; the authoritative pass signal is CI.
- **Executed this session:** `agentverify-runtime` — 81 unit + 11 integration
  tests pass (commands and results in §4).

Known discrepancies between docs and repository reality (recorded, not fixed):

1. `docs/internal/TASKS.md:28` marks MCP, OTel, policy, recovery, and storage
   as "❌ Deferred", but those crates exist with test suites (e.g. recovery 55,
   policy 66, mcp 63, storage 22 test attributes). The task doc understates the
   implemented surface; it predates their implementation.
2. `docs/planning/HANDOFF.md` references
   `docs/planning/HANDOFF_AUDIT_2026-08-13.md` and
   `docs/planning/CODEX_CLI_EXECUTION_PACKETS_2026-08-13.md`; neither path
   exists in this repository (they are central-planning artifacts).
3. `docs/internal/HANDOFF.md:15,113` references `docs/HANDOFF_MINIMAX_M27.md`;
   the actual path is `docs/internal/HANDOFF_MINIMAX_M27.md`. Lines 8-12
   reference central authority files that are not in this repo.

## 3. Defect fixed this session (bounded, verified)

**`RedisIdempotencyStore::claim_or_check` violated its own recorded
degradation policy on the SETNX-error path**
(`crates/agentverify-runtime/src/idempotency_store.rs`, pre-fix lines 394-397
and 387-390).

- Recorded policy (test-pinned): when Redis cannot answer, the store returns
  `ClaimResult::Claimed` so the caller dispatches and reconciles afterwards —
  `redis_idempotency_store_claims_when_redis_is_unreachable` (pool-get failure
  → `Claimed`), and the serialize-failure branch behaved the same way.
- Violation: a `SET ... NX` **command** error (transient timeout, failover
  `READONLY`, auth refusal, cluster error) returned
  `ClaimResult::AlreadyClaimed` with no result. The executor then reports the
  action as a duplicate for a sibling that does not exist
  (`crates/agentverify-runtime/src/executor.rs:814-838`): a fabricated
  `Duplicate` receipt for an action that never dispatched, exactly during
  Redis incidents — the condition the policy says to ride through. The same
  outage produced opposite outcomes depending on whether the pool or the
  command failed.
- Fix: both SETNX error paths now classify via
  `RedisIdempotencyStore::claim_on_setnx_error` — `ErrorKind::TypeError`
  (`WRONGTYPE`) is a definitive "namespace occupied by foreign content" answer
  and still yields `AlreadyClaimed` (pinned by the wrong-type integration
  test); every other kind degrades to `Claimed` per the recorded policy. The
  policy is now documented on the type (`# Degradation Policy`) instead of
  living only in test comments.
- Out of scope (not changed): the availability-over-deduplication stance
  itself (fail-open under total Redis outage) — it is deliberate and
  test-pinned; operators should know duplicates are possible during a Redis
  incident.

### Evidence (commands as run; local Redis 127.0.0.1:6379)

| Command | Result |
|---|---|
| `cargo test -p agentverify-runtime --features redis redis_idempotency` (baseline, env var set via `--config 'env.AGENTVERIFY_TEST_REDIS_URL=…6379'`) | 9 passed |
| Same but pointed at `redis://127.0.0.1:6399` (closed port, differential) | 6 FAILED (connection refused) — proves the env-gated tests genuinely exercise the live server |
| Same filter after fix | 9 passed (both pinned behaviors preserved) |
| `cargo test -p agentverify-runtime --features redis setnx_` (new tests) | 2 passed (`setnx_type_error_is_a_definitive_occupied_key_answer`, `setnx_store_unavailability_degrades_to_claimed`) |
| `cargo test -p agentverify-runtime --features redis` (full crate, env var set) | 81 unit + 11 integration passed, 0 failed |
| `cargo fmt -p agentverify-runtime -- --check` | clean (one rustfmt reflow applied first) |
| `cargo clippy -p agentverify-runtime --all-features --all-targets -- -D warnings` | clean |

Residual seam, stated plainly: the changed behavior (non-`TypeError` SETNX
error → `Claimed`) is covered by the classification unit tests; an end-to-end
trigger would require a Redis that errors on `SET NX` without a type conflict
(e.g. an auth-refusing server), which this checkout's shared local Redis
cannot safely simulate. The `WRONGTYPE` → `TypeError` mapping is confirmed
empirically: the wrong-type test still passes with the new classification, so
the error kind check matches the observed server behavior.

## 4. Prioritized remaining work

| # | Task | Dependency | Acceptance criteria |
|---|---|---|---|
| 1 | **Enable the runtime `redis` feature in CI's test job.** The feature is non-default (`crates/agentverify-runtime/Cargo.toml:37-40`) and no workspace member enables it, so `cargo test --locked --workspace` never compiles `RedisIdempotencyStore` — its 9 tests have never executed in CI; `clippy --all-features` only type-checks them. Add a step running `cargo test -p agentverify-runtime --features redis` with `AGENTVERIFY_TEST_REDIS_URL` (the service already exists in the job). | none | CI logs show the Redis idempotency tests executing (not skipping) on every PR. |
| 2 | **Reconcile task documentation with the implemented surface.** Fix `docs/internal/TASKS.md` phase table (§2 item 1), repair the stale path references (§2 items 2-3), and keep exactly one authoritative handoff (`docs/planning/HANDOFF.md`). | none | A reader can navigate from `docs/planning/HANDOFF.md` to every referenced file without a dead link; task statuses match workspace reality. |
| 3 | **Decide and document engine compound-predicate `Unknown` semantics.** `PredicateEngine::eval_implies` collapses `Unknown` (antecedent → `Verified`, consequent → `Failed`) and `eval_all`/`eval_any` are two-valued, while `eval_not` propagates `Unknown` and the executor is built to receive engine `Unknown` (`crates/agentverify-engine/src/engine.rs:347-381`, `crates/agentverify-runtime/src/executor.rs:747-762`). No leaf evaluator can produce `Unknown` today, so this is latent, not live — no failing-first test is possible, which is why it was not changed here. The decision (three-valued vs. two-valued, documented in `docs/CONCEPTS.md`) must precede implementation; a signed `VERIFIED` derived from an unresolvable antecedent would be the worst-case outcome if it ever became live. | spec decision in docs | Either `CONCEPTS.md` documents two-valued compound semantics (and the dead `eval_not` arm is removed), or three-valued logic is specified and implemented with tests covering `Implies`/`All`/`Any` over `Unknown`. |
| 4 | **Promotion gates (unchanged from prior handoffs):** cross-process replay/idempotency proof, durable `ReceiptStore` wired into execution, real dispatch adapter behind the CLI, Control Center fixtures (orphan/stale/tampered/unauthorized/cross-project/replay rejection). | #1-#3 optional; Control Center availability required for the fixture | See promotion gate in `docs/internal/HANDOFF.md` — remain **deferred/unpromoted** until evidenced. |

## 5. Risks

- **Fail-open idempotency under Redis unavailability is by design** (documented
  in this change): duplicates are possible during a Redis incident. Callers
  needing hard duplicate protection must not treat the store as a lease.
- **Engine `UNKNOWN` collapse (§4 #3)** is latent; if leaf semantics ever grow
  indeterminate outcomes, compound predicates would silently mis-verify.
- **Documentation drift:** internal task docs understate the implemented
  surface; future audits must not infer scope from `TASKS.md` alone.
- **Unverified-from-here claims:** CI run status and coverage percentages were
  not observable this session; nothing in this document should be read as
  claiming a green pipeline.

## 6. Explicitly out of scope (this change)

- Production deployment, credentials, provider/deployment configuration.
- Control Center integration and the P5 fixture (external dependency, blocked).
- Any threshold lowering, lint relaxation, or dependency changes.
- Full-suite local runs (fmt/clippy/test were run focused on the touched
  crate; the workspace pipeline on GitHub/GitForge is the source of truth).
- The `docs/internal/` historical records (`HANDOFF.md`,
  `HANDOFF_MINIMAX_M27.md`, `TASKS.md`) — discrepancies are catalogued in
  §4 #2 rather than rewritten here, so the audit trail stays append-only.
