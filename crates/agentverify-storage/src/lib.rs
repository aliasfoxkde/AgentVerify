//! `AgentVerify` Storage
//!
//! Storage adapters for persisting receipts, actions, and verification state.
//!
//! This crate provides storage backends for `AgentVerify`'s persistent data:
//!
//! - Receipt storage - stores signed verification receipts
//! - Action state - tracks action lifecycle and verification results
//! - Contract registry - maintains validated contracts
//!
//! # Storage Backends
//!
//! - [`FileStorage`] - Local filesystem-based storage
//! - `PostgreSQL` storage (via agentverify-postgres)
//! - Redis storage (via agentverify-redis)
//!
//! # Safety
//!
//! Receipts must be stored durably - a lost receipt means lost evidence.
//! Storage implementations must guarantee receipt persistence.
//!
//! # Example
//!
//! ```ignore
//! use agentverify_storage::FileStorage;
//! use agentverify_core::{Receipt, ActionId, ContractId, VerificationResult};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let storage = FileStorage::new("/tmp/receipts")?;
//!
//!     let receipt = Receipt::new(
//!         ActionId::new(),
//!         ContractId::new(),
//!         VerificationResult::Verified,
//!         1,
//!     );
//!
//!     storage.store(&receipt).await?;
//!     let loaded = storage.load(receipt.id()).await?.unwrap();
//!     assert_eq!(receipt.id(), loaded.id());
//!     Ok(())
//! }
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
use agentverify_core::{ActionId, Receipt, ReceiptId};
use std::path::PathBuf;
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Errors that can occur during storage operations
#[derive(Debug, Error)]
pub enum StorageError {
    /// An underlying filesystem operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A receipt could not be serialized or deserialized.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// No receipt was stored under the requested ID.
    #[error("Receipt not found: {0}")]
    NotFound(ReceiptId),

    /// A receipt is already stored under the requested ID.
    #[error("Receipt already exists: {0}")]
    AlreadyExists(ReceiptId),

    /// The supplied storage path was rejected (e.g. not a directory).
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

/// Result type for storage operations
pub type Result<T> = std::result::Result<T, StorageError>;

/// Storage trait for receipt persistence
///
/// Implement this trait to provide custom receipt storage backends.
///
/// # Key semantics
/// - Receipts are stored by `ReceiptId`
/// - Store operations may overwrite existing receipts with the same ID
/// - Load operations return None if receipt does not exist
pub trait Storage: Send + Sync {
    /// Store a receipt
    fn store(&self, receipt: &Receipt) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Load a receipt by ID
    fn load(
        &self,
        id: &ReceiptId,
    ) -> impl std::future::Future<Output = Result<Option<Receipt>>> + Send;

    /// List all receipt IDs
    fn list_ids(&self) -> impl std::future::Future<Output = Result<Vec<ReceiptId>>> + Send;

    /// List receipts for a specific action
    fn list_by_action(
        &self,
        action_id: &ActionId,
    ) -> impl std::future::Future<Output = Result<Vec<Receipt>>> + Send;

    /// Delete a receipt by ID
    fn delete(&self, id: &ReceiptId) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Check if a receipt exists
    fn exists(&self, id: &ReceiptId) -> impl std::future::Future<Output = Result<bool>> + Send;
}

/// File-based storage for receipts
///
/// # Storage Format
/// - Each receipt stored as a single JSON file named `{receipt_id}.json`
/// - Directory structure: `{base_path}/{receipt_id}.json`
///
/// # Key semantics
/// - Atomic writes: use temp file + rename for crash safety
/// - Receipts are immutable once stored (no updates, only delete)
pub struct FileStorage {
    base_path: PathBuf,
    cache: RwLock<std::collections::HashSet<ReceiptId>>,
}

impl FileStorage {
    /// Create a new file-based storage backend
    ///
    /// # Arguments
    /// * `base_path` - Directory to store receipt files
    ///
    /// # Errors
    /// Returns error if directory cannot be created
    ///
    /// # Example
    /// ```
    /// use agentverify_storage::FileStorage;
    ///
    /// let storage = FileStorage::new("/tmp/receipts").unwrap();
    /// ```
    pub fn new(base_path: impl Into<PathBuf>) -> Result<Self> {
        let base_path = base_path.into();
        if base_path.to_str().is_none() {
            return Err(StorageError::InvalidPath(
                "Path must be a valid UTF-8 string".into(),
            ));
        }
        std::fs::create_dir_all(&base_path)?;
        Ok(Self {
            base_path,
            cache: RwLock::new(std::collections::HashSet::new()),
        })
    }

