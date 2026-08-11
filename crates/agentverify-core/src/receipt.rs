//! Receipt types for verification evidence

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::id::{ActionId, ContractId, ReceiptId};
use super::observation::Observation;
use super::predicate::Predicate;
use super::verification_result::VerificationResult;

/// Result of a single postcondition evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostconditionResult {
    /// The predicate that was evaluated
    pub predicate: Predicate,
    /// Human-readable description
    pub description: String,
    /// Whether it passed
    pub passed: bool,
    /// Optional error message if evaluation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A signed record of verification outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// Unique identifier
    pub id: ReceiptId,
    /// Action that was verified
    pub action_id: ActionId,
    /// Contract used for verification
    pub contract_id: ContractId,
    /// Verification result
    pub result: VerificationResult,
    /// Number of attempts made
    pub attempts: u32,
    /// Observations made during verification
    #[serde(default)]
    pub observations: Vec<Observation>,
    /// Results of postcondition evaluations
    #[serde(default)]
    pub postcondition_results: Vec<PostconditionResult>,
    /// Ed25519 signature (when signed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<Vec<u8>>,
    /// When the receipt was created
    pub timestamp: DateTime<Utc>,
}

impl Receipt {
    /// Create a new receipt
    pub fn new(
        action_id: ActionId,
        contract_id: ContractId,
        result: VerificationResult,
        attempts: u32,
    ) -> Self {
        Self {
            id: ReceiptId::new(),
            action_id,
            contract_id,
            result,
            attempts,
            observations: Vec::new(),
            postcondition_results: Vec::new(),
            signature: None,
            timestamp: Utc::now(),
        }
    }

    /// Add an observation
    pub fn with_observation(mut self, observation: Observation) -> Self {
        self.observations.push(observation);
        self
    }

    /// Add postcondition result
    pub fn with_postcondition_result(mut self, result: PostconditionResult) -> Self {
        self.postcondition_results.push(result);
        self
    }

    /// Sign the receipt
    pub fn sign(mut self, signature: Vec<u8>) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Check if receipt is signed
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }
}
