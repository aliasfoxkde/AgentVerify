//! `FileReceiptStore` degrades instead of erroring when its own base path
//! becomes unusable.
//!
//! The store is built from a directory that is later replaced by a regular
//! file, which is the situation an operator hits when something else claims
//! the path between runs: the listing and the lookups report "no evidence"
//! rather than failing the caller.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use agentverify_core::{
    ActionId, ContractId, FileReceiptStore, Receipt, ReceiptStore, ReceiptStoreError,
    VerificationResult,
};

#[tokio::test]
async fn a_store_whose_base_path_stopped_being_a_directory_reports_no_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("receipts");
    let store = FileReceiptStore::new(base.clone()).unwrap();

    // The directory is empty, so it can be swapped for a regular file. The
    // store can then neither create its base path nor read an index from it.
    std::fs::remove_dir(&base).unwrap();
    std::fs::write(&base, "not a directory").unwrap();

    let action = ActionId::new();
    assert!(
        store.list_by_action(&action).await.is_empty(),
        "an unusable base path must not surface as an error"
    );

    // Writing is equally impossible, and the failure is reported rather than
    // silently swallowed.
    let receipt = Receipt::new(action, ContractId::new(), VerificationResult::Verified, 1);
    let stored = store.store(&receipt).await.unwrap_err();
    assert!(
        matches!(stored, ReceiptStoreError::Persist(_)),
        "unexpected error: {stored:?}"
    );

    // Reading back yields nothing: the receipt was never written.
    assert!(store.get(&receipt.id).await.is_none());
    assert!(!store.exists(&receipt.id).await);
}
