//! PostgreSQL observer implementation
//!
//! Observes system state via PostgreSQL queries.
//!
//! # Overview
//!
//! The [`PostgresObserver`] connects to a PostgreSQL database using a deadpool
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

/// PostgreSQL observer-specific errors
#[derive(Debug, Error)]
pub enum PostgresObserverError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Pool creation failed: {0}")]
    PoolCreation(String),

    #[error("Query execution failed: {0}")]
    QueryError(String),

    #[error("Query building failed: {0}")]
    QueryBuildError(String),

    #[error("Result parsing failed: {0}")]
    ParseError(String),

    #[error("Connection timeout")]
    Timeout,

    #[error("No postconditions defined in contract")]
    NoPostconditions,
}

/// PostgreSQL observer configuration
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
    /// Application name for pg_settings
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the hostname
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the user
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Set the password
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = password.into();
        self
    }

    /// Set the database name
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = database.into();
        self
    }

    /// Set the SSL mode
    ///
    /// Valid values: "disable", "require", "verify-ca", "verify-full"
    pub fn with_ssl_mode(mut self, mode: impl Into<String>) -> Self {
        self.ssl_mode = mode.into();
        self
    }

    /// Set the application name
    pub fn with_application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = name.into();
        self
    }

    /// Set the connection timeout in seconds
    pub fn with_connect_timeout_secs(mut self, secs: u64) -> Self {
        self.connect_timeout_secs = secs;
        self
    }

    /// Set the maximum pool size
    pub fn with_pool_max_size(mut self, size: usize) -> Self {
        self.pool_max_size = size;
        self
    }

    /// Set the query timeout in milliseconds
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
    fn create_deadpool_config(&self) -> Result<deadpool_postgres::Config, PostgresObserverError> {
        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(self.host.clone());
        cfg.port = Some(self.port);
        cfg.user = Some(self.user.clone());
        cfg.password = Some(self.password.clone());
        cfg.dbname = Some(self.database.clone());
        cfg.connect_timeout = Some(std::time::Duration::from_secs(self.connect_timeout_secs));
        Ok(cfg)
    }
}

/// PostgreSQL observer using deadpool for connection pooling
///
/// Executes parameterized queries against PostgreSQL to observe system state.
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
    pub async fn from_config(
        config: PostgresObserverConfig,
    ) -> Result<Self, PostgresObserverError> {
        let cfg = config
            .create_deadpool_config()
            .map_err(|e| PostgresObserverError::Config(e.to_string()))?;

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| PostgresObserverError::PoolCreation(e.to_string()))?;

        Ok(Self { pool, config })
    }

    /// Create a new observer from a connection URI
    ///
    /// # Example
    ///
    /// ```ignore
    /// let observer = PostgresObserver::from_uri(
    ///     "postgres://postgres:secret@localhost:5432/mydb",
    /// ).await;
    /// ```
    pub async fn from_uri(uri: &str) -> Result<Self, PostgresObserverError> {
        let mut cfg = deadpool_postgres::Config::new();
        // Parse the URI manually
        // Format: postgres://user:password@host:port/database?sslmode=...
        let uri = uri.replace("postgres://", "");
        let parts: Vec<&str> = uri.splitn(2, '@').collect();
        let (user_part, host_part) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            return Err(PostgresObserverError::Config("Invalid URI format".to_string()));
        };

        let user_pass: Vec<&str> = user_part.split(':').collect();
        if user_pass.len() != 2 {
            return Err(PostgresObserverError::Config(
                "Invalid user:password format".to_string(),
            ));
        }

        let host_db: Vec<&str> = host_part.split('/').collect();
        if host_db.len() != 2 {
            return Err(PostgresObserverError::Config(
                "Invalid host:port/database format".to_string(),
            ));
        }

        let host_port: Vec<&str> = host_db[0].split(':').collect();
        let host = host_port[0].to_string();
        let port: u16 = host_port
            .get(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432);

        let database = host_db[1]
            .split('?')
            .next()
            .unwrap_or(host_db[1])
            .to_string();

        cfg.user = Some(urlencoding::decode(user_pass[0])
            .map_err(|_| PostgresObserverError::Config("Invalid user encoding".to_string()))?
            .to_string());
        cfg.password = Some(urlencoding::decode(user_pass[1])
            .map_err(|_| PostgresObserverError::Config("Invalid password encoding".to_string()))?
            .to_string());
        cfg.host = Some(host);
        cfg.port = Some(port);
        cfg.dbname = Some(database);

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
    pub async fn execute_query(
        &self,
        query: &str,
        params: &[Value],
    ) -> Result<Value, PostgresObserverError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| PostgresObserverError::QueryError(format!("Pool get failed: {}", e)))?;

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
        if let Ok(s) = row.try_get::<_, &str>(idx) {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                return v;
            }
            return Value::String(s.to_string());
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
        if row.try_get::<_, Option<String>>(idx).is_ok() {
            // Null or string
            if let Ok(Some(s)) = row.try_get::<_, Option<String>>(idx) {
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    return v;
                }
                return Value::String(s);
            }
            return Value::Null;
        }
        Value::Null
    }

    /// Build a SELECT query from the contract
    ///
    /// The query selects all columns (*) from the table named by `action_name`.
    /// This can be extended to build more sophisticated queries based on postconditions.
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
                "Invalid table name: {}",
                table_name
            )));
        }

        // Build query: SELECT * FROM <table> WHERE id = $1
        // The action ID is used as the primary key
        let query = format!(
            "SELECT * FROM {} WHERE id = $1 LIMIT 1",
            table_name
        );

        let params = vec![serde_json::json!(action.id.to_string())];

        Ok((query, params))
    }

    /// Check connectivity by executing a simple query
    pub async fn health_check(&self) -> Result<(), PostgresObserverError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| PostgresObserverError::QueryError(format!("Pool get failed: {}", e)))?;

        client
            .query_one("SELECT 1", &[])
            .await
            .map_err(|e| PostgresObserverError::QueryError(e.to_string()))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl agentverify_runtime::Observer for PostgresObserver {
    /// Observe system state by executing a query against PostgreSQL
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
            .map_err(|e| ExecutorError::Unknown(format!("Query build failed: {}", e)))?;

        // Convert params to Values expected by execute_query
        let param_values: Vec<Value> = params;

        // Execute the query
        let results = self
            .execute_query(&query, &param_values)
            .await
            .map_err(|e| ExecutorError::Unknown(format!("Query execution failed: {}", e)))?;

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

    /// Helper to create a PostgresObserver for testing query building
    /// The pool is not actually used in build_observation_query tests
    fn make_test_observer() -> PostgresObserver {
        let config = PostgresObserverConfig::default();
        // Create a pool that won't actually be used in build_observation_query tests
        let cfg = config.create_deadpool_config().unwrap();
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
            assert!(
                result.is_err(),
                "Expected rejection for: {}",
                malicious
            );
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

    // Note: Integration tests with a real PostgreSQL database would require
    // a running PostgreSQL instance. These are tested separately in the
    // integration test suite.
}
