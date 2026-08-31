//! Downstream-facing tests for the testkit's mock idempotency store.
//!
//! These drive [`MockIdempotencyStore`] the way a test author would: the store
//! is configured through its public setters, called through the
//! [`IdempotencyStore`] trait it exists to stand in for, and inspected through
//! its call history afterwards.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use agentverify_core::VerificationResult;
use agentverify_runtime::{ClaimResult, IdempotencyStore};
use agentverify_testkit::MockIdempotencyStore;

/// A `(result, verification)` pair standing for "the claim was granted".
fn claimed() -> (ClaimResult, Option<VerificationResult>) {
    (ClaimResult::Claimed, None)
}

#[tokio::test]
async fn an_unmarked_key_returns_the_result_configured_for_it() {
    let mut store = MockIdempotencyStore::new();
    store.set_result(
        "refund",
        (ClaimResult::Claimed, Some(VerificationResult::Verified)),
    );

    let (result, verification) = store.claim_or_check("refund").await;
    assert_eq!(result, ClaimResult::Claimed);
    assert_eq!(verification, Some(VerificationResult::Verified));

    // A key nobody configured and no default covers still grants a claim,
    // which is what a first execution needs.
    let (result, verification) = store.claim_or_check("first-attempt").await;
    assert_eq!(result, ClaimResult::Claimed);
    assert_eq!(verification, None);
    assert_eq!(
        store.claim_or_check_calls(),
        vec!["refund", "first-attempt"]
    );
}

/// A key whose name contains `_claimed` models work that is already in
/// flight: the configured result is replaced by the `_already` override, or by
/// a bare `AlreadyClaimed` when no override was given.
#[tokio::test]
async fn a_key_marked_in_flight_reports_the_work_already_claimed() {
    let mut store = MockIdempotencyStore::new();
    store.set_result("refund_claimed", claimed());
    store.set_result(
        "refund_claimed_already",
        (
            ClaimResult::AlreadyClaimed,
            Some(VerificationResult::Verified),
        ),
    );

    for _ in 0..2 {
        let (result, verification) = store.claim_or_check("refund_claimed").await;
        assert_eq!(result, ClaimResult::AlreadyClaimed);
        assert_eq!(verification, Some(VerificationResult::Verified));
    }
}

#[tokio::test]
async fn an_in_flight_key_without_an_override_reports_an_unresolved_claim() {
    let mut store = MockIdempotencyStore::new();
    store.set_result("export_claimed", claimed());

    let (result, verification) = store.claim_or_check("export_claimed").await;
    assert_eq!(result, ClaimResult::AlreadyClaimed);
    assert_eq!(verification, None, "no outcome is known for in-flight work");
}

/// `set_return_already_claimed(false)` stops the simulation, so the configured
/// result is handed back untouched; switching the flag back on resumes it.
#[tokio::test]
async fn in_flight_simulation_can_be_switched_off_and_back_on() {
    let mut store = MockIdempotencyStore::new();
    store.set_result(
        "close_ticket_claimed",
        (ClaimResult::Claimed, Some(VerificationResult::Unknown)),
    );

    store.set_return_already_claimed(false);
    let (result, verification) = store.claim_or_check("close_ticket_claimed").await;
    assert_eq!(result, ClaimResult::Claimed);
    assert_eq!(verification, Some(VerificationResult::Unknown));

    store.set_return_already_claimed(true);
    let (result, verification) = store.claim_or_check("close_ticket_claimed").await;
    assert_eq!(result, ClaimResult::AlreadyClaimed);
    assert_eq!(verification, None);
}

/// `reset_all` restores every configured knob, including the in-flight switch,
/// leaving a store that behaves like a freshly built one.
#[tokio::test]
async fn reset_all_restores_the_in_flight_switch() {
    let mut store = MockIdempotencyStore::new();
    store.set_result("retry_claimed", claimed());
    store.set_return_already_claimed(false);

    let (result, _) = store.claim_or_check("retry_claimed").await;
    assert_eq!(result, ClaimResult::Claimed);

    store.reset_all();
    assert!(
        store.claim_or_check_calls().is_empty(),
        "reset_all clears the recorded history too"
    );

    // Re-arming the same key shows the switch came back on: the configured
    // result is once more replaced by `AlreadyClaimed`.
    store.set_result("retry_claimed", claimed());
    let (result, verification) = store.claim_or_check("retry_claimed").await;
    assert_eq!(result, ClaimResult::AlreadyClaimed);
    assert_eq!(verification, None);
}

/// `Default` is equivalent to `new`, and the handle is a shared view: history
/// recorded through a clone is visible from the original, which is how a test
/// passes the store into a task and inspects it afterwards.
#[tokio::test]
async fn a_default_store_is_empty_and_shares_history_with_its_clones() {
    let store = MockIdempotencyStore::default();
    assert_eq!(store.claim_or_check_call_count(), 0);
    assert_eq!(store.complete_call_count(), 0);
    assert_eq!(store.release_call_count(), 0);

    let handle = store.clone();
    let (result, verification) = handle.claim_or_check("shared-key").await;
    assert_eq!(result, ClaimResult::Claimed);
    assert_eq!(verification, None);

    handle
        .complete("shared-key".to_string(), VerificationResult::Verified)
        .await;
    handle.release("shared-key").await;

    assert_eq!(store.claim_or_check_call_count(), 1);
    assert_eq!(
        store.complete_calls(),
        vec![("shared-key".to_string(), VerificationResult::Verified)]
    );
    assert_eq!(store.release_calls(), vec!["shared-key".to_string()]);
}
