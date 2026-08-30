//! Integration tests for [`PostgresObserver`] against a live `PostgreSQL` server.
//!
//! Every test here runs against a real server so the assertions describe actual
//! wire behaviour. Tests are gated on `AGENTVERIFY_TEST_POSTGRES_URL`; when it is
//! unset they print a one-line notice and return so CI stays green.
//!
//! Note that [`PostgresObserver::from_uri`] requires the `user:password` shape,
//! so the passwordless trust-auth endpoints used here are written with an empty
//! password, e.g. `postgres://postgres:@127.0.0.1:5433/agentverify_test`.
// Test crates may unwrap, panic and write to stderr: these are assertions
// about the system under test, not library error handling.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

use agentverify_core::{Action, Contract, Observation};
use agentverify_observe::{PostgresObserver, PostgresObserverConfig, PostgresObserverError};
use agentverify_runtime::{ExecutorError, Observer};
use serde_json::{json, Value};
use std::sync::Arc;

const CLOSED_PORT: u16 = 5999;
const CONCURRENCY: usize = 24;

/// Live `PostgreSQL` URL, or `None` when the service is not configured.
fn live_url() -> Option<String> {
    match std::env::var("AGENTVERIFY_TEST_POSTGRES_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            eprintln!("skipping service test: AGENTVERIFY_TEST_POSTGRES_URL is not set");
            None
        }
    }
}

/// A direct `tokio_postgres` client, used only to arrange database state.
async fn setup_client(url: &str) -> tokio_postgres::Client {
    let (client, conn) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap_or_else(|e| panic!("setup connection to {url} failed: {e}"));
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("postgres setup connection ended: {e}");
        }
    });
    client
}

/// Reset `<table>` to the schema the observer is pointed at.
async fn create_table(client: &tokio_postgres::Client, table: &str) {
    let sql = format!("DROP TABLE IF EXISTS {table}");
    client.execute(sql.as_str(), &[]).await.unwrap();
    let sql = format!(
        "CREATE TABLE {table} (\
         id TEXT PRIMARY KEY, \
         payload TEXT, \
         status TEXT, \
         revision INTEGER, \
         amount DOUBLE PRECISION, \
         settled BOOLEAN, \
         note TEXT)"
    );
    client.execute(sql.as_str(), &[]).await.unwrap();
}

async fn drop_table(client: &tokio_postgres::Client, table: &str) {
    let sql = format!("DROP TABLE IF EXISTS {table}");
    client.execute(sql.as_str(), &[]).await.unwrap();
}

/// Rewrite the identifier into the form `execute_query` actually binds.
///
/// `execute_query` serialises every bound parameter with `serde_json`, so a
/// JSON string arrives at the server wrapped in double quotes. See
/// `execute_query_binds_json_serialised_parameters` for the proof.
fn bound_id(raw: &str) -> String {
    json!(raw).to_string()
}

/// Rewrite a URI into the `user:password` shape `from_uri` requires.
///
/// A passwordless URI such as `postgres://postgres@host/db` is expressed with an
/// empty password; a URI that already carries a password is left untouched.
fn observer_uri(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    match rest.split_once('@') {
        Some((userinfo, host_part)) if !userinfo.contains(':') => {
            format!("postgres://{userinfo}:@{host_part}")
        }
        _ => url.to_string(),
    }
}

async fn observer_for(url: &str) -> PostgresObserver {
    let uri = observer_uri(url);
    PostgresObserver::from_uri(&uri)
        .await
        .unwrap_or_else(|e| panic!("observer construction from {uri} failed: {e}"))
}

/// Render the error from a fallible construction call.
///
/// `PostgresObserver` has no `Debug` impl, so `Result::unwrap_err` is unavailable
/// and the error has to be pulled out by matching.
async fn construction_error(config: PostgresObserverConfig) -> String {
    match PostgresObserver::from_config(config).await {
        Err(e) => e.to_string(),
        Ok(_) => String::from("<unexpected Ok>"),
    }
}

// --- connectivity ---

#[tokio::test]
async fn from_uri_connects_to_the_parsed_endpoint() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url).await;
    observer.health_check().await.unwrap();
}

