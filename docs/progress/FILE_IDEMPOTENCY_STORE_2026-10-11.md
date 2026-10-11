# File idempotency-store closeout — 2026-10-11

## Scope

Fail file-backed idempotency claims closed when the store cannot prove that a
claim was acquired, and make claim creation exclusive across processes.

Implementation commit: `734f0f0a31ba622f629f26171d8f14e24ffb5f9d` on
`codex/agentverify-idempotency-unknown-20261011` (PR #27).

## Change record

- Claims are created with exclusive file creation; a readable existing claim
  is `AlreadyClaimed`, while unreadable/unparseable state or persistence
  failure is `Unavailable` and is never cached as a successful claim.
- Completion updates use unique temporary paths before rename.
- Added a 16-process race test and refusal tests for corrupt, undecodable, and
  unpersistable entries.
- Clarified the partial-file/write-error and orphaned-temp-file limitations;
  ambiguous entries are not auto-deleted because they may be live claims.
- Independent GLM-5-Turbo review found no correctness blockers. It noted
  low-severity durability/cleanup tradeoffs; those are recorded above and do
  not justify unsafe recovery.

## Validation evidence

Exact GitHub Actions head: `734f0f0a31ba622f629f26171d8f14e24ffb5f9d`,
run `38184124947`.

| Gate | Result |
|---|---|
| Formatting | pass |
| Clippy (default) | pass |
| Clippy (all features) | pass |
| Test | pass |
| Coverage | pass |
| MSRV | pass |
| WASM | pass |
| Documentation | pass |
| Dependency advisory audit | pass |
| License/source policy | pass |

No GitForge remote is configured in this checkout, so this receipt does not
claim a GitForge lane. The project PR remains draft pending the maintainer's
integration review and any repository-specific promotion steps. No release or
deployment occurred.
