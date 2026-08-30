//! AgentVerify OpenTelemetry Export
//!
//! Exports verification traces and spans via OTLP (OpenTelemetry Protocol).
//!
//! # Overview
//!
//! This crate provides OTLP export functionality for AgentVerify's verification
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

use agentverify_core::{
    Action, Contract, Observation, PostconditionResult, Receipt, State, VerificationResult,
};
use opentelemetry::trace::{Span, SpanKind, Status, Tracer};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{ExportConfig, WithExportConfig};
use opentelemetry_sdk::{trace, Resource};
use thiserror::Error;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Errors that can occur in OTLP export
#[derive(Debug, Error)]
pub enum OtlpExporterError {
    #[error("Failed to initialize OTLP exporter: {0}")]
    Initialization(String),

    #[error("Failed to export span: {0}")]
    Export(String),

    #[error("Invalid endpoint: {0}")]
    InvalidEndpoint(String),
}

/// Configuration for the OTLP exporter
#[derive(Debug, Clone)]
pub struct OtlpExporterConfig {
    /// OTLP endpoint (default: http://localhost:4317 for gRPC)
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
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Set the export timeout in milliseconds
    #[allow(dead_code)]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Set the service name
    pub fn with_service_name(mut self, service_name: impl Into<String>) -> Self {
        self.service_name = service_name.into();
        self
    }
}

/// OTLP Exporter for AgentVerify traces
///
/// Exports verification lifecycle as OpenTelemetry spans using OTLP.
/// Uses gRPC transport by default.
#[derive(Clone)]
pub struct OtlpExporter {
    tracer: trace::Tracer,
}

impl OtlpExporter {
    /// Create a new OTLP exporter with the given configuration
    pub fn new(config: OtlpExporterConfig) -> Result<Self, OtlpExporterError> {
        let export_config = ExportConfig {
            endpoint: config.endpoint.clone(),
            timeout: std::time::Duration::from_millis(config.timeout_ms),
            protocol: opentelemetry_otlp::Protocol::Grpc,
        };

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_export_config(export_config),
            )
            .with_trace_config(trace::Config::default().with_resource(Resource::new(vec![
                KeyValue::new("service.name", config.service_name.clone()),
            ])))
            .install_batch(opentelemetry_sdk::runtime::Tokio)
            .map_err(|e| OtlpExporterError::Initialization(e.to_string()))?;

        Ok(Self { tracer })
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
            span.set_status(Status::error(format!("Entered state: {}", to_state)));
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
            observation.evidence.len() as i64,
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
            span.set_status(Status::error(format!("Verification failed: {}", result)));
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
        span.set_attribute(KeyValue::new("receipt.attempts", receipt.attempts as i64));
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
}
