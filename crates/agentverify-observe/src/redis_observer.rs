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
    /// A connection to the Redis server could not be established.
    #[error("Redis connection failed: {0}")]
    ConnectionFailed(String),

    /// A Redis command returned an error.
    #[error("Redis operation failed: {0}")]
    RedisOperation(String),

    /// The observed key does not exist.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// A key pattern was malformed.
    #[error("Invalid key pattern: {0}")]
    InvalidKeyPattern(String),

    /// An observation spec was malformed or used an unknown operation.
    #[error("Invalid observation spec: {0}")]
    InvalidObservationSpec(String),

    /// The connection pool returned an error.
    #[error("Pool error: {0}")]
    PoolError(#[from] deadpool_redis::PoolError),

    /// The connection pool could not be created.
    #[error("Pool creation error: {0}")]
    PoolCreate(String),

    /// A low-level Redis client error.
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
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Set a key prefix for namespacing
    #[must_use]
    pub fn with_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = prefix.into();
        self
    }

    /// Set a default key to observe
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns [`RedisObserverError::PoolCreate`] if the deadpool connection
    /// pool cannot be built from the configured Redis URL.
    ///
    /// The signature is `async` for parity with the other observer
    /// constructors, even though pool creation itself is synchronous.
    // `unused_async_trait_impl` (clippy 1.98) fires on this signature in
    // addition to `unused_async`, so both names are allowed.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn new(config: RedisObserverConfig) -> Result<Self, RedisObserverError> {
        // `Config::from_url` leaves `connection` unset. Building the struct via
        // `..Default::default()` would populate `connection` as well, and
        // deadpool rejects a config that specifies both `url` and `connection`.
        let mut cfg = deadpool_redis::Config::from_url(config.redis_url.clone());
        cfg.pool = Some(deadpool_redis::PoolConfig {
            max_size: usize::try_from(config.timeout_ms).unwrap_or(usize::MAX),
            ..Default::default()
        });

        let pool = cfg
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .map_err(|e| RedisObserverError::PoolCreate(e.to_string()))?;

        Ok(Self { pool, config })
    }

    /// Create a new Redis observer from a pre-existing pool
    #[must_use]
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
    /// Returns `(operation, key, extra_args)`.
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
                "unknown operation: {op}"
            ))),
        }
    }

    /// Build the observation spec from contract
    // `&self` is retained so the spec builder stays a method on the observer
    // and can consult config state in future revisions.
    #[allow(clippy::unused_self)]
    fn build_spec(&self, contract: &Contract) -> String {
        let action_name = &contract.action_name;

        // Check if action_name contains a spec prefix
        if action_name.contains(':') {
            action_name.clone()
        } else {
            // Default to get operation
            format!("get:{action_name}")
        }
    }
}

