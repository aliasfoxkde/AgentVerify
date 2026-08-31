//! `PostgreSQL` observer implementation
//!
//! Observes system state via `PostgreSQL` queries.
//!
//! # Overview
//!
//! The [`PostgresObserver`] connects to a `PostgreSQL` database using a deadpool
//! connection pool and executes queries to observe system state. Observations
//! are returned as JSON and used during the verification phase.
//!
//! # Query Building
//!
//! The observer extracts the table name from `contract.action_name` and builds
//! queries based on postconditions. For each postcondition, it constructs
//! parameterized queries that check whether the expected state exists.
//!
//! # Configuration
//!
//! - Connection via `Config` builder pattern
//! - SSL mode support
//! - Schema-qualified table names
//! - Custom column projections
//!
//! # Example
//!
//! ```ignore
//! let observer = PostgresObserver::from_config(
//!     PostgresObserverConfig::default()
//!         .with_host("localhost")
//!         .with_port(5432)
//!         .with_user("postgres")
//!         .with_password("secret")
//!         .with_database("mydb"),
//! ).await?;
//! ```

use agentverify_core::{Action, Contract, Observation, SourceId};
use agentverify_runtime::ExecutorError;
use deadpool_postgres::{Pool, Runtime};
use serde_json::{json, Value};
use thiserror::Error;
use tokio_postgres::NoTls;

/// `PostgreSQL` observer-specific errors
#[derive(Debug, Error)]
pub enum PostgresObserverError {
    /// A connection setting was missing or invalid.
    #[error("Configuration error: {0}")]
    Config(String),

    /// The deadpool connection pool could not be created.
    #[error("Pool creation failed: {0}")]
    PoolCreation(String),

    /// A query failed while executing against the server.
    #[error("Query execution failed: {0}")]
    QueryError(String),

    /// A query could not be constructed from the contract.
    #[error("Query building failed: {0}")]
    QueryBuildError(String),

    /// A query result could not be converted to JSON.
    #[error("Result parsing failed: {0}")]
    ParseError(String),

    /// The server did not respond within the configured timeout.
    #[error("Connection timeout")]
    Timeout,

    /// The contract declares no postconditions to observe.
    #[error("No postconditions defined in contract")]
    NoPostconditions,
}

/// The fields [`PostgresObserver::from_uri`] extracts from a connection URI.
struct ParsedUri {
    /// Percent-decoded user
    user: String,
    /// Percent-decoded password; `None` when the URI supplies no password at
    /// all and `Some(String::new())` for an explicitly empty one (`user:@host`)
    password: Option<String>,
    /// Hostname or IP address
    host: String,
    /// Port, defaulting to 5432 when the URI omits it
    port: u16,
    /// Percent-decoded database name, without any query parameters
    database: String,
}

/// Percent-decode a single userinfo or database component.
///
/// Escapes such as `%FF` do not decode to valid UTF-8 and are rejected.
fn decode_component(raw: &str, component: &str) -> Result<String, PostgresObserverError> {
    urlencoding::decode(raw)
        .map(|decoded| decoded.to_string())
        .map_err(|_| PostgresObserverError::Config(format!("Invalid {component} encoding")))
}

/// Parse a `postgres://` or `postgresql://` connection URI.
///
/// See [`PostgresObserver::from_uri`] for the grammar this implements and the
/// error returned for each malformed shape.
fn parse_uri(uri: &str) -> Result<ParsedUri, PostgresObserverError> {
    let rest = uri
        .strip_prefix("postgres://")
        .or_else(|| uri.strip_prefix("postgresql://"))
        .ok_or_else(|| {
            PostgresObserverError::Config(format!(
                "Invalid URI scheme: expected `postgres://` or `postgresql://`, got `{uri}`"
            ))
        })?;

    // The userinfo segment is delimited by the last `@`, so an unescaped `@` in
    // a password still parses; percent-encoding (`build_uri`) is still the
    // recommended way to write one.
    let Some((userinfo, authority)) = rest.rsplit_once('@') else {
        return Err(PostgresObserverError::Config(
            "Invalid URI format: missing `user@host` userinfo".to_string(),
        ));
    };

    // `user@host` supplies no password at all, matching libpq; `user:@host`
    // supplies an explicitly empty one. Anything else with a stray `:` is
    // ambiguous and rejected.
    let (user, password) = match userinfo.split_once(':') {
        Some((user, password)) => {
            if password.contains(':') {
                return Err(PostgresObserverError::Config(
                    "Invalid user:password format: percent-encode ':' in the password".to_string(),
                ));
            }
            (user, Some(decode_component(password, "password")?))
        }
        None => (userinfo, None),
    };
    let user = decode_component(user, "user")?;

    let (host_port, database_raw) = authority.split_once('/').ok_or_else(|| {
        PostgresObserverError::Config(
            "Invalid host:port/database format: missing `/database`".to_string(),
        )
    })?;
    // Query parameters are cut before decoding, so an encoded `%3F` stays part
    // of the database name.
    let database_raw = match database_raw.split_once('?') {
        Some((database, _)) => database,
        None => database_raw,
    };
    let database = decode_component(database_raw, "database name")?;

    // An unparsable port is never silently replaced by the default: a typo
    // would otherwise point the observer at an unintended server.
    let (host, port): (&str, u16) = match host_port.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse()
                .map_err(|_| PostgresObserverError::Config(format!("Invalid port: `{port}`")))?,
        ),
        None => (host_port, 5432),
    };

    Ok(ParsedUri {
        user,
        password,
        host: host.to_string(),
        port,
        database,
    })
}

