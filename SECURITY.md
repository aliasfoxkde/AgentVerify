# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |
| < 0.1   | ❌        |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub
issues, discussions, or pull requests.**

Instead, use [GitHub's private vulnerability reporting](
https://github.com/aliasfoxkde/AgentVerify/security/advisories/new) for this
repository. This keeps the report confidential while it is being triaged and
fixed.

Please include as much of the following as you can:

- The affected crate(s) and version(s) (e.g. `agentverify-core 0.1.0`)
- A minimal reproduction — contract JSON/YAML, action input, observed state,
  and any receipt produced
- The incorrect behavior observed (e.g. an action reported `VERIFIED` when
  the postcondition was not satisfied, an `UNKNOWN` misreported as `FAILED`)
- Your assessment of severity and impact
- Any known workarounds

### What to Expect

- **Acknowledgement** within 72 hours
- **Triage and severity assessment** within 7 days
- A fix or mitigation timeline agreed with you before public disclosure
- Credit in the release notes and advisory (unless you prefer anonymity)

## Scope

In scope:

- Incorrect verification outcomes (false `VERIFIED` / false `FAILED`,
  mishandled `UNKNOWN`)
- Receipt integrity: digest collisions, signature bypass, tampering that
  passes `verify_digest()`
- Idempotency failures that lead to duplicate execution of high-risk actions
- Injection through contract files or observed-state JSON (path traversal in
  predicate evaluation, deserialization issues)
- Supply-chain issues in the build (CI privilege escalation, workflow
  injection)

Out of scope:

- Vulnerabilities in downstream agent frameworks or LLM providers
- Reports from automated scanners without a demonstrated impact path
- Social engineering, physical attacks, or denial of service of the demo
  HTTP gateway

## Verification Integrity Notes

AgentVerify treats **UNKNOWN as a first-class state**: a timeout or
unreachable system of record must never be collapsed into `FAILED`. If you
find a path where an indeterminate outcome is reported as a definite one,
that is a security-relevant bug — please report it.

Receipts are the audit evidence for verification. The digest binds the
receipt content and the Ed25519 signature binds the digest; any way to
modify a receipt while keeping a valid digest/signature is critical.
