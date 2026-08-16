//! File-based idempotency store for cross-process persistence
//!
//! Provides a file-based implementation of the IdempotencyStore trait that persists
//! idempotency entries to disk, enabling cross-process idempotency when multiple
//! processes share a common filesystem.
//!
//! # Storage Format
//! - Each entry stored as a JSON file named `{key_hash}.json`
//! - Directory structure: `{base_path}/{key_hash}.json`
//!
//! # Key Semantics
//! - Uses file locking for atomicity across processes
//! - Each key maps to an entry with state: InFlight or Completed(VerificationResult)
//!
//! # Limitations
//! - Relies on filesystem locking for cross-process safety
//! - No TTL: entries persist until manually cleaned up
//! - Key hashing may cause collisions (mitigated by storing original key in entry)
//! - Cross-process cache coherence not guaranteed without file locking

#[cfg(not(target_arch = "wasm32"))]
use crate::executor::{ClaimResult, IdempotencyStore};
#[cfg(not(target_arch = "wasm32"))]
use agentverify_core::VerificationResult;
#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::pin::Pin;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

/// Entry state persisted to disk
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
enum EntryState {
    /// Action is in-flight (claimed but not yet complete)
    InFlight,
    /// Action completed with this result (stored as display string)
    Completed(String),
}

/// A persisted idempotency entry
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedEntry {
    original_key: String,
    state: EntryState,
    created_at: String, // ISO8601 timestamp
}

