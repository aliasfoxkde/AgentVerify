//! `AgentVerify` Contract
//!
//! JSON/YAML contract parsing and validation

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
pub mod contract;

pub use agentverify_core::Contract;
pub use contract::{
    load_file, parse_json, parse_yaml, to_json, to_yaml, validate_contract, ContractContext,
    ContractError, PredicatePath, SourceLocation,
};