/// `PostgreSQL` observer configuration
///
/// # Example
///
/// ```ignore
/// let config = PostgresObserverConfig::default()
///     .with_host("localhost")
///     .with_port(5432)
///     .with_user("postgres")
///     .with_password("secret")
///     .with_database("mydb");
/// ```
#[derive(Debug, Clone)]
pub struct PostgresObserverConfig {
    /// Hostname or IP address
    pub host: String,
    /// Port number
    pub port: u16,
    /// Database user
    pub user: String,
    /// User password
    pub password: String,
    /// Database name
    pub database: String,
    /// SSL mode ("disable", "require", "verify-ca", "verify-full")
    pub ssl_mode: String,
    /// Application name for `pg_settings`
    pub application_name: String,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u64,
    /// Maximum number of connections in pool
    pub pool_max_size: usize,
    /// Query timeout in milliseconds
    pub query_timeout_ms: u64,
}

impl Default for PostgresObserverConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            user: "postgres".to_string(),
            password: String::new(),
            database: "postgres".to_string(),
            ssl_mode: "disable".to_string(),
            application_name: "agentverify".to_string(),
            connect_timeout_secs: 5,
            pool_max_size: 16,
            query_timeout_ms: 5000,
        }
    }
}

impl PostgresObserverConfig {
    /// Create a new config with all defaults
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the hostname
    #[must_use]
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the user
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Set the password
    #[must_use]
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = password.into();
        self
    }

    /// Set the database name
    #[must_use]
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = database.into();
        self
    }

    /// Set the SSL mode
    ///
    /// Valid values: "disable", "require", "verify-ca", "verify-full"
    #[must_use]
    pub fn with_ssl_mode(mut self, mode: impl Into<String>) -> Self {
        self.ssl_mode = mode.into();
        self
    }

    /// Set the application name
    #[must_use]
    pub fn with_application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = name.into();
        self
    }

    /// Set the connection timeout in seconds
    #[must_use]
    pub fn with_connect_timeout_secs(mut self, secs: u64) -> Self {
        self.connect_timeout_secs = secs;
        self
    }

    /// Set the maximum pool size
    #[must_use]
    pub fn with_pool_max_size(mut self, size: usize) -> Self {
        self.pool_max_size = size;
        self
    }

    /// Set the query timeout in milliseconds
    #[must_use]
    pub fn with_query_timeout_ms(mut self, ms: u64) -> Self {
        self.query_timeout_ms = ms;
        self
    }

    /// Build the connection URI
    #[allow(dead_code)]
    fn build_uri(&self) -> String {
        // URL-encode the password to handle special characters
        let encoded_password = urlencoding::encode(&self.password);
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}&application_name={}",
            urlencoding::encode(&self.user),
            encoded_password,
            self.host,
            self.port,
            urlencoding::encode(&self.database),
            self.ssl_mode,
            urlencoding::encode(&self.application_name),
        )
    }

    /// Create a deadpool configuration
    fn create_deadpool_config(&self) -> deadpool_postgres::Config {
        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(self.host.clone());
        cfg.port = Some(self.port);
        cfg.user = Some(self.user.clone());
        cfg.password = Some(self.password.clone());
        cfg.dbname = Some(self.database.clone());
        cfg.connect_timeout = Some(std::time::Duration::from_secs(self.connect_timeout_secs));
        cfg
    }
}

/// `PostgreSQL` observer using deadpool for connection pooling
///
/// Executes parameterized queries against `PostgreSQL` to observe system state.
pub struct PostgresObserver {
    pool: Pool,
    #[allow(dead_code)]
    config: PostgresObserverConfig,
}

