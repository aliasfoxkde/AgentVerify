# Contributing to AgentVerify

Thank you for your interest in contributing to AgentVerify! This document
covers everything you need to get started.

## Code of Conduct

By participating in this project you agree to abide by the
[Code of Conduct](CODE_OF_CONDUCT.md). Report unacceptable behavior to the
contact listed there.

## Project Overview

AgentVerify is an outcome verification library for action-taking AI agents.
It checks that high-risk actions reached the required final state in systems
of record, and preserves evidence in signed receipts.

Before opening a pull request, please read:

- [`docs/PLANNING.md`](docs/PLANNING.md) — full architectural specification
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — system design
- [`docs/CONCEPTS.md`](docs/CONCEPTS.md) — domain concepts (contracts,
  predicates, receipts, verification results)

## Development Setup

### Prerequisites

- Rust 1.88 or newer (see `rust-version` in `Cargo.toml`)
- A Rust proxy/registry connection for crates.io

### Getting Started

```bash
git clone https://github.com/aliasfoxkde/AgentVerify.git
cd AgentVerify
cargo build
cargo test
```

### Quality Gates

All of these must pass before a pull request can be merged. CI enforces the
same checks:

```bash
cargo fmt --all -- --check                              # formatting
cargo clippy --workspace --all-targets -- -D warnings   # lints (default features)
cargo clippy --workspace --all-targets --all-features -- -D warnings  # lints (all features)
cargo test --workspace                                  # tests
cargo doc --workspace --all-features --no-deps          # docs build (warnings are errors)
cargo deny check advisories bans licenses sources       # supply-chain policy
```

### Lint Policy

The workspace enforces strict lints via `[workspace.lints]` in the root
`Cargo.toml`. In production code:

- No `unwrap()` / `expect()` — return `Result` (tests opt out via
  `#[cfg_attr(test, allow(...))]` at the crate root)
- No `panic!()`, `todo!()`, `unimplemented!()`
- No `unsafe` code
- No direct `println!` / `stderr` writes — use `tracing`
- All public items need doc comments (`missing_docs` is enforced)

## Testing Standards

- Unit tests live in `#[cfg(test)]` modules next to the code they test
- Integration tests live in `crates/<crate>/tests/`
- Property tests use `proptest`
- Name tests descriptively: `verify_postcondition_equals_success`,
  `verify_unknown_timeout_handled_correctly`
- **UNKNOWN is a first-class state** — a timeout must never be reported as
  `FAILED`. If you touch verification logic, add a test proving this
- New core code should include benchmarks when it touches hot paths
  (`crates/agentverify-engine/benches/`)

### Coverage

Core crates target 90%+ line coverage. The CI coverage job reports to
Codecov; please add tests for new code paths rather than relying on
existing tests to cover them incidentally.

## Commit and Pull Request Guidelines

### Commits

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(engine): add CelExpression predicate support
fix(core): handle empty state in Exists predicate
docs(contract): add PostgreSQL observer example
test(engine): add property tests for compound predicates
```

- Subject: imperative mood, max 50 characters, no trailing period
- Body: wrapped at 72 characters; explain **what** and **why**
- Keep commits atomic — one logical change per commit

### Pull Requests

- Keep PRs focused; split unrelated changes into separate PRs
- Fill out the pull request template
- Update documentation (`README.md`, `docs/`) when behavior changes
- Add a `.github/CHANGELOG.md` entry under `## [Unreleased]` for user-visible changes
- Squash merges are used; branch names follow `<type>/<short-description>`

## Reporting Issues

- **Bugs** — use the bug report template. For wrong verification outcomes
  (a false `VERIFIED`, a false `FAILED`, or mishandled `UNKNOWN`), use the
  dedicated "Verification bug" template and include the contract, observed
  state, and receipt if available.
- **Security vulnerabilities** — do **not** open a public issue. Follow
  [`SECURITY.md`](SECURITY.md).
- **Questions / design discussions** — GitHub Discussions.

## Design Principles

These are non-negotiable in code review:

1. **Core must be deterministic** — no LLM, database, HTTP, or framework
   dependencies in `agentverify-core`
2. **Verify-before-retry** — never retry without verifying first
3. **Zero-trust annotations** — MCP tool annotations are hints, not proof
4. **UNKNOWN is first-class** — timeout ≠ failure
5. **Receipts for evidence** — every operation produces a structured,
   signable receipt
6. **Independently useful** — AgentVerify must be deployable without any
   sibling framework

## Licensing

By contributing, you agree that your contributions will be licensed under
the MIT License that covers this project.
