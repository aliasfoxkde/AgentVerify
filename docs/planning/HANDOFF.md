# AgentVerify execution handoff

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

1. Refresh the evidence boundary to the exact clean commit.
2. Define and version the authenticated receipt-ingestion contract, including
   project, task, job, agent, commit, verifier, replay, and ownership identity.
3. Wire durable persistence and restart/replay tests across processes.
4. Replace the convenience simulated dispatch path with an explicitly named
   real adapter, or keep it clearly demo-only.
5. Add Control Center fixtures that reject orphan, stale, tampered,
   unauthorized, cross-project, and replayed receipts.
6. Run strict formatting, Clippy, tests, coverage, Aegis, and the GitForge
   pipeline; retain a redacted receipt and rollback path.

Do not add placeholder adapters or claim promotion until these gates have
authoritative evidence. The owner must approve any production observer scope
and deployment credentials.