#[tokio::test]
async fn from_uri_selects_the_database_named_in_the_uri() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url).await;

    // Pull the expected values back out of the URI so the assertion checks the
    // parser rather than a hard-coded endpoint.
    let rest = url.split_once("://").unwrap_or(("", &url)).1;
    let (userinfo, host_db) = rest
        .split_once('@')
        .unwrap_or_else(|| panic!("uri is missing a userinfo segment: {url}"));
    let (user, host_port_db) = match userinfo.split_once(':') {
        Some((user, _)) => (user, host_db),
        None => (userinfo, host_db),
    };
    let (host_port, database) = host_port_db
        .split_once('/')
        .unwrap_or_else(|| panic!("uri is missing a database segment: {url}"));
    let database = database.split('?').next().unwrap_or(database);

    let rows = observer
        .execute_query("SELECT current_database() AS db, current_user AS usr", &[])
        .await
        .unwrap();
    let row = &rows.as_array().unwrap()[0];

    assert_eq!(row["db"], json!(database), "URI database must be selected");
    assert_eq!(row["usr"], json!(user), "URI user must be applied");
    assert!(!host_port.is_empty());
}

#[tokio::test]
async fn from_config_connects_to_the_same_endpoint() {
    let Some(url) = live_url() else { return };
    // Split the URI so the config path is exercised independently of from_uri.
    let rest = url.split_once("://").unwrap_or(("", &url)).1;
    let (userinfo, host_db) = rest
        .split_once('@')
        .unwrap_or_else(|| panic!("uri is missing a userinfo segment: {url}"));
    let (user, host_port_db) = match userinfo.split_once(':') {
        Some((user, _)) => (user, host_db),
        None => (userinfo, host_db),
    };
    let (host_port, database) = host_port_db
        .split_once('/')
        .unwrap_or_else(|| panic!("uri is missing a database segment: {url}"));
    let (host, port) = host_port.split_once(':').unwrap_or((host_port, "5432"));

    let observer = PostgresObserver::from_config(
        PostgresObserverConfig::new()
            .with_host(host)
            .with_port(port.parse().unwrap())
            .with_user(user)
            .with_database(database)
            .with_application_name("agentverify-postgres-it"),
    )
    .await
    .unwrap();

    observer.health_check().await.unwrap();

    let rows = observer
        .execute_query("SELECT current_database() AS db", &[])
        .await
        .unwrap();
    assert_eq!(rows.as_array().unwrap()[0]["db"], json!(database));
}

#[tokio::test]
async fn health_check_succeeds_repeatedly_on_the_same_pool() {
    let Some(url) = live_url() else { return };
    let observer = Arc::new(observer_for(&url).await);
    for _ in 0..5 {
        observer.health_check().await.unwrap();
    }
}

#[tokio::test]
async fn health_check_fails_against_an_unreachable_host() {
    let observer = PostgresObserver::from_config(
        PostgresObserverConfig::new()
            .with_host("127.0.0.1")
            .with_port(CLOSED_PORT)
            .with_connect_timeout_secs(1),
    )
    .await
    .unwrap();

    // `PostgresObserver` has no `Debug` impl, so the error is extracted here
    // rather than through `Result::unwrap_err`.
    let Err(err) = observer.health_check().await else {
        panic!("health check against a closed port must fail")
    };
    assert!(
        err.to_string().contains("Pool get failed"),
        "expected a pool error, got: {err}"
    );
}

#[tokio::test]
async fn from_config_reports_pool_creation_failure_for_an_empty_database() {
    let err = construction_error(PostgresObserverConfig::new().with_database("")).await;
    assert!(err.contains("Pool creation failed"), "got: {err}");
}

// --- execute_query ---

#[tokio::test]
async fn execute_query_binds_json_serialised_parameters() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url).await;

    // Documents the parameter contract: every JSON value is bound as the text
    // produced by `serde_json::to_string`, so a JSON string arrives wrapped in
    // double quotes. `length()` is used so the value is observed exactly as the
    // server received it, without the JSON re-parse `pg_value_to_json` applies
    // to text columns on the way back out.
    let rows = observer
        .execute_query("SELECT length($1::text) AS n", &[json!("abc")])
        .await
        .unwrap();
    assert_eq!(
        rows,
        json!([{"n": 5}]),
        r#"a JSON string must bind as "abc""#
    );

    // Numbers bind as their JSON text, with no quotes.
    let rows = observer
        .execute_query("SELECT length($1::text) AS n", &[json!(7)])
        .await
        .unwrap();
    assert_eq!(rows, json!([{"n": 1}]));

    // Objects bind as their full JSON document, in compact form.
    let rows = observer
        .execute_query("SELECT length($1::text) AS n", &[json!({"a": 1})])
        .await
        .unwrap();
    assert_eq!(
        rows,
        json!([{"n": 7}]),
        "compact JSON object is 7 characters"
    );

    // And the bound text survives a round trip through the server, where the
    // JSON re-parse turns the stored `"abc"` back into a plain string.
    let rows = observer
        .execute_query("SELECT $1::text AS bound", &[json!("abc")])
        .await
        .unwrap();
    assert_eq!(rows, json!([{"bound": "abc"}]));
}

