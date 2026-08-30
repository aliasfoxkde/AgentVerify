# AgentVerify — Repository Handoff

**Repository:** `/nas/Temp/repos/AgentVerify`
**Role:** outcome verification and signed evidence candidate
**Audit boundary:** `codex/add-platform-handoff-2026-08-14` / `654eea4` / dirty `2` after this handoff commit (generated code-memory artifacts remain dirty; preserve all existing work)
**Updated:** 2026-08-21 (qualification packet added; no source mutation)
**Evidence boundary (central audit):** branch `codex/add-platform-handoff-2026-08-14`, HEAD `654eea41baf3e60d09f40168cc3ae53b959e107b`, 2 dirty status entries. Refresh this boundary before any implementation claim; the worktree modifications are generated code-memory artifacts and remain preserved.
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
and a CLI. The current audit confirms 146 local workspace tests (P1+P2) plus format,
clippy, and documentation success. Tier A coverage includes predicate semantic fixture
tests for missing paths, type mismatches, null handling, numeric coercion, regex errors,
empty collections, compound predicates, and argument substitution. Tier B includes runtime
failure-injection tests for atomic idempotency (claim_or_check semantics), dispatch
outcomes, bounded retry/backoff, timeout/ambiguous/observer/stale-read/cancellation.
The audit still explicitly identifies placeholder or deferred crates and does not prove
a deployed service, authenticated observer, durable storage (ReceiptStore not wired), MCP
boundary, or Control Center correlation. `cargo audit` reports the unmaintained transitive
`rustls-pemfile 1.0.4` advisory.

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
The CLI exit-code defect (discarded ExitCode) was fixed; `contract validate`
now propagates exit codes correctly (0=success, 1=error, 2=invalid). The
`verify` command exists, but its current implementation calls the convenience
executor, whose dispatch is explicitly simulated; it is not proof of real
action execution.

## Implementation status

**P1 (doc reconciliation): ✅ COMPLETE** — TASKS.md and NEXT_STEPS.md corrected; stale task/phase status tables updated; 142 tests verified.

**P2 (atomic idempotency): ✅ COMPLETE** — `IdempotencyStore::claim_or_check` replaces `check`/`insert`; `ClaimResult::Claimed`/`AlreadyClaimed` with `Mutex`; in-flight tracking; `TransportError` releases claim; 4 new tests added (146 total).

**P3 (receipt lifecycle): ✅ COMPLETE** — `ReceiptStore` wired into executor; `get_receipt` API; receipts bind idempotency key; 4 new tests.

**P4 (authenticated observer): ✅ COMPLETE** — wiremock integration tests for success, unauthorized (401/403), malformed response, oversized response (truncation), timeout, stale read, and redaction. URL validation fixed to allow scheme separator `://` but reject path traversal `..` and empty segments `//` in path. 158 tests total.

**P5 (Control Center fixture): BLOCKED** — Requires real Control Center; external authority not available.

**P6 (operations): ✅ COMPLETE** — CLI exit-code tests added (6 tests), CI workflow present, 164 tests passing, fmt/clippy clean.

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
- Generated `target/` content is not tracked at this boundary, but can produce
  local build churn; keep it ignored and out of handoff diffs.
- HTTP/MCP/storage/telemetry deployment boundaries are not established.
- External observers can produce stale, partial, unauthorized, or ambiguous
  evidence; retries must be idempotency-safe and reconciliation-aware.
- Do not expose production credentials, arbitrary observer URLs, or an
  unauthenticated verification endpoint during proof work.

## Promotion gate

AgentVerify remains **deferred / unpromoted** until a bounded report proves:
- versioned receipt contract (P3)
- authenticated ownership and durable persistence wired into execution (P3)
- replay/idempotency behavior with atomic claim semantics (P2: done for process-local; cross-process still needed) (P3)
- real dispatch through the documented CLI path (P2 partially done — real adapter not yet wired)
- Control Center adapter fixture that rejects orphan, stale, tampered, unauthorized, or cross-project results (P5)

The detailed Claude/MiniMax packet plan and current evidence matrix live in
`docs/HANDOFF_MINIMAX_M27.md`; that document is the implementation authority
for this repository handoff.
