# AgentVerify — Repository Handoff

**Repository:** `/nas/Temp/repos/AgentVerify`
**Role:** outcome verification and signed evidence candidate
**Audit boundary:** `codex/add-platform-handoff-2026-08-14` / `d8072f35c7643e12773c30e3fc0ac3486791141c` / dirty `2`
**Updated:** 2026-08-14
**Evidence boundary (central audit):** branch `codex/add-platform-handoff-2026-08-14`, HEAD `d8072f35c7643e12773c30e3fc0ac3486791141c`, 2 dirty status entries; refresh this boundary before any implementation claim.
**Central planning:** `AUTHORITY_INDEX_2026-08-14.md`, `MASTER_EXECUTION_PLAN_2026-08-14.md`, and `CODEX_CLI_EXECUTION_PACKETS_2026-08-13.md`
**Provenance markers:** `HANDOFF_AUDIT_2026-08-13.md` and
`CODEX_CLI_EXECUTION_PACKETS_2026-08-13.md` remain recorded for the central
freshness validator; the 2026-08-14 authority index and master plan govern
current sequencing.
**Rating:** not scored; this is not a promotion signal
**Authority:** this file records the repository boundary and current evidence;
`docs/HANDOFF_MINIMAX_M27.md` contains the longer implementation plan. Neither
file is a promotion decision by itself.

## Platform disposition

AgentVerify is a validation/proof component for the agentic platform, not a
Control Center replacement and not an MVP prerequisite. It may eventually
provide independent verification of worker outcomes, receipts, and
postconditions. It must first earn an authenticated, persisted, replay-safe
integration contract through `AGENTVERIFY-P1-01`.

## Current evidence

The repository contains Rust crates for core domain types, contract parsing,
predicate evaluation, runtime orchestration, HTTP observation, receipt types,
and a CLI. The existing handoff reports local workspace tests, formatting, and
clippy success, but explicitly identifies placeholder or deferred crates and
does not prove a deployed service, authenticated observer, durable storage,
MCP boundary, or Control Center correlation.

Treat these as separate evidence tiers:

| Tier | Provenance | Interpretation |
|---|---|---|
| A | core/contract/engine unit tests | deterministic local semantics only |
| B | runtime failure-injection tests | executor/retry behavior in the tested seam |
| C | HTTP/receipt tests | bounded observer/signature behavior if rerun at this SHA |
| D | authenticated cross-process service + Control Center receipt | promotion evidence; currently open |

Do not promote from Tier A–C to Tier D by inference. In particular,
`UNKNOWN` must remain distinct from `FAILED`; a timeout after dispatch is not
proof that the action did not happen; and a signed receipt is not proof of
ownership or persistence unless its source and verifier identities are bound.

## Immediate packet — AGENTVERIFY-P1-01

Use the central packet in
`/nas/Temp/repos/Platform-Architecture/docs/planning/CODEX_CLI_EXECUTION_PACKETS_2026-08-13.md`
and the evidence template at
`/nas/Temp/repos/Platform-Architecture/docs/planning/AGENTVERIFY_P1_EVIDENCE_TEMPLATE.md`.

Execute one subpass per process in this order:

1. **P1-01A — baseline:** preserve the two graph-artifact modifications,
   record the full SHA, and prove which generated files are tracked. Do not
   delete or reset `target/` automatically.
2. **P1-01B — behavior:** run bounded format/test/clippy/doc probes and map
   advertised versus executable CLI/runtime features. Label placeholder tests.
3. **P1-01C — contract:** define the versioned receipt/result envelope,
   authentication, ownership, persistence, replay/idempotency, and the exact
   Control Center project/task/job correlation fields. Do not add a listener or
   edit Control Center in this packet.
4. **P1-01D — promotion decision:** record the strongest proven tier, exact
   unmet gate, and whether AgentVerify remains deferred. A compile or unit
   result alone cannot close this packet.

Every worker must use disposable build/artifact paths, bounded commands, and
return the standard packet report. Preserve unrelated checkout changes.

## Integration contract to preserve

The future adapter should carry at least:

```text
project_id, task_id, work_request_id, job_id, agent_id,
contract_id/version, action_id/idempotency_key,
source/workspace/commit identity, outcome,
evidence digest + bounded evidence, observed_at,
verifier identity/version, signature, replay key
```

Control Center owns authorization, task/workspace ownership, lease state, and
promotion. AgentVerify owns evaluation and evidence semantics. Aegis may scan
the implementation/receipt path; Oracle may independently validate a claimed
result; neither may silently replace the other’s authority.

## Known risks

- Placeholder crates and CLI commands may make the workspace appear more
  complete than the runnable surface is.
- Generated `target/` content is tracked or otherwise produces source-control
  churn; resolve this in a deliberate repository-hygiene packet.
- HTTP/MCP/storage/telemetry deployment boundaries are not established.
- External observers can produce stale, partial, unauthorized, or ambiguous
  evidence; retries must be idempotency-safe and reconciliation-aware.
- Do not expose production credentials, arbitrary observer URLs, or an
  unauthenticated verification endpoint during proof work.

## Promotion gate

AgentVerify remains **deferred / unpromoted** until a bounded report proves a
versioned receipt contract, authenticated ownership, durable or explicitly
scoped persistence, replay/idempotency behavior, and a Control Center adapter
fixture that rejects orphan, stale, tampered, or cross-project results.