impl PostgresObserver {
    /// Create a new observer from configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the connection pool cannot be created.
    ///
    /// The signature is `async` for parity with [`Self::from_uri`], even though
    /// pool creation itself is synchronous.
    // `unused_async_trait_impl` (clippy 1.98) fires on these signatures in
    // addition to `unused_async`, so both names are allowed.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn from_config(
        config: PostgresObserverConfig,
    ) -> Result<Self, PostgresObserverError> {
        let cfg = config.create_deadpool_config();

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| PostgresObserverError::PoolCreation(e.to_string()))?;

        Ok(Self { pool, config })
    }

    /// Create a new observer from a connection URI
    ///
    /// # Accepted grammar
    ///
    /// ```text
    /// (postgres|postgresql)://[user[:password]@]host[:port]/database[?params]
    /// ```
    ///
    /// * `user`, `password`, and `database` are percent-decoded, so `:` and `@`
    ///   inside them must be escaped as `%3A` and `%40` (as
    ///   `PostgresObserverConfig::build_uri` does when composing a URI).
    ///   An escape that does not decode to UTF-8 is rejected.
    /// * The userinfo segment is delimited by the last `@`. A userinfo segment
    ///   with no colon supplies **no** password, matching libpq:
    ///   `postgres://user@host/db` configures the pool with no password, while
    ///   `postgres://user:@host/db` configures an explicitly empty one.
    /// * `port` defaults to 5432 and must parse as a `u16`.
    /// * `params` are ignored: the database name is everything between the last
    ///   `/` and the first `?`.
    /// * Bracketed IPv6 literals (`[::1]:5432`) are not supported.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let observer = PostgresObserver::from_uri(
    ///     "postgres://postgres:secret@localhost:5432/mydb",
    /// ).await;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`PostgresObserverError::Config`] when `uri` does not match the
    /// grammar above (wrong scheme, missing userinfo or `/database` segment,
    /// more than one `:` in the userinfo, an unparsable port, or a user or
    /// password that is not valid percent-encoded UTF-8), and
    /// [`PostgresObserverError::PoolCreation`] when the pool cannot be created.
    ///
    /// The signature is `async` for parity with [`Self::from_config`], even
    /// though pool creation itself is synchronous.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn from_uri(uri: &str) -> Result<Self, PostgresObserverError> {
        let parsed = parse_uri(uri)?;

        let mut cfg = deadpool_postgres::Config::new();
        cfg.user = Some(parsed.user);
        cfg.password = parsed.password;
        cfg.host = Some(parsed.host);
        cfg.port = Some(parsed.port);
        cfg.dbname = Some(parsed.database);

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| PostgresObserverError::PoolCreation(e.to_string()))?;

        Ok(Self {
            pool,
            config: PostgresObserverConfig::default(),
        })
    }

    /// Execute a parameterized query and return results as JSON
    ///
    /// # Arguments
    ///
    /// * `query` - The SQL query string with $1, $2, ... placeholders
    /// * `params` - The parameter values to bind (JSON values)
    ///
    /// # Returns
    ///
    /// Returns a JSON array of objects, one per row.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresObserverError::QueryError`] if a connection cannot be
    /// acquired from the pool or the query fails to execute.
    pub async fn execute_query(
        &self,
        query: &str,
        params: &[Value],
    ) -> Result<Value, PostgresObserverError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| PostgresObserverError::QueryError(format!("Pool get failed: {e}")))?;

        // Convert JSON values to strings for postgres query
        let string_params: Vec<String> = params
            .iter()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()))
            .collect();

        // Build parameter references vec with explicit type
        let mut param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
        for s in &string_params {
            param_refs.push(s as &(dyn tokio_postgres::types::ToSql + Sync));
        }

        let rows = client
            .query(query, param_refs.as_slice())
            .await
            .map_err(|e| PostgresObserverError::QueryError(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            let mut obj = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                let col_name = col.name();
                let value = Self::pg_value_to_json(&row, i);
                obj.insert(col_name.to_string(), value);
            }
            results.push(Value::Object(obj));
        }

        Ok(Value::Array(results))
    }

    /// Convert a postgres column value to JSON
    ///
    /// Text columns surface as JSON when their contents parse as JSON and as
    /// strings otherwise; `NULL` in any column becomes `Value::Null`.
    ///
    /// Only the `String` attempt is made for text: `String::accepts` is defined
    /// as `&str::accepts` in `postgres-types`, so a `&str` retry can never
    /// succeed where it failed, and `Option<String>` is only reachable when the
    /// value was `NULL` — which falls through to `Value::Null` below anyway.
    fn pg_value_to_json(row: &tokio_postgres::Row, idx: usize) -> Value {
        // Try to get as JSON-compatible string first
        if let Ok(s) = row.try_get::<_, String>(idx) {
            // Try to parse as JSON
            if let Ok(v) = serde_json::from_str::<Value>(&s) {
                return v;
            }
            // Return as string
            return Value::String(s);
        }
        if let Ok(n) = row.try_get::<_, i64>(idx) {
            return Value::Number(n.into());
        }
        if let Ok(n) = row.try_get::<_, i32>(idx) {
            return Value::Number(n.into());
        }
        if let Ok(n) = row.try_get::<_, f64>(idx) {
            if let Some(v) = serde_json::Number::from_f64(n) {
                return Value::Number(v);
            }
        }
        if let Ok(b) = row.try_get::<_, bool>(idx) {
            return Value::Bool(b);
        }
        // Everything left is either `NULL` (the `String` attempt above already
        // failed for it) or a column type this converter does not handle.
        Value::Null
    }

    /// Build a SELECT query from the contract
    ///
    /// The query selects all columns (*) from the table named by `action_name`.
    /// This can be extended to build more sophisticated queries based on postconditions.
    // `&self` is retained so the query builder stays a method on the observer
    // and can consult pool/config state in future revisions.
    #[allow(clippy::unused_self)]
    fn build_observation_query(
        &self,
        action: &Action,
        contract: &Contract,
    ) -> Result<(String, Vec<Value>), PostgresObserverError> {
        let table_name = &contract.action_name;

        // Validate table name to prevent SQL injection
        if table_name.contains(';')
            || table_name.contains("--")
            || table_name.contains("/*")
            || table_name.contains("*/")
            || table_name.to_lowercase().starts_with("drop")
            || table_name.to_lowercase().starts_with("insert")
            || table_name.to_lowercase().starts_with("update")
            || table_name.to_lowercase().starts_with("delete")
            || table_name.to_lowercase().starts_with("truncate")
            || table_name.to_lowercase().starts_with("alter")
            || table_name.to_lowercase().starts_with("create")
        {
            return Err(PostgresObserverError::QueryBuildError(format!(
                "Invalid table name: {table_name}"
            )));
        }

        // Build query: SELECT * FROM <table> WHERE id = $1
        // The action ID is used as the primary key
        let query = format!("SELECT * FROM {table_name} WHERE id = $1 LIMIT 1");

        let params = vec![serde_json::json!(action.id.to_string())];

        Ok((query, params))
    }

    /// Check connectivity by executing a simple query
    ///
    /// # Errors
    ///
    /// Returns [`PostgresObserverError::QueryError`] if a connection cannot be
    /// acquired from the pool or the probe query fails.
    pub async fn health_check(&self) -> Result<(), PostgresObserverError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| PostgresObserverError::QueryError(format!("Pool get failed: {e}")))?;

        client
            .query_one("SELECT 1", &[])
            .await
            .map_err(|e| PostgresObserverError::QueryError(e.to_string()))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl agentverify_runtime::Observer for PostgresObserver {
    /// Observe system state by executing a query against `PostgreSQL`
    ///
    /// # Process
    ///
    /// 1. Build a SELECT query from the contract's `action_name` (used as table name)
    /// 2. Execute with the action ID as parameter
    /// 3. Return results as JSON observation
    async fn observe(
        &self,
        action: &Action,
        contract: &Contract,
    ) -> Result<Observation, ExecutorError> {
        // Build the observation query
        let (query, params) = self
            .build_observation_query(action, contract)
            .map_err(|e| ExecutorError::Unknown(format!("Query build failed: {e}")))?;

        // Convert params to Values expected by execute_query
        let param_values: Vec<Value> = params;

        // Execute the query
        let results = self
            .execute_query(&query, &param_values)
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Query execution failed: {e}")))?;

        // Build the observation state
        // If no rows returned, return empty state to indicate resource not found
        let state = if let Value::Array(ref arr) = results {
            if arr.is_empty() {
                json!({
                    "found": false,
                    "table": contract.action_name,
                    "action_id": action.id.to_string()
                })
            } else {
                // Return the first row as the state
                arr.first().cloned().unwrap_or(Value::Null)
            }
        } else {
            results
        };

        Ok(Observation::new(SourceId("postgres".into()), state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Helper to create a `PostgresObserver` for testing query building
    /// The pool is not actually used in `build_observation_query` tests
    fn make_test_observer() -> PostgresObserver {
        let config = PostgresObserverConfig::default();
        // Create a pool that won't actually be used in build_observation_query tests
        let cfg = config.create_deadpool_config();
        let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();
        PostgresObserver { pool, config }
    }

    #[test]
    fn config_default() {
        let config = PostgresObserverConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5432);
        assert_eq!(config.user, "postgres");
        assert_eq!(config.database, "postgres");
        assert_eq!(config.ssl_mode, "disable");
        assert_eq!(config.pool_max_size, 16);
        assert_eq!(config.query_timeout_ms, 5000);
    }

    #[test]
    fn config_builder() {
        let config = PostgresObserverConfig::new()
            .with_host("db.example.com")
            .with_port(5433)
            .with_user("admin")
            .with_password("secret123")
            .with_database("production")
            .with_ssl_mode("require")
            .with_application_name("agentverify-test")
            .with_connect_timeout_secs(10)
            .with_pool_max_size(32)
            .with_query_timeout_ms(10000);

        assert_eq!(config.host, "db.example.com");
        assert_eq!(config.port, 5433);
        assert_eq!(config.user, "admin");
        assert_eq!(config.password, "secret123");
        assert_eq!(config.database, "production");
        assert_eq!(config.ssl_mode, "require");
        assert_eq!(config.application_name, "agentverify-test");
        assert_eq!(config.connect_timeout_secs, 10);
        assert_eq!(config.pool_max_size, 32);
        assert_eq!(config.query_timeout_ms, 10000);
    }

    #[test]
    fn config_build_uri() {
        let config = PostgresObserverConfig::new()
            .with_host("localhost")
            .with_port(5432)
            .with_user("postgres")
            .with_password("secret")
            .with_database("mydb");

        let uri = config.build_uri();
        assert!(uri.contains("localhost:5432"));
        assert!(uri.contains("mydb"));
        assert!(uri.contains("sslmode=disable"));
    }

    #[test]
    fn config_build_uri_with_special_chars_in_password() {
        let config = PostgresObserverConfig::new()
            .with_host("localhost")
            .with_user("postgres")
            .with_password("p@ss:word/123")
            .with_database("mydb");

        let uri = config.build_uri();
        // Password should be URL-encoded
        assert!(uri.contains("p%40ss%3Aword%2F123"));
    }

    #[test]
    fn query_building_valid_table_name() {
        let observer = make_test_observer();

        let action = Action::new("test", serde_json::json!({"id": "123"}));
        let contract = Contract::new("users");

        let result = observer.build_observation_query(&action, &contract);
        assert!(result.is_ok());

        let (query, params) = result.unwrap();
        assert!(query.contains("SELECT * FROM users"));
        assert!(query.contains("WHERE id = $1"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn query_building_sql_injection_rejected() {
        let observer = make_test_observer();

        let action = Action::new("test", serde_json::json!({}));
        let malicious_contracts = [
            "users; DROP TABLE users;--",
            "users/*comment*/",
            "users--comment",
            "DROP TABLE users",
            "INSERT INTO users",
            "UPDATE users SET",
            "DELETE FROM users",
            "TRUNCATE users",
            "ALTER TABLE users",
            "CREATE TABLE evil",
        ];

        for malicious in &malicious_contracts {
            let contract = Contract::new(*malicious);
            let result = observer.build_observation_query(&action, &contract);
            assert!(result.is_err(), "Expected rejection for: {malicious}");
        }
    }

    #[test]
    fn query_building_case_insensitive_keywords_rejected() {
        let observer = make_test_observer();

        let action = Action::new("test", serde_json::json!({}));

        // Test DROP in different cases
        let contract_drop = Contract::new("DROP TABLE users");
        assert!(observer
            .build_observation_query(&action, &contract_drop)
            .is_err());

        let contract_drop_lower = Contract::new("drop table users");
        assert!(observer
            .build_observation_query(&action, &contract_drop_lower)
            .is_err());
    }

    #[test]
    fn query_building_with_schema_qualified_table() {
        let observer = make_test_observer();

        let action = Action::new("test", serde_json::json!({}));
        let contract = Contract::new("public.users");

        let result = observer.build_observation_query(&action, &contract);
        assert!(result.is_ok());

        let (query, _) = result.unwrap();
        assert!(query.contains("public.users"));
    }

    #[test]
    fn config_with_empty_password() {
        let config = PostgresObserverConfig::new()
            .with_host("localhost")
            .with_user("postgres")
            .with_password("")
            .with_database("mydb");

        let uri = config.build_uri();
        // Should not panic with empty password
        assert!(uri.contains("localhost"));
    }

    #[test]
    fn config_application_name_preserved() {
        let config = PostgresObserverConfig::new()
            .with_host("localhost")
            .with_user("postgres")
            .with_database("mydb")
            .with_application_name("agentverify-verify");

        let uri = config.build_uri();
        assert!(uri.contains("agentverify-verify"));
    }

    // --- construction and URI parsing ---

    /// Parse `uri`, panicking with the rendered error when it is rejected.
    fn parsed(uri: &str) -> ParsedUri {
        match parse_uri(uri) {
            Ok(parsed) => parsed,
            Err(e) => panic!("parsing `{uri}` failed: {e}"),
        }
    }

    /// Render the error from a fallible call without requiring `Debug` on the
    /// success type (`PostgresObserver` has no `Debug` impl).
    fn error_of<T>(result: Result<T, PostgresObserverError>) -> String {
        match result {
            Err(e) => e.to_string(),
            Ok(_) => String::from("<unexpected Ok>"),
        }
    }

    /// `parsed` carries the assertion message of every accepted-shape test, so
    /// a rejection has to name both the URI and the reason it was rejected.
    #[test]
    fn parsed_panics_with_the_uri_and_the_reason_for_a_rejected_uri() {
        // `ParsedUri` has no `Debug` impl, so the panic payload is bound with
        // `let ... else` rather than unwrapped through `expect_err`.
        let Err(payload) =
            std::panic::catch_unwind(|| parsed("postgres://127.0.0.1:5433/no_userinfo"))
        else {
            panic!("a URI without a userinfo segment must be rejected");
        };
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .expect("the panic payload is a formatted message");

        assert!(
            message.contains("postgres://127.0.0.1:5433/no_userinfo"),
            "the rejected URI must be named: {message}"
        );
        assert!(
            message.contains("Invalid URI format"),
            "the rejection reason must be named: {message}"
        );
    }

    /// `error_of` marks the `Ok` side with a sentinel rather than an empty
    /// string, so an assertion that fires on the wrong arm identifies itself.
    #[test]
    fn error_of_marks_an_ok_result_with_a_sentinel() {
        let ok: Result<(), PostgresObserverError> = Ok(());
        assert_eq!(error_of(ok), "<unexpected Ok>");
    }

    #[tokio::test]
    async fn from_config_builds_a_pool() {
        let observer = PostgresObserver::from_config(
            PostgresObserverConfig::new()
                .with_host("127.0.0.1")
                .with_port(5433)
                .with_user("postgres")
                .with_database("agentverify_test"),
        )
        .await;
        // Pool creation is lazy, so an unreachable host still succeeds here.
        assert!(observer.is_ok(), "got: {}", error_of(observer));
    }

    #[tokio::test]
    async fn from_config_rejects_an_empty_database_name() {
        let rendered = error_of(
            PostgresObserver::from_config(PostgresObserverConfig::new().with_database("")).await,
        );
        assert!(
            rendered.contains("Pool creation failed"),
            "expected 'Pool creation failed', got: {rendered}"
        );
    }

    // --- accepted shapes, through the public entry point ---

    #[tokio::test]
    async fn from_uri_accepts_a_passwordless_userinfo_segment() {
        // libpq accepts `user@host` for trust-authenticated servers, and so
        // must the parser: such a URI carries no password at all.
        let result =
            PostgresObserver::from_uri("postgres://postgres@127.0.0.1:5433/agentverify_test").await;
        assert!(result.is_ok(), "got: {}", error_of(result));
    }

    #[tokio::test]
    async fn from_uri_accepts_a_user_and_password() {
        let result = PostgresObserver::from_uri(
            "postgres://postgres:secret@127.0.0.1:5433/agentverify_test",
        )
        .await;
        assert!(result.is_ok(), "got: {}", error_of(result));
    }

    #[tokio::test]
    async fn from_uri_accepts_an_explicitly_empty_password() {
        let result =
            PostgresObserver::from_uri("postgres://postgres:@127.0.0.1:5433/agentverify_test")
                .await;
        assert!(result.is_ok(), "got: {}", error_of(result));
    }

    #[tokio::test]
    async fn from_uri_accepts_query_parameters_and_percent_encoded_user() {
        let result = PostgresObserver::from_uri(
            "postgres://post%67res@127.0.0.1:5433/agentverify_test?sslmode=disable&application_name=agentverify-it",
        )
        .await;
        assert!(result.is_ok(), "got: {}", error_of(result));
    }

    // --- accepted shapes, field by field ---

    #[test]
    fn parse_uri_reads_user_password_host_port_and_database() {
        let parsed = parsed("postgres://svc:secret@db.example.com:5434/prod_db");
        assert_eq!(parsed.user, "svc");
        assert_eq!(parsed.password.as_deref(), Some("secret"));
        assert_eq!(parsed.host, "db.example.com");
        assert_eq!(parsed.port, 5434);
        assert_eq!(parsed.database, "prod_db");
    }

    #[test]
    fn parse_uri_accepts_a_passwordless_userinfo_segment() {
        let parsed = parsed("postgres://postgres@127.0.0.1:5433/agentverify_test");
        assert_eq!(parsed.user, "postgres");
        // No password at all, exactly as libpq treats it — distinct from "".
        assert_eq!(parsed.password, None);
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 5433);
        assert_eq!(parsed.database, "agentverify_test");
    }

    #[test]
    fn parse_uri_treats_a_bare_colon_as_an_explicitly_empty_password() {
        let parsed = parsed("postgres://postgres:@127.0.0.1:5433/agentverify_test");
        assert_eq!(parsed.user, "postgres");
        assert_eq!(parsed.password, Some(String::new()));
    }

    #[test]
    fn parse_uri_percent_decodes_a_password_with_special_characters() {
        // `p@ss:w/123` is exactly what `build_uri` writes for that password.
        let parsed = parsed("postgres://svc:p%40ss%3Aw%2F123@db.example.com/prod_db");
        assert_eq!(parsed.user, "svc");
        assert_eq!(parsed.password.as_deref(), Some("p@ss:w/123"));
    }

    #[test]
    fn build_uri_output_round_trips_through_parse_uri() {
        let password = "p@ss:w/123%";
        let uri = PostgresObserverConfig::new()
            .with_host("db.example.com")
            .with_port(5434)
            .with_user("svc user")
            .with_password(password)
            .with_database("prod db")
            .build_uri();

        let parsed = parsed(&uri);
        assert_eq!(parsed.user, "svc user");
        assert_eq!(parsed.password.as_deref(), Some(password));
        assert_eq!(parsed.host, "db.example.com");
        assert_eq!(parsed.port, 5434);
        assert_eq!(parsed.database, "prod db");
    }

    #[test]
    fn parse_uri_percent_decodes_the_user() {
        let parsed = parsed("postgres://post%67res@127.0.0.1:5433/db");
        assert_eq!(parsed.user, "postgres");
    }

    #[test]
    fn parse_uri_defaults_the_port_to_5432() {
        let parsed = parsed("postgres://postgres@127.0.0.1/agentverify_test");
        assert_eq!(parsed.port, 5432);
    }

    #[test]
    fn parse_uri_drops_query_parameters_from_the_database_name() {
        let parsed = parsed(
            "postgres://postgres@127.0.0.1:5433/agentverify_test?sslmode=disable&application_name=av",
        );
        assert_eq!(parsed.database, "agentverify_test");
    }

    #[test]
    fn parse_uri_takes_the_last_at_sign_as_the_userinfo_separator() {
        let parsed = parsed("postgres://svc:p@ss@db.example.com/prod_db");
        assert_eq!(parsed.user, "svc");
        assert_eq!(parsed.password.as_deref(), Some("p@ss"));
        assert_eq!(parsed.host, "db.example.com");
    }

    #[test]
    fn parse_uri_accepts_the_postgresql_scheme() {
        let parsed = parsed("postgresql://postgres@127.0.0.1:5433/agentverify_test");
        assert_eq!(parsed.user, "postgres");
        assert_eq!(parsed.database, "agentverify_test");
    }

    // --- rejected shapes ---

    #[test]
    fn parse_uri_rejects_a_non_postgres_scheme() {
        let rendered = error_of(parse_uri("mysql://u:p@127.0.0.1:5433/db"));
        assert!(
            rendered.contains("Invalid URI scheme"),
            "expected 'Invalid URI scheme', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_a_uri_without_a_userinfo_segment() {
        // No `@`: the URI names no user. libpq would fall back to `PGUSER` or
        // the OS user, which this parser cannot guess at, so it is rejected.
        let rendered = error_of(parse_uri("postgres://127.0.0.1:5433/agentverify_test"));
        assert!(
            rendered.contains("Invalid URI format"),
            "expected 'Invalid URI format', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_too_many_password_separators() {
        let rendered = error_of(parse_uri("postgres://u:p:w@127.0.0.1:5433/db"));
        assert!(
            rendered.contains("Invalid user:password format"),
            "expected 'Invalid user:password format', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_a_missing_database_segment() {
        let rendered = error_of(parse_uri("postgres://u:p@127.0.0.1:5433"));
        assert!(
            rendered.contains("Invalid host:port/database format"),
            "expected 'Invalid host:port/database format', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_a_non_numeric_port() {
        let rendered = error_of(parse_uri("postgres://u:p@127.0.0.1:notaport/db"));
        assert!(
            rendered.contains("Invalid port"),
            "expected 'Invalid port', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_an_out_of_range_port() {
        let rendered = error_of(parse_uri("postgres://u:p@127.0.0.1:99999/db"));
        assert!(
            rendered.contains("Invalid port"),
            "expected 'Invalid port', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_an_invalid_percent_encoded_user() {
        // %FF decodes to a byte that is not valid UTF-8.
        let rendered = error_of(parse_uri("postgres://%FF:pw@127.0.0.1:5433/db"));
        assert!(
            rendered.contains("Invalid user encoding"),
            "expected 'Invalid user encoding', got: {rendered}"
        );
    }

    #[test]
    fn parse_uri_rejects_an_invalid_percent_encoded_password() {
        let rendered = error_of(parse_uri("postgres://u:%FF@127.0.0.1:5433/db"));
        assert!(
            rendered.contains("Invalid password encoding"),
            "expected 'Invalid password encoding', got: {rendered}"
        );
    }

    // --- the rejected shapes also surface through `from_uri` ---

    #[tokio::test]
    async fn from_uri_rejects_a_uri_without_a_userinfo_segment() {
        let rendered = error_of(
            PostgresObserver::from_uri("postgres://127.0.0.1:5433/agentverify_test").await,
        );
        assert!(
            rendered.contains("Invalid URI format"),
            "expected 'Invalid URI format', got: {rendered}"
        );
    }

    #[tokio::test]
    async fn from_uri_rejects_an_invalid_percent_encoded_user() {
        // %FF decodes to a byte that is not valid UTF-8.
        let rendered =
            error_of(PostgresObserver::from_uri("postgres://%FF:pw@127.0.0.1:5433/db").await);
        assert!(
            rendered.contains("Invalid user encoding"),
            "expected 'Invalid user encoding', got: {rendered}"
        );
    }

    // --- error Display strings for every variant ---

    #[test]
    fn error_display_covers_every_variant() {
        let cases: Vec<(PostgresObserverError, Vec<&str>)> = vec![
            (
                PostgresObserverError::Config("missing host".to_string()),
                vec!["Configuration error", "missing host"],
            ),
            (
                PostgresObserverError::PoolCreation("no dbname".to_string()),
                vec!["Pool creation failed", "no dbname"],
            ),
            (
                PostgresObserverError::QueryError("relation not found".to_string()),
                vec!["Query execution failed", "relation not found"],
            ),
            (
                PostgresObserverError::QueryBuildError("bad table".to_string()),
                vec!["Query building failed", "bad table"],
            ),
            (
                PostgresObserverError::ParseError("bad row".to_string()),
                vec!["Result parsing failed", "bad row"],
            ),
            (PostgresObserverError::Timeout, vec!["Connection timeout"]),
            (
                PostgresObserverError::NoPostconditions,
                vec!["No postconditions defined"],
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

    // --- build_uri completeness ---

    #[test]
    fn build_uri_encodes_user_database_and_application_name() {
        let uri = PostgresObserverConfig::new()
            .with_host("10.0.0.8")
            .with_port(6543)
            .with_user("svc user")
            .with_password("p@ss:w")
            .with_database("prod db")
            .with_ssl_mode("verify-full")
            .with_application_name("agentverify it")
            .build_uri();

        assert_eq!(
            uri,
            "postgres://svc%20user:p%40ss%3Aw@10.0.0.8:6543/prod%20db?sslmode=verify-full&application_name=agentverify%20it"
        );
    }

    // --- value conversion against a live PostgreSQL server ---

    /// Live `PostgreSQL` URL; unset on machines without the service container.
    fn live_url() -> Option<String> {
        match std::env::var("AGENTVERIFY_TEST_POSTGRES_URL") {
            Ok(url) if !url.trim().is_empty() => Some(url),
            _ => None,
        }
    }

    fn skip_notice() {
        eprintln!("skipping service test: AGENTVERIFY_TEST_POSTGRES_URL is not set");
    }

    async fn connect(url: &str) -> tokio_postgres::Client {
        let (client, conn) = tokio_postgres::connect(url, NoTls).await.unwrap();
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("postgres connection task ended: {e}");
            }
        });
        client
    }

    const TYPE_MATRIX_TABLE: &str = "av_observe_pg_type_matrix";

    /// One row of every column type the converter is expected to handle.
    async fn seeded_type_matrix(client: &tokio_postgres::Client) {
        let ddl = format!("DROP TABLE IF EXISTS {TYPE_MATRIX_TABLE}");
        client.execute(ddl.as_str(), &[]).await.unwrap();
        let ddl = format!(
            "CREATE TABLE {TYPE_MATRIX_TABLE} (\
             id TEXT PRIMARY KEY, \
             json_payload TEXT, \
             plain_text TEXT, \
             big_int BIGINT, \
             small_int INTEGER, \
             ratio DOUBLE PRECISION, \
             flag BOOLEAN, \
             nullable TEXT)"
        );
        client.execute(ddl.as_str(), &[]).await.unwrap();
        let insert = format!(
            "INSERT INTO {TYPE_MATRIX_TABLE} \
             (id, json_payload, plain_text, big_int, small_int, ratio, flag, nullable) \
             VALUES ('m1', '{{\"a\": 1}}', 'plain text', 9007199254740993, 42, 1.5, true, NULL)"
        );
        client.execute(insert.as_str(), &[]).await.unwrap();
    }

    #[tokio::test]
    async fn pg_value_to_json_maps_every_handled_column_type() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let client = connect(&url).await;
        seeded_type_matrix(&client).await;

        let rows = client
            .query(
                &format!("SELECT * FROM {TYPE_MATRIX_TABLE} WHERE id = 'm1'"),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "seeded row must be present");

        let row = &rows[0];

        // Column name -> expected JSON, one entry per storage type the
        // converter is expected to handle.
        let expected: [(&str, Value, &str); 8] = [
            ("id", Value::String("m1".to_string()), "TEXT literal"),
            (
                "json_payload",
                serde_json::json!({"a": 1}),
                "TEXT holding JSON",
            ),
            (
                "plain_text",
                Value::String("plain text".to_string()),
                "TEXT literal",
            ),
            (
                "big_int",
                Value::Number(9_007_199_254_740_993i64.into()),
                "BIGINT",
            ),
            ("small_int", Value::Number(42.into()), "INTEGER"),
            (
                "ratio",
                Value::Number(serde_json::Number::from_f64(1.5).unwrap()),
                "DOUBLE PRECISION",
            ),
            ("flag", Value::Bool(true), "BOOLEAN"),
            ("nullable", Value::Null, "NULL TEXT"),
        ];

        for (idx, column) in row.columns().iter().enumerate() {
            let name = column.name();
            let value = PostgresObserver::pg_value_to_json(row, idx);
            let Some((_, want, kind)) = expected.iter().find(|(n, _, _)| *n == name) else {
                panic!("unexpected column {name}");
            };
            assert_eq!(&value, want, "column {name} ({kind})");
        }
        assert_eq!(
            row.columns().len(),
            expected.len(),
            "every declared column must be asserted"
        );
    }

    #[tokio::test]
    async fn pg_value_to_json_maps_non_finite_floats_to_null() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let client = connect(&url).await;
        let rows = client
            .query(
                "SELECT 'NaN'::float8 AS nan_col, 'Infinity'::float8 AS inf_col",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);

        let row = &rows[0];
        assert_eq!(PostgresObserver::pg_value_to_json(row, 0), Value::Null);
        assert_eq!(PostgresObserver::pg_value_to_json(row, 1), Value::Null);
    }

    #[tokio::test]
    async fn pg_value_to_json_maps_every_null_column_to_null() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let client = connect(&url).await;
        let rows = client
            .query(
                "SELECT NULL::text AS t, NULL::int8 AS i8, NULL::int4 AS i4, \
                 NULL::float8 AS f8, NULL::bool AS b",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);

        let row = &rows[0];
        for idx in 0..row.columns().len() {
            assert_eq!(
                PostgresObserver::pg_value_to_json(row, idx),
                Value::Null,
                "column {}",
                row.columns()[idx].name()
            );
        }
    }

    /// A backend the server terminates must stop answering, and the background
    /// connection task `connect` spawns must end instead of hanging: that task
    /// is what keeps the client's socket serviced.
    #[tokio::test]
    async fn a_backend_terminated_by_the_server_fails_its_client() {
        let Some(url) = live_url() else {
            skip_notice();
            return;
        };
        let victim = connect(&url).await;
        let executioner = connect(&url).await;

        let pid: i32 = victim
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .unwrap()
            .get(0);
        executioner
            .execute("SELECT pg_terminate_backend($1)", &[&pid])
            .await
            .unwrap();

        // The server closes the terminated backend's socket, so the client
        // reports a connection error rather than answering `SELECT 1`.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut still_serving = true;
        while Instant::now() < deadline {
            if victim.query_one("SELECT 1", &[]).await.is_err() {
                still_serving = false;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            !still_serving,
            "the terminated backend kept serving queries"
        );
    }
}
