# AgentVerify Commit Guidelines

## Commit Format

```
<type>(<scope>): <subject>

<body>
```

## Types

| Type | Description |
|------|-------------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation changes |
| `style` | Formatting (no code change) |
| `refactor` | Code restructuring |
| `test` | Adding tests |
| `chore` | Maintenance tasks |
| `perf` | Performance improvement |
| `ci` | CI/CD changes |

## Format Rules

### Subject Line
- Maximum 50 characters
- Use imperative mood ("Add feature" not "Added feature")
- No trailing period
- Reference issues: `Closes #123`

### Body
- Wrap at 72 characters
- Explain **what** and **why**, not how
- Reference issue: `Fixes #123`

## Examples

```
feat(core): add VerificationResult enum with UNKNOWN state

Implements the core verification result types including VERIFIED,
FAILED, UNKNOWN, PARTIAL, and DUPLICATE states. UNKNOWN is first-class
to handle timeout scenarios correctly.

Closes #10
```

```
fix(engine): handle empty state in Exists predicate

When state is None, Exists predicate should return false, not error.
This matches the expected semantics for missing resources.

Fixes #25
```

```
docs(contract): add PostgreSQL observer example

Adds a complete example showing how to use the PostgreSQL observer
with query-based postconditions.

Closes #42
```

```
test(core): add property tests for predicate evaluation

Uses proptest to verify predicate evaluation is deterministic
and correct across a wide range of inputs.

Refs #15
```

## Pre-commit Checklist

- [ ] Tests pass: `cargo test --workspace`
- [ ] Clippy clean: `cargo clippy --workspace -- -D warnings`
- [ ] Formatted: `cargo fmt --all`
- [ ] No secrets committed
- [ ] Commits are atomic (one logical change per commit)

## Branch Naming

```
<type>/<issue-number>-<short-description>

feat/10-add-unknown-state
fix/25-exists-predicate-empty-state
docs/42-postgres-observer-example
```

## Merge Strategy

- Use squash merge for feature branches
- Use merge commit for release branches
- Never force push to main
