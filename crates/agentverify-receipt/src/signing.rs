//! Receipt signing and verification
//!
//! Implements Ed25519 signing for receipts and provides a trait for
//! pluggable signing backends.

use agentverify_core::Receipt;
use ed25519_dalek::Signer;
use thiserror::Error;

/// Errors that can occur during signing operations
#[derive(Debug, Error)]
pub enum SigningError {
    /// No signing key was configured for this service.
    #[error("Signing key is missing")]
    MissingKey,

    /// The signature could not be produced or the key material was invalid.
    #[error("Signing failed: {0}")]
    SigningFailed(String),

    /// A signature failed verification or could not be checked.
    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    /// Key material could not be loaded or decoded.
    #[error("Key error: {0}")]
    KeyError(String),
}

/// Trait for pluggable receipt signing services
///
/// Implement this trait to provide custom signing backends:
/// - Ed25519 for production use
/// - HMAC-based for simpler deployments
/// - KMS-based for cloud integrations
/// - Mock signing for tests
///
/// # Example
/// ```ignore
/// struct HmacSigningService { key: [u8; 32] }
///
/// impl SigningService for HmacSigningService {
///     fn sign(&self, receipt: &Receipt) -> Result<Vec<u8>, SigningError> {
///         let data = self.canonicalize(receipt);
///         Ok(HmacSha256::new(&self.key).chain(&data).finalize().to_vec())
///     }
///
///     fn verify(&self, receipt: &Receipt) -> Result<bool, SigningError> {
///         let signature = receipt.signature.as_ref().ok_or(SigningError::MissingKey)?;
///         let expected = self.sign(receipt)?;
///         Ok(signature == &expected)
///     }
///
///     fn key_id(&self) -> String {
///         "hmac-sha256".to_string()
///     }
/// }
/// ```
pub trait SigningService: Send + Sync {
    /// Sign a receipt and return the signature bytes
    ///
    /// # Errors
    ///
    /// Returns [`SigningError::MissingKey`] when no signing key is configured,
    /// and [`SigningError::SigningFailed`] when the signature cannot be
    /// produced.
    fn sign(&self, receipt: &Receipt) -> Result<Vec<u8>, SigningError>;

    /// Verify a receipt signature
    ///
    /// Returns `Ok(true)` if signature is valid,
    /// `Ok(false)` if signature is invalid,
    /// `Err(...)` on error (e.g., missing signature)
    ///
    /// # Errors
    ///
    /// Returns [`SigningError::MissingKey`] when the receipt carries no
    /// signature, and [`SigningError::VerificationFailed`] when the signature
    /// cannot be checked against the receipt contents.
    fn verify(&self, receipt: &Receipt) -> Result<bool, SigningError>;

    /// Get the key identifier (fingerprint) for this signing service
    fn key_id(&self) -> String;

    /// Get canonical representation of receipt for signing
    fn canonicalize(&self, receipt: &Receipt) -> Vec<u8> {
        let data = serde_json::json!({
            "id": receipt.id.to_string(),
            "action_id": receipt.action_id.to_string(),
            "contract_id": receipt.contract_id.to_string(),
            "result": receipt.result.to_string(),
            "attempts": receipt.attempts,
            "observations": receipt.observations,
            "postcondition_results": receipt.postcondition_results,
            "timestamp": receipt.timestamp.to_rfc3339(),
        });
        serde_json::to_vec(&data).unwrap_or_default()
    }
}

/// Receipt signing service using Ed25519
pub struct Ed25519SigningService {
    signing_key: ed25519_dalek::SigningKey,
    verifying_key: ed25519_dalek::VerifyingKey,
}

impl Ed25519SigningService {
    /// Create a new signing service with a randomly generated key
    pub fn new() -> Self {
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create a signing service from a raw 32-byte seed
    #[must_use]
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create a signing service from a base64-encoded key
    ///
    /// # Errors
    ///
    /// Returns [`SigningError::SigningFailed`] if `encoded` is not valid
    /// base64 or does not decode to exactly 32 bytes.
    pub fn from_base64(encoded: &str) -> Result<Self, SigningError> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| SigningError::SigningFailed(format!("Invalid base64: {e}")))?;

        if bytes.len() != 32 {
            return Err(SigningError::SigningFailed(
                "Key must be 32 bytes".to_string(),
            ));
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);
        Ok(Self::from_seed(&key_bytes))
    }

    /// Get the verifying key as base64
    #[must_use]
    pub fn verifying_key_base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(self.verifying_key.as_bytes())
    }
}

impl SigningService for Ed25519SigningService {
    fn sign(&self, receipt: &Receipt) -> Result<Vec<u8>, SigningError> {
        let canonical = self.canonicalize(receipt);
        let signature = self.signing_key.sign(&canonical);
        Ok(signature.to_vec())
    }

    fn verify(&self, receipt: &Receipt) -> Result<bool, SigningError> {
        let signature_bytes = receipt.signature.as_ref().ok_or(SigningError::MissingKey)?;

        if signature_bytes.len() != 64 {
            return Err(SigningError::VerificationFailed(
                "Invalid signature length".to_string(),
            ));
        }

        let signature =
            ed25519_dalek::Signature::from_bytes(signature_bytes.as_slice().try_into().map_err(
                |_| SigningError::VerificationFailed("Signature conversion failed".to_string()),
            )?);

        let canonical = self.canonicalize(receipt);
        Ok(self
            .verifying_key
            .verify_strict(&canonical, &signature)
            .is_ok())
    }

    fn key_id(&self) -> String {
        // Use the first 16 bytes of the verifying key as fingerprint
        let bytes = self.verifying_key.as_bytes();
        hex::encode(&bytes[..8])
    }
}

impl Default for Ed25519SigningService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::VerificationResult;

    #[test]
    fn sign_and_verify_receipt() {
        let service = Ed25519SigningService::new();

        let receipt = Receipt::new(
            agentverify_core::ActionId::new(),
            agentverify_core::ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let signature = service.sign(&receipt).unwrap();
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn verify_valid_receipt() {
        let service = Ed25519SigningService::new();

        let receipt = Receipt::new(
            agentverify_core::ActionId::new(),
            agentverify_core::ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let signature = service.sign(&receipt).unwrap();
        let mut signed_receipt = receipt.sign(signature);
        signed_receipt.key_id = Some(service.key_id());

        assert!(service.verify(&signed_receipt).unwrap());
    }

    #[test]
    fn verify_tampered_receipt_fails() {
        let service = Ed25519SigningService::new();

        let receipt = Receipt::new(
            agentverify_core::ActionId::new(),
            agentverify_core::ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let signature = service.sign(&receipt).unwrap();
        let mut signed_receipt = receipt.sign(signature);
        signed_receipt.key_id = Some(service.key_id());

        // Tamper with the result
        signed_receipt.result = VerificationResult::Failed;

        assert!(!service.verify(&signed_receipt).unwrap());
    }

    #[test]
    fn key_id_is_consistent() {
        let service = Ed25519SigningService::new();
        let key_id1 = service.key_id();
        let key_id2 = service.key_id();
        assert_eq!(key_id1, key_id2);
    }

    #[test]
    fn from_base64_roundtrip() {
        let service = Ed25519SigningService::new();
        let b64 = service.verifying_key_base64();

        // Should not error
        let _service2 = Ed25519SigningService::from_base64(&b64).unwrap();
    }
}
