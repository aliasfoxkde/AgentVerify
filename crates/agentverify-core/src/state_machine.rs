//! State machine for action lifecycle

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Errors that can occur in state transitions
#[derive(Debug, Clone, Error)]
pub enum StateError {
    #[error("Invalid transition from {from} to {to}")]
    InvalidTransition { from: State, to: State },

    #[error("Action is in terminal state {0}")]
    TerminalState(State),
}

/// State in the verification lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Action proposed, not yet validated
    Proposed,
    /// Contract being validated
    Validating,
    /// Contract rejected
    Rejected,
    /// Authorized to proceed
    Authorized,
    /// Action executing
    Executing,
    /// Execution completed, result pending observation
    Executed,
    /// Execution failed
    Failed,
    /// Execution timed out
    Timeout,
    /// Result unknown (ambiguous)
    Unknown,
    /// Observing system state
    Observing,
    /// Verifying postconditions
    Verifying,
    /// Verification succeeded
    Verified,
    /// Verification failed
    VerificationFailed,
    /// Recovering
    Recovering,
    /// Recovery succeeded
    Recovered,
    /// Escalated to human
    Escalated,
    /// Action committed
    Committed,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            State::Proposed => write!(f, "proposed"),
            State::Validating => write!(f, "validating"),
            State::Rejected => write!(f, "rejected"),
            State::Authorized => write!(f, "authorized"),
            State::Executing => write!(f, "executing"),
            State::Executed => write!(f, "executed"),
            State::Failed => write!(f, "failed"),
            State::Timeout => write!(f, "timeout"),
            State::Unknown => write!(f, "unknown"),
            State::Observing => write!(f, "observing"),
            State::Verifying => write!(f, "verifying"),
            State::Verified => write!(f, "verified"),
            State::VerificationFailed => write!(f, "verification_failed"),
            State::Recovering => write!(f, "recovering"),
            State::Recovered => write!(f, "recovered"),
            State::Escalated => write!(f, "escalated"),
            State::Committed => write!(f, "committed"),
        }
    }
}

impl State {
    /// Check if state is terminal
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            State::Committed | State::Rejected | State::Escalated | State::VerificationFailed
        )
    }

    /// Get valid next states
    pub fn valid_transitions(&self) -> Vec<State> {
        match self {
            State::Proposed => vec![State::Validating],
            State::Validating => vec![State::Authorized, State::Rejected],
            State::Rejected => vec![], // Terminal
            State::Authorized => vec![State::Executing],
            State::Executing => vec![
                State::Executed,
                State::Failed,
                State::Timeout,
                State::Unknown,
            ],
            State::Executed => vec![State::Observing],
            State::Failed => vec![State::Verifying],
            State::Timeout => vec![State::Observing],
            State::Unknown => vec![State::Observing],
            State::Observing => vec![State::Verifying],
            State::Verifying => vec![State::Verified, State::VerificationFailed],
            State::Verified => vec![State::Committed],
            State::VerificationFailed => vec![State::Recovering],
            State::Recovering => vec![State::Recovered, State::Escalated],
            State::Recovered => vec![State::Verifying],
            State::Escalated => vec![], // Terminal
            State::Committed => vec![], // Terminal
        }
    }

    /// Try to transition to next state
    pub fn transition(&self, next: State) -> Result<State, StateError> {
        if self.is_terminal() {
            return Err(StateError::TerminalState(*self));
        }
        if self.valid_transitions().contains(&next) {
            Ok(next)
        } else {
            Err(StateError::InvalidTransition {
                from: *self,
                to: next,
            })
        }
    }
}

/// State machine for managing action lifecycle
#[derive(Debug, Clone)]
pub struct StateMachine {
    state: State,
}

impl StateMachine {
    /// Create new state machine in Proposed state
    pub fn new() -> Self {
        Self {
            state: State::Proposed,
        }
    }

    /// Create with specific initial state
    pub fn with_state(state: State) -> Self {
        Self { state }
    }

    /// Get current state
    pub fn current(&self) -> State {
        self.state
    }

    /// Transition to next state
    pub fn advance(&mut self, next: State) -> Result<State, StateError> {
        let next_state = self.state.transition(next)?;
        self.state = next_state;
        Ok(next_state)
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let mut sm = StateMachine::new();
        assert_eq!(sm.current(), State::Proposed);

        sm.advance(State::Validating).unwrap();
        assert_eq!(sm.current(), State::Validating);

        sm.advance(State::Authorized).unwrap();
        assert_eq!(sm.current(), State::Authorized);

        sm.advance(State::Executing).unwrap();
        assert_eq!(sm.current(), State::Executing);

        sm.advance(State::Unknown).unwrap();
        assert_eq!(sm.current(), State::Unknown);

        sm.advance(State::Observing).unwrap();
        assert_eq!(sm.current(), State::Observing);

        sm.advance(State::Verifying).unwrap();
        assert_eq!(sm.current(), State::Verifying);

        sm.advance(State::Verified).unwrap();
        assert_eq!(sm.current(), State::Verified);

        sm.advance(State::Committed).unwrap();
        assert_eq!(sm.current(), State::Committed);
    }

    #[test]
    fn invalid_transition() {
        let mut sm = StateMachine::new();
        sm.advance(State::Validating).unwrap();

        // Can't go from Validating directly to Executing
        assert!(sm.advance(State::Executing).is_err());
    }

    #[test]
    fn terminal_state() {
        let mut sm = StateMachine::new();
        sm.advance(State::Validating).unwrap();
        sm.advance(State::Rejected).unwrap();
        assert_eq!(sm.current(), State::Rejected);

        // Can't transition from terminal state
        assert!(sm.advance(State::Authorized).is_err());
    }
}