#[tokio::test]
async fn execute_query_cannot_bind_into_non_text_columns() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_bind").await;

    let observer = observer_for(&url).await;
    // `revision` is an INTEGER column, but `execute_query` always binds Rust
    // `String`s, so the server rejects the parameter type.
    let err = observer
        .execute_query(
            "INSERT INTO av_obs_pg_bind (id, revision) VALUES ($1, $2)",
            &[json!("b1"), json!(1)],
        )
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("Query execution failed"),
        "got: {err}"
    );
    assert!(
        err.to_string().contains("error serializing parameter"),
        "the type mismatch must surface as a parameter error, got: {err}"
    );

    drop_table(&client, "av_obs_pg_bind").await;
}

#[tokio::test]
async fn execute_query_roundtrips_every_supported_column_type() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_types").await;
    client
        .execute(
            "INSERT INTO av_obs_pg_types \
             (id, payload, status, revision, amount, settled, note) \
             VALUES ('r1', '{\"amount\": 250}', 'settled', 3, 19.5, true, NULL)",
            &[],
        )
        .await
        .unwrap();

    let observer = observer_for(&url).await;
    let rows = observer
        .execute_query("SELECT * FROM av_obs_pg_types WHERE id = 'r1'", &[])
        .await
        .unwrap();

    assert_eq!(
        rows,
        json!([{
            "id": "r1",
            "payload": {"amount": 250},
            "status": "settled",
            "revision": 3,
            "amount": 19.5,
            "settled": true,
            "note": null,
        }]),
        "each column type must round-trip to the expected JSON shape"
    );

    drop_table(&client, "av_obs_pg_types").await;
}

#[tokio::test]
async fn execute_query_returns_an_empty_array_when_nothing_matches() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_empty").await;

    let observer = observer_for(&url).await;
    let rows = observer
        .execute_query(
            "SELECT * FROM av_obs_pg_empty WHERE id = 'not-present'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows, json!([]));

    drop_table(&client, "av_obs_pg_empty").await;
}

#[tokio::test]
async fn execute_query_upserts_are_visible_on_reread() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_upsert").await;

    let observer = observer_for(&url).await;
    // Only the three TEXT columns are bound, because `execute_query` always
    // binds Rust `String`s. `revision` is maintained server-side so the test
    // still proves the row is rewritten rather than appended.
    let sql = "\
        INSERT INTO av_obs_pg_upsert (id, payload, status, revision) \
        VALUES ($1, $2, $3, 1) \
        ON CONFLICT (id) DO UPDATE SET payload = EXCLUDED.payload, \
                                      status = EXCLUDED.status, \
                                      revision = av_obs_pg_upsert.revision + 1";

    // Bind the same three parameters twice, with different values, to prove the
    // upsert actually rewrites the row.
    let initial = vec![json!("u1"), json!({ "n": 1 }), json!("pending")];
    observer.execute_query(sql, &initial).await.unwrap();
    let rows = observer
        .execute_query(
            r#"SELECT payload, status, revision FROM av_obs_pg_upsert WHERE id = '"u1"'"#,
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        rows,
        json!([{"payload": {"n": 1}, "status": "pending", "revision": 1}])
    );

    let updated = vec![json!("u1"), json!({ "n": 2 }), json!("settled")];
    observer.execute_query(sql, &updated).await.unwrap();
    let rows = observer
        .execute_query(
            r#"SELECT payload, status, revision FROM av_obs_pg_upsert WHERE id = '"u1"'"#,
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        rows,
        json!([{"payload": {"n": 2}, "status": "settled", "revision": 2}])
    );

    // Exactly one row exists after two writes.
    let rows = observer
        .execute_query("SELECT count(*)::bigint AS n FROM av_obs_pg_upsert", &[])
        .await
        .unwrap();
    assert_eq!(rows, json!([{"n": 1}]));

    drop_table(&client, "av_obs_pg_upsert").await;
}

