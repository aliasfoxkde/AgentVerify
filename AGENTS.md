# AgentVerify agent guidance

AgentVerify is the outcome-verification and signed-evidence layer. It evaluates
postconditions and produces receipts; it does not own Control Center
authorization, GitForge execution, Amortyx routing, or VIVERE experimentation.

## Change workflow

1. Inspect `docs/planning/HANDOFF.md`, `docs/internal/HANDOFF.md`, the current
   branch, and the workspace status before editing.
2. Keep changes focused and preserve the distinction between deterministic
   verification semantics and real external dispatch/observation.
3. Run `cargo fmt --all -- --check`, focused tests, workspace tests, strict
   Clippy, and the applicable security/contract checks through GitForge for
   expensive validation.
4. Record exact commit, commands, test counts, coverage artifacts, and
   integration limitations in a dated receipt before claiming completion.
5. Commit and push focused changes; do not promote from local unit tests alone.

## Safety invariants

- `UNKNOWN` is not `FAILED`; an ambiguous external outcome must remain
  explicitly ambiguous.
- Receipts are evidence, not authorization or ownership proof. Bind project,
  task, job, agent, commit, verifier, and replay identities before integration.
- Never dispatch an external action again without verify-before-retry and
  idempotency protection.
- Keep observer URLs, credentials, generated artifacts, and runtime state out
  of source, fixtures, logs, and documentation.
- The current `verify` convenience path must not be described as real external
  dispatch until a production adapter is wired and tested.

## Current readiness

The Rust workspace and deterministic/runtime test layers are evidenced, but
authenticated cross-process service integration, durable production persistence,
and Control Center correlation remain open promotion gates.
