# P5 Proposal: Control Center Verification Receipt Correlation

**Date:** 2026-08-15
**Author:** Claude (AgentVerify implementation)
**Status:** PROPOSAL — awaiting owner approval

## Current State

Control Center at `/nas/Temp/repos/Control-Center` has been inspected. It contains:
- Deployment and staging services (`staging_service.rs`)
- Work request routes (`work_request_routes.rs`)
- Integration routes for external services (`integration_routes.rs`)
- Bearer/JWT middleware for authentication (`middleware/auth.rs`)

**Missing:** No verification receipt model, endpoint, or validation logic exists.

## Required Addition (per P5 gate)

The P5 gate requires: "authenticated cross-process test fixture produces a receipt accepted for the matching project/task/job and rejects every negative case."

To satisfy this, Control Center needs:

### 1. New Model: VerificationReceipt

```rust
struct VerificationReceipt {
    id: String,
    project_id: String,
    task_id: Option<String>,
    work_request_id: Option<String>,
    job_id: Option<String>,
    agent_id: Option<String>,
    contract_id: String,
    contract_version: String,
    action_id: String,
    idempotency_key: String,
    source_workspace: String,
    source_commit: String,
    outcome: String, // VERIFIED, FAILED, UNKNOWN, PARTIAL, DUPLICATE
    evidence_digest: String, // SHA-256
    bounded_evidence: String, // JSON
    observed_at: DateTime<Utc>,
    verifier_id: String,
    verifier_version: String,
    key_id: Option<String>,
    signature: Option<String>, // Ed25519 signature
    replay_key: Option<String>,
    submitted_at: DateTime<Utc>,
}
```

### 2. New Endpoint: POST /verification-receipts

- Validates authenticated caller identity
- Validates project/work-request/workspace ownership
- Validates task/job correlation
- Validates contract and commit freshness
- Validates receipt signature/key
- Validates replay uniqueness
- Accepts only VERIFIED outcome for promotion

### 3. Validation Rules

| Check | Failure Mode |
|-------|-------------|
| Signature valid | Reject: "Invalid signature" |
| Key known | Reject: "Unknown signing key" |
| Project ownership | Reject: "Receipt not for this project" |
| Replay uniqueness | Reject: "Receipt already submitted" |
| Contract version current | Reject: "Stale contract" |
| Commit current | Reject: "Stale commit" |

## Blocker

**Owner must approve and implement the above before P5 can proceed.**

Per HANDOFF rules: "If the proposal cannot be accepted because the owner has not supplied the required endpoint/schema/key/lease authority, stop P5 at the boundary and report the exact missing artifact."

## Recommendation

1. Owner reviews and approves (or modifies) this proposal
2. Owner implements the endpoint in Control Center
3. AgentVerify implements a client adapter to submit receipts
4. Integration tests added in both repos

**This proposal cannot proceed without owner decision.**