    /// Get the path for a receipt file
    fn receipt_path(&self, id: &ReceiptId) -> PathBuf {
        self.base_path.join(format!("{id}.json"))
    }

    /// Populate the cache from existing files
    async fn refresh_cache(&self) -> Result<()> {
        let mut cache = self.cache.write().await;
        let mut dir = fs::read_dir(&self.base_path).await?;
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(uuid) = Uuid::parse_str(stem) {
                        cache.insert(ReceiptId(uuid));
                    }
                }
            }
        }
        Ok(())
    }

    /// Store a receipt to the filesystem
    async fn store_inner(&self, receipt: &Receipt) -> Result<()> {
        let path = self.receipt_path(&receipt.id);

        // Check if already exists
        if path.exists() {
            return Err(StorageError::AlreadyExists(receipt.id));
        }

        // Write to temp file then rename for atomicity
        let temp_path = self.base_path.join(format!("{}.tmp", receipt.id));
        let content = serde_json::to_string_pretty(receipt)?;

        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(content.as_bytes()).await?;
        file.sync_all().await?;
        drop(file);

        fs::rename(&temp_path, &path).await?;

        // Update cache
        let mut cache = self.cache.write().await;
        cache.insert(receipt.id);

        Ok(())
    }

    /// Load a receipt from the filesystem
    async fn load_inner(&self, id: &ReceiptId) -> Result<Option<Receipt>> {
        let path = self.receipt_path(id);

        if !path.exists() {
            return Ok(None);
        }

        let mut file = fs::File::open(&path).await?;
        let mut content = String::new();
        file.read_to_string(&mut content).await?;
        drop(file);

        let receipt: Receipt = serde_json::from_str(&content)?;
        Ok(Some(receipt))
    }

    /// Delete a receipt from the filesystem
    async fn delete_inner(&self, id: &ReceiptId) -> Result<()> {
        let path = self.receipt_path(id);

        if !path.exists() {
            return Err(StorageError::NotFound(*id));
        }

        fs::remove_file(&path).await?;

        // Update cache
        let mut cache = self.cache.write().await;
        cache.remove(id);

        Ok(())
    }

    /// Check if a receipt exists
    async fn exists_inner(&self, id: &ReceiptId) -> Result<bool> {
        let cache = self.cache.read().await;
        if cache.contains(id) {
            return Ok(true);
        }
        drop(cache);

        // Refresh cache if not found (might be from external modification)
        self.refresh_cache().await?;
        let cache = self.cache.read().await;
        Ok(cache.contains(id))
    }
}

impl Storage for FileStorage {
    /// Store a receipt
    ///
    /// # Errors
    /// Returns error if receipt already exists or if write fails
    async fn store(&self, receipt: &Receipt) -> Result<()> {
        self.store_inner(receipt).await
    }

    /// Load a receipt by ID
    async fn load(&self, id: &ReceiptId) -> Result<Option<Receipt>> {
        self.load_inner(id).await
    }

    /// List all receipt IDs
    async fn list_ids(&self) -> Result<Vec<ReceiptId>> {
        self.refresh_cache().await?;
        let cache = self.cache.read().await;
        Ok(cache.iter().copied().collect())
    }

    /// List receipts for a specific action
    async fn list_by_action(&self, action_id: &ActionId) -> Result<Vec<Receipt>> {
        let ids = self.list_ids().await?;
        let mut receipts = Vec::new();

        for id in ids {
            if let Ok(Some(receipt)) = self.load_inner(&id).await {
                if receipt.action_id == *action_id {
                    receipts.push(receipt);
                }
            }
        }

        Ok(receipts)
    }

    /// Delete a receipt by ID
    async fn delete(&self, id: &ReceiptId) -> Result<()> {
        self.delete_inner(id).await
    }

