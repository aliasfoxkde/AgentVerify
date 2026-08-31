#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Branch coverage for `agentverify-runtime` failure paths the main suite
//! does not reach.
//!
//! Covered here:
//! - `Executor::execute` rejecting an action whose *pre-dispatch* observation
//!   fails (the `execute_with_executor` variant was already covered),
//! - preconditions and postconditions whose predicate cannot be evaluated at
//!   all (a malformed regular expression), on both execution paths,
//! - `FileIdempotencyStore::new` refusing a base path that cannot host entries,
//! - `FileIdempotencyStore` claiming a key whose entry can neither be read nor
//!   replaced on disk,
//! - `RedisIdempotencyStore` when Redis refuses writes: claims fail closed
//!   instead of reporting a fresh claim.

use agentverify_core::{
    Action, BackoffConfig, BackoffType, Contract as CoreContract, Observation, Predicate,
    RecoveryConfig, RecoveryStrategy, SourceId, VerificationResult,
};
use agentverify_runtime::{
    ActionExecutor, ClaimResult, DispatchError, DispatchOutcome, Executor, ExecutorConfig,
    ExecutorError, FileIdempotencyStore, IdempotencyStore, Observer,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Recovery config that would allow three attempts, so a test can prove a path
/// is terminal rather than merely un-retried because retries are disabled.
fn recovery_allowing_three_attempts() -> RecoveryConfig {
    RecoveryConfig {
        strategy: RecoveryStrategy::VerifyThenRetry,
        max_attempts: 3,
        backoff: Some(BackoffConfig {
            backoff_type: BackoffType::Linear,
            initial: chrono::Duration::milliseconds(1),
            max: chrono::Duration::milliseconds(2),
            multiplier: 2.0,
        }),
        on_unknown: vec![],
    }
}

fn executor_with_retries() -> Executor {
    Executor::with_config(ExecutorConfig {
        verification_timeout_ms: 1_000,
        max_retries: 3,
        verify_before_retry: true,
    })
}

/// Observer that refuses every observation, counting the attempts it refused.
struct UnobservableObserver {
    reason: &'static str,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Observer for UnobservableObserver {
    async fn observe(
        &self,
        _action: &Action,
        _contract: &CoreContract,
    ) -> Result<Observation, ExecutorError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(ExecutorError::Unknown(self.reason.to_string()))
    }
}

/// Observer that always reports the same state from a fixed source.
struct StaticObserver {
    state: serde_json::Value,
}

#[async_trait]
impl Observer for StaticObserver {
    async fn observe(
        &self,
        _action: &Action,
        _contract: &CoreContract,
    ) -> Result<Observation, ExecutorError> {
        Ok(Observation::new(
            SourceId("coverage-fixture".into()),
            self.state.clone(),
        ))
    }
}

/// Action executor that reports a synchronous completion.
struct CompletedDispatch;

#[async_trait]
impl ActionExecutor for CompletedDispatch {
    async fn execute(&self, _action: &Action) -> Result<DispatchOutcome, DispatchError> {
        Ok(DispatchOutcome::Completed)
    }
}

#[tokio::test]
async fn execute_rejects_when_the_pre_dispatch_observation_fails() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observer = Arc::new(UnobservableObserver {
        reason: "state source unavailable",
        calls: Arc::clone(&calls),
    });

    // Both the precondition and the postcondition would hold if any state could
    // be observed, so the only possible reason for the rejection is the failed
    // pre-dispatch observation.
    let contract = CoreContract::new("gate")
        .with_precondition(Predicate::exists("gate"), "gate must be present")
        .with_postcondition(Predicate::exists("gate"), "gate is still present")
        .with_recovery(recovery_allowing_three_attempts());

    let (result, receipt) = executor_with_retries()
        .execute(
            Action::new("gate", serde_json::json!({})),
            contract,
            Some(observer),
        )
        .await
        .expect("a rejected action is a Failed outcome, not an executor error");

    assert_eq!(result, VerificationResult::Failed);
    assert_eq!(receipt.result, VerificationResult::Failed);
    assert_eq!(
        receipt.attempts, 1,
        "an unobservable precondition is terminal, never retried"
    );
    assert!(
        receipt.postcondition_results.is_empty(),
        "postconditions are never evaluated for a rejected action"
    );
    assert!(
        receipt.observations.is_empty(),
        "no observation succeeded, so none can be recorded as evidence"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the pre-dispatch observation is the only attempt made"
    );
}

