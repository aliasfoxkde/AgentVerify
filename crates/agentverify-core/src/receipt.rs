//! Receipt types for verification evidence
//!
//! # Versioned Receipt Envelope
//!
//! The receipt system uses a versioned envelope design to support:
//! - Schema evolution and backward compatibility
//! - Tamper-evident digest binding
//! - Key identity and rotation
//! - Replay protection
//!
//! ## Envelope Structure
//!
//! ```text
//! +------------------------------------------+
//! | ReceiptEnvelope                           |
//! +------------------------------------------+
//! | version: "1.0"                          |
//! | receipt_id: ReceiptId                    |
//! | action_id: ActionId                      |
//! | contract_id: ContractId                  |
//! | contract_version: String                 |
//! | result: VerificationResult               |
//! | attempts: u32                            |
//! | timestamp: DateTime<Utc>                 |
//! +------------------------------------------+
//! | digest: String (SHA-256 of canonical)    |
//! | key_id: String (verifying key fingerprint)|
//! | idempotency_key: Option<String>          |
//! +------------------------------------------+
//! | observations: Vec<Observation>           |
//! | postcondition_results: Vec<PostcondRes>  |
//! +------------------------------------------+
//! | signature: Option<Vec<u8>> (Ed25519)     |
//! +------------------------------------------+
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

use super::id::{ActionId, ContractId, ReceiptId};
use super::observation::Observation;
use super::predicate::Predicate;
use super::verification_result::VerificationResult;

/// Current receipt schema version
pub const RECEIPT_SCHEMA_VERSION: &str = "1.0";

/// Result of a single postcondition evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostconditionResult {
    /// The predicate that was evaluated
    pub predicate: Predicate,
    /// Human-readable description
    pub description: String,
    /// Whether it passed
    pub passed: bool,
    /// Optional error message if evaluation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A signed record of verification outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// Schema version for compatibility
    pub version: String,
    /// Unique identifier
    pub id: ReceiptId,
    /// Action that was verified
    pub action_id: ActionId,
    /// Contract used for verification
    pub contract_id: ContractId,
    /// Contract version for binding
    pub contract_version: String,
    /// Verification result
    pub result: VerificationResult,
    /// Number of attempts made
    pub attempts: u32,
    /// Observations made during verification
    #[serde(default)]
    pub observations: Vec<Observation>,
    /// Results of postcondition evaluations
    #[serde(default)]
    pub postcondition_results: Vec<PostconditionResult>,
    /// SHA-256 digest of canonical receipt data (hex-encoded)
    pub digest: String,
    /// Key identifier (fingerprint of verifying key used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    /// Idempotency key for replay protection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Ed25519 signature (when signed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<Vec<u8>>,
    /// When the receipt was created
    pub timestamp: DateTime<Utc>,
}

impl Receipt {
    /// Create a new receipt
    pub fn new(
        action_id: ActionId,
        contract_id: ContractId,
        result: VerificationResult,
        attempts: u32,
    ) -> Self {
        let receipt = Self {
            version: RECEIPT_SCHEMA_VERSION.to_string(),
            id: ReceiptId::new(),
            action_id,
            contract_id,
            contract_version: String::new(),
            result,
            attempts,
            observations: Vec::new(),
            postcondition_results: Vec::new(),
            digest: String::new(),
            key_id: None,
            idempotency_key: None,
            signature: None,
            timestamp: Utc::now(),
        };
        receipt.with_digest()
    }

    /// Create a new receipt with contract version and idempotency key
    pub fn with_contract_version_and_key(
        action_id: ActionId,
        contract_id: ContractId,
        contract_version: impl Into<String>,
        result: VerificationResult,
        attempts: u32,
        idempotency_key: Option<String>,
    ) -> Self {
        let contract_version = contract_version.into();
        let receipt = Self {
            version: RECEIPT_SCHEMA_VERSION.to_string(),
            id: ReceiptId::new(),
            action_id,
            contract_id,
            contract_version: contract_version.clone(),
            result,
            attempts,
            observations: Vec::new(),
            postcondition_results: Vec::new(),
            digest: String::new(),
            key_id: None,
            idempotency_key: idempotency_key.clone(),
            signature: None,
            timestamp: Utc::now(),
        };
        receipt.with_digest()
    }

    /// Compute and set the digest from canonical representation
    fn with_digest(mut self) -> Self {
        self.digest = self.compute_digest();
        self
    }

    /// Compute canonical digest (SHA-256) of the receipt data
    pub fn compute_digest(&self) -> String {
        let canonical = self.canonical_data();
        let mut hasher = Sha256::new();
        hasher.update(&canonical);
        hex::encode(hasher.finalize())
    }

    /// Get canonical representation for signing/digest
    fn canonical_data(&self) -> Vec<u8> {
        let data = serde_json::json!({
            "version": self.version,
            "id": self.id.to_string(),
            "action_id": self.action_id.to_string(),
            "contract_id": self.contract_id.to_string(),
            "contract_version": self.contract_version,
            "result": self.result.to_string(),
            "attempts": self.attempts,
            "observations": self.observations,
            "postcondition_results": self.postcondition_results,
            "idempotency_key": self.idempotency_key,
            "timestamp": self.timestamp.to_rfc3339(),
        });
        serde_json::to_vec(&data).unwrap_or_default()
    }

    /// Add an observation
    pub fn with_observation(mut self, observation: Observation) -> Self {
        self.observations.push(observation);
        self
    }

    /// Add postcondition result
    pub fn with_postcondition_result(mut self, result: PostconditionResult) -> Self {
        self.postcondition_results.push(result);
        self
    }