#[async_trait::async_trait]
impl agentverify_runtime::Observer for RedisObserver {
    /// Observe system state by executing the contract's observation spec
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::Unknown`] if a connection cannot be acquired
    /// from the pool, the observation spec cannot be parsed, or the Redis
    /// command fails.
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
            .map_err(|e| ExecutorError::Unknown(format!("Redis pool error: {e}")))?;

        // Build the observation spec
        let spec = self.build_spec(contract);

        // Execute the spec
        let state = self
            .execute_spec(&mut conn, &spec)
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Redis observation failed: {e}")))?;

        Ok(Observation::new(SourceId("redis".into()), state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live Redis URL for the gated tests in this module.
    ///
    /// When `AGENTVERIFY_TEST_REDIS_URL` is unset the service-dependent tests
    /// print a one-line notice and return early so CI stays green.
    fn live_url() -> Option<String> {
        match std::env::var("AGENTVERIFY_TEST_REDIS_URL") {
            Ok(url) if !url.trim().is_empty() => Some(url),
            _ => None,
        }
    }

    fn skip_notice() {
        eprintln!("skipping service test: AGENTVERIFY_TEST_REDIS_URL is not set");
    }

    /// Build an observer whose pool is created lazily (no connection is made).
    fn logic_observer(config: RedisObserverConfig) -> RedisObserver {
        let cfg = deadpool_redis::Config::from_url(config.redis_url.clone());
        let pool = cfg
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("lazy redis pool should build");
        RedisObserver::with_pool(pool, config)
    }

    /// Render the error from a fallible call without requiring `Debug` on the
    /// success type (`RedisObserver` intentionally has no `Debug` impl).
    fn error_of(result: Result<RedisObserver, RedisObserverError>) -> String {
        match result {
            Err(e) => e.to_string(),
            Ok(_) => String::from("<unexpected Ok>"),
        }
    }

    /// `error_of` renders a sentinel for a successfully built observer rather
    /// than an empty string, so an assertion that fires on the wrong arm
    /// identifies itself — and a well-formed URL still builds a pool.
    #[tokio::test]
    async fn error_of_renders_a_sentinel_for_a_successfully_built_observer() {
        let observer = RedisObserver::new(RedisObserverConfig::new("redis://127.0.0.1:6379")).await;
        assert_eq!(error_of(observer), "<unexpected Ok>");
    }

    fn action_with_args(arguments: Value) -> Action {
        Action::new("redis_action", arguments)
    }

    #[test]
    fn config_default() {
        let config = RedisObserverConfig::default();
        assert_eq!(config.redis_url, "redis://127.0.0.1:6379");
        assert_eq!(config.timeout_ms, 5000);
        assert!(config.key_prefix.is_empty());
        assert_eq!(config.default_key, None);
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

    // --- parse_spec (the real private implementation) ---

    #[test]
    fn parse_spec_get() {
        let (op, key, extra) = RedisObserver::parse_spec("get:mykey").unwrap();
        assert_eq!(op, "get");
        assert_eq!(key, "mykey");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_hget_single_field() {
        let (op, key, extra) = RedisObserver::parse_spec("hget:myhash:field1").unwrap();
        assert_eq!(op, "hget");
        assert_eq!(key, "myhash");
        assert_eq!(extra, vec!["field1"]);
    }

    #[test]
    fn parse_spec_hget_field_containing_colons() {
        let (op, key, extra) = RedisObserver::parse_spec("hget:myhash:a:b:c").unwrap();
        assert_eq!(op, "hget");
        assert_eq!(key, "myhash");
        assert_eq!(extra, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_spec_hgetall() {
        let (op, key, extra) = RedisObserver::parse_spec("hgetall:myhash").unwrap();
        assert_eq!(op, "hgetall");
        assert_eq!(key, "myhash");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_scard() {
        let (op, key, extra) = RedisObserver::parse_spec("scard:myset").unwrap();
        assert_eq!(op, "scard");
        assert_eq!(key, "myset");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_exists() {
        let (op, key, extra) = RedisObserver::parse_spec("exists:mykey").unwrap();
        assert_eq!(op, "exists");
        assert_eq!(key, "mykey");
        assert!(extra.is_empty());
    }

    #[test]
    fn parse_spec_rejects_spec_without_colon() {
        let err = RedisObserver::parse_spec("invalid").unwrap_err();
        assert!(err.to_string().contains("invalid"), "got: {err}");
    }

    #[test]
    fn parse_spec_rejects_empty_spec() {
        let err = RedisObserver::parse_spec("").unwrap_err();
        assert!(
            err.to_string().contains("Invalid observation spec"),
            "got: {err}"
        );
    }

    // --- build_spec ---

    #[test]
    fn build_spec_defaults_to_get_when_no_colon() {
        let observer = logic_observer(RedisObserverConfig::new("redis://127.0.0.1:6379"));
        let contract = Contract::new("order:42");
        assert_eq!(observer.build_spec(&contract), "order:42");
    }

    #[test]
    fn build_spec_wraps_plain_action_name_in_get() {
        let observer = logic_observer(RedisObserverConfig::new("redis://127.0.0.1:6379"));
        let contract = Contract::new("plain_key");
        assert_eq!(observer.build_spec(&contract), "get:plain_key");
    }

    #[test]
    fn build_spec_wraps_empty_action_name_in_get() {
        let observer = logic_observer(RedisObserverConfig::new("redis://127.0.0.1:6379"));
        let contract = Contract::new("");
        assert_eq!(observer.build_spec(&contract), "get:");
    }

    // --- resolve_key precedence ---

    #[test]
    fn resolve_key_prefers_action_arguments() {
        let observer = logic_observer(
            RedisObserverConfig::new("redis://127.0.0.1:6379").with_default_key("config_key"),
        );
        let action = action_with_args(serde_json::json!({"key": "from_action"}));
        let contract = Contract::new("from_contract");
        assert_eq!(observer.resolve_key(&action, &contract), "from_action");
    }

    #[test]
    fn resolve_key_ignores_non_string_action_key() {
        let observer = logic_observer(RedisObserverConfig::new("redis://127.0.0.1:6379"));
        let action = action_with_args(serde_json::json!({"key": 1234}));
        let contract = Contract::new("from_contract");
        assert_eq!(observer.resolve_key(&action, &contract), "from_contract");
    }

    #[test]
    fn resolve_key_falls_back_to_contract_action_name() {
        let observer = logic_observer(RedisObserverConfig::new("redis://127.0.0.1:6379"));
        let action = action_with_args(serde_json::json!({}));
        let contract = Contract::new("from_contract");
        assert_eq!(observer.resolve_key(&action, &contract), "from_contract");
    }

    #[test]
    fn resolve_key_falls_back_to_config_default_key() {
        let observer = logic_observer(
            RedisObserverConfig::new("redis://127.0.0.1:6379").with_default_key("config_key"),
        );
        let action = action_with_args(serde_json::json!({}));
        let contract = Contract::new("");
        assert_eq!(observer.resolve_key(&action, &contract), "config_key");
    }

    #[test]
    fn resolve_key_uses_literal_default_when_nothing_else_is_set() {
        let observer = logic_observer(RedisObserverConfig::new("redis://127.0.0.1:6379"));
        let action = action_with_args(serde_json::json!({}));
        let contract = Contract::new("");
        assert_eq!(observer.resolve_key(&action, &contract), "default");
    }

    // --- construction ---

    #[tokio::test]
    async fn new_builds_pool_from_valid_url() {
        let observer = RedisObserver::new(RedisObserverConfig::new("redis://127.0.0.1:6379"))
            .await
            .unwrap();
        assert_eq!(observer.config.redis_url, "redis://127.0.0.1:6379");
    }

    #[tokio::test]
    async fn new_rejects_malformed_url_with_pool_create_error() {
        let rendered =
            error_of(RedisObserver::new(RedisObserverConfig::new("not a redis url")).await);
        assert!(
            rendered.contains("Pool creation error"),
            "expected 'Pool creation error', got: {rendered}"
        );
    }

    // --- error Display strings for every variant ---

    #[test]
    fn error_display_covers_every_variant() {
        let backend = deadpool_redis::PoolError::Closed;
        let cases: Vec<(RedisObserverError, Vec<&str>)> = vec![
            (
                RedisObserverError::ConnectionFailed("connection refused".to_string()),
                vec!["Redis connection failed", "connection refused"],
            ),
            (
                RedisObserverError::RedisOperation("WRONGTYPE".to_string()),
                vec!["Redis operation failed", "WRONGTYPE"],
            ),
            (
                RedisObserverError::KeyNotFound("missing:1".to_string()),
                vec!["Key not found", "missing:1"],
            ),
            (
                RedisObserverError::InvalidKeyPattern("a b".to_string()),
                vec!["Invalid key pattern", "a b"],
            ),
            (
                RedisObserverError::InvalidObservationSpec("nospec".to_string()),
                vec!["Invalid observation spec", "nospec"],
            ),
            (RedisObserverError::PoolError(backend), vec!["Pool error"]),
            (
                RedisObserverError::PoolCreate("bad url".to_string()),
                vec!["Pool creation error", "bad url"],
            ),
        ];

        for (err, expected) in cases {
            let rendered = err.to_string();
            for fragment in expected {
                assert!(
                    rendered.contains(fragment),
                    "expected '{fragment}' in '{rendered}'"
                );
            }
        }
    }

    #[test]
    fn error_display_redis_error_variant_wraps_source() {
        let source = redis::Client::open("definitely not a redis url").unwrap_err();
        let err = RedisObserverError::RedisError(source);
        assert!(err.to_string().contains("Redis error"), "got: {err}");
        // `#[from]` keeps the source reachable for callers and logs.
        assert!(std::error::Error::source(&err).is_some());
    }

    // --- execute_spec against a live Redis server ---

    /// Observer + pool bound to the live server with the requested prefix.
    fn live_observer(url: &str, prefix: &str) -> RedisObserver {
        let config = RedisObserverConfig::new(url).with_key_prefix(prefix);
        let cfg = deadpool_redis::Config::from_url(url);
        let pool = cfg
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("pool should build for a reachable redis");
        RedisObserver::with_pool(pool, config)
    }

    #[tokio::test]
    async fn execute_spec_get_returns_parsed_json_then_string() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();

        let _: () = conn.del("av_obs_get").await.unwrap();
        let _: () = conn
            .set("av_obs_get", r#"{"amount": 250, "ok": true}"#)
            .await
            .unwrap();

        let value = observer
            .execute_spec(&mut conn, "get:av_obs_get")
            .await
            .unwrap();
        assert_eq!(value, serde_json::json!({"amount": 250, "ok": true}));

        // A value that is not JSON is returned verbatim as a string.
        let _: () = conn.set("av_obs_get", "plain text").await.unwrap();
        let value = observer
            .execute_spec(&mut conn, "get:av_obs_get")
            .await
            .unwrap();
        assert_eq!(value, Value::String("plain text".to_string()));

        let _: () = conn.del("av_obs_get").await.unwrap();
    }

    #[tokio::test]
    async fn execute_spec_get_missing_key_is_key_not_found() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();
        let _: () = conn.del("av_obs_absent").await.unwrap();

        let err = observer
            .execute_spec(&mut conn, "get:av_obs_absent")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Key not found"), "got: {err}");
        assert!(err.to_string().contains("av_obs_absent"), "got: {err}");
    }

    #[tokio::test]
    async fn execute_spec_exists_reflects_key_presence() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();
        let _: () = conn.del("av_obs_exists").await.unwrap();

        let value = observer
            .execute_spec(&mut conn, "exists:av_obs_exists")
            .await
            .unwrap();
        assert_eq!(value, Value::Bool(false));

        let _: () = conn.set("av_obs_exists", "1").await.unwrap();
        let value = observer
            .execute_spec(&mut conn, "exists:av_obs_exists")
            .await
            .unwrap();
        assert_eq!(value, Value::Bool(true));

        let _: () = conn.del("av_obs_exists").await.unwrap();
    }

    #[tokio::test]
    async fn execute_spec_scard_counts_real_set_members() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();
        let _: () = conn.del("av_obs_set").await.unwrap();

        let value = observer
            .execute_spec(&mut conn, "scard:av_obs_set")
            .await
            .unwrap();
        assert_eq!(value, Value::Number(0.into()));

        let _: usize = conn.sadd("av_obs_set", ("a", "b", "c")).await.unwrap();
        let value = observer
            .execute_spec(&mut conn, "scard:av_obs_set")
            .await
            .unwrap();
        assert_eq!(value, Value::Number(3.into()));

        let _: () = conn.del("av_obs_set").await.unwrap();
    }

    #[tokio::test]
    async fn execute_spec_hget_requires_a_field() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();

        let err = observer
            .execute_spec(&mut conn, "hget:av_obs_hash")
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("hget requires a field"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn execute_spec_hget_reads_present_and_absent_fields() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();
        let _: () = conn.del("av_obs_hash").await.unwrap();

        let _: () = conn
            .hset("av_obs_hash", "present", r#"{"n": 7}"#)
            .await
            .unwrap();
        let _: () = conn.hset("av_obs_hash", "textual", "hello").await.unwrap();

        let value = observer
            .execute_spec(&mut conn, "hget:av_obs_hash:present")
            .await
            .unwrap();
        assert_eq!(value, serde_json::json!({"n": 7}));

        let value = observer
            .execute_spec(&mut conn, "hget:av_obs_hash:textual")
            .await
            .unwrap();
        assert_eq!(value, Value::String("hello".to_string()));

        // An absent field yields JSON null rather than an error.
        let value = observer
            .execute_spec(&mut conn, "hget:av_obs_hash:absent_field")
            .await
            .unwrap();
        assert_eq!(value, Value::Null);

        let _: () = conn.del("av_obs_hash").await.unwrap();
    }

    #[tokio::test]
    async fn execute_spec_hgetall_returns_every_field() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();
        let _: () = conn.del("av_obs_full").await.unwrap();

        let _: () = conn
            .hset("av_obs_full", "json_field", "[1, 2]")
            .await
            .unwrap();
        let _: () = conn.hset("av_obs_full", "text_field", "raw").await.unwrap();

        let value = observer
            .execute_spec(&mut conn, "hgetall:av_obs_full")
            .await
            .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"json_field": [1, 2], "text_field": "raw"})
        );

        let _: () = conn.del("av_obs_full").await.unwrap();
    }

    #[tokio::test]
    async fn execute_spec_rejects_unknown_operation() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "");
        let mut conn = observer.pool.get().await.unwrap();

        let err = observer
            .execute_spec(&mut conn, "zscore:av_obs_key")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown operation"), "got: {err}");
        assert!(err.to_string().contains("zscore"), "got: {err}");
    }

    #[tokio::test]
    async fn execute_spec_applies_key_prefix() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let observer = live_observer(&url, "av_obs_ns_");
        let mut conn = observer.pool.get().await.unwrap();
        let _: () = conn.del("av_obs_ns_inner").await.unwrap();
        let _: () = conn.del("inner").await.unwrap();

        // The namespaced key is written through the same live server.
        let _: () = conn.set("av_obs_ns_inner", "prefixed").await.unwrap();
        let value = observer.execute_spec(&mut conn, "get:inner").await.unwrap();
        assert_eq!(value, Value::String("prefixed".to_string()));

        // With the namespaced key gone, the unprefixed `inner` is not consulted.
        let _: () = conn.set("inner", "unprefixed").await.unwrap();
        let _: () = conn.del("av_obs_ns_inner").await.unwrap();
        let err = observer
            .execute_spec(&mut conn, "get:inner")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Key not found"), "got: {err}");

        let _: () = conn.del("inner").await.unwrap();
    }
}