#[tokio::test]
async fn execute_treats_an_unevaluable_precondition_as_unsatisfied() {
    // "(" is not a valid regular expression, so the precondition cannot be
    // evaluated at all. Fail-closed: an uncheckable precondition must not be
    // treated as satisfied, so the action is rejected before dispatch.
    let contract = CoreContract::new("gate")
        .with_precondition(Predicate::matches("gate", "("), "gate must read open")
        .with_postcondition(Predicate::equals("gate", "open"), "gate is open");

    let observer = Arc::new(StaticObserver {
        state: serde_json::json!({"gate": "open"}),
    });

    let (result, receipt) = executor_with_retries()
        .execute(
            Action::new("gate", serde_json::json!({})),
            contract,
            Some(observer),
        )
        .await
        .expect("an unevaluable precondition rejects the action, it does not fail the executor");

    assert_eq!(result, VerificationResult::Failed);
    assert_eq!(receipt.result, VerificationResult::Failed);
    assert_eq!(receipt.attempts, 1);
    assert!(
        receipt.postcondition_results.is_empty(),
        "a rejected action must not carry postcondition evidence"
    );
}

#[tokio::test]
async fn execute_reports_an_unevaluable_postcondition_as_verification_failed() {
    // The observed state holds a string, so the predicate engine attempts to
    // compile the pattern and fails: no verdict can be produced.
    let contract = CoreContract::new("gate").with_postcondition(
        Predicate::matches("gate", "*invalid("),
        "gate must read open",
    );

    let observer = Arc::new(StaticObserver {
        state: serde_json::json!({"gate": "open"}),
    });

    let error = executor_with_retries()
        .execute(
            Action::new("gate", serde_json::json!({})),
            contract,
            Some(observer),
        )
        .await
        .expect_err("an unevaluable postcondition cannot produce a verdict");

    match error {
        ExecutorError::VerificationFailed(message) => {
            assert!(
                message.contains("Regex error"),
                "the engine's cause must be preserved: {message}"
            );
        }
        other => panic!("expected VerificationFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn execute_with_executor_reports_an_unevaluable_postcondition_as_verification_failed() {
    // Same predicate fault as the simulated path, but through real dispatch, so
    // the failure is known to come from verification rather than from dispatch.
    let contract = CoreContract::new("gate").with_postcondition(
        Predicate::matches("gate", "*invalid("),
        "gate must read open",
    );

    let error = executor_with_retries()
        .execute_with_executor(
            Action::new("gate", serde_json::json!({})),
            contract,
            Arc::new(CompletedDispatch),
            Some(Arc::new(StaticObserver {
                state: serde_json::json!({"gate": "open"}),
            })),
        )
        .await
        .expect_err("an unevaluable postcondition cannot produce a verdict");

    match error {
        ExecutorError::VerificationFailed(message) => {
            assert!(
                message.contains("Regex error"),
                "the engine's cause must be preserved: {message}"
            );
        }
        other => panic!("expected VerificationFailed, got {other:?}"),
    }
}

#[test]
fn file_idempotency_store_refuses_a_base_path_that_cannot_host_entries() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("occupied-by-a-file");
    std::fs::write(&blocker, b"not a directory").unwrap();

    let error = FileIdempotencyStore::new(blocker.clone())
        .expect_err("a regular file is not a usable base directory");
    assert_eq!(
        error.kind(),
        std::io::ErrorKind::AlreadyExists,
        "the store must report why the directory could not be created: {error}"
    );
}

#[test]
fn file_idempotency_store_claims_even_when_the_entry_cannot_be_written_back() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let key = "entry-write-blocked";

    // Discover the on-disk name the store uses for this key by claiming it once.
    {
        let probe = FileIdempotencyStore::new(dir.path()).unwrap();
        let (claim, _) = runtime.block_on(probe.claim_or_check(key));
        assert_eq!(claim, ClaimResult::Claimed);
    }

    let mut entries = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        1,
        "the claim must have been persisted exactly once: {entries:?}"
    );
    let entry_path = entries.swap_remove(0);

    // Occupy that path with a directory: reading the entry fails and replacing
    // it fails too, so the store can neither read nor persist this key.
    std::fs::remove_file(&entry_path).unwrap();
    std::fs::create_dir(&entry_path).unwrap();

    // A fresh store instance has no cached opinion about the key, so it must
    // read from disk, fail to persist its own claim, and still hand the key out.
    let store = FileIdempotencyStore::new(dir.path()).unwrap();
    let (claim, observed) = runtime.block_on(store.claim_or_check(key));
    assert_eq!(
        claim,
        ClaimResult::Claimed,
        "an entry that cannot be read looks unclaimed, so the key is handed out"
    );
    assert_eq!(observed, None);

    // The in-process cache is authoritative from here on.
    let (claim, observed) = runtime.block_on(store.claim_or_check(key));
    assert_eq!(claim, ClaimResult::AlreadyClaimed);
    assert_eq!(observed, None);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "redis"))]
mod redis_write_failures {
    //! Redis paths that need a server whose behaviour is under the test's
    //! control, so failures can be produced deterministically.

