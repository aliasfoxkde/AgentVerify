# Redis Concurrency Test Invariant Fix — 2026-10-11

**Branch:** `codex/agentverify-redis-isolation-20261011` (base `origin/main` @ `25571b7`)
**Trigger:** CI failure in PR #27 run `38106630891`
**Scope:** `crates/agentverify-observe/tests/redis_integration.rs` — test only; no library code changed.

---

## Observed failure

`concurrent_writers_are_all_counted_by_concurrent_observers` failed at the
ordering assertion (former line 530):

```
assert_eq!(observed.last().copied(), sorted.last().copied());
left: 22, right: 24
```

The final persisted-cardinality check (`scard == CONCURRENCY`) had not yet run;
the failure was purely in the collected-observation comparison.

## Why the old invariant was invalid

The assertion was justified by the comment *"the cardinality only ever grows,
so the last observation is the highest."* That conflates two different orders:

- **Cardinality growth over time** — true on the server.
- **Collection order in the test** — false as a proxy for time.

Each of the 24 spawned tasks writes its own set member and then reads
`SCARD` through the observer, producing a point-in-time snapshot
(`redis_observer.rs` returns `u64 -> Value::Number` for `scard`). The test
collects those snapshots by awaiting handles in spawn order
(`for handle in handles`), which is **not** the order the snapshots were
taken. Task `i`'s handle is awaited last, but its `SCARD` may have executed
before other tasks' final writes landed — exactly what CI saw: the
last-awaited snapshot was `22` while the maximum ever observed was `24`
(the `right` side being 24 confirms the full set *was* observed by some task).

## Replacement invariant

The concurrency guarantees something the old assertion only hoped to check:
every task writes before it reads, and Redis serializes commands on a single
server, so the task whose `SADD` serializes last performs its `SCARD` after
all 24 members exist. Therefore:

```rust
assert_eq!(
    observed.iter().copied().max(),
    Some(CONCURRENCY as u64),
    "some observer must have seen the fully populated set"
);
```

This is deterministic (no timing dependence beyond what the test already
assumes) and strictly expresses the test's intent: concurrent observers all
count a concurrently written set.

## Assertions retained / changed

| Assertion | Status |
|---|---|
| Per-observation in-flight range `1..=CONCURRENCY` (line ~520) | **Retained unchanged** |
| Final persisted cardinality `scard == CONCURRENCY` (line ~537) | **Retained unchanged** |
| `observed.last() == sorted.last()` (ordering) | **Removed** — invalid invariant |
| `max(observed) == CONCURRENCY` | **Added** — guaranteed by write-before-read + server-side serialization |

## Explicitly not done

- No sleeps/retries, no `CONCURRENCY` reduction, no weakening of the final
  persisted-total check, no fabricated test data.
- No library (non-test) code changed.

## Verification

- Focused test run locally against live Redis (`AGENTVERIFY_TEST_REDIS_URL`
  set, single server) — see PR description for the exact command and result.
- Fresh hosted CI run on the PR branch is the authoritative gate.
