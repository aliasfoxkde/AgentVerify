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
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            State::Committed | State::Rejected | State::Escalated | State::VerificationFailed
        )
    }

    /// Get valid next states
    #[must_use]
    pub fn valid_transitions(&self) -> Vec<State> {
        match self {
            State::Proposed => vec![State::Validating],
            State::Validating => vec![State::Authorized, State::Rejected],
            State::Authorized => vec![State::Executing],
            State::Executing => vec![
                State::Executed,
                State::Failed,
                State::Timeout,
                State::Unknown,
            ],
            State::Executed | State::Timeout | State::Unknown => vec![State::Observing],
            State::Failed | State::Observing | State::Recovered => vec![State::Verifying],
            State::Verifying => vec![State::Verified, State::VerificationFailed],
            State::Verified => vec![State::Committed],
            State::VerificationFailed => vec![State::Recovering],
            State::Recovering => vec![State::Recovered, State::Escalated],
            // Terminal
            State::Rejected | State::Escalated | State::Committed => vec![],
        }
    }

    /// Try to transition to next state
    ///
    /// # Errors
    ///
    /// Returns `StateError::TerminalState` if the current state is terminal,
    /// or `StateError::InvalidTransition` if `next` is not a permitted
    /// successor of the current state.
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Proposed,
        }
    }

    /// Create with specific initial state
    #[must_use]
    pub fn with_state(state: State) -> Self {
        Self { state }
    }

    /// Get current state
    #[must_use]
    pub fn current(&self) -> State {
        self.state
    }

    /// Transition to next state
    ///
    /// # Errors
    ///
    /// Returns the error from [`State::transition`] and leaves the current
    /// state unchanged when the transition is not permitted.
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

    /// Every reachable lifecycle walk, including the failure, timeout and
    /// unknown branches that make UNKNOWN a first-class state.
    #[test]
    fn every_legal_lifecycle_walk_completes() {
        let walks: Vec<Vec<State>> = vec![
            // Straight success
            vec![
                State::Proposed,
                State::Validating,
                State::Authorized,
                State::Executing,
                State::Executed,
                State::Observing,
                State::Verifying,
                State::Verified,
                State::Committed,
            ],
            // UNKNOWN is not failure: observe, verify, commit
            vec![
                State::Proposed,
                State::Validating,
                State::Authorized,
                State::Executing,
                State::Unknown,
                State::Observing,
                State::Verifying,
                State::Verified,
                State::Committed,
            ],
            // TIMEOUT is not failure either
            vec![
                State::Proposed,
                State::Validating,
                State::Authorized,
                State::Executing,
                State::Timeout,
                State::Observing,
                State::Verifying,
                State::Verified,
                State::Committed,
            ],
            // Execution fails outright, observation still verifies success
            vec![
                State::Proposed,
                State::Validating,
                State::Authorized,
                State::Executing,
                State::Failed,
                State::Verifying,
                State::Verified,
                State::Committed,
            ],
            // Verification concludes the action failed: terminal for the action
            vec![
                State::Proposed,
                State::Validating,
                State::Authorized,
                State::Executing,
                State::Executed,
                State::Observing,
                State::Verifying,
                State::VerificationFailed,
            ],
        ];

        for walk in walks {
            let mut sm = StateMachine::new();
            for pair in walk.windows(2) {
                let attempted = sm.advance(pair[1]);
                assert_eq!(
                    attempted.unwrap(),
                    pair[1],
                    "{} -> {} must be allowed",
                    pair[0],
                    pair[1]
                );
                assert_eq!(sm.current(), pair[1]);
            }
            assert_eq!(sm.current(), *walk.last().unwrap());
        }
    }

    /// `Recovering`, `Recovered` and `Escalated` are only reachable by resuming
    /// a persisted state machine, which is exactly what `with_state` models.
    #[test]
    fn recovery_states_are_entered_by_resuming_a_persisted_state() {
        let mut resumed = StateMachine::with_state(State::Recovering);
        resumed.advance(State::Recovered).unwrap();
        resumed.advance(State::Verifying).unwrap();
        resumed.advance(State::Verified).unwrap();
        resumed.advance(State::Committed).unwrap();
        assert_eq!(resumed.current(), State::Committed);
    }

    #[test]
    fn recovering_may_escalate_to_a_human() {
        let mut sm = StateMachine::with_state(State::Recovering);
        sm.advance(State::Escalated).unwrap();
        assert_eq!(sm.current(), State::Escalated);
        assert!(sm.current().is_terminal());
        assert!(sm.advance(State::Recovering).is_err());
    }

    /// `VerificationFailed` is reported as terminal, so the `Recovering`
    /// successor declared in its transition table cannot be reached by
    /// `advance`/`transition`; recovery states are entered by resuming instead.
    #[test]
    fn verification_failed_refuses_every_transition_including_its_declared_successor() {
        assert_eq!(
            State::VerificationFailed.valid_transitions(),
            vec![State::Recovering]
        );
        let attempted = State::VerificationFailed.transition(State::Recovering);
        assert!(
            matches!(
                attempted,
                Err(StateError::TerminalState(State::VerificationFailed))
            ),
            "terminal check wins over the declared successor"
        );
    }

    #[test]
    fn every_state_reports_terminal_status() {
        let dead_ends = [State::Committed, State::Rejected, State::Escalated];
        let live = [
            State::Proposed,
            State::Validating,
            State::Authorized,
            State::Executing,
            State::Executed,
            State::Failed,
            State::Timeout,
            State::Unknown,
            State::Observing,
            State::Verifying,
            State::Verified,
            State::Recovering,
            State::Recovered,
        ];

        for state in dead_ends {
            assert!(state.is_terminal(), "{state} must be terminal");
            assert!(
                state.valid_transitions().is_empty(),
                "{state} must have no successors"
            );
        }
        for state in live {
            assert!(!state.is_terminal(), "{state} must not be terminal");
            assert!(!state.valid_transitions().is_empty(), "{state} is live");
        }

        // VerificationFailed is terminal for the action but still hands off to
        // recovery, which is why it has exactly one successor.
        assert!(State::VerificationFailed.is_terminal());
        assert_eq!(
            State::VerificationFailed.valid_transitions(),
            vec![State::Recovering]
        );
    }

    /// The transition table is exhaustive: any state not listed as a valid
    /// successor must be rejected, which is what keeps the lifecycle honest.
    #[test]
    fn all_undeclared_transitions_are_rejected() {
        let all = [
            State::Proposed,
            State::Validating,
            State::Rejected,
            State::Authorized,
            State::Executing,
            State::Executed,
            State::Failed,
            State::Timeout,
            State::Unknown,
            State::Observing,
            State::Verifying,
            State::Verified,
            State::VerificationFailed,
            State::Recovering,
            State::Recovered,
            State::Escalated,
            State::Committed,
        ];

        for from in all {
            for to in all {
                let attempted = from.transition(to);
                let declared = !from.is_terminal() && from.valid_transitions().contains(&to);
                if declared {
                    assert_eq!(attempted.unwrap(), to, "{from} -> {to} must be allowed");
                } else {
                    let err = attempted.expect_err("undeclared transition must fail");
                    match err {
                        StateError::InvalidTransition { from: f, to: t } => {
                            assert_eq!(f, from);
                            assert_eq!(t, to);
                            assert_eq!(
                                err.to_string(),
                                format!("Invalid transition from {f} to {t}")
                            );
                        }
                        StateError::TerminalState(s) => {
                            assert_eq!(s, from, "only terminal states may refuse this way");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn terminal_states_report_themselves_in_errors() {
        for state in [State::Committed, State::Rejected, State::Escalated] {
            let err = state.transition(State::Proposed).expect_err("terminal");
            assert!(matches!(err, StateError::TerminalState(s) if s == state));
            assert_eq!(
                err.to_string(),
                format!("Action is in terminal state {state}")
            );
        }
        // VerificationFailed is terminal but still leads to Recovering.
        let err = State::VerificationFailed
            .transition(State::Proposed)
            .expect_err("undeclared");
        assert!(matches!(
            err,
            StateError::TerminalState(State::VerificationFailed)
        ));
    }

    #[test]
    fn every_state_displays_as_snake_case() {
        let pairs = [
            (State::Proposed, "proposed"),
            (State::Validating, "validating"),
            (State::Rejected, "rejected"),
            (State::Authorized, "authorized"),
            (State::Executing, "executing"),
            (State::Executed, "executed"),
            (State::Failed, "failed"),
            (State::Timeout, "timeout"),
            (State::Unknown, "unknown"),
            (State::Observing, "observing"),
            (State::Verifying, "verifying"),
            (State::Verified, "verified"),
            (State::VerificationFailed, "verification_failed"),
            (State::Recovering, "recovering"),
            (State::Recovered, "recovered"),
            (State::Escalated, "escalated"),
            (State::Committed, "committed"),
        ];
        for (state, name) in pairs {
            assert_eq!(state.to_string(), name);
        }
    }

    #[test]
    fn state_machine_default_starts_at_proposed() {
        let mut sm = StateMachine::default();
        assert_eq!(sm.current(), State::Proposed);
        sm.advance(State::Validating).unwrap();
        assert_eq!(sm.current(), State::Validating);
    }

    #[test]
    fn with_state_starts_at_requested_state() {
        let mut sm = StateMachine::with_state(State::Observing);
        assert_eq!(sm.current(), State::Observing);
        sm.advance(State::Verifying).unwrap();
        assert_eq!(sm.current(), State::Verifying);
    }

    #[test]
    fn failed_advance_leaves_state_unchanged() {
        let mut sm = StateMachine::new();
        sm.advance(State::Validating).unwrap();
        let before = sm.current();
        assert!(sm.advance(State::Committed).is_err());
        assert_eq!(sm.current(), before);
    }

    #[test]
    fn state_roundtrips_through_serde() {
        let all = [
            State::Proposed,
            State::Rejected,
            State::Executing,
            State::Unknown,
            State::VerificationFailed,
            State::Committed,
        ];
        for state in all {
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, format!(r#""{state}""#));
            let back: State = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn state_machine_is_debug_and_clone() {
        let sm = StateMachine::with_state(State::Verifying);
        let cloned = sm.clone();
        assert_eq!(cloned.current(), sm.current());
        assert!(std::format!("{sm:?}").contains("Verifying"));
    }

    #[test]
    fn state_errors_are_debug_and_cloneable() {
        let err = StateError::InvalidTransition {
            from: State::Proposed,
            to: State::Committed,
        };
        let cloned = err.clone();
        assert!(std::format!("{cloned:?}").contains("InvalidTransition"));
    }
}
