//! Integration tests for [`RedisObserver`] against a live Redis server.
//!
//! Every test here runs against a real server so the assertions describe actual
//! wire behaviour. Tests are gated on `AGENTVERIFY_TEST_REDIS_URL`; when it is
//! unset they print a one-line notice and return so CI stays green.
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

use agentverify_core::{Action, Contract, Observation, SourceId};
use agentverify_observe::{RedisObserver, RedisObserverConfig};
use agentverify_runtime::{ExecutorError, Observer};
use deadpool_redis::{Connection, Pool};
use redis::AsyncCommands;
use serde_json::{json, Value};
use std::sync::Arc;

const CLOSED_PORT: u16 = 5999;
const CONCURRENCY: usize = 24;
const COUNTER_KEY: &str = "av_obs_redis_counter";
const ALTERNATE_DB: u8 = 15;

/// Live `Redis` URL, or `None` when the service is not configured.
fn live_url() -> Option<String> {
    match std::env::var("AGENTVERIFY_TEST_REDIS_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            eprintln!("skipping service test: AGENTVERIFY_TEST_REDIS_URL is not set");
            None
        }
    }
}

/// A connection pool shared between the test's own setup commands and the
/// observer under test, so both talk to the same live server.
fn shared_pool(url: &str) -> Pool {
    deadpool_redis::Config::from_url(url)
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .unwrap_or_else(|e| panic!("pool creation for {url} failed: {e}"))
}

async fn connection(pool: &Pool) -> Connection {
    pool.get()
        .await
        .unwrap_or_else(|e| panic!("pool get failed: {e}"))
}

async fn observer_for(url: &str, prefix: &str) -> RedisObserver {
    RedisObserver::new(RedisObserverConfig::new(url).with_key_prefix(prefix))
        .await
        .unwrap_or_else(|e| panic!("observer construction for {url} failed: {e}"))
}

// --- construction ---

#[tokio::test]
async fn new_builds_a_usable_pool_for_a_valid_url() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url, "").await;
    // The pool is lazy, so prove it really works by issuing a command.
    let observation = observer
        .observe(
            &Action::new("ping", json!({})),
            &Contract::new("exists:av_obs_redis_absent"),
        )
        .await
        .unwrap();
    assert_eq!(observation.state, Value::Bool(false));
}

#[tokio::test]
async fn new_rejects_a_malformed_url() {
    // `RedisObserver` has no `Debug` impl, so the error is extracted by matching.
    let rendered =
        match RedisObserver::new(RedisObserverConfig::new("definitely not a redis url")).await {
            Err(e) => e.to_string(),
            Ok(_) => String::from("<unexpected Ok>"),
        };
    assert!(rendered.contains("Pool creation error"), "got: {rendered}");
}

#[tokio::test]
async fn with_pool_shares_the_callers_pool() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn
            .set("av_obs_redis_shared", "via-shared-pool")
            .await
            .unwrap();
    }

    let observer = RedisObserver::with_pool(
        pool.clone(),
        RedisObserverConfig::new(url).with_default_key("av_obs_redis_shared"),
    );
    let observation = observer
        .observe(
            &Action::new("noop", json!({})),
            &Contract::new("get:av_obs_redis_shared"),
        )
        .await
        .unwrap();
    assert_eq!(observation.state, json!("via-shared-pool"));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_shared").await.unwrap();
}

// --- Observer::observe across every supported spec ---

#[tokio::test]
async fn observe_get_returns_a_json_value_parsed_from_the_stored_string() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn
            .set("av_obs_redis_json", r#"{"status": "settled", "n": 3}"#)
            .await
            .unwrap();
    }

    let observer = observer_for(&url, "").await;
    let action = Action::new("settle", json!({"key": "av_obs_redis_json"}));
    let observation = observer
        .observe(&action, &Contract::new("get:av_obs_redis_json"))
        .await
        .unwrap();

    assert_eq!(observation.source, SourceId("redis".to_string()));
    assert_eq!(observation.state, json!({"status": "settled", "n": 3}));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_json").await.unwrap();
}

