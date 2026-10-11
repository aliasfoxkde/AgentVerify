# AgentVerify file-store claim fail-closed handoff — 2026-10-11

**Branch:** `codex/agentverify-file-store-claim-failclosed-20261011`
**Stacked on:** `codex/agentverify-idempotency-unknown-20261011` (head `dc956d0`)
**Scope:** close the `FileIdempotencyStore` gap left open in
`HANDOFF_REDIS_CLAIM_UNAVAILABLE_2026-10-11.md` §5 (item 1) / §6 (item 4).
**Evidence tier:** Tier B (deterministic store + executor tests over a real
filesystem failure). Not Tier D promotion evidence.

## 1. Diagnosis

`FileIdempotencyStore::claim_or_check` treated a failed claim persist as
non-fatal: a `save_entry` error was logged and the store still inserted the
in-flight entry into its cache and answered `(Claimed, None)`. Two violations
of the `IdempotencyStore::claim_or_check` implementor contract ("must return
`Unavailable` instead of guessing whenever the store cannot prove the claim
state"):

1. The caller could dispatch with no durable idempotency claim — measured in
   RED below: the executor recorded `attempts: 3` and produced a full
   `Failed` receipt from an unpersisted claim (the verify-before-retry loop
   re-dispatched each time, unprotected).
2. The unpersisted entry stayed in the cache, so later calls in the same
   process answered `(AlreadyClaimed, …)` from a phantom claim.

The executor side was already correct: `ClaimResult::Unavailable` is rejected
with a retryable `ExecutorError::IdempotencyUnavailable` before any dispatch,
`complete`, or `release` (pinned by `unavailable_claim_errors_instead_of_dispatching`).
Only the file store violated the producer side of the contract.

## 2. Change

On persist failure the claim path now returns `(Unavailable, None)` and
leaves the cache untouched. Everything else is unchanged: persist success,
cache hits, on-disk hits, in-flight and completed known-duplicate answers,
and `complete`/`release`. The seam is the existing public
`IdempotencyStore::claim_or_check`; no API broadening.

The failure seam is a real filesystem failure, deterministic and not
root-sensitive: the store is constructed over an existing directory which is
then removed, so persisting `<dir>/<hash>.tmp` fails with `ENOENT`.

## 3. Evidence (commands as run, worktree on NVMe)

RED — tests added first, run against the unmodified claim logic:

```
cargo test -p agentverify-runtime --lib -- \
  file_idempotency_store_claim_fails_closed_when_persistence_fails \
  file_store_persistence_failure_never_dispatches
→ 2 failed; 0 passed
  store:    assertion `left == right` failed: left: Claimed, right: Unavailable
  executor: "a claim that cannot be persisted must not reach dispatch:"
            (Failed, Receipt { …, attempts: 3, …, idempotency_key:
            Some("unpersisted-claim-key") })
```

GREEN — same command after the fix:

```
→ 2 passed; 0 failed
```

Focused package gates:

```
cargo test -p agentverify-runtime --lib                           → 73 passed / 0 failed
cargo clippy -p agentverify-runtime --all-targets -- -D warnings  → clean
cargo fmt --all -- --check                                        → clean
```

New/changed coverage:

- `file_idempotency_store_claim_fails_closed_when_persistence_fails`
  (rewrites `file_idempotency_store_warns_when_claims_cannot_be_persisted`,
  which asserted the old fail-open contract): persistence failure →
  `(Unavailable, None)`, and a second attempt answers `Unavailable` again —
  no phantom cached claim.
- `file_store_persistence_failure_never_dispatches` (executor, real
  `FileIdempotencyStore` + `CountingDispatch`): end-to-end — zero
  dispatches and `ExecutorError::IdempotencyUnavailable` naming the key.
- `file_idempotency_store_serializes_concurrent_claims_to_one_winner`:
  comment updated (the store can now report `Unavailable`); assertions
  unchanged and still passing.

## 4. Files

- `crates/agentverify-runtime/src/idempotency_store.rs` — claim fail-closed,
  regression test rewrite, comment fix.
- `crates/agentverify-runtime/src/executor.rs` — end-to-end regression test.
- `docs/internal/HANDOFF_FILE_STORE_CLAIM_FAILCLOSED_2026-10-11.md` — this receipt.

## 5. Limitations

- `complete`/`release` persist failures remain logged non-fatal
  (pre-existing, same policy as the Redis store); a failed `complete` after
  dispatch leaves the entry in-flight in-process only.
- `WasmIdempotencyStore` (agentverify-http) keeps its own `ClaimResult` enum
  and was intentionally untouched.
- An `Unavailable` claim is retryable once the filesystem recovers: nothing
  was dispatched and no claim is held, so a later successful claim is safe.
- Workspace-wide sweeps were not run locally (AGENTS.md routes expensive
  validation through GitForge); the evidence here is the focused
  runtime-package run above plus hosted CI.
