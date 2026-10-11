# AgentVerify execution handoff

**Last updated:** 2026-10-11
**Evidence boundary (central audit):** branch `main`, HEAD `2b6d021`, 0 dirty
status entries at audit start. Source code at `2b6d021` is identical to the
previously recorded boundary `a7163422837c6f85cf42fa61b3c29653b247b1ff`; the
delta between them is a docs-only commit. Worker changes on
`codex/agentverify-platform-handoff-20261011` (2026-10-10) and
`codex/agentverify-idempotency-unknown-20261011` (2026-10-11) are recorded
separately in `docs/internal/HANDOFF_PLATFORM_2026-10-10.md` and
`docs/internal/HANDOFF_REDIS_CLAIM_UNAVAILABLE_2026-10-11.md`.
**Rating:** 3/5 — deterministic verification source is qualified; production integration is open.

This handoff is registered against the Platform audit in
`docs/planning/HANDOFF_AUDIT_2026-08-13.md` and the execution packets in
`docs/planning/CODEX_CLI_EXECUTION_PACKETS_2026-08-13.md` (central-planning
artifacts; neither file is present in this repository — see the 2026-10-10
handoff, §2).
**Status:** Source and deterministic verification layers are evidenced; authenticated cross-process integration and production promotion remain open.

This is the canonical repository-local handoff for Platform-Architecture work.
The longer historical evidence record remains in `docs/internal/HANDOFF.md`.

## Current evidence

- Rust workspace, core contracts, predicate engine, runtime, observers, receipt
  signing, policy, recovery, telemetry, MCP, HTTP, storage, and CLI crates are
  present in the workspace manifest.
- The repository has a committed MIT license, version `0.1.0`, CI workflow, and
  extensive unit/integration coverage claims documented in the internal
  handoff and changelog.
- 2026-10-10: `RedisIdempotencyStore::claim_or_check` SETNX error paths were
  aligned with the recorded degradation policy (unavailable store → `Claimed`;
  `WRONGTYPE` → `AlreadyClaimed`), with regression coverage. The runtime
  `redis` feature remains non-default, so those tests still need an explicit
  `--features redis` CI step to execute (2026-10-10 handoff, §4 #1).
- 2026-10-11: The 2026-10-10 degradation policy above was **reversed** after
  independent review: an unanswerable claim now reports the new
  `ClaimResult::Unavailable` and the executor rejects the action with a
  retryable, non-duplicate `ExecutorError::IdempotencyUnavailable` instead of
  dispatching unprotected or fabricating a `Duplicate` receipt. An occupied
  key whose outcome cannot be read is reported as `(AlreadyClaimed, None)`
  and its claim is never retried. CI now also runs
  `cargo test --workspace --all-features` against the Redis service. See
  `docs/internal/HANDOFF_REDIS_CLAIM_UNAVAILABLE_2026-10-11.md`.
- Local evidence does not by itself prove a deployed authenticated service,
  durable cross-process receipt persistence, real external dispatch through the
  CLI, or Control Center correlation.

## Integration boundary

Control Center owns authorization, project/task/job correlation, workspace
ownership, leases, and promotion. AgentVerify owns deterministic postcondition
evaluation, verify-before-retry semantics, and signed evidence. GitForge owns
execution and artifact delivery. Aegis scans the implementation and receipt
path; Oracle may independently critique results. Amortyx and VIVERE remain
separate boundaries.

## Next implementation packet

1. Define and version the authenticated receipt-ingestion contract, including
   project, task, job, agent, commit, verifier, replay, and ownership identity.
2. Wire durable persistence and restart/replay tests across processes.
3. Replace the convenience simulated dispatch path with an explicitly named
   real adapter, or keep it clearly demo-only.
4. Add Control Center fixtures that reject orphan, stale, tampered,
   unauthorized, cross-project, and replayed receipts.
5. Run strict formatting, Clippy, tests, coverage, Aegis, and the GitForge
   pipeline; retain a redacted receipt and rollback path.

The exact boundary above is documentation-only advancement from the previously
recorded source state. It does not assert that the open integration packets
have been implemented or that the clean checkout is production-ready.

Do not add placeholder adapters or claim promotion until these gates have
authoritative evidence. The owner must approve any production observer scope
and deployment credentials.