#[tokio::test]
async fn observe_get_falls_back_to_a_plain_string_for_non_json_payloads() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn
            .set("av_obs_redis_text", "payment settled by operator")
            .await
            .unwrap();
    }

    let observer = observer_for(&url, "").await;
    let observation = observer
        .observe(
            &Action::new("settle", json!({})),
            &Contract::new("get:av_obs_redis_text"),
        )
        .await
        .unwrap();
    assert_eq!(observation.state, json!("payment settled by operator"));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_text").await.unwrap();
}

#[tokio::test]
async fn observe_get_on_a_missing_key_is_unknown_not_failed() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn.del("av_obs_redis_nope").await.unwrap();
    }

    let observer = observer_for(&url, "").await;
    let action = Action::new("settle", json!({}));
    let err: ExecutorError = observer
        .observe(&action, &Contract::new("get:av_obs_redis_nope"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("Key not found"), "got: {err}");
    assert!(matches!(err, ExecutorError::Unknown(_)), "got: {err:?}");
}

#[tokio::test]
async fn observe_exists_tracks_key_presence() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    let observer = observer_for(&url, "").await;
    let action = Action::new("write", json!({}));

    let absent = observer
        .observe(&action, &Contract::new("exists:av_obs_redis_presence"))
        .await
        .unwrap();
    assert_eq!(absent.state, Value::Bool(false));

    {
        let mut conn = connection(&pool).await;
        let _: () = conn.set("av_obs_redis_presence", "1").await.unwrap();
    }

    let present = observer
        .observe(&action, &Contract::new("exists:av_obs_redis_presence"))
        .await
        .unwrap();
    assert_eq!(present.state, Value::Bool(true));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_presence").await.unwrap();
}

#[tokio::test]
async fn observe_scard_counts_real_set_members() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    let observer = observer_for(&url, "").await;
    let action = Action::new("enqueue", json!({}));

    let empty = observer
        .observe(&action, &Contract::new("scard:av_obs_redis_set"))
        .await
        .unwrap();
    assert_eq!(empty.state, json!(0));

    {
        let mut conn = connection(&pool).await;
        let _: usize = conn
            .sadd("av_obs_redis_set", ("job-a", "job-b", "job-c"))
            .await
            .unwrap();
    }

    let populated = observer
        .observe(&action, &Contract::new("scard:av_obs_redis_set"))
        .await
        .unwrap();
    assert_eq!(populated.state, json!(3));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_set").await.unwrap();
}

#[tokio::test]
async fn observe_hget_reads_present_and_absent_fields() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn
            .hset("av_obs_redis_hash", "json_field", r#"{"ok": true}"#)
            .await
            .unwrap();
        let _: () = conn
            .hset("av_obs_redis_hash", "text_field", "raw")
            .await
            .unwrap();
    }

    let observer = observer_for(&url, "").await;
    let action = Action::new("read_hash", json!({}));

    let json_field = observer
        .observe(&action, &Contract::new("hget:av_obs_redis_hash:json_field"))
        .await
        .unwrap();
    assert_eq!(json_field.state, json!({"ok": true}));

    let text_field = observer
        .observe(&action, &Contract::new("hget:av_obs_redis_hash:text_field"))
        .await
        .unwrap();
    assert_eq!(text_field.state, json!("raw"));

    // An absent field is a legitimate observation of "no value", not an error.
    let absent = observer
        .observe(
            &action,
            &Contract::new("hget:av_obs_redis_hash:no_such_field"),
        )
        .await
        .unwrap();
    assert_eq!(absent.state, Value::Null);

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_hash").await.unwrap();
}