    /// Set the key identifier
    pub fn with_key_id(mut self, key_id: impl Into<String>) -> Self {
        self.key_id = Some(key_id.into());
        self
    }

    /// Sign the receipt
    pub fn sign(mut self, signature: Vec<u8>) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Check if receipt is signed
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }

    /// Verify the digest matches the receipt content
    pub fn verify_digest(&self) -> bool {
        self.digest == self.compute_digest()
    }

    /// Get the schema version
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get the key identifier if set
    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }
}

/// Receipt store trait for persistence
///
/// Implement this trait to provide custom receipt storage:
/// - In-memory for tests
/// - File-based for local persistence
/// - Database for durable storage
///
/// # Key semantics
/// - Key scope: receipts are stored by ReceiptId
/// - Collision: overwrite with newer receipt (same ID)
/// - Expiry: implementors may choose to expire entries after TTL
pub trait ReceiptStore: Send + Sync {
    /// Store a receipt
    fn store<'a>(&'a self, receipt: &'a Receipt) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    /// Retrieve a receipt by ID
    fn get<'a>(
        &'a self,
        id: &'a ReceiptId,
    ) -> Pin<Box<dyn Future<Output = Option<Receipt>> + Send + 'a>>;

    /// List receipts for an action
    fn list_by_action<'a>(
        &'a self,
        action_id: &'a ActionId,
    ) -> Pin<Box<dyn Future<Output = Vec<Receipt>> + Send + 'a>>;

    /// Check if a receipt exists
    fn exists<'a>(&'a self, id: &'a ReceiptId) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

/// In-memory receipt store for process-local use
///
/// # Limitations
/// - Process-local only: does not persist across restarts
/// - No TTL: entries live until process exits
///
/// For production, use a distributed store implementing ReceiptStore.
pub struct InMemoryReceiptStore {
    receipts: tokio::sync::RwLock<std::collections::HashMap<ReceiptId, Receipt>>,
    by_action: tokio::sync::RwLock<std::collections::HashMap<ActionId, Vec<ReceiptId>>>,
}

impl InMemoryReceiptStore {
    pub fn new() -> Self {
        Self {
            receipts: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            by_action: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl ReceiptStore for InMemoryReceiptStore {
    fn store<'a>(&'a self, receipt: &'a Receipt) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let mut receipts = self.receipts.write().await;
            let mut by_action = self.by_action.write().await;
            receipts.insert(receipt.id, receipt.clone());
            by_action
                .entry(receipt.action_id)
                .or_default()
                .push(receipt.id);
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a ReceiptId,
    ) -> Pin<Box<dyn Future<Output = Option<Receipt>> + Send + 'a>> {
        Box::pin(async move { self.receipts.read().await.get(id).cloned() })
    }

    fn list_by_action<'a>(
        &'a self,
        action_id: &'a ActionId,
    ) -> Pin<Box<dyn Future<Output = Vec<Receipt>> + Send + 'a>> {
        Box::pin(async move {
            let by_action = self.by_action.read().await;
            let receipts = self.receipts.read().await;
            by_action
                .get(action_id)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| receipts.get(id).cloned())
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    fn exists<'a>(&'a self, id: &'a ReceiptId) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { self.receipts.read().await.contains_key(id) })
    }
}

impl Default for InMemoryReceiptStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceId;

    #[test]
    fn receipt_has_version() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );
        assert_eq!(receipt.version(), "1.0");
    }

    #[test]
    fn receipt_computes_digest() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );
        assert!(!receipt.digest.is_empty());
        assert_eq!(receipt.digest.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn receipt_verify_digest() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );
        assert!(receipt.verify_digest());
    }

    #[test]
    fn receipt_with_contract_version_and_key() {
        let receipt = Receipt::with_contract_version_and_key(
            ActionId::new(),
            ContractId::new(),
            "2.0",
            VerificationResult::Verified,
            1,
            Some("idem-key-123".to_string()),
        );
        assert_eq!(receipt.contract_version, "2.0");
        assert_eq!(receipt.idempotency_key, Some("idem-key-123".to_string()));
    }

    #[test]
    fn receipt_with_key_id() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        )
        .with_key_id("key-fingerprint-abc");
        assert_eq!(receipt.key_id(), Some("key-fingerprint-abc"));
    }

    #[test]
    fn receipt_digest_changes_on_tamper() {
        let mut receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );
        let original_digest = receipt.digest.clone();
        assert!(receipt.verify_digest());

        // Tamper with result
        receipt.result = VerificationResult::Failed;
        assert!(!receipt.verify_digest());
        assert_ne!(receipt.compute_digest(), original_digest);
    }

    #[test]
    fn receipt_is_signed_false_when_unsigned() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );
        assert!(!receipt.is_signed());
    }

    #[test]
    fn receipt_is_signed_true_when_signed() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        )
        .sign(vec![0u8; 64]);
        assert!(receipt.is_signed());
    }

    #[test]
    fn receipt_serialization_roundtrip() {
        let receipt = Receipt::with_contract_version_and_key(
            ActionId::new(),
            ContractId::new(),
            "1.0",
            VerificationResult::Verified,
            2,
            Some("test-key".to_string()),
        )
        .with_key_id("my-key")
        .with_observation(Observation::new(
            SourceId("test".into()),
            serde_json::json!({"status": "ok"}),
        ));

        let json = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();

        assert_eq!(receipt.id, deserialized.id);
        assert_eq!(receipt.digest, deserialized.digest);
        assert_eq!(receipt.version, deserialized.version);
        assert_eq!(receipt.contract_version, deserialized.contract_version);
        assert_eq!(receipt.idempotency_key, deserialized.idempotency_key);
    }
}