    /// Check if a receipt exists
    async fn exists(&self, id: &ReceiptId) -> Result<bool> {
        self.exists_inner(id).await
    }
}

impl std::fmt::Debug for FileStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `cache` mirrors on-disk state and its contents would dominate
        // debug output, so it is elided rather than printed in full.
        f.debug_struct("FileStorage")
            .field("base_path", &self.base_path)
            .finish_non_exhaustive()
    }
}

/// In-memory storage for testing and short-lived use
///
/// # Limitations
/// - Not durable: data is lost on process restart
/// - Not suitable for multi-process access
pub struct MemStorage {
    receipts: RwLock<std::collections::HashMap<ReceiptId, Receipt>>,
}

impl MemStorage {
    /// Create a new in-memory storage
    #[must_use]
    pub fn new() -> Self {
        Self {
            receipts: RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MemStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for MemStorage {
    async fn store(&self, receipt: &Receipt) -> Result<()> {
        let mut receipts = self.receipts.write().await;
        receipts.insert(receipt.id, receipt.clone());
        Ok(())
    }

    async fn load(&self, id: &ReceiptId) -> Result<Option<Receipt>> {
        let receipts = self.receipts.read().await;
        Ok(receipts.get(id).cloned())
    }

    async fn list_ids(&self) -> Result<Vec<ReceiptId>> {
        let receipts = self.receipts.read().await;
        Ok(receipts.keys().copied().collect())
    }

    async fn list_by_action(&self, action_id: &ActionId) -> Result<Vec<Receipt>> {
        let receipts = self.receipts.read().await;
        Ok(receipts
            .values()
            .filter(|r| r.action_id == *action_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &ReceiptId) -> Result<()> {
        let mut receipts = self.receipts.write().await;
        if receipts.remove(id).is_none() {
            return Err(StorageError::NotFound(*id));
        }
        Ok(())
    }

    async fn exists(&self, id: &ReceiptId) -> Result<bool> {
        let receipts = self.receipts.read().await;
        Ok(receipts.contains_key(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::{ContractId, Observation, VerificationResult};
    use std::ffi::OsString;
    use tempfile::TempDir;

    fn create_test_receipt(action_id: ActionId, contract_id: ContractId) -> Receipt {
        Receipt::with_contract_version_and_key(
            action_id,
            contract_id,
            "1.0",
            VerificationResult::Verified,
            1,
            Some("test-key".to_string()),
        )
        .with_observation(Observation::new(
            agentverify_core::SourceId("test-source".into()),
            serde_json::json!({"status": "ok"}),
        ))
    }

    #[tokio::test]
    async fn file_storage_store_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        let id = receipt.id;

        storage.store(&receipt).await.unwrap();

        let loaded = storage.load(&id).await.unwrap().unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.action_id, receipt.action_id);
        assert_eq!(loaded.contract_id, receipt.contract_id);
    }

    #[tokio::test]
    async fn file_storage_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let loaded = storage.load(&ReceiptId::new()).await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn file_storage_delete() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        let id = receipt.id;

        storage.store(&receipt).await.unwrap();
        assert!(storage.exists(&id).await.unwrap());

        storage.delete(&id).await.unwrap();
        assert!(!storage.exists(&id).await.unwrap());
    }

    #[tokio::test]
    async fn file_storage_delete_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let result = storage.delete(&ReceiptId::new()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn file_storage_exists() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        let id = receipt.id;

        assert!(!storage.exists(&id).await.unwrap());

        storage.store(&receipt).await.unwrap();
        assert!(storage.exists(&id).await.unwrap());
    }

    #[tokio::test]
    async fn file_storage_list_ids() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt1 = create_test_receipt(ActionId::new(), ContractId::new());
        let receipt2 = create_test_receipt(ActionId::new(), ContractId::new());

        storage.store(&receipt1).await.unwrap();
        storage.store(&receipt2).await.unwrap();

        let ids = storage.list_ids().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&receipt1.id));
        assert!(ids.contains(&receipt2.id));
    }

    #[tokio::test]
    async fn file_storage_list_by_action() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let action_id = ActionId::new();
        let other_action_id = ActionId::new();
        let contract_id = ContractId::new();

        let primary = create_test_receipt(action_id, contract_id);
        let secondary = create_test_receipt(action_id, contract_id);
        let unrelated = create_test_receipt(other_action_id, contract_id);

        storage.store(&primary).await.unwrap();
        storage.store(&secondary).await.unwrap();
        storage.store(&unrelated).await.unwrap();

        let receipts = storage.list_by_action(&action_id).await.unwrap();
        assert_eq!(receipts.len(), 2);
    }

    #[tokio::test]
    async fn file_storage_store_duplicate() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());

