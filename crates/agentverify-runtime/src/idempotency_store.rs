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
            // The cache guard is held for the whole claim: checking, loading from
            // disk, and inserting must happen as one unit, otherwise two callers
            // can both observe a cache miss and both believe they own the key.
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);

            if let Some(entry) = cache.get(key) {
                return match &entry.state {
                    EntryState::InFlight => (ClaimResult::AlreadyClaimed, None),
                    EntryState::Completed(_) => (ClaimResult::AlreadyClaimed, entry.to_result()),
                };
            }

            // Load from disk
            if let Some(entry) = self.load_entry(key) {
                let result = entry.to_result();
                cache.insert(key.to_string(), entry);
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
                cache.insert(key.to_string(), entry);
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
    ///
    /// # Errors
    ///
    /// Returns [`deadpool_redis::CreatePoolError`] if the connection pool
    /// cannot be built from `url`.
    ///
    /// The signature is `async` for parity with [`Self::new`]'s async
    /// counterparts on other stores; pool creation itself is synchronous.
    #[allow(clippy::unused_async)]
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
                .arg(i64::try_from(ttl_secs).unwrap_or(i64::MAX))
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
                            .arg(i64::try_from(ttl_secs).unwrap_or(i64::MAX))
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
                .arg(i64::try_from(ttl_secs).unwrap_or(i64::MAX))
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

    /// Monotonic counter so keys generated inside a single test run never collide.
    static KEY_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn next_key(label: &str) -> String {
        use std::sync::atomic::Ordering;
        let seq = KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        format!("{label}-{nanos}-{seq}")
    }

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

    #[test]
    fn file_idempotency_store_roundtrips_every_verification_result() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let expected_results = [
            VerificationResult::Verified,
            VerificationResult::Failed,
            VerificationResult::Unknown,
            VerificationResult::Partial,
            VerificationResult::Duplicate,
        ];

        for expected in expected_results {
            // A fresh directory per result forces the reading store to load the
            // entry from disk rather than from the in-process cache, so the
            // persisted (string-encoded) form is what gets round-tripped.
            let dir = tempfile::tempdir().unwrap();
            let key = next_key("file-roundtrip");

            let writer = FileIdempotencyStore::new(dir.path()).unwrap();
            let (claim, existing) = rt.block_on(writer.claim_or_check(&key));
            assert_eq!(claim, ClaimResult::Claimed);
            assert_eq!(existing, None);
            rt.block_on(writer.complete(key.clone(), expected));

            let reader = FileIdempotencyStore::new(dir.path()).unwrap();
            let (claim, observed) = rt.block_on(reader.claim_or_check(&key));
            assert_eq!(claim, ClaimResult::AlreadyClaimed);
            assert_eq!(
                observed,
                Some(expected),
                "result {expected} did not round-trip"
            );
        }
    }

    #[test]
    fn file_idempotency_store_reports_in_flight_entries_without_a_result() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-in-flight");
        let rt = tokio::runtime::Runtime::new().unwrap();

        let claimer = FileIdempotencyStore::new(dir.path()).unwrap();
        let (claim, _) = rt.block_on(claimer.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::Claimed);

        // A second process-wide store instance sees the on-disk in-flight entry:
        // it is claimed, but no outcome is known yet.
        let second_instance = FileIdempotencyStore::new(dir.path()).unwrap();
        let (claim, observed) = rt.block_on(second_instance.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::AlreadyClaimed);
        assert_eq!(observed, None, "in-flight entries must not report a result");

        // Completing in the first instance persists the outcome.
        rt.block_on(claimer.complete(key.clone(), VerificationResult::Partial));

        // The claiming instance resolves the key from its own cache.
        let (claim, observed) = rt.block_on(claimer.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::AlreadyClaimed);
        assert_eq!(observed, Some(VerificationResult::Partial));

        // A store instance that has not yet cached the key reads the completed
        // outcome from disk.
        let third_instance = FileIdempotencyStore::new(dir.path()).unwrap();
        let (claim, observed) = rt.block_on(third_instance.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::AlreadyClaimed);
        assert_eq!(observed, Some(VerificationResult::Partial));
    }

    #[test]
    fn file_idempotency_store_ignores_outcomes_written_by_an_unknown_build() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-foreign-outcome");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = FileIdempotencyStore::new(dir.path()).unwrap();

        // An entry whose recorded outcome this build does not recognise, as
        // would be left behind by a different AgentVerify version sharing the
        // same idempotency directory.
        let entry_path = dir.path().join(format!(
            "{}.json",
            FileIdempotencyStore::key_to_filename(&key)
        ));
        let foreign = format!(
            "{{\"original_key\":\"{key}\",\"state\":{{\"Completed\":\"provisional\"}},\
             \"created_at\":\"2026-01-01T00:00:00+00:00\"}}"
        );
        std::fs::write(&entry_path, foreign).unwrap();

        // The key is claimed, but the unrecognised outcome must not be reported
        // as if it were a known verdict.
        let (claim, observed) = rt.block_on(store.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::AlreadyClaimed);
        assert_eq!(observed, None, "unrecognised outcomes must not be surfaced");
    }

    #[test]
    fn file_idempotency_store_survives_unreadable_entry_file() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-corrupt-json");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = FileIdempotencyStore::new(dir.path()).unwrap();

        let entry_path = dir.path().join(format!(
            "{}.json",
            FileIdempotencyStore::key_to_filename(&key)
        ));
        std::fs::write(&entry_path, "{\"state\": \"Completed\"").unwrap();

        // The malformed entry is unreadable, so the key looks unclaimed: the
        // store must claim it rather than report a stale outcome.
        let (claim, observed) = rt.block_on(store.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::Claimed);
        assert_eq!(observed, None);
    }

    #[test]
    fn file_idempotency_store_survives_undecodable_entry_file() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-corrupt-bytes");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = FileIdempotencyStore::new(dir.path()).unwrap();

        let entry_path = dir.path().join(format!(
            "{}.json",
            FileIdempotencyStore::key_to_filename(&key)
        ));
        // Invalid UTF-8: the entry cannot even be read as text.
        std::fs::write(&entry_path, [0xFF, 0xFE, 0x00, 0xC3]).unwrap();

        let (claim, observed) = rt.block_on(store.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::Claimed);
        assert_eq!(observed, None);
    }

    #[test]
    fn file_idempotency_store_warns_when_claims_cannot_be_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-unwritable");
        let rt = tokio::runtime::Runtime::new().unwrap();

        // The store is constructed while the directory exists, then the
        // directory is removed: persistence fails, but claiming and completing
        // must still succeed in-process so the caller is not stranded.
        let store = FileIdempotencyStore::new(dir.path()).unwrap();
        let orphaned_path = dir.path().to_path_buf();
        drop(dir);

        let (claim, observed) = rt.block_on(store.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::Claimed);
        assert_eq!(observed, None);
        assert!(
            !orphaned_path.exists(),
            "precondition: backing directory must be gone"
        );

        rt.block_on(store.complete(key.clone(), VerificationResult::Verified));

        // The in-process cache still answers authoritatively.
        let (claim, observed) = rt.block_on(store.claim_or_check(&key));
        assert_eq!(claim, ClaimResult::AlreadyClaimed);
        assert_eq!(observed, Some(VerificationResult::Verified));
    }

    #[test]
    fn file_idempotency_store_release_of_unknown_key_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-release-missing");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = FileIdempotencyStore::new(dir.path()).unwrap();

        // No entry was ever written for this key: release must be a no-op.
        rt.block_on(store.release(&key));
        assert!(dir.path().exists());
    }

    #[test]
    fn file_idempotency_store_release_warns_when_entry_cannot_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-release-blocked");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = FileIdempotencyStore::new(dir.path()).unwrap();

        // Replace the entry file with a directory: `remove_file` now fails with
        // something other than NotFound, which must not panic the caller.
        let entry_path = dir.path().join(format!(
            "{}.json",
            FileIdempotencyStore::key_to_filename(&key)
        ));
        std::fs::create_dir_all(&entry_path).unwrap();

        rt.block_on(store.release(&key));
    }

    #[test]
    fn file_idempotency_store_debug_includes_backing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileIdempotencyStore::new(dir.path()).unwrap();

        let rendered = format!("{store:?}");
        assert!(rendered.starts_with("FileIdempotencyStore"));
        assert!(rendered.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn file_idempotency_store_serializes_concurrent_claims_to_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let key = next_key("file-concurrent");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = std::sync::Arc::new(FileIdempotencyStore::new(dir.path()).unwrap());

        let attempts = 16;
        let mut handles = Vec::with_capacity(attempts);
        for _ in 0..attempts {
            let store = std::sync::Arc::clone(&store);
            let key = key.clone();
            handles.push(rt.spawn(async move { store.claim_or_check(&key).await }));
        }

        let mut claimed = 0;
        let mut already_claimed = 0;
        for handle in handles {
            let (claim, _) = rt.block_on(handle).unwrap();
            match claim {
                ClaimResult::Claimed => claimed += 1,
                ClaimResult::AlreadyClaimed => already_claimed += 1,
            }
        }

        assert_eq!(claimed, 1, "exactly one caller may claim a key");
        assert_eq!(already_claimed, attempts - 1);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
    mod redis_tests {
        use super::*;

        // =========================================================================
        // RedisIdempotencyStore — exercised against a live Redis when
        // AGENTVERIFY_TEST_REDIS_URL is set; skipped (with a notice) otherwise so
        // CI without services stays green.
        // =========================================================================

        /// Read the Redis endpoint used for integration coverage.
        ///
        /// Returns `None` when the endpoint is not configured so the caller can
        /// skip the test; the notice goes to stderr so it is visible in test output.
        #[allow(clippy::print_stderr)] // test-skip notices are not structured logs
        fn redis_url() -> Option<String> {
            match std::env::var("AGENTVERIFY_TEST_REDIS_URL") {
                Ok(url) if !url.trim().is_empty() => Some(url),
                _ => {
                    eprintln!(
                        "skipping Redis idempotency store test: \
                         AGENTVERIFY_TEST_REDIS_URL is not set"
                    );
                    None
                }
            }
        }

        async fn redis_store(url: &str, ttl_secs: u64) -> RedisIdempotencyStore {
            RedisIdempotencyStore::from_url(url, ttl_secs)
                .await
                .expect("redis pool builds from the test URL")
        }

        #[tokio::test]
        async fn redis_idempotency_store_claims_completes_and_replays() {
            let Some(url) = redis_url() else { return };
            let store = redis_store(&url, 300).await;
            let key = next_key("redis-basic");

            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::Claimed);
            assert_eq!(observed, None);

            // Re-claiming while in-flight reports no result.
            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::AlreadyClaimed);
            assert_eq!(observed, None);

            store
                .complete(key.clone(), VerificationResult::Verified)
                .await;

            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::AlreadyClaimed);
            assert_eq!(observed, Some(VerificationResult::Verified));

            store.release(&key).await;

            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(
                claim,
                ClaimResult::Claimed,
                "released key must be claimable"
            );
            assert_eq!(observed, None);
            store.release(&key).await;
        }

        #[tokio::test]
        async fn redis_idempotency_store_roundtrips_every_verification_result() {
            let Some(url) = redis_url() else { return };
            let store = redis_store(&url, 300).await;
            let expected_results = [
                VerificationResult::Verified,
                VerificationResult::Failed,
                VerificationResult::Unknown,
                VerificationResult::Partial,
                VerificationResult::Duplicate,
            ];

            for expected in expected_results {
                let key = next_key("redis-roundtrip");
                store.complete(key.clone(), expected).await;

                let (claim, observed) = store.claim_or_check(&key).await;
                assert_eq!(claim, ClaimResult::AlreadyClaimed);
                assert_eq!(
                    observed,
                    Some(expected),
                    "result {expected} did not round-trip"
                );

                store.release(&key).await;
            }
        }

        #[tokio::test]
        async fn redis_idempotency_store_release_removes_only_the_target_key() {
            let Some(url) = redis_url() else { return };
            let store = redis_store(&url, 300).await;
            let key = next_key("redis-release");
            let other = next_key("redis-release-neighbour");

            store
                .complete(key.clone(), VerificationResult::Failed)
                .await;
            store
                .complete(other.clone(), VerificationResult::Verified)
                .await;

            store.release(&key).await;

            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::Claimed);
            assert_eq!(observed, None);

            let (_, observed) = store.claim_or_check(&other).await;
            assert_eq!(observed, Some(VerificationResult::Verified));

            store.release(&key).await;
            store.release(&other).await;
        }

        #[tokio::test]
        async fn redis_idempotency_store_applies_ttl_to_entries() {
            let Some(url) = redis_url() else { return };
            let ttl_secs = 120u64;
            let store = redis_store(&url, ttl_secs).await;
            let key = next_key("redis-ttl");

            let (claim, _) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::Claimed);

            let mut conn = store.pool.get().await.expect("redis connection available");
            let ttl: i64 = redis::cmd("TTL")
                .arg(RedisIdempotencyStore::redis_key(&key))
                .query_async(&mut conn)
                .await
                .expect("TTL is answerable for a live key");
            assert!(
                ttl > 0 && ttl <= i64::try_from(ttl_secs).unwrap_or(i64::MAX),
                "expected a positive TTL within {ttl_secs}s, got {ttl}"
            );
            drop(conn);

            store.release(&key).await;

            let mut conn = store.pool.get().await.expect("redis connection available");
            let ttl: i64 = redis::cmd("TTL")
                .arg(RedisIdempotencyStore::redis_key(&key))
                .query_async(&mut conn)
                .await
                .expect("TTL query succeeds even for a missing key");
            assert_eq!(ttl, -2, "released key must be absent from Redis");
        }

        #[tokio::test]
        async fn redis_idempotency_store_persists_the_documented_entry_shape() {
            let Some(url) = redis_url() else { return };
            let store = redis_store(&url, 300).await;
            let key = next_key("redis-shape");

            let (claim, _) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::Claimed);

            let mut conn = store.pool.get().await.expect("redis connection available");
            let raw: String = redis::cmd("GET")
                .arg(RedisIdempotencyStore::redis_key(&key))
                .query_async(&mut conn)
                .await
                .expect("claimed key holds a value");
            drop(conn);

            let entry: RedisEntry = serde_json::from_str(&raw).expect("value is a RedisEntry");
            assert_eq!(entry.original_key, key);
            assert!(
                matches!(entry.state, RedisEntryState::InFlight),
                "freshly claimed entry must be in flight, got {:?}",
                entry.state
            );
            // created_at must be a real RFC 3339 timestamp, not a placeholder.
            let parsed_created_at = chrono::DateTime::parse_from_rfc3339(&entry.created_at);
            assert!(
                parsed_created_at.is_ok(),
                "created_at must be RFC 3339, got {:?}",
                entry.created_at
            );

            store.release(&key).await;
        }

        #[tokio::test]
        async fn redis_idempotency_store_treats_corrupted_entries_as_claimed_without_a_result() {
            let Some(url) = redis_url() else { return };
            let store = redis_store(&url, 300).await;
            let key = next_key("redis-corrupt");

            // Write a value that is not a valid idempotency entry.
            let mut conn = store.pool.get().await.expect("redis connection available");
            redis::cmd("SET")
                .arg(RedisIdempotencyStore::redis_key(&key))
                .arg("not-a-valid-entry")
                .query_async::<_, ()>(&mut conn)
                .await
                .expect("SET succeeds");
            drop(conn);

            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::AlreadyClaimed);
            assert_eq!(observed, None, "corrupt entries must not yield a result");

            store.release(&key).await;
        }

        #[tokio::test]
        async fn redis_idempotency_store_survives_a_key_holding_the_wrong_redis_type() {
            let Some(url) = redis_url() else { return };
            let store = redis_store(&url, 300).await;
            let key = next_key("redis-wrong-type");

            // The idempotency namespace is shared with whatever else uses this
            // Redis. A key that holds a list instead of an entry makes SETNX report
            // the key as taken and makes the follow-up GET fail with WRONGTYPE, so
            // the store falls into its retry path.
            let mut conn = store.pool.get().await.expect("redis connection available");
            redis::cmd("RPUSH")
                .arg(RedisIdempotencyStore::redis_key(&key))
                .arg("not")
                .arg("an")
                .arg("entry")
                .query_async::<_, ()>(&mut conn)
                .await
                .expect("RPUSH succeeds");
            drop(conn);

            // Both the retried SET NX and the fallback agree: the key is taken and
            // no outcome can be read from it.
            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::AlreadyClaimed);
            assert_eq!(observed, None, "a foreign key must not yield a result");

            store.release(&key).await;

            // After release the key is gone and can be claimed normally.
            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::Claimed);
            assert_eq!(observed, None);
            store.release(&key).await;
        }

        #[tokio::test]
        async fn redis_idempotency_store_claims_when_redis_is_unreachable() {
            // A pool pointed at a closed port: no connection can be established
            // within the configured window, so every operation must fail closed
            // without panicking or blocking indefinitely.
            let mut config = deadpool_redis::Config::from_url("redis://127.0.0.1:1/");
            config.pool = Some(deadpool_redis::PoolConfig {
                timeouts: deadpool_redis::Timeouts {
                    create: Some(std::time::Duration::from_millis(250)),
                    wait: Some(std::time::Duration::from_millis(250)),
                    recycle: Some(std::time::Duration::from_millis(250)),
                },
                ..deadpool_redis::PoolConfig::default()
            });
            let pool = config
                .create_pool(Some(deadpool_redis::Runtime::Tokio1))
                .expect("pool builds even for an unreachable server");
            let store = RedisIdempotencyStore::new(pool, 300);
            let key = next_key("redis-unreachable");

            // Fail-closed: the caller is told it claimed the key so it will attempt
            // dispatch and reconcile afterwards.
            let (claim, observed) = store.claim_or_check(&key).await;
            assert_eq!(claim, ClaimResult::Claimed);
            assert_eq!(observed, None);

            store
                .complete(key.clone(), VerificationResult::Failed)
                .await;
            store.release(&key).await;
        }

        #[test]
        fn redis_idempotency_store_keys_are_namespaced_and_debuggable() {
            let pool = deadpool_redis::Config::from_url("redis://127.0.0.1:6379/")
                .create_pool(Some(deadpool_redis::Runtime::Tokio1))
                .expect("pool builds from a well-formed URL");
            let store = RedisIdempotencyStore::new(pool, 60);

            assert_eq!(
                RedisIdempotencyStore::redis_key("order/42"),
                "idempotency:order/42"
            );

            let rendered = format!("{store:?}");
            assert!(
                rendered.starts_with("RedisIdempotencyStore"),
                "got {rendered}"
            );
            assert!(rendered.contains("ttl_secs: 60"), "got {rendered}");
        }

        #[test]
        fn redis_entry_converts_known_and_unknown_result_strings() {
            for (stored, expected) in [
                ("verified", Some(VerificationResult::Verified)),
                ("failed", Some(VerificationResult::Failed)),
                ("unknown", Some(VerificationResult::Unknown)),
                ("partial", Some(VerificationResult::Partial)),
                ("duplicate", Some(VerificationResult::Duplicate)),
            ] {
                let entry = RedisEntry {
                    original_key: "k".to_string(),
                    state: RedisEntryState::Completed(stored.to_string()),
                    created_at: chrono::Utc::now().to_rfc3339(),
                };
                assert_eq!(entry.to_result(), expected, "stored value {stored:?}");
            }

            // An outcome written by a newer/older build must degrade to "no result"
            // rather than be misread as a known outcome.
            let entry = RedisEntry {
                original_key: "k".to_string(),
                state: RedisEntryState::Completed("provisional".to_string()),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            assert_eq!(entry.to_result(), None);

            let in_flight = RedisEntry::new_in_flight("k".to_string());
            assert_eq!(in_flight.to_result(), None);
            assert_eq!(in_flight.original_key, "k");

            let completed = RedisEntry::new_completed("k".to_string(), VerificationResult::Unknown);
            assert_eq!(completed.to_result(), Some(VerificationResult::Unknown));
            assert!(
                matches!(&completed.state, RedisEntryState::Completed(stored) if stored == "unknown"),
                "expected a completed entry, got {:?}",
                completed.state
            );
            assert!(format!("{completed:?}").contains("unknown"));
        }
    }
}
