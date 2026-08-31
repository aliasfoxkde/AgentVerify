//! Tests for the failure branches of receipt signing: malformed key material,
//! missing and malformed signatures, key-fingerprint stability, and the
//! `Default` constructor.
//!
//! The sign/verify happy paths are covered by unit tests inside the module;
//! these tests pin the behavior callers rely on when signing or verification
//! *cannot* proceed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use agentverify_core::{ActionId, ContractId, Receipt, VerificationResult};
use agentverify_receipt::{Ed25519SigningService, SigningError, SigningService};
use base64::Engine as _;

/// A minimal unsigned receipt; `signature` is `None` until `sign` is applied.
fn unsigned_receipt() -> Receipt {
    Receipt::new(
        ActionId::new(),
        ContractId::new(),
        VerificationResult::Verified,
        1,
    )
}

#[test]
fn from_base64_rejects_key_material_that_is_not_base64() {
    // `Ed25519SigningService` deliberately has no `Debug` impl (it holds key
    // material), so the error is unpacked without `unwrap_err`.
    let Err(err) = Ed25519SigningService::from_base64("definitely not base64!!") else {
        panic!("key material that is not base64 must be rejected");
    };

    assert!(
        matches!(&err, SigningError::SigningFailed(msg) if msg.starts_with("Invalid base64")),
        "unexpected error: {err:?}"
    );
    // The rendered error names the failure and carries the decoder detail.
    assert!(err
        .to_string()
        .starts_with("Signing failed: Invalid base64"));
}

#[test]
fn from_base64_rejects_key_material_that_is_not_32_bytes() {
    // 24 zero bytes decode cleanly but are the wrong length for an Ed25519 seed.
    let encoded = base64::engine::general_purpose::STANDARD.encode([0u8; 24]);
    let Err(err) = Ed25519SigningService::from_base64(&encoded) else {
        panic!("valid base64 of the wrong length must still be rejected");
    };

    assert!(matches!(&err, SigningError::SigningFailed(msg) if msg == "Key must be 32 bytes"));
    assert_eq!(err.to_string(), "Signing failed: Key must be 32 bytes");
}

#[test]
fn from_base64_yields_the_same_service_as_the_equivalent_seed() {
    let seed = [0x5au8; 32];
    let encoded = base64::engine::general_purpose::STANDARD.encode(seed);

    let from_encoded = Ed25519SigningService::from_base64(&encoded).unwrap();
    let from_seed = Ed25519SigningService::from_seed(&seed);

    // Same key material means the same fingerprint...
    assert_eq!(from_encoded.key_id(), from_seed.key_id());
    // ...and, because Ed25519 is deterministic, the same signature bytes.
    let receipt = unsigned_receipt();
    assert_eq!(
        from_encoded.sign(&receipt).unwrap(),
        from_seed.sign(&receipt).unwrap()
    );
}

#[test]
fn verify_reports_missing_signature_as_an_error() {
    let service = Ed25519SigningService::new();
    let receipt = unsigned_receipt();

    assert!(receipt.signature.is_none());
    let err = service.verify(&receipt).unwrap_err();

    assert!(matches!(err, SigningError::MissingKey), "got: {err:?}");
    assert_eq!(err.to_string(), "Signing key is missing");
}

#[test]
fn verify_rejects_signatures_that_are_not_64_bytes() {
    let service = Ed25519SigningService::new();
    let mut receipt = unsigned_receipt();
    receipt.signature = Some(vec![0u8; 32]);

    let err = service.verify(&receipt).unwrap_err();

    assert!(
        matches!(&err, SigningError::VerificationFailed(msg) if msg == "Invalid signature length"),
        "unexpected error: {err:?}"
    );
    assert_eq!(
        err.to_string(),
        "Verification failed: Invalid signature length"
    );
}

#[test]
fn verify_rejects_well_formed_signatures_from_another_key() {
    let owner = Ed25519SigningService::new();
    let unrelated = Ed25519SigningService::new();

    let receipt = unsigned_receipt();
    let signature = owner.sign(&receipt).unwrap();
    let signed = receipt.sign(signature);

    // A foreign key is not an error: verification simply fails.
    assert!(!unrelated.verify(&signed).unwrap());
    assert!(owner.verify(&signed).unwrap());
}

#[test]
fn default_service_signs_and_verifies_like_new() {
    let service = Ed25519SigningService::default();
    let receipt = unsigned_receipt();

    let signature = service.sign(&receipt).unwrap();
    assert_eq!(signature.len(), 64);
    assert!(service.verify(&receipt.sign(signature)).unwrap());

    // `key_id` is the hex fingerprint of the first 8 verifying-key bytes.
    let key_id = service.key_id();
    assert_eq!(key_id.len(), 16);
    assert!(
        key_id.chars().all(|c| c.is_ascii_hexdigit()),
        "key id was not hex: {key_id}"
    );
}
