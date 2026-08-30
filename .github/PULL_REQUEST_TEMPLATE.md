<!--
Thank you for contributing to AgentVerify!

Before opening this PR, please review CONTRIBUTING.md. Security-sensitive
changes (verification logic, receipt signing, idempotency) deserve extra
scrutiny — call them out below.
-->

## Summary

<!-- One or two sentences: what does this PR do? -->

## Motivation and Context

<!-- Why is this change needed? Link issues with "Fixes #123" / "Closes #123". -->

## Changes

<!-- Bullet list of the significant changes. -->

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing usage to change)
- [ ] Documentation only
- [ ] Refactor / internal (no behavior change)

## Verification-Sensitivity

Does this PR touch verification logic, predicate evaluation, receipts,
idempotency, or state transitions?

- [ ] Yes — I have added/updated tests that specifically prove the
      verification outcome behavior, including that `UNKNOWN` is never
      collapsed into `FAILED`
- [ ] No

## Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo doc --workspace --all-features --no-deps` builds without warnings
- [ ] New/changed public API items have doc comments
- [ ] Tests added for new code paths
- [ ] `CHANGELOG.md` updated under `## [Unreleased]` (if user-visible)
- [ ] Documentation updated (README / docs/) if behavior changed
