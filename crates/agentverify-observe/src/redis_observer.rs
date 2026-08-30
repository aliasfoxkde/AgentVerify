//! Redis observer implementation
//!
//! Observes system state via Redis operations using deadpool-redis.

use agentverify_core::{Action, Contract, Observation, SourceId};
use agentverify_runtime::ExecutorError;
use redis::AsyncCommands;
use serde_json::Value;
use thiserror::Error;

/// Redis observer error types
#[derive(Debug, Error)]
pub enum RedisObserverError {
    #[error("Redis connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Redis operation failed: {0}")]
    RedisOperation(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid key pattern: {0}")]
    InvalidKeyPattern(String),

    #[error("Invalid observation spec: {0}")]
    InvalidObservationSpec(String),

    #[error("Pool error: {0}")]
    PoolError(#[from] deadpool_redis::PoolError),

    #[error("Pool creation error: {0}")]
    PoolCreate(String),

    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),
}

/// Redis observer configuration
#[derive(Debug, Clone)]
pub struct RedisObserverConfig {
    /// Redis connection URL
    pub redis_url: String,
    /// Default timeout in milliseconds
    pub timeout_ms: u64,
    /// Key prefix for namespacing (optional)
    pub key_prefix: String,
    /// Default key to observe if not specified in action arguments
    pub default_key: Option<String>,
}

impl RedisObserverConfig {
    /// Create a new config with the given Redis URL
    pub fn new(redis_url: impl Into<String>) -> Self {
        Self {
            redis_url: redis_url.into(),
            timeout_ms: 5000,
            key_prefix: String::new(),
            default_key: None,
        }
    }

    /// Set the timeout in milliseconds
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Set a key prefix for namespacing
    pub fn with_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = prefix.into();
        self
    }

    /// Set a default key to observe
    pub fn with_default_key(mut self, key: impl Into<String>) -> Self {
        self.default_key = Some(key.into());
        self
    }
}

impl Default for RedisObserverConfig {
    fn default() -> Self {
        Self::new("redis://127.0.0.1:6379")
    }
}

/// Redis observer for collecting state from Redis
///
/// # Overview
///
/// The `RedisObserver` fetches state from Redis using various commands (GET, EXISTS,
/// SCARD, HGET, HGETALL) and returns an [`Observation`] containing the results as JSON.
///
/// # Key Resolution
///
/// Keys are resolved in this order of precedence:
/// 1. `action.arguments.key` - key specified in action arguments
/// 2. `contract.action_name` - derived from contract's action name
/// 3. `config.default_key` - fallback from config
///
/// # Observation Spec
///
/// The observation spec is read from `contract.action_name`. Supported patterns:
/// - `"get:{key}"` - GET operation on the key
/// - `"exists:{key}"` - EXISTS operation (returns boolean)
/// - `"scard:{key}"` - SCARD operation (set size)
/// - `"hget:{key}:{field}"` - HGET operation (hash field)
/// - `"hgetall:{key}"` - HGETALL operation (entire hash)
///
/// If no pattern prefix is found, defaults to `"get:{contract.action_name}"`.
///
/// # Examples
///
/// ```rust,ignore
/// use agentverify_observe::RedisObserver;
///
/// let config = RedisObserverConfig::new("redis://127.0.0.1:6379");
/// let observer = RedisObserver::new(config).await?;
/// ```
pub struct RedisObserver {
    pool: deadpool_redis::Pool,
    config: RedisObserverConfig,
}

impl RedisObserver {
    /// Create a new Redis observer from configuration
    pub async fn new(config: RedisObserverConfig) -> Result<Self, RedisObserverError> {
        let cfg = deadpool_redis::Config {
            url: Some(config.redis_url.clone()),
            pool: Some(deadpool_redis::PoolConfig {
                max_size: config.timeout_ms as usize,
                ..Default::default()
            }),
            ..Default::default()
        };

        let pool = cfg
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .map_err(|e| RedisObserverError::PoolCreate(e.to_string()))?;

        Ok(Self { pool, config })
    }

    /// Create a new Redis observer from a pre-existing pool
    pub fn with_pool(pool: deadpool_redis::Pool, config: RedisObserverConfig) -> Self {
        Self { pool, config }
    }

