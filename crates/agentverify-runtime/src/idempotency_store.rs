//! Idempotency stores for tracking executed actions
//!
//! This module provides multiple implementations of the `IdempotencyStore` trait:
//! - `FileIdempotencyStore`: File-based persistence for single-machine deployments
//! - `RedisIdempotencyStore`: Distributed Redis-based store for multi-instance deployments
//!
//! # Storage Format (Redis)
//! - Key format: `idempotency:{key}`
//! - Value: JSON with `state`, `result`, and `created_at` fields
//! - TTL: Configurable, defaults to 24 hours

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
use std::sync::{Mutex, PoisonError};

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
/// For production distributed use, implement `IdempotencyStore` with Redis or `PostgreSQL`.
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
                let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
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
            if let Some(entry) = self.load_entry(key) {
                let result = entry.to_result();
                // Update cache
                {
                    let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
                    cache.insert(key.to_string(), entry);
                }
                match result {
                    None => (ClaimResult::AlreadyClaimed, None),
                    Some(r) => (ClaimResult::AlreadyClaimed, Some(r)),
                }
            } else {
                // Key doesn't exist — claim it
                let entry = PersistedEntry::new_in_flight(key.to_string());
                if let Err(e) = self.save_entry(&entry) {
                    tracing::warn!("failed to persist claim: {e}");
                }
                {
                    let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
                    cache.insert(key.to_string(), entry);
                }
                (ClaimResult::Claimed, None)
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
                tracing::warn!("failed to persist completion: {e}");
            }
            {
                let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
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
                    tracing::warn!("failed to remove entry: {e}");
                }
            }
            // Remove from in-process cache
            let mut cache_guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
            cache_guard.remove(&key_str);
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for FileIdempotencyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileIdempotencyStore")
            .field("base_path", &self.base_path)
            .finish_non_exhaustive()
    }
}

// =============================================================================
// Redis Idempotency Store
// =============================================================================

/// Redis key prefix for idempotency entries
#[cfg(feature = "redis")]
const REDIS_KEY_PREFIX: &str = "idempotency:";

/// Redis-based distributed idempotency store
///
/// Uses Redis SETNX (SET if Not eXists) for atomic claim semantics and supports
/// configurable TTL for automatic expiration of stale entries.
///
/// # Storage Format
/// - Key: `idempotency:{key}`
/// - Value: JSON `{"state":"InFlight"|"Completed","result":"verified"|"failed"|...,"created_at":"..."}`
///
/// # Atomic Semantics
/// - `claim_or_check` uses SETNX for atomic claim
/// - `complete` uses SET to update with result
/// - `release` uses DEL to remove entry
///
/// # TTL
/// Default TTL is 24 hours. Entries are automatically expired to prevent
/// unbounded growth and allow retry of genuinely failed actions.
#[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
pub struct RedisIdempotencyStore {
    pool: deadpool_redis::Pool,
    /// Default TTL in seconds
    ttl_secs: u64,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
impl RedisIdempotencyStore {
    /// Create a new Redis idempotency store
    ///
    /// # Arguments
    /// * `pool` - deadpool-redis connection pool
    /// * `ttl_secs` - Default TTL for entries in seconds (default: 86400 = 24 hours)
    #[must_use]
    pub fn new(pool: deadpool_redis::Pool, ttl_secs: u64) -> Self {
        Self { pool, ttl_secs }
    }