        storage.store(&receipt).await.unwrap();
        let result = storage.store(&receipt).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            StorageError::AlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn file_storage_preserves_receipt_data() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        let id = receipt.id;

        storage.store(&receipt).await.unwrap();
        let loaded = storage.load(&id).await.unwrap().unwrap();

        // Verify all fields are preserved
        assert_eq!(loaded.version, receipt.version);
        assert_eq!(loaded.id, receipt.id);
        assert_eq!(loaded.action_id, receipt.action_id);
        assert_eq!(loaded.contract_id, receipt.contract_id);
        assert_eq!(loaded.contract_version, receipt.contract_version);
        assert_eq!(loaded.result, receipt.result);
        assert_eq!(loaded.attempts, receipt.attempts);
        assert_eq!(loaded.observations.len(), receipt.observations.len());
        assert_eq!(
            loaded.postcondition_results.len(),
            receipt.postcondition_results.len()
        );
        assert_eq!(loaded.digest, receipt.digest);
        assert_eq!(loaded.idempotency_key, receipt.idempotency_key);
    }

    #[tokio::test]
    async fn mem_storage_basic_operations() {
        let storage = MemStorage::new();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        let id = receipt.id;

        storage.store(&receipt).await.unwrap();
        assert!(storage.exists(&id).await.unwrap());

        let loaded = storage.load(&id).await.unwrap().unwrap();
        assert_eq!(loaded.id, id);

        storage.delete(&id).await.unwrap();
        assert!(!storage.exists(&id).await.unwrap());
    }

    #[tokio::test]
    async fn mem_storage_list_by_action() {
        let storage = MemStorage::new();

        let action_id = ActionId::new();
        let receipt = create_test_receipt(action_id, ContractId::new());

        storage.store(&receipt).await.unwrap();

        let receipts = storage.list_by_action(&action_id).await.unwrap();
        assert_eq!(receipts.len(), 1);
    }

    #[tokio::test]
    async fn mem_storage_default_matches_new() {
        let storage = MemStorage::default();
        assert!(!storage.exists(&ReceiptId::new()).await.unwrap());
    }

    #[tokio::test]
    async fn mem_storage_list_ids_returns_every_stored_receipt() {
        let storage = MemStorage::new();

        let receipt1 = create_test_receipt(ActionId::new(), ContractId::new());
        let receipt2 = create_test_receipt(ActionId::new(), ContractId::new());
        storage.store(&receipt1).await.unwrap();
        storage.store(&receipt2).await.unwrap();

        let ids = storage.list_ids().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&receipt1.id));
        assert!(ids.contains(&receipt2.id));

        // Deleting an unknown ID reports NotFound rather than silently succeeding.
        let unknown = ReceiptId::new();
        let err = storage.delete(&unknown).await.unwrap_err();
        assert_eq!(err.to_string(), format!("Receipt not found: {unknown}"));
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn storage_error_display_covers_every_variant() {
        let id = ReceiptId::new();

        let io_error = std::fs::File::open("/agentverify-nonexistent-path/receipt.json")
            .expect_err("missing file should fail");
        assert!(StorageError::Io(io_error)
            .to_string()
            .starts_with("IO error: "));

        let serialization_error: StorageError = serde_json::from_str::<Receipt>("{not json")
            .unwrap_err()
            .into();
        assert!(serialization_error
            .to_string()
            .starts_with("Serialization error: "));

        assert_eq!(
            StorageError::NotFound(id).to_string(),
            format!("Receipt not found: {id}")
        );
        assert_eq!(
            StorageError::AlreadyExists(id).to_string(),
            format!("Receipt already exists: {id}")
        );
        assert_eq!(
            StorageError::InvalidPath("not utf-8".to_string()).to_string(),
            "Invalid path: not utf-8"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_storage_rejects_paths_that_are_not_valid_utf8() {
        use std::os::unix::ffi::OsStringExt;

        // `FileStorage` builds receipt filenames from the base path, so a
        // non-UTF-8 path cannot be represented and must be rejected up front.
        let invalid = PathBuf::from(OsString::from_vec(vec![0x2f, 0xff, 0xfe]));
        let err = FileStorage::new(invalid).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Invalid path: Path must be a valid UTF-8 string"
        );
    }

    #[tokio::test]
    async fn file_storage_new_surfaces_io_failures() {
        let temp_dir = TempDir::new().unwrap();
        let blocking_file = temp_dir.path().join("occupied");
        std::fs::write(&blocking_file, b"not a directory").unwrap();

        // `create_dir_all` cannot create a directory beneath a regular file.
        let err = FileStorage::new(blocking_file.join("receipts")).unwrap_err();
        assert!(err.to_string().starts_with("IO error: "));
    }

    #[tokio::test]
    async fn file_storage_store_surfaces_io_failures_when_directory_vanishes() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        // Removing the directory out from under the adapter must surface an
        // error instead of losing the receipt silently.
        std::fs::remove_dir(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        let err = storage.store(&receipt).await.unwrap_err();
        assert!(err.to_string().starts_with("IO error: "));

        let err = storage.list_ids().await.unwrap_err();
        assert!(err.to_string().starts_with("IO error: "));
    }

    #[tokio::test]
    async fn file_storage_debug_reports_base_path_without_cache_contents() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let rendered = format!("{storage:?}");
        assert!(rendered.starts_with("FileStorage"));
        assert!(rendered.contains("base_path"));
        // The cache is elided on purpose so debug output stays bounded.
        assert!(rendered.contains(".."));
    }

    #[tokio::test]
    async fn file_storage_load_reports_serialization_errors() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let id = ReceiptId::new();
        std::fs::write(temp_dir.path().join(format!("{id}.json")), "{not a receipt").unwrap();

        let err = storage.load(&id).await.unwrap_err();
        assert!(err.to_string().starts_with("Serialization error: "));
    }

    #[tokio::test]
    async fn file_storage_list_by_action_skips_unreadable_receipts() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let action_id = ActionId::new();
        let good = create_test_receipt(action_id, ContractId::new());
        storage.store(&good).await.unwrap();

        // A corrupt file whose name is still a valid receipt ID.
        let corrupt_id = ReceiptId::new();
        std::fs::write(
            temp_dir.path().join(format!("{corrupt_id}.json")),
            "{corrupt",
        )
        .unwrap();

        assert_eq!(storage.list_ids().await.unwrap().len(), 2);

        let receipts = storage.list_by_action(&action_id).await.unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].id, good.id);
    }

    #[tokio::test]
    async fn file_storage_refresh_cache_ignores_files_that_are_not_receipts() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        storage.store(&receipt).await.unwrap();

        // Foreign files that a human or another process may have left behind
        // must be ignored, not surfaced as receipts.
        std::fs::write(temp_dir.path().join("README.txt"), b"notes").unwrap();
        std::fs::write(temp_dir.path().join("not-a-uuid.json"), b"{}").unwrap();
        std::fs::write(
            temp_dir
                .path()
                .join("00000000-0000-0000-0000-000000000000.json"),
            b"{}",
        )
        .unwrap();

        let ids = storage.list_ids().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&receipt.id));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_storage_refresh_cache_ignores_non_utf8_filenames() {
        use std::os::unix::ffi::OsStrExt;

        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path()).unwrap();

        let receipt = create_test_receipt(ActionId::new(), ContractId::new());
        storage.store(&receipt).await.unwrap();

        // A `.json` file whose stem is not valid UTF-8 has no parseable
        // receipt ID and must be skipped rather than corrupting the listing.
        let foreign = std::ffi::OsStr::from_bytes(&[0xff, 0xfe, 0x2e, 0x6a, 0x73, 0x6f, 0x6e]);
        std::fs::write(temp_dir.path().join(foreign), b"{}").unwrap();

        let ids = storage.list_ids().await.unwrap();
        assert_eq!(ids, vec![receipt.id]);
    }
}
