# AgentVerify → Platform Architecture handoff (2026-10-10)

**Author role:** Platform-Architecture execution agent
**Scope:** assessment of AgentVerify's verification workflow, reliability risks,
tests/CI, and GitForge/Control Center integration seams; plus one scoped fix
and the next implementation packet.
**Evidence boundary:** branch `codex/pa-handoff-agentverify-20261010`, base =
`main` @ `2b6d021` (origin/main in sync at branch point). Working tree at
inspection: this handoff, `docs/internal/TASKS.md` ledger refresh, an
idempotency-claim fix in `agentverify-runtime`, a doc-comment fix in
`agentverify-http`, and a changelog entry. Nothing else.
**Method:** source read of all 14 crates (no local builds/tests run, per
campaign rules), codebase-memory index `nas-Temp-repos-AgentVerify`, GitHub
Actions evidence via `gh`, GitForge repo inventory via harness MCP. Every
claim below is tagged **[confirmed]** (read from source or CI at the SHAs
named), **[inferred]** (source-grounded reasoning about unexercised
conditions), or **[unverified]** (no evidence either way).
**Related authority:** `AGENTS.md` (change workflow + safety invariants),
`docs/planning/HANDOFF.md` (repo-local handoff), `docs/internal/HANDOFF.md`
(tier model A–D), `docs/internal/TASKS.md` (task ledger, refreshed in this
branch). This file does not supersede them; it is the 2026-10-10 assessment.

---

## 1. Verdict

AgentVerify's deterministic verification core is real, coherent, and well
tested at the unit/property level: contract parsing → precondition gate →
dispatch outcome classification → bounded observation → per-postcondition
evaluation → aggregate verdict → signed, digest-bound receipt. `UNKNOWN` is
preserved end-to-end on timeout/ambiguous paths **[confirmed]**.

It is still a library without a production dispatch adapter: the only
executor shipped is `SimulatedActionExecutor` (always returns `Completed`),
so nothing in this repo can *cause* an external action, only *observe and
judge* one **[confirmed]**. The durable, authenticated, cross-process
promotion path (internal Tier D) remains unbuilt: the Control Center client
is one-directional and never exercised against a real CC; GitForge has no
code-level seam at all **[confirmed]**. crates.io publication of v0.1.0 never
happened (registry token absent in the release run) **[confirmed]**.

Rating: **3/5 as a verification library, 2/5 as a platform-integrated
component** — deterministic semantics evidenced; integration, durability,
and real dispatch open. Same posture as `docs/planning/HANDOFF.md`; this
handoff sharpens it with concrete defects and file-level next steps.

## 2. Verification workflow as implemented

All in `crates/agentverify-runtime/src/executor.rs` unless noted; line
numbers refer to this branch.

1. **Atomic idempotency claim** — `claim_or_check` (`IdempotencyStore`,
   executor.rs:124–141); process-local `IdempotencyRegistry` (Mutex +
   `InFlight`/`Completed` states, executor.rs:157–224). First caller
   dispatches; concurrent callers get cached verdict or `Duplicate`
   after one 50 ms poll (executor.rs:845–868).
2. **Precondition gate (fail-closed)** — contracts declaring preconditions
   require a pre-dispatch observation; no observer ⇒ dispatch blocked, result
   `Failed` (executor.rs:668–731, `validate_preconditions` /
   `observe_pre_state`). Preconditions are evaluated against pre-dispatch
   state, not the post-dispatch observation **[confirmed]**.
3. **Dispatch** — `ActionExecutor::execute` returns `DispatchOutcome`
   (`Completed`/`Accepted`/`TimeoutBeforeDispatch`/`TimeoutAfterDispatch`/
   `TransportError`/`Ambiguous`, `action_executor.rs:41–92`). Mapping:
   timeouts → state `Unknown` then observation proceeds;
   `TransportError` → `Failed`, claim released, no retry;
   `Ambiguous` → verdict `Unknown` (executor.rs:942–990). Only the shipped
   `SimulatedActionExecutor` exists in-repo and always returns `Completed`
   (action_executor.rs:104–128) **[confirmed]**.
