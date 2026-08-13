//! Receipt signing and verification
//!
//! Implements Ed25519 signing for receipts.

use agentverify_core::Receipt;
use ed25519_dalek::Signer;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SigningError {
    #[error("Signing key is missing")]
    MissingKey,

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Verification failed: {0}")]
    VerificationFailed(String),
}

/// Receipt signing service using Ed25519
pub struct SigningService {
    signing_key: ed25519_dalek::SigningKey,
    verifying_key: ed25519_dalek::VerifyingKey,
}

impl SigningService {
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
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create a signing service from a base64-encoded key
    pub fn from_base64(encoded: &str) -> Result<Self, SigningError> {
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
            .map_err(|e| SigningError::SigningFailed(format!("Invalid base64: {}", e)))?;

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
    pub fn verifying_key_base64(&self) -> String {
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            self.verifying_key.as_bytes(),
        )
    }

    /// Sign a receipt and return the signed receipt
    pub fn sign_receipt(&self, receipt: &Receipt) -> Result<Vec<u8>, SigningError> {
        let canonical = self.canonicalize(receipt);
        let signature = self.signing_key.sign(&canonical);
        Ok(signature.to_vec())
    }

    /// Verify a receipt signature
    pub fn verify_receipt(&self, receipt: &Receipt) -> Result<bool, SigningError> {
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

    /// Create a canonical JSON representation of the receipt for signing
    fn canonicalize(&self, receipt: &Receipt) -> Vec<u8> {
        // Include all fields that affect the receipt's validity
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

        // Canonical JSON (no extra whitespace)
        serde_json::to_vec(&data).unwrap_or_default()
    }
}

impl Default for SigningService {
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
        let service = SigningService::new();

        let receipt = Receipt::new(
            agentverify_core::ActionId::new(),
            agentverify_core::ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let signature = service.sign_receipt(&receipt).unwrap();
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn verify_valid_receipt() {
        let service = SigningService::new();

        let receipt = Receipt::new(
            agentverify_core::ActionId::new(),
            agentverify_core::ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let signature = service.sign_receipt(&receipt).unwrap();
        let signed_receipt = receipt.sign(signature);
        assert!(service.verify_receipt(&signed_receipt).unwrap());
    }

    #[test]
    fn verify_tampered_receipt_fails() {
        let service = SigningService::new();

        let receipt = Receipt::new(
            agentverify_core::ActionId::new(),
            agentverify_core::ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let signature = service.sign_receipt(&receipt).unwrap();
        let mut signed_receipt = receipt.sign(signature);

        // Tamper with the result
        signed_receipt.result = VerificationResult::Failed;

        assert!(!service.verify_receipt(&signed_receipt).unwrap());
    }
}