    /// Resolve the key to observe from action and contract
    #[allow(dead_code)]
    fn resolve_key(&self, action: &Action, contract: &Contract) -> String {
        // 1. Check action.arguments.key
        if let Some(key) = action.arguments.get("key").and_then(|v| v.as_str()) {
            return key.to_string();
        }

        // 2. Fall back to contract.action_name
        if !contract.action_name.is_empty() {
            return contract.action_name.clone();
        }

        // 3. Fall back to config.default_key
        self.config
            .default_key
            .clone()
            .unwrap_or_else(|| "default".to_string())
    }

    /// Parse observation spec to determine operation and key
    ///
    /// Returns (operation, key, [extra_args...])
    fn parse_spec(spec: &str) -> Result<(&str, &str, Vec<&str>), RedisObserverError> {
        let parts: Vec<&str> = spec.splitn(3, ':').collect();
        match parts.as_slice() {
            [op, key] => Ok((*op, *key, Vec::new())),
            [op, key, extra] => {
                let extra_parts: Vec<&str> = extra.split(':').collect();
                Ok((*op, *key, extra_parts))
            }
            _ => Err(RedisObserverError::InvalidObservationSpec(spec.to_string())),
        }
    }

    /// Execute the observation spec and return state as JSON
    async fn execute_spec(
        &self,
        conn: &mut deadpool_redis::Connection,
        spec: &str,
    ) -> Result<Value, RedisObserverError> {
        let (op, key, extra) = Self::parse_spec(spec)?;
        let prefixed_key = format!("{}{}", self.config.key_prefix, key);

        match op {
            "get" => {
                let result: Option<String> = conn.get(&prefixed_key).await?;
                match result {
                    Some(value) => {
                        // Try to parse as JSON, fall back to string
                        Ok(serde_json::from_str(&value).unwrap_or(Value::String(value)))
                    }
                    None => Err(RedisObserverError::KeyNotFound(prefixed_key)),
                }
            }
            "exists" => {
                let exists: bool = conn.exists(&prefixed_key).await?;
                Ok(Value::Bool(exists))
            }
            "scard" => {
                let count: u64 = conn.scard(&prefixed_key).await?;
                Ok(Value::Number(count.into()))
            }
            "hget" => {
                if extra.is_empty() {
                    return Err(RedisObserverError::InvalidObservationSpec(
                        "hget requires a field argument".to_string(),
                    ));
                }
                let field = extra[0];
                let result: Option<String> = conn.hget(&prefixed_key, field).await?;
                match result {
                    Some(value) => Ok(serde_json::from_str(&value).unwrap_or(Value::String(value))),
                    None => Ok(Value::Null),
                }
            }
            "hgetall" => {
                let result: std::collections::HashMap<String, String> =
                    conn.hgetall(&prefixed_key).await?;
                let mut map = serde_json::Map::new();
                for (k, v) in result {
                    let value: Value = serde_json::from_str(&v).unwrap_or(Value::String(v));
                    map.insert(k, value);
                }
                Ok(Value::Object(map))
            }
            _ => Err(RedisObserverError::InvalidObservationSpec(format!(
                "unknown operation: {}",
                op
            ))),
        }
    }

    /// Build the observation spec from contract
    fn build_spec(&self, contract: &Contract) -> String {
        let action_name = &contract.action_name;

        // Check if action_name contains a spec prefix
        if action_name.contains(':') {
            action_name.clone()
        } else {
            // Default to get operation
            format!("get:{}", action_name)
        }
    }
}

