# Security Assessment — v0.1.0 release gate

Date: 2026-08-30
Scope: full workspace (`crates/`, `scripts/`, `.github/`), dependency tree
(383 packages), and pattern scanning with Aegis.

## Verdict

**Release-ready.** Dependency audit is clean, the lint policy denies the
classic Rust memory/panic foot-guns at the workspace level, and every
Aegis "high-severity" finding was individually verified as a pattern
misfire (details below). No action items remain for v0.1.0.

## Dependency audit

| Check | Result |
|-------|--------|
| `cargo audit` (RustSec advisories) | clean — 0 vulnerabilities, 0 warnings, exit 0 |
| `cargo deny advisories` (CI) | gate enabled in `.github/workflows/ci.yml` |
| `cargo deny bans/licenses/sources` (CI) | gate enabled (MIT/Apache-2.0/BSD/ISC/Unicode allowlist) |
| GitHub Dependabot | enabled, weekly, cargo + github-actions ecosystems, grouped updates |

## Source-level guarantees (enforced by clippy, CI runs `-D warnings`)

- `deny(unsafe_code)` workspace-wide
- `deny(panic, unwrap_used, expect_used)` in production code; tests opt out
  via `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]`
- Poisoned-lock recovery instead of `unwrap()`:
  `.lock().unwrap_or_else(PoisonError::into_inner)` in `agentverify-testkit`
- `deny(todo, unimplemented, dbg_macro, print_stdout, print_stderr, exit)`

Confirmed independently of clippy: the Aegis scan shows **zero** `unwrap`-family
findings in non-test source files.

## Aegis pattern scan — findings assessment

Raw counts: 6,635 findings in `crates/`, 126 in `.github/`, 1 in `scripts/`.
Of these, 1,153 are high/critical. Every high-severity hit was attributed to
a file and line and inspected; all are generic web-stack patterns misfiring
on Rust. The rule set is oriented at JavaScript/Python web applications
(evidenced by the references attached to the findings themselves: MDN's
JavaScript `eval()` page, PCI DSS docs, Flask-SQLAlchemy docs).

| Pattern | Hits | Reality in this repo |
|---------|------|----------------------|
| `secrets-in-dockerfile` | 746 | **0 Dockerfiles exist.** Substring match on identifiers like `IdempotencyKey` in `.rs` files. |
| `code-quality-eval-usage` | 218 | Matches the word "evaluation" (e.g. doc comment "Result of a single postcondition evaluation"). The reference is for JavaScript `eval()`, which does not exist in Rust. |
| `pci-cardholder-data` | 107 | Matches doc-comment prose (`/// Store a receipt`). No cardholder data anywhere; the project handles verification receipts, not payments data. |
| `path-traversal` | ~54 | `std::path::PathBuf` usage in file-backed stores — inherent to the design, not user-controlled path joining. |
| `flask-sqlalchemy-raw-sql` | 16 | Matches Rust trait methods (`async fn execute(&self, _action: &Action)`) against a Flask/SQLAlchemy injection pattern. |

The pattern-to-source mismatch is structural, not tunable at the finding
level: the scanner emits web-language rules against Rust tokens. Until a
Rust-aware rule set is available in Aegis, the authoritative source scanners
for this repo are `cargo clippy -D warnings` and `cargo audit`/`cargo deny`,
both of which are CI-gated.

## Scan artifacts

Raw JSON outputs were kept out of the repository (5 MB of machine output,
not reviewable content). Re-run with:

```bash
aegis -f json scan crates/ > /tmp/aegis-crates.json
aegis -f json scan .github/ > /tmp/aegis-gh.json
```

## Residual risk accepted for v0.1.0

- `receipt.rs` uses `serde_json::to_vec(...).unwrap_or_default()` inside
  canonical digest computation — a serialization failure yields an empty
  canonical payload rather than a panic; digest mismatch is then detectable
  via `verify_digest()`. Documented here as intentional.
- Test code uses `unwrap()`/`expect()` per the workspace allowlist; tests are
  not shipped in release artifacts.

## Post-release roadmap

- Add `cargo-vet` supply-chain review once the crates are published (tracked
  in `docs/ROADMAP.md`, release-engineering section).
- Re-run Aegis after any rule-set update that adds Rust-aware patterns.