    /// Create from a Redis URL string
    ///
    /// # Arguments
    /// * `url` - Redis connection URL (e.g., `redis://127.0.0.1:6379`)
    /// * `ttl_secs` - Default TTL for entries in seconds
    pub async fn from_url(
        url: &str,
        ttl_secs: u64,
    ) -> Result<Self, deadpool_redis::CreatePoolError> {
        let pool = deadpool_redis::Config::from_url(url)
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))?;
        Ok(Self::new(pool, ttl_secs))
    }

    fn redis_key(key: &str) -> String {
        format!("{REDIS_KEY_PREFIX}{key}")
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
impl IdempotencyStore for RedisIdempotencyStore {
    fn claim_or_check<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = (ClaimResult, Option<VerificationResult>)> + Send + 'a>> {
        let redis_key = Self::redis_key(key);
        let ttl_secs = self.ttl_secs;
        let pool = self.pool.clone();
        Box::pin(async move {
            let mut conn = match pool.get().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!("failed to get Redis connection: {e}");
                    return (ClaimResult::Claimed, None);
                }
            };

            // Try to claim with SETNX (SET if Not eXists)
            let entry = RedisEntry::new_in_flight(key.to_string());
            let value = match serde_json::to_string(&entry) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("failed to serialize entry: {e}");
                    return (ClaimResult::Claimed, None);
                }
            };

            // SETNX with TTL - atomic claim operation
            let setnx_result: Result<bool, _> = redis::cmd("SET")
                .arg(&redis_key)
                .arg(&value)
                .arg("NX")
                .arg("EX")
                .arg(ttl_secs as i64)
                .query_async(&mut conn)
                .await;

            match setnx_result {
                Ok(true) => {
                    // We successfully claimed the key
                    (ClaimResult::Claimed, None)
                }
                Ok(false) => {
                    // Key already exists - check its state
                    let get_result: Option<String> = redis::cmd("GET")
                        .arg(&redis_key)
                        .query_async(&mut conn)
                        .await
                        .ok();

                    if let Some(raw) = get_result {
                        match serde_json::from_str::<RedisEntry>(&raw) {
                            Ok(entry) => {
                                let result = entry.to_result();
                                (ClaimResult::AlreadyClaimed, result)
                            }
                            Err(_) => {
                                // Corrupted entry - treat as already claimed
                                (ClaimResult::AlreadyClaimed, None)
                            }
                        }
                    } else {
                        // Race condition: entry expired between SETNX and GET
                        // Try again (recursive retry once)
                        let entry = RedisEntry::new_in_flight(key.to_string());
                        let value = serde_json::to_string(&entry).unwrap_or_default();
                        let retry_result: Result<bool, _> = redis::cmd("SET")
                            .arg(&redis_key)
                            .arg(&value)
                            .arg("NX")
                            .arg("EX")
                            .arg(ttl_secs as i64)
                            .query_async(&mut conn)
                            .await;
                        match retry_result {
                            Ok(true) => (ClaimResult::Claimed, None),
                            Ok(false) => (ClaimResult::AlreadyClaimed, None),
                            Err(e) => {
                                tracing::warn!("Redis retry failed: {e}");
                                (ClaimResult::AlreadyClaimed, None)
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Redis SETNX failed: {e}");
                    (ClaimResult::AlreadyClaimed, None)
                }
            }
        })
    }

    fn complete(
        &self,
        key: String,
        result: VerificationResult,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let redis_key = Self::redis_key(&key);
        let ttl_secs = self.ttl_secs;
        let pool = self.pool.clone();
        Box::pin(async move {
            let mut conn = match pool.get().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!("failed to get Redis connection: {e}");
                    return;
                }
            };

            let entry = RedisEntry::new_completed(key.clone(), result);
            let value = match serde_json::to_string(&entry) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("failed to serialize completion: {e}");
                    return;
                }
            };

            // SET with TTL - update existing entry
            let _: Result<(), _> = redis::cmd("SET")
                .arg(&redis_key)
                .arg(&value)
                .arg("EX")
                .arg(ttl_secs as i64)
                .query_async(&mut conn)
                .await;
        })
    }

    fn release(&self, key: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let redis_key = Self::redis_key(key);
        let pool = self.pool.clone();
        Box::pin(async move {
            if let Ok(mut conn) = pool.get().await {
                if let Err(e) = redis::cmd("DEL")
                    .arg(&redis_key)
                    .query_async::<_, ()>(&mut conn)
                    .await
                {
                    tracing::warn!("Redis DEL failed: {e}");
                }
            }
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
impl std::fmt::Debug for RedisIdempotencyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisIdempotencyStore")
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

/// Entry stored in Redis
#[cfg(feature = "redis")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedisEntry {
    original_key: String,
    state: RedisEntryState,
    created_at: String,
}

#[cfg(feature = "redis")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RedisEntryState {
    InFlight,
    Completed(String),
}

#[cfg(feature = "redis")]
impl RedisEntry {
    fn new_in_flight(key: String) -> Self {
        Self {
            original_key: key,
            state: RedisEntryState::InFlight,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn new_completed(key: String, result: VerificationResult) -> Self {
        Self {
            original_key: key,
            state: RedisEntryState::Completed(result.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn to_result(&self) -> Option<VerificationResult> {
        match &self.state {
            RedisEntryState::InFlight => None,
            RedisEntryState::Completed(s) => match s.as_str() {
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
