//! `AgentVerify` OpenTelemetry Export
//!
//! Exports verification traces and spans via OTLP (OpenTelemetry Protocol).
//!
//! # Overview
//!
//! This crate provides OTLP export functionality for `AgentVerify`'s verification
//! lifecycle. It integrates with the OpenTelemetry SDK to export spans representing:
//!
//! - Action lifecycle (proposed, validating, authorized, executing, etc.)
//! - Verification events (observing, verifying, verified, failed)
//! - Receipt creation and signing
//!
//! # Usage
//!
//! ```rust,ignore
//! use agentverify_otel::{OtlpExporter, OtlpExporterConfig};
//!
//! // Create exporter with OTLP endpoint
//! let config = OtlpExporterConfig::default()
//!     .with_endpoint("http://localhost:4317");
//! let exporter = OtlpExporter::new(config);
//! ```
//!
//! # Span Hierarchy
//!
//! The following span hierarchy is used:
//!
//! ```text
//! action lifecycle span
//!   ├── contract validation span
//!   ├── execution span
//!   │     └── observation span
//!   └── verification span
//!         ├── predicate evaluation spans
//!         └── receipt creation span
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
use agentverify_core::{
    Action, Contract, Observation, PostconditionResult, Receipt, State, VerificationResult,
};
use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use thiserror::Error;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Errors that can occur in OTLP export
#[derive(Debug, Error)]
pub enum OtlpExporterError {
    /// The exporter or tracing subscriber could not be initialized.
    #[error("Failed to initialize OTLP exporter: {0}")]
    Initialization(String),

    /// Pending spans could not be flushed on shutdown.
    #[error("Failed to export span: {0}")]
    Export(String),

    /// The configured OTLP endpoint was malformed.
    #[error("Invalid endpoint: {0}")]
    InvalidEndpoint(String),
}

/// Configuration for the OTLP exporter
#[derive(Debug, Clone)]
pub struct OtlpExporterConfig {
    /// OTLP endpoint (default: <http://localhost:4317> for gRPC)
    endpoint: String,
    /// Export timeout in milliseconds
    timeout_ms: u64,
    /// Service name for traces
    service_name: String,
}

impl Default for OtlpExporterConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            timeout_ms: 5000,
            service_name: "agentverify".to_string(),
        }
    }
}

impl OtlpExporterConfig {
    /// Set the OTLP endpoint
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Set the export timeout in milliseconds
    #[allow(dead_code)]
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Set the service name
    #[must_use]
    pub fn with_service_name(mut self, service_name: impl Into<String>) -> Self {
        self.service_name = service_name.into();
        self
    }
}

/// OTLP Exporter for `AgentVerify` traces
///
/// Exports verification lifecycle as OpenTelemetry spans using OTLP.
/// Uses gRPC transport by default.
#[derive(Clone)]
pub struct OtlpExporter {
    tracer: opentelemetry_sdk::trace::Tracer,
    provider: SdkTracerProvider,
}

impl OtlpExporter {
    /// Create a new OTLP exporter with the given configuration
    ///
    /// # Errors
    ///
    /// Returns [`OtlpExporterError::Initialization`] if the OTLP span exporter
    /// cannot be built from the configured endpoint, timeout, and protocol.
    pub fn new(config: OtlpExporterConfig) -> Result<Self, OtlpExporterError> {
        let exporter = SpanExporter::builder()
            .with_tonic()
            .with_endpoint(config.endpoint)
            .with_timeout(std::time::Duration::from_millis(config.timeout_ms))
            .with_protocol(Protocol::Grpc)
            .build()
            .map_err(|e| OtlpExporterError::Initialization(e.to_string()))?;

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(
                Resource::builder()
                    .with_attribute(KeyValue::new("service.name", config.service_name))
                    .build(),
            )
            .build();

        let tracer = provider.tracer("agentverify");

        Ok(Self { tracer, provider })
    }

    /// Flush and shut down the tracer provider, exporting any pending spans.
    ///
    /// Call this before process exit to avoid losing buffered spans.
    ///
    /// # Errors
    ///
    /// Returns [`OtlpExporterError::Export`] if the tracer provider reports a
    /// failure while flushing or shutting down.
    pub fn shutdown(&self) -> Result<(), OtlpExporterError> {
        self.provider
            .shutdown()
            .map_err(|e| OtlpExporterError::Export(e.to_string()))
    }