4. **Observation** — bounded by `ExecutorConfig.verification_timeout_ms`
   (default 5000); timeout ⇒ `ExecutorError::Timeout` (hard error, not a
   verdict). `RestObserver` (agentverify-http/src/observer.rs) maps every
   fetch failure — including non-2xx — to `ExecutorError::Unknown`
   (observer.rs:222–233), which the executor converts into a `Unknown`
   *verdict*, preserving ambiguity (executor.rs:975–982) **[confirmed]**.
5. **Verification** — per-postcondition `PredicateEngine::evaluate` with
   per-predicate `PostconditionResult` evidence recorded into the receipt;
   evaluation stops at the first indeterminate result (executor.rs:739–803).
   Aggregate: all passed ⇒ `Verified`; any failed mandatory ⇒ `Failed`;
   otherwise `Partial` **[confirmed]**.
6. **Retry** — `Failed` retries only when `verify_before_retry` is on and
   budget remains, with linear/exponential backoff from
   `contract.recovery` (executor.rs:1041–1066); **`Unknown` retries
   unconditionally** until the attempt budget is spent (executor.rs:1068–1090).
7. **Receipt + persistence** — receipt binds action, contract + schema
   version, attempts, idempotency key, per-postcondition evidence, optional
   observation, SHA-256 digest (`agentverify-core/src/receipt.rs`);
   Ed25519 signing via `agentverify-receipt/src/signing.rs`. Receipt-store
   write failure is logged, never aborts the verdict (executor.rs:303–315)
   **[confirmed]**.

## 3. Reliability / false-verdict risk register

| # | Risk | Class | Source | Consequence |
|---|------|-------|--------|-------------|
| R1 | **Idempotency claim leak on error paths** (fixed in this branch): adapter `Err` (`?` at old executor.rs:904), non-`Unknown` observer error, and observation timeout returned without `complete`/`release`; key stayed `InFlight` forever, so every later call with that key returned a false `Duplicate` and the action could never run again in that process. | confirmed (pre-fix) | old code path; now `resolve_claim_on_error` (executor.rs:323–339) + 4 regression tests (executor.rs:1662+) | false `DUPLICATE`, liveness loss |
| R2 | **Evidence-shape errors collapse to `Failed`**: leaf predicates never return `Unknown` — missing path, type mismatch, non-string on `matches` all yield `Failed` (engine.rs:91–97, 107–120, 160–201). `All`/`Any` collapse an `Unknown` child to `Failed`/skip; `Implies` is vacuously `Verified` on a non-`Verified` antecedent, including `Unknown` (engine.rs eval_all/eval_any/eval_implies). | confirmed (semantics); consequences inferred | agentverify-engine/src/engine.rs | "evidence not evaluable" is indistinguishable from "postcondition false" ⇒ wrong terminal verdicts; `Implies` can report `Verified` from an unevaluable antecedent (false positive) |
| R3 | **Oversize evidence becomes `Failed`**: `RestObserver::truncate` replaces an over-limit payload with the string `"[TRUNCATED: …]"` (observer.rs:178–190); all postconditions then evaluate against that string and fail — an infrastructure condition producing a `Failed` verdict and, with `verify_before_retry`, a re-dispatch of an action that may have succeeded. | confirmed mechanism; double-dispatch consequence inferred | observer.rs; executor retry loop | false `FAILED` → possible re-dispatch |
| R4 | **`Unknown` retries re-dispatch without a verified state**: in `execute_with_executor`, a `Unknown` postcondition verdict loops back to dispatch (executor.rs:1068–1090) — the action is re-sent while its prior effect is unconfirmed. `verify_before_retry` gates only the `Failed` arm. Tension with AGENTS.md invariant "never dispatch again without verify-before-retry". | inferred (behavior read from source; not covered by an explicit test) | executor.rs | duplicate external effect for non-idempotent targets |
| R5 | **CLI `verify` is observation-only** (simulated dispatch) and its `--max-retries` flag is ignored (`_max_retries`, main.rs:345; `Executor::new()` defaults are used); the inline comment "(real executor path)" (main.rs:391) refers to executor plumbing, not real dispatch — a doc-surface hazard AGENTS.md explicitly warns against. | confirmed | agentverify-cli/src/main.rs:341–398 | users can believe a real dispatch happened |
| R6 | **Process-local idempotency and receipts**: no TTL, no cross-process store shipped in-repo (`IdempotencyRegistry` docs, executor.rs:150–156); receipt persistence failure is silent-by-design (warn log). | confirmed | executor.rs | duplicates across processes/restarts; evidence gaps |
| R7 | **Predicate-engine `Unknown` is near-unreachable from leaves**, so R2 cannot be fixed by callers reasoning about `Unknown`; the fix must be semantic (see P-ENG-01). | confirmed | engine.rs | — |

