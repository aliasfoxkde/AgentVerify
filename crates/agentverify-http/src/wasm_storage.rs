//! WASM-native storage using browser APIs
//!
//! Provides ReceiptStore and IdempotencyStore implementations for wasm32 targets
//! using browser localStorage or IndexedDB.

use agentverify_core::{ActionId, Receipt, ReceiptId, VerificationResult};
use web_sys::Storage;

/// Error type for WASM storage operations
#[derive(Debug, thiserror::Error)]
pub enum WasmStorageError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Not found: {0}")]
    NotFound(String),
}

/// WASM-native receipt store using browser localStorage
#[derive(Clone)]
pub struct WasmReceiptStore {
    storage: Storage,
    prefix: String,
}

impl WasmReceiptStore {
    /// Create a new WASM receipt store
    pub fn new(namespace: &str) -> Result<Self, WasmStorageError> {
        let window = web_sys::window().ok_or_else(|| WasmStorageError::Storage("No window".to_string()))?;
        let storage = window.local_storage().map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?.ok_or_else(|| WasmStorageError::Storage("No local storage".to_string()))?;
        
        Ok(Self {
            storage,
            prefix: format!("av_receipts_{namespace}_"),
        })
    }

    fn key(&self, id: &ReceiptId) -> String {
        format!("{}{}", self.prefix, id)
    }

    fn action_key(&self, action_id: &ActionId) -> String {
        format!("{}by_action_{}", self.prefix, action_id)
    }

    /// Store a receipt
    pub async fn store(&self, receipt: &Receipt) -> Result<(), WasmStorageError> {
        let key = self.key(&receipt.id);
        let json = serde_json::to_string(receipt).map_err(|e| WasmStorageError::Serialization(e.to_string()))?;
        
        self.storage.set_item(&key, &json).map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?;
        
        // Update action index
        let action_key = self.action_key(&receipt.action_id);
        let mut ids: Vec<String> = self.get_action_ids(&receipt.action_id).unwrap_or_default();
        if !ids.contains(&receipt.id.to_string()) {
            ids.push(receipt.id.to_string());
            let ids_json = serde_json::to_string(&ids).map_err(|e| WasmStorageError::Serialization(e.to_string()))?;
            self.storage.set_item(&action_key, &ids_json).map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?;
        }
        
        Ok(())
    }

    /// Get a receipt by ID
    pub async fn get(&self, id: &ReceiptId) -> Result<Option<Receipt>, WasmStorageError> {
        let key = self.key(id);
        match self.storage.get_item(&key) {
            Ok(Some(json)) => {
                let receipt = serde_json::from_str(&json).map_err(|e| WasmStorageError::Serialization(e.to_string()))?;
                Ok(Some(receipt))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(WasmStorageError::Storage(format!("{:?}", e))),
        }
    }

    /// List receipts by action ID
    pub async fn list_by_action(&self, action_id: &ActionId) -> Result<Vec<Receipt>, WasmStorageError> {
        let ids = self.get_action_ids(action_id)?;
        let mut receipts = Vec::new();

        for id_str in ids {
            let key = self.key_str(&id_str);
            if let Ok(Some(json)) = self.storage.get_item(&key) {
                if let Ok(receipt) = serde_json::from_str::<Receipt>(&json) {
                    receipts.push(receipt);
                }
            }
        }

        Ok(receipts)
    }

    fn key_str(&self, id: &str) -> String {
        format!("{}{}", self.prefix, id)
    }

    /// Check if a receipt exists
    pub async fn exists(&self, id: &ReceiptId) -> Result<bool, WasmStorageError> {
        let key = self.key(id);
        Ok(self.storage.get_item(&key).map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?.is_some())
    }

    fn get_action_ids(&self, action_id: &ActionId) -> Result<Vec<String>, WasmStorageError> {
        let action_key = self.action_key(action_id);
        match self.storage.get_item(&action_key) {
            Ok(Some(json)) => {
                let ids: Vec<String> = serde_json::from_str(&json).map_err(|e| WasmStorageError::Serialization(e.to_string()))?;
                Ok(ids)
            }
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(WasmStorageError::Storage(format!("{:?}", e))),
        }
    }
}

/// Claim result for idempotency
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    /// Successfully claimed this key
    Claimed,
    /// Another process has this key in-flight
    InFlight,
    /// This key was already completed
    Completed,
}

/// WASM-native idempotency store using browser localStorage
#[derive(Clone)]
pub struct WasmIdempotencyStore {
    storage: Storage,
    prefix: String,
}

impl WasmIdempotencyStore {
    /// Create a new WASM idempotency store
    pub fn new(namespace: &str) -> Result<Self, WasmStorageError> {
        let window = web_sys::window().ok_or_else(|| WasmStorageError::Storage("No window".to_string()))?;
        let storage = window.local_storage().map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?.ok_or_else(|| WasmStorageError::Storage("No local storage".to_string()))?;
        
        Ok(Self {
            storage,
            prefix: format!("av_idempotency_{namespace}_"),
        })
    }

    fn key(&self, k: &str) -> String {
        format!("{}{}", self.prefix, k)
    }

    /// Claim an idempotency key
    /// Returns (ClaimResult, previous_result_if_completed)
    pub async fn claim_or_check(&self, key: &str) -> Result<(ClaimResult, Option<VerificationResult>), WasmStorageError> {
        let storage_key = self.key(key);
        
        match self.storage.get_item(&storage_key) {
            Ok(Some(value)) => {
                // Key exists - check state
                if value == "in_flight" {
                    Ok((ClaimResult::InFlight, None))
                } else {
                    // It's a completed result
                    let result = match value.as_str() {
                        "verified" => VerificationResult::Verified,
                        "failed" => VerificationResult::Failed,
                        "unknown" => VerificationResult::Unknown,
                        "partial" => VerificationResult::Partial,
                        "duplicate" => VerificationResult::Duplicate,
                        _ => return Err(WasmStorageError::Storage(format!("Invalid state: {}", value))),
                    };
                    Ok((ClaimResult::Completed, Some(result)))
                }
            }
            Ok(None) => {
                // Key doesn't exist - claim it
                self.storage.set_item(&storage_key, "in_flight")
                    .map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?;
                Ok((ClaimResult::Claimed, None))
            }
            Err(e) => Err(WasmStorageError::Storage(format!("{:?}", e))),
        }
    }

    /// Mark a key as completed with result
    pub async fn complete(&self, key: &str, result: VerificationResult) -> Result<(), WasmStorageError> {
        let storage_key = self.key(key);
        let value = match result {
            VerificationResult::Verified => "verified",
            VerificationResult::Failed => "failed",
            VerificationResult::Unknown => "unknown",
            VerificationResult::Partial => "partial",
            VerificationResult::Duplicate => "duplicate",
        };
        
        self.storage.set_item(&storage_key, value)
            .map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?;
        Ok(())
    }

    /// Release a claimed key (e.g., on error)
    pub async fn release(&self, key: &str) -> Result<(), WasmStorageError> {
        let storage_key = self.key(key);
        self.storage.remove_item(&storage_key)
            .map_err(|e| WasmStorageError::Storage(format!("{:?}", e)))?;
        Ok(())
    }
}