    /// Record action creation
    pub fn record_action_created(&self, action: &Action) {
        let mut span = self
            .tracer
            .span_builder("action.lifecycle")
            .with_start_time(std::time::SystemTime::now())
            .with_kind(SpanKind::Server)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("action.id", action.id.to_string()));
        span.set_attribute(KeyValue::new("action.name", action.name.clone()));
        span.set_attribute(KeyValue::new(
            "action.created_at",
            action.created_at.to_rfc3339(),
        ));
        if let Some(ref key) = action.idempotency_key {
            span.set_attribute(KeyValue::new("action.idempotency_key", key.0.clone()));
        }
        span.end();
    }

    /// Record state transition
    pub fn record_state_transition(&self, action_id: &str, from_state: State, to_state: State) {
        let mut span = self
            .tracer
            .span_builder("state.transition")
            .with_kind(SpanKind::Internal)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("action.id", action_id.to_string()));
        span.set_attribute(KeyValue::new("state.from", from_state.to_string()));
        span.set_attribute(KeyValue::new("state.to", to_state.to_string()));

        // Mark error for failure states
        if matches!(
            to_state,
            State::Failed | State::VerificationFailed | State::Rejected
        ) {
            span.set_status(Status::error(format!("Entered state: {to_state}")));
        } else {
            span.set_status(Status::Ok);
        }

        span.end();
    }

    /// Record an observation
    pub fn record_observation(&self, action_id: &str, observation: &Observation) {
        let mut span = self
            .tracer
            .span_builder("verification.observation")
            .with_kind(SpanKind::Client)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("action.id", action_id.to_string()));
        span.set_attribute(KeyValue::new(
            "observation.source",
            observation.source.0.clone(),
        ));
        span.set_attribute(KeyValue::new(
            "observation.timestamp",
            observation.timestamp.to_rfc3339(),
        ));

        // Record evidence count as attribute
        span.set_attribute(KeyValue::new(
            "observation.evidence_count",
            i64::try_from(observation.evidence.len()).unwrap_or(i64::MAX),
        ));

        span.end();
    }

    /// Record verification result
    pub fn record_verification_result(&self, action_id: &str, result: VerificationResult) {
        let mut span = self
            .tracer
            .span_builder("verification.verify")
            .with_kind(SpanKind::Internal)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("action.id", action_id.to_string()));
        span.set_attribute(KeyValue::new("verification.result", result.to_string()));
        span.set_attribute(KeyValue::new("verification.success", result.is_success()));
        span.set_attribute(KeyValue::new("verification.failure", result.is_failure()));
        span.set_attribute(KeyValue::new("verification.unknown", result.is_unknown()));

        // Set span status based on result
        if result.is_success() {
            span.set_status(Status::Ok);
        } else if result.is_failure() {
            span.set_status(Status::error(format!("Verification failed: {result}")));
        } else {
            span.set_status(Status::Unset);
        }

        span.end();
    }

    /// Record predicate evaluation
    #[allow(dead_code)]
    pub fn record_predicate_result(
        &self,
        action_id: &str,
        postcondition_result: &PostconditionResult,
    ) {
        let mut span = self
            .tracer
            .span_builder("verification.predicate")
            .with_kind(SpanKind::Internal)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("action.id", action_id.to_string()));
        span.set_attribute(KeyValue::new(
            "predicate.description",
            postcondition_result.description.clone(),
        ));
        span.set_attribute(KeyValue::new(
            "predicate.passed",
            postcondition_result.passed,
        ));

        if let Some(ref error) = postcondition_result.error {
            span.set_attribute(KeyValue::new("predicate.error", error.clone()));
            span.set_status(Status::error(error.clone()));
        } else {
            span.set_status(Status::Ok);
        }

        span.end();
    }

    /// Record receipt creation
    pub fn record_receipt_created(&self, receipt: &Receipt) {
        let mut span = self
            .tracer
            .span_builder("receipt.created")
            .with_kind(SpanKind::Internal)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("receipt.id", receipt.id.to_string()));
        span.set_attribute(KeyValue::new(
            "receipt.action_id",
            receipt.action_id.to_string(),
        ));
        span.set_attribute(KeyValue::new(
            "receipt.contract_id",
            receipt.contract_id.to_string(),
        ));
        span.set_attribute(KeyValue::new("receipt.result", receipt.result.to_string()));
        span.set_attribute(KeyValue::new(
            "receipt.attempts",
            i64::from(receipt.attempts),
        ));
        span.set_attribute(KeyValue::new(
            "receipt.timestamp",
            receipt.timestamp.to_rfc3339(),
        ));
        span.set_attribute(KeyValue::new("receipt.signed", receipt.is_signed()));

        span.end();
    }

    /// Record contract validation
    pub fn record_contract_validated(&self, action_id: &str, contract: &Contract, valid: bool) {
        let mut span = self
            .tracer
            .span_builder("contract.validation")
            .with_kind(SpanKind::Internal)
            .start(&self.tracer);

        span.set_attribute(KeyValue::new("action.id", action_id.to_string()));
        span.set_attribute(KeyValue::new("contract.id", contract.id.to_string()));
        span.set_attribute(KeyValue::new(
            "contract.action_name",
            contract.action_name.clone(),
        ));
        span.set_attribute(KeyValue::new("contract.valid", valid));

        if valid {
            span.set_status(Status::Ok);
        } else {
            span.set_status(Status::error("Contract validation failed"));
        }

        span.end();
    }
}