    use super::*;
    use agentverify_runtime::{ClaimResult, IdempotencyStore, RedisIdempotencyStore};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Child, Command, Stdio};

    /// RESP-encoded `PING`, used to wait for the private server to come up.
    const PING: &[u8] = b"*1\r\n$4\r\nPING\r\n";
    /// The simple-string reply `PING` produces once the server accepts commands.
    const PONG: &[u8] = b"+PONG\r\n";

    /// A private `redis-server` whose `maxmemory` is already exhausted: reads
    /// keep working, every write is refused with `OOM`.
    struct MemoryCappedRedis {
        child: Child,
        port: u16,
        _dir: tempfile::TempDir,
    }

    impl MemoryCappedRedis {
        /// Start the server, or return `None` (with a notice) when no
        /// `redis-server` binary is available.
        #[allow(clippy::print_stderr)] // test-skip notices are not structured logs
        fn start() -> Option<Self> {
            if std::env::var("AGENTVERIFY_TEST_REDIS_URL")
                .map_or(true, |value| value.trim().is_empty())
            {
                eprintln!(
                    "skipping Redis write-failure test: \
                     AGENTVERIFY_TEST_REDIS_URL is not set"
                );
                return None;
            }

            let binary = std::env::var("AGENTVERIFY_REDIS_SERVER")
                .unwrap_or_else(|_| "redis-server".to_string());
            if Command::new(&binary)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_err()
            {
                eprintln!("skipping Redis write-failure test: {binary} is not available");
                return None;
            }

            for _ in 0..3 {
                // The directory must outlive the server, so it is owned by the
                // guard that is returned alongside the child process.
                let dir = tempfile::tempdir().expect("working directory for the private server");
                let port = free_port();
                let child = Command::new(&binary)
                    .arg("--port")
                    .arg(port.to_string())
                    .arg("--bind")
                    .arg("127.0.0.1")
                    .arg("--save")
                    .arg("")
                    .arg("--appendonly")
                    .arg("no")
                    .arg("--maxmemory")
                    .arg("1")
                    .arg("--maxmemory-policy")
                    .arg("noeviction")
                    .arg("--dir")
                    .arg(dir.path())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("redis-server can be spawned once --version succeeded");

                let mut server = Self {
                    child,
                    port,
                    _dir: dir,
                };

                if server.await_ready() {
                    return Some(server);
                }
            }
            eprintln!("skipping Redis write-failure test: no server could be started");
            None
        }

        fn url(&self) -> String {
            format!("redis://127.0.0.1:{}/", self.port)
        }

        /// Wait until the server answers a `PING` on the wire.
        fn await_ready(&mut self) -> bool {
            for _ in 0..100 {
                if self
                    .child
                    .try_wait()
                    .map_or(true, |status| status.is_some())
                {
                    return false;
                }
                if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.port)) {
                    if stream.write_all(PING).is_ok() {
                        let mut reply = [0u8; PONG.len()];
                        if stream.read_exact(&mut reply).is_ok() && reply == *PONG {
                            return true;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        }
    }

    impl Drop for MemoryCappedRedis {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Claim an ephemeral port and release it for the server to bind.
    fn free_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0))
            .expect("an ephemeral port can always be claimed")
            .local_addr()
            .expect("listener reports its own address")
            .port()
    }

    #[tokio::test]
    async fn redis_claim_fails_closed_when_redis_refuses_writes() {
        let Some(server) = MemoryCappedRedis::start() else {
            return;
        };

        let store = RedisIdempotencyStore::from_url(server.url().as_str(), 300)
            .await
            .expect("a pool can be built against the private server");

        let key = "redis-refused-write";
        let (claim, observed) = store.claim_or_check(key).await;
        assert_eq!(
            claim,
            ClaimResult::AlreadyClaimed,
            "a refused write must not be reported as a freshly claimed key"
        );
        assert_eq!(
            observed, None,
            "nothing could be written, so no outcome can be known"
        );

        // Completing and releasing must not panic even though every write is
        // refused: the store degrades to in-memory semantics and logs.
        store
            .complete(key.to_string(), VerificationResult::Verified)
            .await;
        store.release(key).await;

        // The write is still refused, so nothing was ever stored for the key
        // and no outcome has become visible.
        let (claim, observed) = store.claim_or_check(key).await;
        assert_eq!(claim, ClaimResult::AlreadyClaimed);
        assert_eq!(observed, None);
    }

    #[tokio::test]
    async fn redis_store_from_url_rejects_a_malformed_url() {
        let error = RedisIdempotencyStore::from_url("not-a-redis-url", 300).await;
        assert!(
            error.is_err(),
            "a URL that is not a Redis URI must be rejected when the pool is built"
        );
    }
}
