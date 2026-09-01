# AgentVerify execution handoff

**Last updated:** 2026-09-01
**Evidence boundary (central audit):** branch `docs/platform-handoff-provenance-20260901`, HEAD `55aa90c2e28fe123ed6bb0d79ac98beb097315d2`, 0 dirty status entries.
**Rating:** 3/5 — deterministic verification source is qualified; production integration is open.

This handoff is registered against the Platform audit in
`docs/planning/HANDOFF_AUDIT_2026-08-13.md` and the execution packets in
`docs/planning/CODEX_CLI_EXECUTION_PACKETS_2026-08-13.md`.
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