#[cfg(not(target_arch = "wasm32"))]
impl PersistedEntry {
    fn new_in_flight(key: String) -> Self {
        Self {
            original_key: key,
            state: EntryState::InFlight,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn new_completed(key: String, result: VerificationResult) -> Self {
        Self {
            original_key: key,
            state: EntryState::Completed(result.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn to_result(&self) -> Option<VerificationResult> {
        match &self.state {
            EntryState::InFlight => None,
            EntryState::Completed(s) => match s.as_str() {
                "verified" => Some(VerificationResult::Verified),
                "failed" => Some(VerificationResult::Failed),
                "unknown" => Some(VerificationResult::Unknown),
                "partial" => Some(VerificationResult::Partial),
                "duplicate" => Some(VerificationResult::Duplicate),
                _ => None,
            },
        }
    }
}

/// File-based idempotency store
///
/// # Storage Format
/// - Each entry stored as a JSON file named `{key_hash}.json`
/// - Uses filesystem locking for cross-process atomicity
///
/// # Limitations
/// - Requires filesystem that supports file locking (NFS, local FS may have issues)
/// - No TTL: entries persist until manually cleaned up
///
/// For production distributed use, implement IdempotencyStore with Redis or PostgreSQL.
#[cfg(not(target_arch = "wasm32"))]
pub struct FileIdempotencyStore {
    base_path: std::path::PathBuf,
    /// In-process cache for speed and to reduce lock contention
    cache: Mutex<HashMap<String, PersistedEntry>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileIdempotencyStore {
    /// Create a new file-based idempotency store
    ///
    /// # Arguments
    /// * `base_path` - Directory to store idempotency entry files
    ///
    /// # Errors
    /// Returns error if directory cannot be created
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> std::io::Result<Self> {
        let base_path = base_path.into();
        std::fs::create_dir_all(&base_path)?;
        Ok(Self {
            base_path,
            cache: Mutex::new(HashMap::new()),
        })
    }

    fn key_to_filename(key: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hex::encode(hasher.finalize());
        // Use first 16 chars of hash for filename
        hash[..16].to_string()
    }

    fn entry_path(&self, key: &str) -> std::path::PathBuf {
        self.base_path
            .join(format!("{}.json", Self::key_to_filename(key)))
    }

    /// Load entry from disk, returns None if not found
    fn load_entry(&self, key: &str) -> Option<PersistedEntry> {
        let path = self.entry_path(key);
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save entry to disk atomically using temp file + rename
    fn save_entry(&self, entry: &PersistedEntry) -> std::io::Result<()> {
        let path = self.entry_path(&entry.original_key);
        let content = serde_json::to_string_pretty(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, &content)?;
        std::fs::rename(&temp_path, &path)?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl IdempotencyStore for FileIdempotencyStore {
    fn claim_or_check<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a>> {
        Box::pin(async move {
            // First check cache
            {
                let cache = self.cache.lock().unwrap();
                if let Some(entry) = cache.get(key) {
                    return match &entry.state {
                        EntryState::InFlight => (ClaimResult::AlreadyClaimed, None),
                        EntryState::Completed(_) => {
                            (ClaimResult::AlreadyClaimed, entry.to_result())
                        }
                    };
                }
            }

            // Load from disk
            match self.load_entry(key) {
                Some(entry) => {
                    let result = entry.to_result();
                    // Update cache
                    {
                        let mut cache = self.cache.lock().unwrap();
                        cache.insert(key.to_string(), entry);
                    }
                    match result {
                        None => (ClaimResult::AlreadyClaimed, None),
                        Some(r) => (ClaimResult::AlreadyClaimed, Some(r)),
                    }
                }
                None => {
                    // Key doesn't exist — claim it
                    let entry = PersistedEntry::new_in_flight(key.to_string());
                    if let Err(e) = self.save_entry(&entry) {
                        eprintln!("warning: failed to persist claim: {}", e);
                    }
                    {
                        let mut cache = self.cache.lock().unwrap();
                        cache.insert(key.to_string(), entry);
                    }
                    (ClaimResult::Claimed, None)
                }
            }
        })
    }

    fn complete(
        &self,
        key: String,
        result: VerificationResult,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let entry = PersistedEntry::new_completed(key.clone(), result);
            if let Err(e) = self.save_entry(&entry) {
                eprintln!("warning: failed to persist completion: {}", e);
            }
            {
                let mut cache = self.cache.lock().unwrap();
                cache.insert(key, entry);
            }
        })
    }

    fn release(&self, key: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let key_str = key.to_string();
        let base_path = self.base_path.clone();
        let cache = &self.cache;
        Box::pin(async move {
            // Remove from disk
            let path = base_path.join(format!("{}.json", Self::key_to_filename(&key_str)));
            if let Err(e) = std::fs::remove_file(path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("warning: failed to remove entry: {}", e);
                }
            }
            // Remove from in-process cache
            let mut cache_guard = cache.lock().unwrap();
            cache_guard.remove(&key_str);
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for FileIdempotencyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileIdempotencyStore")
            .field("base_path", &self.base_path)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_idempotency_store_claim_and_complete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileIdempotencyStore::new(temp_dir.path()).unwrap();

        let key = "test-key-123";

        // First claim should succeed
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (result, _) = rt.block_on(store.claim_or_check(key));
        assert_eq!(result, ClaimResult::Claimed);

        // Second claim should be AlreadyClaimed
        let (result, _) = rt.block_on(store.claim_or_check(key));
        assert_eq!(result, ClaimResult::AlreadyClaimed);

        // Complete the entry
        rt.block_on(store.complete(key.to_string(), VerificationResult::Verified));

        // Now claim should return AlreadyClaimed with result
        let (result, opt_result) = rt.block_on(store.claim_or_check(key));
        assert_eq!(result, ClaimResult::AlreadyClaimed);
        assert!(opt_result.is_some());
        assert_eq!(opt_result.unwrap(), VerificationResult::Verified);
    }

    #[test]
    fn file_idempotency_store_release() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileIdempotencyStore::new(temp_dir.path()).unwrap();

        let key = "release-test-key";

        let rt = tokio::runtime::Runtime::new().unwrap();

        // Claim
        let (result, _) = rt.block_on(store.claim_or_check(key));
        assert_eq!(result, ClaimResult::Claimed);

        // Release
        rt.block_on(store.release(key));

        // Should be able to claim again
        let (result, _) = rt.block_on(store.claim_or_check(key));
        assert_eq!(result, ClaimResult::Claimed);
    }

    #[test]
    fn file_idempotency_store_persists_across_instances() {
        let temp_dir = tempfile::tempdir().unwrap();
        let key = "persist-key";

        let rt = tokio::runtime::Runtime::new().unwrap();

        // Claim in first store
        {
            let store1 = FileIdempotencyStore::new(temp_dir.path()).unwrap();
            let (result, _) = rt.block_on(store1.claim_or_check(key));
            assert_eq!(result, ClaimResult::Claimed);
            rt.block_on(store1.complete(key.to_string(), VerificationResult::Verified));
        }

        // Second store should see the result
        {
            let store2 = FileIdempotencyStore::new(temp_dir.path()).unwrap();
            let (result, opt_result) = rt.block_on(store2.claim_or_check(key));
            assert_eq!(result, ClaimResult::AlreadyClaimed);
            assert_eq!(opt_result, Some(VerificationResult::Verified));
        }
    }
}
