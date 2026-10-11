# AgentVerify Redis claim-outcome handoff — 2026-10-11

**Branch:** `codex/agentverify-idempotency-unknown-20261011`
**Base:** `main` at `c66fa32` (includes the reviewed change `6d94ff3`)
**Scope:** correctness fix for the four independent-review findings against
`6d94ff3` (`fix(runtime): align Redis claim degradation policy`).
**Evidence tier:** Tier B (deterministic executor/store tests) plus live-Redis
integration runs. This is not Tier D promotion evidence.

## 1. Findings addressed

Independent review of `6d94ff3` established that the recorded degradation
policy ("an unanswerable SETNX degrades to `Claimed` so the caller dispatches
and reconciles afterwards") was itself the defect:

1. **Dispatch without a proven claim.** Pool exhaustion, serialization
   failures, and SETNX command errors all returned `(Claimed, None)`, so a
   Redis incident stripped idempotency protection exactly when it was needed.
2. **Claim retry after a proven occupied answer.** After `SET ... NX`
   answered nil (key occupied), a failed `GET` was conflated with a nil GET
   and the claim was retried; a retry error degraded to `Claimed`, so the
   caller could dispatch against a key another request still owned.
3. **Unreachable WRONGTYPE special case.** The `TypeError → AlreadyClaimed`
   exception on SETNX errors is unreachable on Redis ≥ 8.0 (§3) and over-
   generalised on Redis ≤ 7.x.
4. **No CI execution of the redis feature.** CI's Test job ran
   `cargo test --workspace` with default features only; the Redis store tests
   never compiled or ran in CI despite a Redis service being provisioned.

## 2. Design decision

The store never guesses. `claim_or_check` now answers with exactly one of:

| Outcome | Condition | Executor behavior |
|---|---|---|
| `(Claimed, None)` | Redis answered `OK` | dispatch proceeds |
| `(AlreadyClaimed, Some(r))` | occupied, outcome readable | cached result returned |
| `(AlreadyClaimed, None)` | occupied (SET NX answered nil, incl. corrupted/foreign content), outcome unreadable | `Duplicate`, **no dispatch**, no claim retry |
| `(Unavailable, None)` | new variant: pool error, serialization error, or any `SET ... NX` error (incl. `WRONGTYPE`), or an error on the single expiry-race re-claim | **no dispatch**, no receipt; `ExecutorError::IdempotencyUnavailable` (retryable: nothing was sent, no claim is held) |

Key points:

- `ClaimResult::Unavailable` is a new public enum variant. The `claim_or_check`
  contract documents that implementors must return it rather than guess; a
  fabricated `Claimed` double-dispatches and a fabricated `AlreadyClaimed`
  hides live work behind a duplicate verdict.
- `ExecutorError::IdempotencyUnavailable(String)` is a new error variant. Both
  `execute` and `execute_with_executor` reject with it before any dispatch and
  before any `complete`/`release` side effect: no receipt is produced, so no
  `Duplicate` evidence can be fabricated for an action that never ran.
- The occupied-but-unreadable answer stays `Duplicate` (no dispatch). This is
  honest: SET NX answering nil proves a key exists at that exact name. The
  50 ms poll still runs; if the sibling completes inside the window its real
  verdict is returned.
- The only remaining claim retry is the genuine expiry race: SET NX nil →
  GET returns nil (key provably absent) → single re-claim. A re-claim error
  is `Unavailable`, never a fabricated `Claimed`.
- `claim_on_setnx_error` (the classifier under review) is **deleted**. The
  claim decision moved into `RedisIdempotencyStore::claim_with_connection`,
  generic over `redis::aio::ConnectionLike`, so every command-error path is
  unit-tested deterministically over a scripted connection (no server, no
  sleeps) — including the exact command sequence sent.

### Redis semantics as documented now (measured, §3)

- `SET key value NX EX ttl` answers `OK` (claimed, value written), nil (key
  exists, value untouched), or an error. On Redis ≥ 8.0 a wrong-type key gets
  **nil**, not `WRONGTYPE`; on Redis ≤ 7.x it raises `WRONGTYPE`
  (`ErrorKind::TypeError` in the crate). Both shapes end in a non-dispatch
  outcome (`Unavailable` on ≤7.x via the error path; `AlreadyClaimed` on ≥8.0
  via the failing follow-up GET).
- After a nil answer the key holds a string, so the follow-up `GET` cannot be
  `WRONGTYPE` *for our own entries*; `WRONGTYPE` on the GET is the Redis ≥ 8
  foreign-key shape. GET can return the entry, nil (expired/released between
  commands), or a store-level error.

## 3. Evidence (commands as run; local Redis 127.0.0.1:6379)

Wire-level semantics probes (redis-cli / raw RESP via nc, Redis **8.0.2**):

```
RPUSH zzz:probe a b                       → 2
SET zzz:probe val NX EX 300 (raw RESP)    → $-1   (nil; no WRONGTYPE)
SET zzz:probe val            (raw RESP)   → +OK   (plain SET overwrites; TYPE → string)
```

On Redis ≤ 7.x (the CI `redis:7` image) the same claim command raises
`WRONGTYPE`; both server behaviors are pinned server-independently by the
scripted-connection tests.

Focused tests and gates (worktree on NVMe; workspace-wide sweep routed to
GitForge per AGENTS.md):

```
AGENTVERIFY_TEST_REDIS_URL=redis://127.0.0.1:6379 \
  cargo test -p agentverify-runtime --features redis
  → 88 passed / 0 failed (lib), 11 passed / 0 failed (integration)

cargo test -p agentverify-runtime          (default features)
  → 72 passed / 0 failed (lib), 11 passed / 0 failed (integration)

cargo clippy -p agentverify-runtime --features redis --all-targets -- -D warnings  → clean
cargo clippy -p agentverify-runtime --all-targets -- -D warnings                   → clean
cargo fmt --all -- --check                                                          → clean
```

New deterministic coverage (both command-error paths end-to-end through the
real claim logic, proving command sequences, not a classifier):

- `claim_command_error_is_unavailable_and_sends_nothing_else` — SETNX
  `WRONGTYPE` and transport errors → `Unavailable`; exactly `["SET"]` sent.
- `get_error_after_occupied_answer_reports_claimed_without_a_retry` — nil →
  GET error (WRONGTYPE and transport) → `(AlreadyClaimed, None)`; exactly
  `["SET", "GET"]` sent: the claim is never retried.
- `expired_entry_reclaims_exactly_once`, `lost_reclaim_race_reports_already_claimed`,
  `failed_reclaim_is_unavailable_not_claimed`, `completed_sibling_entry_is_read_back_after_occupied_answer`.
- `redis_idempotency_store_does_not_claim_when_redis_is_unreachable` (now
  runs without a live Redis) — pool failure → `Unavailable`.
- `redis_idempotency_store_does_not_claim_a_key_holding_the_wrong_redis_type`
  (live Redis) — foreign key never claimable, `release` repairs the namespace.
- Executor level: `unavailable_claim_errors_instead_of_dispatching` and
  `simulated_execute_unavailable_claim_errors_without_side_effects` prove
  zero dispatches, zero completions, zero releases, and a non-Duplicate
  error; `occupied_claim_with_unreadable_outcome_is_duplicate_without_dispatch`
  proves zero dispatches with an honest `Duplicate`.

## 4. Files

- `crates/agentverify-runtime/src/executor.rs` — `ClaimResult::Unavailable`,
  `ExecutorError::IdempotencyUnavailable`, claim handling in both execute
  paths, executor tests.
- `crates/agentverify-runtime/src/idempotency_store.rs` — claim rewrite,
  scripted-connection tests, rewritten live tests, deleted classifier.
- `.github/workflows/ci.yml` — Test job now also runs
  `cargo test --locked --workspace --all-features` against the provisioned
  Redis/Postgres services.
- `docs/planning/HANDOFF.md` — the reversed 2026-10-10 policy bullet is
  corrected; this receipt is referenced.

## 5. Limitations

- `FileIdempotencyStore` keeps its own claim-on-unreadable policy; the review
  findings and this fix are Redis-specific. A later pass should decide whether
  the file store also needs an `Unavailable` outcome (its cross-process
  semantics differ: the in-process cache remains authoritative).
- `WasmIdempotencyStore` (agentverify-http) has its own `ClaimResult` enum and
  was intentionally untouched.
- Adding a variant to the public `ClaimResult` enum is a breaking change for
  downstream exhaustive matches; acceptable pre-1.0, and the trait docs now
  state the implementor contract.
- `complete`/`release` Redis failures remain logged non-fatal (pre-existing);
  a failed `complete` after dispatch still leaves an in-flight entry until
  TTL expiry.
- Workspace-wide local test/clippy sweeps are blocked by policy and were not
  run locally; the GitForge/CI pipeline is the broad gate for this branch.

## 6. Next steps

1. Land the GitForge pipeline run for this branch (fmt/clippy/tests/coverage
   with all features, including the new CI step against `redis:7`).
2. Decide whether CI should matrix Redis 7 and 8: the wrong-type answer shape
   differs (`WRONGTYPE` vs nil) and today's live test asserts only the
   version-stable contract (never `Claimed`).
3. Consider recording claim provenance (`Duplicate` due to a readable sibling
   verdict vs an unreadable occupied key) on the receipt.
4. Extend the `Unavailable` outcome decision to `FileIdempotencyStore` and any
   PostgreSQL store built on `IdempotencyStore`.
