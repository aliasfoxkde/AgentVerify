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
    #[must_use]
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
            idempotency_key,
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
    #[must_use]
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
    ///
    /// Recomputes the digest so [`Self::verify_digest`] stays true: the
    /// canonical payload covers observations, so mutating them without
    /// refreshing the digest would make every receipt with evidence read as
    /// tampered.
    #[must_use]
    pub fn with_observation(mut self, observation: Observation) -> Self {
        self.observations.push(observation);
        self.digest = self.compute_digest();
        self
    }

    /// Add a postcondition result
    ///
    /// Recomputes the digest for the same reason as [`Self::with_observation`].
    #[must_use]
    pub fn with_postcondition_result(mut self, result: PostconditionResult) -> Self {
        self.postcondition_results.push(result);
        self.digest = self.compute_digest();
        self
    }

    /// Set the key identifier
    #[must_use]
    pub fn with_key_id(mut self, key_id: impl Into<String>) -> Self {
        self.key_id = Some(key_id.into());
        self
    }

    /// Sign the receipt
    #[must_use]
    pub fn sign(mut self, signature: Vec<u8>) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Check if receipt is signed
    #[must_use]
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }

    /// Verify the digest matches the receipt content
    #[must_use]
    pub fn verify_digest(&self) -> bool {
        self.digest == self.compute_digest()
    }

    /// Get the schema version
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get the key identifier if set
    #[must_use]
    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }
}

/// Errors that can occur while persisting a receipt.
#[derive(Debug, thiserror::Error)]
pub enum ReceiptStoreError {
    /// The receipt could not be persisted to the backing store.
    #[error("failed to persist receipt: {0}")]
    Persist(String),

    /// The receipt could not be serialized.
    #[error("failed to serialize receipt: {0}")]
    Serialize(String),
}

/// Receipt store trait for persistence
///
/// Implement this trait to provide custom receipt storage:
/// - In-memory for tests
/// - File-based for local persistence
/// - Database for durable storage
///
/// # Key semantics
/// - Key scope: receipts are stored by `ReceiptId`
/// - Collision: overwrite with newer receipt (same ID)
/// - Expiry: implementors may choose to expire entries after TTL
///
/// Asynchronous by design: each method returns a boxed future so
/// implementations can be backed by a database, filesystem, or network service
/// without the trait itself being `async` (which would not be object-safe).
pub trait ReceiptStore: Send + Sync {
    /// Store a receipt
    ///
    /// # Errors
    ///
    /// Returns `ReceiptStoreError` if the receipt cannot be serialized or
    /// written to the backing store. Implementations must not panic on I/O
    /// failure: evidence persistence is best-effort but its failure is
    /// always observable to the caller.
    fn store<'a>(
        &'a self,
        receipt: &'a Receipt,
    ) -> Pin<Box<dyn Future<Output = Result<(), ReceiptStoreError>> + Send + 'a>>;

    /// Retrieve a receipt by ID
    ///
    /// Returns `None` when no receipt with that ID is held by the store.
    fn get<'a>(
        &'a self,
        id: &'a ReceiptId,
    ) -> Pin<Box<dyn Future<Output = Option<Receipt>> + Send + 'a>>;

    /// List receipts for an action
    ///
    /// Returns an empty vector when the action has no recorded receipts.
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
/// For production, use a distributed store implementing `ReceiptStore`.
pub struct InMemoryReceiptStore {
    receipts: tokio::sync::RwLock<std::collections::HashMap<ReceiptId, Receipt>>,
    by_action: tokio::sync::RwLock<std::collections::HashMap<ActionId, Vec<ReceiptId>>>,
}