Mitigating context: the failure direction of R2/R3 is conservative for
promotion (a wrong verdict reads "not verified"), and `Unknown` paths
(timeouts, observer 404/5xx, ambiguity) genuinely stay `Unknown`
**[confirmed]**. The dangerous direction is retry: `Failed` (including
evidence-shaped `Failed`) triggers re-dispatch when `verify_before_retry` is
on, so R2/R3/R4 compound into a duplicate-execution hazard for
non-idempotent targets **[inferred]**.

## 4. Tests and CI

**Gates** (`.github/workflows/ci.yml`, all `--locked`): fmt; clippy default +
all-features (`-D warnings`); test with live Postgres 17 (:5433) and Redis 7
services via `AGENTVERIFY_TEST_POSTGRES_URL`/`AGENTVERIFY_TEST_REDIS_URL`
(skip-cleanly when absent) + contract-example validation + bench compile;
rustdoc `-D warnings`; MSRV via cargo-hack; cargo-deny (advisories; bans/
licenses/sources); wasm32-wasip1 check of core/contract/engine/receipt/
policy; llvm-cov coverage → Codecov (non-blocking).

**Last green evidence at the branch base** (`main` @ `2b6d021`): GitHub
Actions run **33482496746** (2026-09-01), all 10 jobs green — Formatting,
MSRV, Documentation, Deny ×2, Clippy ×2, Coverage, **Test (1m34s, with the
PG/Redis services)**, WASM **[confirmed]**.

**Toolchain drift has left main red since that run** [confirmed]: the CI
toolchain is unpinned stable and moved to clippy/rustc 1.99, whose new
`assert_is_empty` lint (and rustc `unused-qualifications`) now fail the
clippy/test gates on pre-existing code. Plus RUSTSEC-2026-0285 (rustls
0.23.43). Proven-firing sites fixed in this branch: `redis_observer.rs` ×5,
`agentverify-mcp/client.rs` ×2, `core/receipt.rs` ×1, `policy` ×8, one
rustls lock bump. Note the lint's actual reach, measured from run
**38096769946** logs: it fires on `String`/`&str` and `Vec`-of-primitive
receivers (including the negated `assert!(!x.is_empty())` form) but **not**
on `Vec<Struct>`/`HashMap` receivers — `core/contract.rs` and
`core/observation.rs` assert sites were fully linted in that run and did
not fire; they are left untouched. This PR's CI runs so far:
**38096454733** (failed: my test's qualified `sleep`, the observe
`assert_is_empty` site, rustls advisory) and **38096769946** (failed: my
test's still-qualified call, 8 further drift sites in observe/mcp/core —
all fixed in commits `acfd8da`/`b5e9af1`/`b424775`). The latest run on the
PR is the evidence of record for this handoff.

**Volume**: ~741 test functions counted statically (`#[test]` +
`#[tokio::test]` across src and tests/, this branch) **[confirmed as a
static count; not a CI pass-claim]**. Largest suites: engine 108,
recovery 55, runtime executor 49, postgres observer 41+21, contract 40+21.
Integration coverage includes wiremock Control Center client tests (13,
http/tests/control_center_client.rs), HTTP observer integration
(http/tests/integration.rs), Postgres/Redis observer integration
(agentverify-observe/tests/), MCP stdio/channel, CLI exit codes, runtime
failure-injection (transport error, retry exhaustion, observer-error→Unknown,
double-dispatch prevention) **[confirmed from test sources]**.