#[tokio::test]
async fn concurrent_writes_are_all_persisted() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_concurrent").await;
    drop_table(&client, "av_obs_pg_concurrent").await;
    create_table(&client, "av_obs_pg_concurrent").await;

    let observer = Arc::new(observer_for(&url).await);
    let inserts: Vec<_> = (0..CONCURRENCY)
        .map(|i| {
            let observer = Arc::clone(&observer);
            tokio::spawn(async move {
                let sql = format!(
                    "INSERT INTO av_obs_pg_concurrent (id, status) VALUES ('c{i}', 'done')"
                );
                observer.execute_query(&sql, &[]).await.unwrap();
                i
            })
        })
        .collect();

    let mut seen = Vec::new();
    for handle in inserts {
        seen.push(handle.await.unwrap());
    }
    seen.sort_unstable();
    assert_eq!(seen.len(), CONCURRENCY, "every writer must report success");

    let rows = observer
        .execute_query(
            "SELECT count(*)::bigint AS n FROM av_obs_pg_concurrent",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        rows,
        json!([{"n": i64::try_from(CONCURRENCY).unwrap()}]),
        "all rows must survive"
    );

    drop_table(&client, "av_obs_pg_concurrent").await;
}

#[tokio::test]
async fn execute_query_reports_invalid_sql_as_a_query_error() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url).await;

    let err = observer
        .execute_query("SELECT * FROM av_obs_pg_table_that_does_not_exist", &[])
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Query execution failed"),
        "got: {err}"
    );
    // The wrapper carries only the flattened message: `QueryError(String)`
    // stores no cause, so the typed server error is not reachable by callers.
    assert!(
        std::error::Error::source(&err).is_none(),
        "QueryError flattens the cause into a String, so no source is exposed"
    );
    assert!(
        matches!(err, PostgresObserverError::QueryError(_)),
        "got: {err:?}"
    );
}

#[tokio::test]
async fn execute_query_fails_against_an_unreachable_host() {
    let observer = PostgresObserver::from_config(
        PostgresObserverConfig::new()
            .with_host("127.0.0.1")
            .with_port(CLOSED_PORT)
            .with_connect_timeout_secs(1),
    )
    .await
    .unwrap();

    let err = observer.execute_query("SELECT 1", &[]).await.unwrap_err();
    assert!(err.to_string().contains("Pool get failed"), "got: {err}");
}

// --- Observer::observe ---

#[tokio::test]
async fn observe_returns_the_first_row_when_the_target_exists() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_observe").await;

    let action = Action::new("create_payment", json!({"amount": 250}));
    client
        .execute(
            "INSERT INTO av_obs_pg_observe (id, payload, status) VALUES ($1, $2, $3)",
            &[
                &bound_id(&action.id.to_string()),
                &"{\"amount\": 250}",
                &"settled",
            ],
        )
        .await
        .unwrap();

    let observer = observer_for(&url).await;
    let contract = Contract::new("av_obs_pg_observe");
    let observation: Observation = observer.observe(&action, &contract).await.unwrap();

    assert_eq!(
        observation.source,
        agentverify_core::SourceId("postgres".to_string())
    );
    assert_eq!(
        observation.state,
        json!({
            // The row is stored under the JSON-quoted id (that is what
            // `execute_query` binds), and `pg_value_to_json` re-parses that
            // text on the way out, so the observation reports it unquoted.
            "id": action.id.to_string(),
            "payload": {"amount": 250},
            "status": "settled",
            "revision": Value::Null,
            "amount": Value::Null,
            "settled": Value::Null,
            "note": Value::Null,
        })
    );

    drop_table(&client, "av_obs_pg_observe").await;
}

#[tokio::test]
async fn observe_reports_not_found_when_no_row_matches() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_absent").await;

    let action = Action::new("create_payment", json!({}));
    let observer = observer_for(&url).await;
    let contract = Contract::new("av_obs_pg_absent");
    let observation = observer.observe(&action, &contract).await.unwrap();

    assert_eq!(
        observation.state,
        json!({
            "found": false,
            "table": "av_obs_pg_absent",
            "action_id": action.id.to_string(),
        }),
        "an empty result must be reported as an explicit not-found, not an error"
    );

    drop_table(&client, "av_obs_pg_absent").await;
}