impl InMemoryReceiptStore {
    /// Create an empty in-memory receipt store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            receipts: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            by_action: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl ReceiptStore for InMemoryReceiptStore {
    fn store<'a>(
        &'a self,
        receipt: &'a Receipt,
    ) -> Pin<Box<dyn Future<Output = Result<(), ReceiptStoreError>> + Send + 'a>> {
        Box::pin(async move {
            let mut receipts = self.receipts.write().await;
            let mut by_action = self.by_action.write().await;
            receipts.insert(receipt.id, receipt.clone());
            by_action
                .entry(receipt.action_id)
                .or_default()
                .push(receipt.id);
            Ok(())
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

#[cfg(not(target_arch = "wasm32"))]
/// File-based receipt store for local persistence
///
/// # Storage Format
/// - Each receipt stored as a single JSON file named `{receipt_id}.json`
/// - Index file `index.json` maps `action_id` → list of `receipt_ids`
/// - Directory structure: `{base_path}/{receipt_id}.json`
///
/// # Key semantics
/// - Key scope: receipts stored by `ReceiptId`
/// - Collision: overwrite with newer receipt (same ID)
/// - Atomic writes: use temp file + rename for crash safety
///
/// # Limitations
/// - Local filesystem only, not suitable for multi-process access without file locking
/// - No TTL: entries persist until manually cleaned up
///
/// For production distributed use, implement `ReceiptStore` with a proper
/// distributed store (Postgres, Redis, etc.).
pub struct FileReceiptStore {
    base_path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileReceiptStore {
    /// Create a new file-based receipt store
    ///
    /// # Arguments
    /// * `base_path` - Directory to store receipt files
    ///
    /// # Errors
    /// Returns error if directory cannot be created
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> std::io::Result<Self> {
        let base_path = base_path.into();
        std::fs::create_dir_all(&base_path)?;
        Ok(Self { base_path })
    }

    fn index_path(&self) -> std::path::PathBuf {
        self.base_path.join("index.json")
    }

    /// Read index using blocking I/O in async context
    async fn read_index_async(
        &self,
    ) -> std::io::Result<std::collections::HashMap<String, Vec<String>>> {
        let index_path = self.index_path();
        if !index_path.exists() {
            return Ok(std::collections::HashMap::new());
        }
        let content = tokio::fs::read_to_string(&index_path).await?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Write index using blocking I/O in async context
    async fn write_index_async(
        &self,
        index: &std::collections::HashMap<String, Vec<String>>,
    ) -> std::io::Result<()> {
        let index_path = self.index_path();
        let content = serde_json::to_string_pretty(index)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let temp_path = self.base_path.join("index.tmp");
        tokio::fs::write(&temp_path, &content).await?;
        tokio::fs::rename(&temp_path, &index_path).await?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ReceiptStore for FileReceiptStore {
    fn store<'a>(
        &'a self,
        receipt: &'a Receipt,
    ) -> Pin<Box<dyn Future<Output = Result<(), ReceiptStoreError>> + Send + 'a>> {
        let base_path = self.base_path.clone();
        Box::pin(async move {
            let receipt_id = receipt.id.to_string();
            let action_id = receipt.action_id.to_string();

            // Serialize receipt
            let content = serde_json::to_string_pretty(receipt)
                .map_err(|e| ReceiptStoreError::Serialize(e.to_string()))?;

            // Write to temp file then rename for atomicity
            let receipt_path = base_path.join(format!("{receipt_id}.json"));
            let temp_path = base_path.join(format!("{receipt_id}.tmp"));
            tokio::fs::write(&temp_path, &content)
                .await
                .map_err(|e| ReceiptStoreError::Persist(e.to_string()))?;
            tokio::fs::rename(&temp_path, &receipt_path)
                .await
                .map_err(|e| ReceiptStoreError::Persist(e.to_string()))?;

            // Update index
            let index_store = Self::new(base_path.clone())
                .map_err(|e| ReceiptStoreError::Persist(e.to_string()))?;
            let mut index = index_store.read_index_async().await.unwrap_or_default();
            index.entry(action_id).or_default().push(receipt_id);
            index_store
                .write_index_async(&index)
                .await
                .map_err(|e| ReceiptStoreError::Persist(e.to_string()))?;
            Ok(())
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a ReceiptId,
    ) -> Pin<Box<dyn Future<Output = Option<Receipt>> + Send + 'a>> {
        let base_path = self.base_path.clone();
        Box::pin(async move {
            let path = base_path.join(format!("{id}.json"));
            if !path.exists() {
                return None;
            }
            let content = tokio::fs::read_to_string(&path).await.ok()?;
            serde_json::from_str(&content).ok()
        })
    }

    fn list_by_action<'a>(
        &'a self,
        action_id: &'a ActionId,
    ) -> Pin<Box<dyn Future<Output = Vec<Receipt>> + Send + 'a>> {
        let base_path = self.base_path.clone();
        Box::pin(async move {
            let Ok(store) = Self::new(base_path.clone()) else {
                return Vec::new();
            };
            let Ok(index) = store.read_index_async().await else {
                return Vec::new();
            };
            let receipt_ids = match index.get(action_id.to_string().as_str()) {
                Some(ids) => ids.clone(),
                None => return Vec::new(),
            };
            let mut receipts = Vec::new();
            for receipt_id in receipt_ids {
                let path = base_path.join(format!("{receipt_id}.json"));
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(receipt) = serde_json::from_str::<Receipt>(&content) {
                        receipts.push(receipt);
                    }
                }
            }
            receipts
        })
    }

    fn exists<'a>(&'a self, id: &'a ReceiptId) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        let base_path = self.base_path.clone();
        Box::pin(async move { base_path.join(format!("{id}.json")).exists() })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for FileReceiptStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileReceiptStore")
            .field("base_path", &self.base_path)
            .finish()
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
    fn receipt_digest_covers_added_observation() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        )
        .with_observation(Observation::new(
            SourceId("ledger".into()),
            serde_json::json!({"status": "ok"}),
        ));

        assert!(
            receipt.verify_digest(),
            "adding an observation must keep the digest valid"
        );
        assert_eq!(receipt.observations.len(), 1);
    }

    #[test]
    fn receipt_digest_covers_added_postcondition_result() {
        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        )
        .with_postcondition_result(PostconditionResult {
            predicate: Predicate::exists("refund.status"),
            description: "refund succeeded".into(),
            passed: true,
            error: None,
        });

        assert!(
            receipt.verify_digest(),
            "adding a postcondition result must keep the digest valid"
        );
        assert_eq!(receipt.postcondition_results.len(), 1);
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

    #[test]
    fn file_receipt_store_persists_and_retrieves_receipt() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileReceiptStore::new(temp_dir.path()).unwrap();

        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        // Store the receipt
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store_ref = &store;
        let receipt_ref = &receipt;
        let _ = rt.block_on(store_ref.store(receipt_ref));

        // Retrieve it
        let retrieved = rt.block_on(store.get(&receipt.id));

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, receipt.id);
        assert_eq!(retrieved.digest, receipt.digest);
    }

    #[test]
    fn file_receipt_store_exists_check() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileReceiptStore::new(temp_dir.path()).unwrap();

        let receipt = Receipt::new(
            ActionId::new(),
            ContractId::new(),
            VerificationResult::Verified,
            1,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let store_ref = &store;
        let receipt_ref = &receipt;
        let _ = rt.block_on(store_ref.store(receipt_ref));

        let exists = rt.block_on(store.exists(&receipt.id));
        assert!(exists);

        let non_existent = rt.block_on(store.exists(&ReceiptId::new()));
        assert!(!non_existent);
    }
}