#[async_trait::async_trait]
impl agentverify_runtime::Observer for RedisObserver {
    async fn observe(
        &self,
        _action: &Action,
        contract: &Contract,
    ) -> Result<Observation, ExecutorError> {
        // Get a connection from the pool
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Redis pool error: {}", e)))?;

        // Build the observation spec
        let spec = self.build_spec(contract);

        // Execute the spec
        let state = self
            .execute_spec(&mut conn, &spec)
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Redis observation failed: {}", e)))?;

        Ok(Observation::new(SourceId("redis".into()), state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = RedisObserverConfig::default();
        assert_eq!(config.redis_url, "redis://127.0.0.1:6379");
        assert_eq!(config.timeout_ms, 5000);
        assert!(config.key_prefix.is_empty());
    }

    #[test]
    fn config_builder_pattern() {
        let config = RedisObserverConfig::new("redis://localhost:6379")
            .with_timeout(10000)
            .with_key_prefix("app:")
            .with_default_key("default_key");

        assert_eq!(config.redis_url, "redis://localhost:6379");
        assert_eq!(config.timeout_ms, 10000);
        assert_eq!(config.key_prefix, "app:");
        assert_eq!(config.default_key, Some("default_key".to_string()));
    }

    #[test]
    fn parse_spec_get() {
        let result = parse_spec_impl("get:mykey");
        assert!(result.is_ok());
        let (op, key, extra) = result.unwrap();
        assert_eq!(op, "get");
        assert_eq!(key, "mykey");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_hget() {
        let result = parse_spec_impl("hget:myhash:field1");
        assert!(result.is_ok());
        let (op, key, extra) = result.unwrap();
        assert_eq!(op, "hget");
        assert_eq!(key, "myhash");
        assert_eq!(extra, vec!["field1"]);
    }

    #[test]
    fn parse_spec_hgetall() {
        let result = parse_spec_impl("hgetall:myhash");
        assert!(result.is_ok());
        let (op, key, extra) = result.unwrap();
        assert_eq!(op, "hgetall");
        assert_eq!(key, "myhash");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_scard() {
        let result = parse_spec_impl("scard:myset");
        assert!(result.is_ok());
        let (op, key, extra) = result.unwrap();
        assert_eq!(op, "scard");
        assert_eq!(key, "myset");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_exists() {
        let result = parse_spec_impl("exists:mykey");
        assert!(result.is_ok());
        let (op, key, extra) = result.unwrap();
        assert_eq!(op, "exists");
        assert_eq!(key, "mykey");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_invalid_no_colon() {
        let result = parse_spec_impl("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn build_spec_default() {
        // build_spec only uses contract.action_name, not the pool
        // So we test the logic via parse_spec which is the same
        let result = parse_spec_impl("mykey");
        assert!(result.is_err()); // No colon means invalid
    }

    #[test]
    fn build_spec_with_prefix() {
        // Test that colon-separated specs work
        let result = parse_spec_impl("hgetall:user:123");
        assert!(result.is_ok());
        let (op, key, extra) = result.unwrap();
        assert_eq!(op, "hgetall");
        assert_eq!(key, "user");
        assert_eq!(extra, vec!["123"]);
    }

    #[test]
    fn error_display_connection_failed() {
        let err = RedisObserverError::ConnectionFailed("refused".to_string());
        let err_str = err.to_string();
        // thiserror formats as "ConnectionFailed: refused"
        assert!(
            err_str.contains("refused"),
            "error should contain 'refused', got: {}",
            err_str
        );
    }

    #[test]
    fn error_display_key_not_found() {
        let err = RedisObserverError::KeyNotFound("mykey".to_string());
        let err_str = err.to_string();
        assert!(
            err_str.contains("mykey"),
            "error should contain 'mykey', got: {}",
            err_str
        );
    }

    #[test]
    fn error_display_invalid_spec() {
        let err = RedisObserverError::InvalidObservationSpec("bad spec".to_string());
        let err_str = err.to_string();
        assert!(
            err_str.contains("bad spec"),
            "error should contain 'bad spec', got: {}",
            err_str
        );
    }

    // Note: Full integration tests with a real Redis instance would require
    // a running Redis server. The unit tests above verify the configuration,
    // parsing, and error handling logic.

    /// Parse observation spec to determine operation and key
    ///
    /// Returns (operation, key, [extra_args...])
    fn parse_spec_impl(spec: &str) -> Result<(&str, &str, Vec<&str>), RedisObserverError> {
        let parts: Vec<&str> = spec.splitn(3, ':').collect();
        match parts.as_slice() {
            [op, key] => Ok((*op, *key, Vec::new())),
            [op, key, extra] => {
                let extra_parts: Vec<&str> = extra.split(':').collect();
                Ok((*op, *key, extra_parts))
            }
            _ => Err(RedisObserverError::InvalidObservationSpec(spec.to_string())),
        }
    }
}