#[tokio::test]
async fn observe_hget_without_a_field_is_rejected() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url, "").await;
    let action = Action::new("read_hash", json!({}));
    let err: ExecutorError = observer
        .observe(&action, &Contract::new("hget:av_obs_redis_hash"))
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("hget requires a field"),
        "got: {err}"
    );
}

#[tokio::test]
async fn observe_hgetall_returns_every_field() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn.del("av_obs_redis_full").await.unwrap();
        let _: () = conn
            .hset_multiple("av_obs_redis_full", &[("a", "[1]"), ("b", "raw")])
            .await
            .unwrap();
    }

    let observer = observer_for(&url, "").await;
    let action = Action::new("read_hash", json!({}));
    let observation = observer
        .observe(&action, &Contract::new("hgetall:av_obs_redis_full"))
        .await
        .unwrap();

    assert_eq!(observation.state, json!({"a": [1], "b": "raw"}));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_full").await.unwrap();
}

#[tokio::test]
async fn observe_rejects_an_unknown_operation() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url, "").await;
    let action = Action::new("zread", json!({}));
    let err: ExecutorError = observer
        .observe(&action, &Contract::new("zscore:av_obs_redis_sorted"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unknown operation"), "got: {err}");
    assert!(err.to_string().contains("zscore"), "got: {err}");
}

// --- key resolution and namespacing ---

#[tokio::test]
async fn key_prefix_is_applied_before_the_command_is_issued() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn.del("av_obs_redis_ns:inner").await.unwrap();
        let _: () = conn.del("inner").await.unwrap();
        let _: () = conn
            .set("av_obs_redis_ns:inner", "namespaced")
            .await
            .unwrap();
        let _: () = conn.set("inner", "unnamespaced").await.unwrap();
    }

    let observer = observer_for(&url, "av_obs_redis_ns:").await;
    let action = Action::new("read", json!({}));

    let observation = observer
        .observe(&action, &Contract::new("get:inner"))
        .await
        .unwrap();
    assert_eq!(observation.state, json!("namespaced"));

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_ns:inner").await.unwrap();
    let _: () = conn.del("inner").await.unwrap();
}

#[tokio::test]
async fn action_arguments_key_is_ignored_by_observe() {
    let Some(url) = live_url() else { return };
    let pool = shared_pool(&url);
    {
        let mut conn = connection(&pool).await;
        let _: () = conn.del("av_obs_redis_from_args").await.unwrap();
        let _: () = conn.del("av_obs_redis_from_contract").await.unwrap();
        let _: () = conn.set("av_obs_redis_from_args", "42").await.unwrap();
        let _: () = conn.set("av_obs_redis_from_contract", "7").await.unwrap();
    }

    let observer = observer_for(&url, "").await;
    // `observe` builds its spec purely from `contract.action_name`; the
    // `arguments.key` precedence documented on the observer belongs to
    // `resolve_key`, which is not on the `observe` path at all.
    let action = Action::new("read", json!({"key": "av_obs_redis_from_args"}));
    let observation = observer
        .observe(&action, &Contract::new("get:av_obs_redis_from_contract"))
        .await
        .unwrap();
    assert_eq!(
        observation.state,
        json!(7),
        "the contract's key must win, not the action's arguments.key"
    );

    let mut conn = connection(&pool).await;
    let _: () = conn.del("av_obs_redis_from_args").await.unwrap();
    let _: () = conn.del("av_obs_redis_from_contract").await.unwrap();
}

#[tokio::test]
async fn an_empty_action_name_degrades_to_an_empty_key_lookup() {
    let Some(url) = live_url() else { return };
    let observer = observer_for(&url, "").await;
    let action = Action::new("read", json!({}));

    let err: ExecutorError = observer
        .observe(&action, &Contract::new(""))
        .await
        .unwrap_err();
    // `get:` resolves to the empty key, which is never set.
    assert!(err.to_string().contains("Key not found"), "got: {err}");
}

// --- database isolation and connectivity failures ---