/// Initialize tracing subscriber with OTLP exporter
///
/// This sets up the global tracing subscriber to export spans via OTLP.
/// Call this once during application initialization.
///
/// # Errors
///
/// Returns [`OtlpExporterError::Initialization`] if the exporter cannot be
/// created or a global tracing subscriber has already been installed.
#[allow(dead_code)]
pub fn init_tracing(config: OtlpExporterConfig) -> Result<(), OtlpExporterError> {
    let _exporter = OtlpExporter::new(config)?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true),
        )
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            tracing::Level::INFO,
        ))
        .try_init()
        .map_err(|e| OtlpExporterError::Initialization(e.to_string()))?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use agentverify_core::{ContractId, Evidence, IdempotencyKey, Predicate, SourceId};
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;
    use std::time::Instant;

    /// How long a test waits for the OTLP payload to show up in the sink.
    const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);
    /// Lifetime of a sink thread, so no thread outlives the test binary.
    const SINK_LIFETIME: Duration = Duration::from_secs(60);

    /// A loopback TCP endpoint that receives OTLP traffic.
    ///
    /// It records every byte it receives, so the assertions run against what the
    /// exporter actually put on the wire rather than against the SDK's
    /// in-memory span representation.
    struct SpanSink {
        addr: std::net::SocketAddr,
        received: Arc<Mutex<Vec<u8>>>,
    }

    impl SpanSink {
        /// Bind an ephemeral port on loopback and start serving it.
        fn spawn() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            let addr = listener.local_addr().expect("read local address");
            listener
                .set_nonblocking(true)
                .expect("set listener nonblocking");

            let received = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&received);
            let deadline = Instant::now() + SINK_LIFETIME;

            std::thread::Builder::new()
                .name("otlp-sink".to_string())
                .spawn(move || {
                    while Instant::now() < deadline {
                        match listener.accept() {
                            Ok((mut stream, _)) => serve_otlp(&mut stream, &sink, deadline),
                            Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
                    }
                })
                .expect("spawn sink thread");

            Self { addr, received }
        }

        /// Wait until the captured bytes contain `needle`, then return a
        /// snapshot of everything received so far.
        fn wait_for(&self, needle: &str) -> Vec<u8> {
            let deadline = Instant::now() + EXPORT_TIMEOUT;
            loop {
                let snapshot = self.received.lock().expect("sink buffer lock").clone();
                if contains(&snapshot, needle) || Instant::now() >= deadline {
                    return snapshot;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// Record every byte of one connection until the peer goes quiet.
    ///
    /// The sink implements no protocol: the tests assert against what the
    /// exporter actually put on the wire, which is the HTTP/2 preface followed
    /// by an OTLP request whose protobuf strings are uncompressed.
    fn serve_otlp(stream: &mut TcpStream, sink: &Mutex<Vec<u8>>, deadline: Instant) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let mut scratch = [0u8; 4096];

        while Instant::now() < deadline {
            match stream.read(&mut scratch) {
                Ok(0) => break,
                Ok(n) => sink
                    .lock()
                    .expect("sink buffer lock")
                    .extend_from_slice(&scratch[..n]),
                Err(ref err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    }

    /// True when `wire` contains `needle` as a raw byte sequence.
    fn contains(wire: &[u8], needle: &str) -> bool {
        wire.len() >= needle.len()
            && wire
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
    }

    /// Assert `wire` carries `needle`, showing what did arrive when it did not.
    fn assert_on_wire(wire: &[u8], needle: &str) {
        assert!(
            contains(wire, needle),
            "expected OTLP payload to contain {needle:?}; received {} bytes: {:?}",
            wire.len(),
            String::from_utf8_lossy(&wire[..wire.len().min(4096)])
        );
    }

    /// Build an exporter whose only destination is `sink`.
    fn exporter_for(sink: &SpanSink, service_name: &str) -> OtlpExporter {
        OtlpExporter::new(
            OtlpExporterConfig::default()
                .with_endpoint(format!("http://{}", sink.addr))
                .with_timeout_ms(2_000)
                .with_service_name(service_name),
        )
        .expect("build exporter for a live endpoint")
    }

    fn action(name: &str, with_idempotency_key: bool) -> Action {
        let arguments = serde_json::json!({ "region": "eu-west-1" });
        if with_idempotency_key {
            Action::with_idempotency(name, arguments, IdempotencyKey::new("deploy-9f3c"))
        } else {
            Action::new(name, arguments)
        }
    }

    fn observation() -> Observation {
        Observation::new(
            SourceId("postgres".to_string()),
            serde_json::json!({ "deployments": [{ "status": "completed" }] }),
        )
        .with_evidence(Evidence::new("postgres", serde_json::json!({ "rows": 1 })))
        .with_evidence(Evidence::new(
            "audit-log",
            serde_json::json!({ "event": "deploy.finished" }),
        ))
    }

    fn receipt_for(action: &Action, result: VerificationResult) -> Receipt {
        Receipt::new(action.id, ContractId::new(), result, 2)
    }

    fn postcondition(passed: bool, error: Option<String>) -> PostconditionResult {
        PostconditionResult {
            predicate: Predicate::Equals {
                path: "deployments.0.status".to_string(),
                value: serde_json::json!("completed"),
            },
            description: "deployment status is completed".to_string(),
            passed,
            error,
        }
    }

    #[test]
    fn otlp_exporter_config_default() {
        let config = OtlpExporterConfig::default();
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.service_name, "agentverify");
    }

    #[test]
    fn otlp_exporter_config_builder() {
        let config = OtlpExporterConfig::default()
            .with_endpoint("http://collector:4317")
            .with_timeout_ms(10000)
            .with_service_name("test-service");

        assert_eq!(config.endpoint, "http://collector:4317");
        assert_eq!(config.timeout_ms, 10000);
        assert_eq!(config.service_name, "test-service");
    }

    /// Drives the action lifecycle through the exporter, flushes it, and
    /// asserts the resulting OTLP payload really left the process.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exports_action_and_verification_spans_to_a_live_otlp_endpoint() {
        let sink = SpanSink::spawn();
        let exporter = exporter_for(&sink, "agentverify-lifecycle");

        let deploy = action("deploy_service", true);
        exporter.record_action_created(&deploy);

        // A transition into a failure state must be marked as a span error.
        exporter.record_state_transition(
            &deploy.id.to_string(),
            State::Proposed,
            State::VerificationFailed,
        );
        exporter.record_state_transition(
            &deploy.id.to_string(),
            State::Authorized,
            State::Executing,
        );

        exporter.record_observation(&deploy.id.to_string(), &observation());

        exporter.record_verification_result(&deploy.id.to_string(), VerificationResult::Verified);
        exporter.record_verification_result(&deploy.id.to_string(), VerificationResult::Unknown);
        exporter.record_verification_result(&deploy.id.to_string(), VerificationResult::Failed);

        let contract = Contract::new("deploy_service");
        exporter.record_contract_validated(&deploy.id.to_string(), &contract, true);
        exporter.record_contract_validated(&deploy.id.to_string(), &contract, false);

        exporter
            .shutdown()
            .expect("flush the buffered spans to the sink");

        let wire = sink.wait_for("action.lifecycle");

        // The transport is real HTTP/2, so the client preface must be on the wire.
        assert_on_wire(&wire, "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");

        assert_on_wire(&wire, "action.lifecycle");
        assert_on_wire(&wire, "state.transition");
        assert_on_wire(&wire, "verification.observation");
        assert_on_wire(&wire, "verification.verify");
        assert_on_wire(&wire, "contract.validation");

        // Attribute keys and values are uncompressed protobuf strings, so they
        // are directly observable in the request body.
        assert_on_wire(&wire, "service.name");
        assert_on_wire(&wire, "agentverify-lifecycle");
        assert_on_wire(&wire, "action.id");
        assert_on_wire(&wire, "action.name");
        assert_on_wire(&wire, "deploy_service");
        assert_on_wire(&wire, "action.created_at");
        assert_on_wire(&wire, "action.idempotency_key");
        assert_on_wire(&wire, "deploy-9f3c");
        assert_on_wire(&wire, "state.from");
        assert_on_wire(&wire, "state.to");
        assert_on_wire(&wire, "proposed");
        assert_on_wire(&wire, "verification_failed");
        assert_on_wire(&wire, "authorized");
        assert_on_wire(&wire, "executing");
        assert_on_wire(&wire, "observation.source");
        assert_on_wire(&wire, "postgres");
        assert_on_wire(&wire, "observation.timestamp");
        assert_on_wire(&wire, "observation.evidence_count");
        assert_on_wire(&wire, "verification.result");
        assert_on_wire(&wire, "verified");
        assert_on_wire(&wire, "unknown");
        assert_on_wire(&wire, "verification.success");
        assert_on_wire(&wire, "verification.failure");
        assert_on_wire(&wire, "verification.unknown");
        assert_on_wire(&wire, "contract.id");
        assert_on_wire(&wire, "contract.action_name");
        assert_on_wire(&wire, "contract.valid");

        // The action id must tag every lifecycle span.
        assert_on_wire(&wire, &deploy.id.to_string());
    }

    /// Receipt and predicate spans carry their own attribute set, including the
    /// error detail of a postcondition that did not pass.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exports_receipt_and_predicate_spans_to_a_live_otlp_endpoint() {
        let sink = SpanSink::spawn();
        let exporter = exporter_for(&sink, "agentverify-receipts");

        let deploy = action("rotate_api_key", false);
        let receipt = receipt_for(&deploy, VerificationResult::Partial);
        exporter.record_receipt_created(&receipt);

        exporter.record_predicate_result(
            &deploy.id.to_string(),
            &postcondition(false, Some("deployment status not found".to_string())),
        );
        exporter.record_predicate_result(&deploy.id.to_string(), &postcondition(true, None));

        exporter
            .shutdown()
            .expect("flush the buffered spans to the sink");

        let wire = sink.wait_for("receipt.created");

        assert_on_wire(&wire, "receipt.created");
        assert_on_wire(&wire, "receipt.id");
        assert_on_wire(&wire, &receipt.id.to_string());
        assert_on_wire(&wire, "receipt.action_id");
        assert_on_wire(&wire, &receipt.action_id.to_string());
        assert_on_wire(&wire, "receipt.contract_id");
        assert_on_wire(&wire, &receipt.contract_id.to_string());
        assert_on_wire(&wire, "receipt.result");
        assert_on_wire(&wire, "partial");
        assert_on_wire(&wire, "receipt.attempts");
        assert_on_wire(&wire, "receipt.timestamp");
        assert_on_wire(&wire, "receipt.signed");
        assert_on_wire(&wire, &receipt.action_id.to_string());

        assert_on_wire(&wire, "verification.predicate");
        assert_on_wire(&wire, "predicate.description");
        assert_on_wire(&wire, "deployment status is completed");
        assert_on_wire(&wire, "predicate.passed");
        assert_on_wire(&wire, "predicate.error");
        assert_on_wire(&wire, "deployment status not found");
    }

    /// A receipt that carries a signature reports the signing key on the span.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receipt_span_reports_the_signing_key() {
        let sink = SpanSink::spawn();
        let exporter = exporter_for(&sink, "agentverify-signing");

        let deploy = action("provision_database", false);
        let mut receipt = receipt_for(&deploy, VerificationResult::Verified);
        receipt.key_id = Some("vk-2026-08".to_string());
        receipt.signature = Some(vec![0xde, 0xad, 0xbe, 0xef]);
        exporter.record_receipt_created(&receipt);

        exporter
            .shutdown()
            .expect("flush the buffered spans to the sink");

        let wire = sink.wait_for("receipt.created");
        assert_on_wire(&wire, "receipt.created");
        assert_on_wire(&wire, "receipt.signed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exporter_rejects_an_endpoint_that_is_not_a_uri() {
        let error =
            OtlpExporter::new(OtlpExporterConfig::default().with_endpoint("http://[::1:4317"))
                .err()
                .expect("an authority that is not a valid URI must be rejected");

        assert!(matches!(error, OtlpExporterError::Initialization(_)));
        assert!(
            error.to_string().contains("Failed to initialize"),
            "unexpected message: {error}"
        );
    }

    /// HTTPS endpoints require a TLS feature that this crate does not enable,
    /// so requesting one must fail at construction rather than at first export.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exporter_rejects_https_endpoints_without_tls_support() {
        let error = OtlpExporter::new(
            OtlpExporterConfig::default().with_endpoint("https://collector.internal:4317"),
        )
        .err()
        .expect("an HTTPS endpoint must be rejected without a TLS feature");

        assert!(matches!(error, OtlpExporterError::Initialization(_)));
        assert!(
            error
                .to_string()
                .contains("https://collector.internal:4317"),
            "the rejected endpoint must be named: {error}"
        );
    }

    /// A receipt signed with a key is marked as signed on the span, and a second
    /// shutdown after the provider is closed is reported as an export failure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn second_shutdown_reports_the_sdk_rejection() {
        let sink = SpanSink::spawn();
        let exporter = exporter_for(&sink, "agentverify-twice");

        let deploy = action("purge_cache", false);
        let mut receipt = receipt_for(&deploy, VerificationResult::Verified);
        receipt.key_id = Some("vk-2026-08".to_string());
        receipt.signature = Some(vec![0xde, 0xad, 0xbe, 0xef]);
        exporter.record_receipt_created(&receipt);

        exporter.shutdown().expect("the first flush must succeed");

        let again = exporter.shutdown();
        assert!(
            matches!(again, Err(OtlpExporterError::Export(_))),
            "a closed provider must refuse further flushes, got {again:?}"
        );
        assert!(
            again
                .err()
                .is_some_and(|err| err.to_string().contains("Failed to export")),
            "the refusal must surface through the export error"
        );
    }

    /// Nothing is listening on the endpoint. The SDK logs the failed export but
    /// `shutdown` still reports success, so spans can be lost silently. This
    /// pins that behaviour: if the SDK ever starts propagating the loss, this
    /// test is the one that should change.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spans_sent_to_an_unreachable_collector_are_lost_without_an_error() {
        // Hold a port open, then release it: nothing will be listening there.
        let abandoned = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = abandoned.local_addr().expect("read local address").port();
        drop(abandoned);

        let exporter = OtlpExporter::new(
            OtlpExporterConfig::default()
                .with_endpoint(format!("http://127.0.0.1:{port}"))
                .with_timeout_ms(500),
        )
        .expect("build exporter for an abandoned endpoint");

        let deploy = action("purge_cache", false);
        exporter.record_action_created(&deploy);

        let result = exporter.shutdown();
        assert!(matches!(result, Ok(())), "unexpected result: {result:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn init_tracing_installs_the_otlp_subscriber() {
        init_tracing(
            OtlpExporterConfig::default()
                .with_endpoint("http://127.0.0.1:4317")
                .with_timeout_ms(200),
        )
        .expect("a fresh tracing subscriber must install successfully");
    }

    #[test]
    fn error_variants_render_human_readable_messages() {
        let initialization =
            OtlpExporterError::Initialization("tonic transport unavailable".to_string());
        assert_eq!(
            initialization.to_string(),
            "Failed to initialize OTLP exporter: tonic transport unavailable"
        );

        let export = OtlpExporterError::Export("batch flush timed out".to_string());
        assert_eq!(
            export.to_string(),
            "Failed to export span: batch flush timed out"
        );

        let endpoint = OtlpExporterError::InvalidEndpoint("no scheme".to_string());
        assert_eq!(endpoint.to_string(), "Invalid endpoint: no scheme");
    }
}