**Release evidence**: tag `v0.1.0` GitHub Release exists (2026-08-30) with
all 6 target builds + WASI libs + checksums green (run 33324170554); its
"Publish to crates.io" job **failed** with "CARGO_REGISTRY_TOKEN is not
configured; skipping crates.io publish" (exit 1), and no later release run
exists ⇒ **crates.io publication never happened** **[confirmed]**. The
crates.io API returns 403 (bot protection) to anonymous checks, so direct
registry confirmation is **[unverified]**; the job log is authoritative
enough to treat publication as open. This is a user-only blocker (token
minting), unchanged since 2026-08-31.

**Coverage %**: not re-verified at this SHA (Codecov not fetched). Last
recorded local claim: 98.4 % lines, 2026-08-31 **[unverified here]**.

## 5. Integration seams

**Control Center — code-level, one-directional [confirmed].**
`agentverify-http/src/client.rs`: `ControlCenterClient` POSTs receipts to
`{base}/api/v1/verification-receipts` with optional bearer auth, the receipt
digest as `X-Idempotency-Key`, a 1 MB size cap refused in-band, field
redaction, and a `submit_receipt_with_unknown_on_timeout` wrapper that
converts timeout to a non-accepted response. 13 wiremock tests cover
success/401/403/parse/timeout/redaction paths. It **submits only** — it
cannot mutate promotion state (by design). No fixture exists that proves CC
rejects orphan/stale/tampered/unauthorized/cross-project/replayed receipts;
whether any CC deployment implements this endpoint is **[unverified]**.
Doc defect fixed in this branch: `submit_receipt`'s doc claimed
timeout-to-UNKNOWN conversion it does not perform.

**GitForge — no code-level seam [confirmed].** Zero references to GitForge
in `crates/` or `examples/`. GitForge's repo inventory (harness MCP,
2026-10-10) does not list AgentVerify. The relationship is operational only:
AgentVerify's actual forge/CI lane is **GitHub**
(`github.com/aliasfoxkde/AgentVerify`; PRs #20–#24 merged there, dependabot
active). Any claim that GitForge runs AgentVerify's gates is false today.
If the platform wants GitForge to own AgentVerify CI, that is a new
integration packet (mirror + pipeline), not a configuration fact.

**Amortyx / Atheon / VIVERE — none [confirmed]** (no references in code).
Per AGENTS.md these are separate boundaries; AgentVerify must stay
independently deployable.

**Tier D gap** (internal HANDOFF tiers): everything evidenced is Tier A–C
(local deterministic semantics, in-process executor seams, mock-server
HTTP). Nothing evidences authenticated cross-process service behavior,
durable receipts across restarts, or CC correlation **[confirmed]**.

## 6. Change shipped in this branch

One scoped fix — **idempotency claim resolution on error paths**
(R1), plus a doc-comment correction and ledger/handoff docs:

- `crates/agentverify-runtime/src/executor.rs` — new private
  `resolve_claim_on_error(action, unknown_effect)` (executor.rs:323–339):
  releases the claim when dispatch could not have started
  (`ActionNotSupported`, `TimeoutBeforeDispatch`, `Rejected`), consumes the
  key with `VerificationResult::Unknown` when the external effect is unknown
  (`TransportError`/`TimeoutAfterDispatch`/`Ambiguous` adapter errors, and
  any post-dispatch observation failure/timeout), so the action is never
  silently re-dispatched and the key is never left `InFlight`. Wired into
  both `execute` and `execute_with_executor`. Public signatures and
  success-path behavior unchanged. 4 regression tests added
  (executor.rs:1662–1895).
- `crates/agentverify-http/src/client.rs` — corrected
  `submit_receipt` doc (timeout semantics belong to the
  `_with_unknown_on_timeout` wrapper).