#[tokio::test]
async fn observe_sees_rows_written_after_the_observer_was_created() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_late").await;

    let action = Action::new("create_payment", json!({}));
    let observer = observer_for(&url).await;
    let contract = Contract::new("av_obs_pg_late");

    // Nothing written yet.
    let before = observer.observe(&action, &contract).await.unwrap();
    assert_eq!(before.state["found"], json!(false));

    // The real system of record changes underneath the observer.
    client
        .execute(
            "INSERT INTO av_obs_pg_late (id, status) VALUES ($1, $2)",
            &[&bound_id(&action.id.to_string()), &"settled"],
        )
        .await
        .unwrap();

    let after = observer.observe(&action, &contract).await.unwrap();
    assert_eq!(after.state["status"], json!("settled"));

    drop_table(&client, "av_obs_pg_late").await;
}

#[tokio::test]
async fn observe_surfaces_schema_qualified_table_names() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    client
        .execute("CREATE SCHEMA IF NOT EXISTS av_obs", &[])
        .await
        .unwrap();
    let sql = "DROP TABLE IF EXISTS av_obs.events";
    client.execute(sql, &[]).await.unwrap();
    client
        .execute(
            "CREATE TABLE av_obs.events (id TEXT PRIMARY KEY, status TEXT)",
            &[],
        )
        .await
        .unwrap();

    let action = Action::new("emit_event", json!({}));
    client
        .execute(
            "INSERT INTO av_obs.events (id, status) VALUES ($1, $2)",
            &[&bound_id(&action.id.to_string()), &"emitted"],
        )
        .await
        .unwrap();

    let observer = observer_for(&url).await;
    let contract = Contract::new("av_obs.events");
    let observation = observer.observe(&action, &contract).await.unwrap();
    assert_eq!(observation.state["status"], json!("emitted"));

    client
        .execute("DROP TABLE IF EXISTS av_obs.events", &[])
        .await
        .unwrap();
    client
        .execute("DROP SCHEMA IF EXISTS av_obs CASCADE", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn observe_rejects_table_names_that_look_like_sql() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url).await;
    let action = Action::new("attack", json!({}));

    for table in [
        "av_obs_pg_x; DROP TABLE av_obs_pg_x;--",
        "DROP TABLE av_obs_pg_x",
        "av_obs_pg_x--comment",
    ] {
        let contract = Contract::new(table);
        let err: ExecutorError = observer.observe(&action, &contract).await.unwrap_err();
        assert!(
            err.to_string().contains("Query build failed"),
            "table '{table}' must be rejected at build time, got: {err}"
        );
    }
}

#[tokio::test]
async fn observe_surfaces_query_failures_as_unknown() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_gone").await;

    let action = Action::new("create_payment", json!({}));
    let observer = observer_for(&url).await;
    let contract = Contract::new("av_obs_pg_gone");

    // The observation succeeds while the table is present but empty.
    let ok = observer.observe(&action, &contract).await.unwrap();
    assert_eq!(ok.state["found"], json!(false));

    // Once the table disappears the observer must report UNKNOWN, not FAILED:
    // it cannot determine the outcome.
    drop_table(&client, "av_obs_pg_gone").await;
    let err: ExecutorError = observer.observe(&action, &contract).await.unwrap_err();
    assert!(
        err.to_string().contains("Query execution failed"),
        "got: {err}"
    );
    assert!(matches!(err, ExecutorError::Unknown(_)), "got: {err:?}");
}

#[tokio::test]
async fn observe_through_a_shared_pool_from_concurrent_tasks() {
    let Some(url) = live_url() else { return };
    let client = setup_client(&url).await;
    create_table(&client, "av_obs_pg_parallel").await;

    let observer = Arc::new(observer_for(&url).await);
    let actions: Vec<Action> = (0..CONCURRENCY)
        .map(|i| Action::new(format!("action_{i}"), json!({"i": i})))
        .collect();

    let mut handles = Vec::new();
    for action in actions {
        let observer = Arc::clone(&observer);
        handles.push(tokio::spawn(async move {
            let contract = Contract::new("av_obs_pg_parallel");
            observer.observe(&action, &contract).await
        }));
    }

    for handle in handles {
        let observation = handle.await.unwrap().unwrap();
        assert_eq!(observation.state["found"], json!(false));
    }

    drop_table(&client, "av_obs_pg_parallel").await;
}