#[tokio::test]
async fn a_different_logical_database_does_not_leak_keys() {
    let Some(url) = live_url() else { return };
    // Write the key into a non-default logical database.
    let other_db_url = format!("{url}/{ALTERNATE_DB}");
    let other_pool = shared_pool(&other_db_url);
    {
        let mut conn = connection(&other_pool).await;
        let _: () = conn.set("av_obs_redis_isolated", "in-db-15").await.unwrap();
    }

    // The observer uses the default database, where the key does not exist.
    let observer = observer_for(&url, "").await;
    let action = Action::new("read", json!({}));
    let err: ExecutorError = observer
        .observe(&action, &Contract::new("get:av_obs_redis_isolated"))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Key not found"),
        "key written to db {ALTERNATE_DB} must not be visible, got: {err}"
    );

    // The same key is readable when the observer is pointed at that database.
    let db_observer = observer_for(&other_db_url, "").await;
    let observation = db_observer
        .observe(&action, &Contract::new("get:av_obs_redis_isolated"))
        .await
        .unwrap();
    assert_eq!(observation.state, json!("in-db-15"));

    let mut conn = connection(&other_pool).await;
    let _: () = conn.del("av_obs_redis_isolated").await.unwrap();
}

#[tokio::test]
async fn observe_fails_against_an_unreachable_host() {
    let observer = observer_for(&format!("redis://127.0.0.1:{CLOSED_PORT}"), "").await;
    let action = Action::new("read", json!({}));
    let err: ExecutorError = observer
        .observe(&action, &Contract::new("get:av_obs_redis_anything"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("Redis pool error"), "got: {err}");
    assert!(matches!(err, ExecutorError::Unknown(_)), "got: {err:?}");
}

// --- concurrency ---

#[tokio::test]
async fn concurrent_writers_are_all_counted_by_concurrent_observers() {
    let Some(url) = live_url() else { return };
    let pool = Arc::new(shared_pool(&url));
    let set_key = format!("{COUNTER_KEY}-set");
    {
        let mut conn = connection(&pool).await;
        let _: () = conn.del(&set_key).await.unwrap();
    }

    let mut handles = Vec::new();
    for i in 0..CONCURRENCY {
        let writer = Arc::clone(&pool);
        let key = set_key.clone();
        let url = url.clone();
        // Each task writes one real member through its own connection and then
        // reads the set cardinality back through the observer under test.
        handles.push(tokio::spawn(async move {
            {
                let mut conn = writer.get().await.unwrap();
                let _: usize = conn.sadd(&key, format!("member-{i}")).await.unwrap();
            }
            let observer =
                RedisObserver::with_pool((*writer).clone(), RedisObserverConfig::new(url));
            let action = Action::new("count_set", json!({}));
            let contract = Contract::new(format!("scard:{key}"));
            observer.observe(&action, &contract).await
        }));
    }

    let mut observed = Vec::new();
    for handle in handles {
        let observation: Observation = handle.await.unwrap().unwrap();
        let count = observation.state.as_u64().unwrap_or_else(|| {
            panic!(
                "scard observation must be numeric, got: {}",
                observation.state
            )
        });
        assert!(
            (1..=CONCURRENCY as u64).contains(&count),
            "observed count must be within the in-flight range, got: {count}"
        );
        observed.push(count);
    }

    // The cardinality only ever grows, so the last observation is the highest.
    let mut sorted = observed.clone();
    sorted.sort_unstable();
    assert_eq!(observed.last().copied(), sorted.last().copied());

    let final_total: u64 = {
        let mut conn = connection(&pool).await;
        let total: u64 = conn.scard(&set_key).await.unwrap();
        total
    };
    assert_eq!(
        final_total, CONCURRENCY as u64,
        "every member must be persisted"
    );

    let mut conn = connection(&pool).await;
    let _: () = conn.del(&set_key).await.unwrap();
}