- CI unblocks for pre-existing main drift (needed for any green run):
  `Cargo.lock` rustls 0.23.43 → 0.23.45 (RUSTSEC-2026-0285, the advisory's
  own recommendation) and clippy-1.99 `assert_is_empty` rewrites at 16
  proven-firing sites (`redis_observer.rs` ×5, `agentverify-mcp/client.rs`
  ×2, `core/receipt.rs` ×1, `policy/error.rs` ×1, `policy/policy.rs` ×7) —
  each rewritten to the lint's own suggested form; assertion semantics
  unchanged (see §4 for the run evidence and the lint's measured reach).
- `.github/CHANGELOG.md` — Unreleased/Fixed entries.
- `docs/internal/TASKS.md` — ledger refresh (§7 of the ledger).

**Validation actually run:** `cargo fmt --all -- --check` clean, and —
after two CI runs spent discovering mechanical failures (runs
38096454733/38096769946) — one scoped local lint: `cargo clippy -p
agentverify-core -p agentverify-policy -p agentverify-mcp -p
agentverify-observe -p agentverify-runtime --all-targets -- -D warnings`,
clean in 1m16s (covers the runtime lib test with the new regression tests
and the observe/mcp/core/policy lint edits). No local test run otherwise;
the full workspace build/test gates are delegated to the repository's CI
lane (GitHub Actions) and the PR's latest run is the evidence of record.
**If CI is red on the runtime crate, treat the fix as unproven and do not
merge.**

**Rollback:** revert the single fix commit (or close the PR); no schema,
API, or data migration involved. The doc-only changes are independently
revertable. Behavioral delta to review: adapter `Err` on an
unknown-effect variant now consumes the key as `Unknown` — a caller that
previously *relied* on the leak-as-dedup (unlikely; it was a liveness bug)
would see the key finalized instead of stuck.

## 7. Next implementation packet (owner: @aliasfoxkde unless noted)

Ordered by platform value; each item names files and executable acceptance
criteria. P-CC-01 and P-DIS-01 are the Tier-D blockers.

**P-DIS-01 — Real dispatch adapter (Tier-D blocker).**
Files: new `crates/agentverify-http/src/action_executor.rs`
(`HttpActionExecutor: ActionExecutor` — POST contract action to a configured
base URL, map transport/timeout/2xx/4xx/5xx onto
`DispatchOutcome`/`DispatchError` exactly per `action_executor.rs:10–34`);
`crates/agentverify-cli/src/main.rs` (`--executor simulated|http`, default
`simulated`; also wire the ignored `--max-retries` into `ExecutorConfig` and
delete the "(real executor path)" comment).
Accept: (a) wiremock integration test — adapter POSTs, server 201s, observer
verifies postcondition, receipt `Verified`; (b) adapter timeout maps to
`TimeoutAfterDispatch` and final verdict `Unknown`, never `Failed`;
(c) CLI help states simulated default in the command description; (d) CI
green including clippy `-D warnings`.

**P-ENG-01 — Evidence-shape semantics decision (owner decision required).**
Files: `crates/agentverify-engine/src/engine.rs`,
`crates/agentverify-core/src/predicate.rs` (or a new evaluation-outcome
type), `crates/agentverify-runtime/src/executor.rs`,
`crates/agentverify-http/src/observer.rs` (truncation).
Decision to make: should "path missing / type mismatch / truncated evidence"
be `Failed` (current) or a distinct unevaluable/`Unknown` class? Current
behavior converts infrastructure conditions into verdict-failures and, with
retry, into re-dispatches (R2/R3).
Accept: written decision recorded in `docs/CONCEPTS.md`; fixture matrix
(missing path, wrong type, truncated, null, array-vs-object) asserting the
chosen semantics for every predicate incl. compounds; `Implies` with
`Unknown` antecedent no longer returns `Verified` unless explicitly chosen;
property tests extended.

**P-UNK-01 — Make `Unknown` retries observe-first.**
Files: `crates/agentverify-runtime/src/executor.rs` (retry arms),
`crates/agentverify-runtime/tests/` or in-file tests.
Change: on a `Unknown` verdict, the next iteration must attempt observation
*before* re-dispatch; re-dispatch only if observation is inconclusive after
budget. Accept: test — with an observer that returns state late, a `Unknown`
verdict is resolved by observation without a second adapter call; existing
retry-exhaustion tests still pass.

**P-CC-01 — Receipt ingestion contract + rejection fixtures (Tier-D
blocker; co-owner: Control Center).**
Files: `crates/agentverify-core/src/receipt.rs` (identity fields:
project/task/work-request/job/agent/commit/verifier/replay + contract
version), `crates/agentverify-http/src/client.rs`,
new `crates/agentverify-http/tests/receipt_ingestion_contract.rs`.
Accept: versioned schema doc in `docs/INTEGRATIONS.md`; wiremock fixtures
where a simulated CC rejects orphan, stale, tampered (digest mismatch),
unauthorized, cross-project, and replayed submissions and the client
surfaces rejection reasons; no production credentials in fixtures
(AGENTS.md invariant).

**P-DUR-01 — Cross-process idempotency + durable receipts.**
Files: `crates/agentverify-storage/src/lib.rs` (durable `IdempotencyStore`
+ `ReceiptStore` adapters; Postgres backend already has observer plumbing in
`crates/agentverify-observe/src/postgres.rs` to model on).
Accept: cross-process test (two executor instances, shared store) proves
single dispatch under concurrency; restart/replay test proves receipts
survive; TTL exercised.

**P-OPS-01 — Release hygiene (user-gated).** Mint
`CARGO_REGISTRY_TOKEN` → `gh secret set` → `gh workflow run release.yml
-f publish_crates=true`; verify 14 crates on crates.io; then migrate to
trusted publishing and delete the token. No code changes; owner-only.

Ledger: these are recorded as open rows in `docs/internal/TASKS.md`
(§ "Handoff packets 2026-10-10") — update that ledger, do not fork a new one.

## 8. Claim ledger

- **Confirmed** (source or CI, at named SHAs): workflow semantics (§2), R1
  pre-fix behavior, engine failure-collapse semantics, observer truncation +
  error→Unknown mapping, simulated-only dispatch, CLI flag ignored, CI gate
  list, run 33482496746 all-green at `2b6d021`, v0.1.0 assets published /
  crates.io publish failed for lack of token, no GitForge seam, CC client
  one-directional with 13 wiremock tests.
- **Inferred**: double-dispatch chains from R2/R3/R4; InFlight
  poll-then-Duplicate behavior for long overlaps; "Tier A–C only" framing.
- **Unverified**: coverage % at this SHA; crates.io registry state directly
  (403 to anonymous API); existence of a CC deployment implementing
  `/api/v1/verification-receipts`; behavior of PG/Redis observers against
  services other than the CI ones; this branch's final CI result (runs
  38096454733 and 38096769946 failed and were fixed; the latest run on the
  PR is the evidence of record — check it before merging).

## 9. Source index

Code: `crates/agentverify-runtime/src/executor.rs`,
`.../action_executor.rs`, `.../idempotency_store.rs`,
`crates/agentverify-engine/src/engine.rs`,
`crates/agentverify-http/src/{observer,client}.rs`,
`crates/agentverify-cli/src/main.rs`,
`crates/agentverify-core/src/{receipt,verification_result}.rs`,
`crates/agentverify-receipt/src/signing.rs`,
`.github/workflows/{ci,release}.yml`.
Docs: `AGENTS.md`, `docs/planning/HANDOFF.md`,
`docs/internal/{HANDOFF,TASKS,NEXT_STEPS}.md`, `.github/CHANGELOG.md`.
CI/PR evidence (GitHub, `aliasfoxkde/AgentVerify`): run 33482496746 (CI,
green, main@2b6d021, 2026-09-01); run 33324170554 (Release v0.1.0, builds
green, crates.io publish failed, 2026-08-30); merged PRs #20–#24.
GitForge: repo inventory via harness MCP, 2026-10-10 (no AgentVerify entry).
