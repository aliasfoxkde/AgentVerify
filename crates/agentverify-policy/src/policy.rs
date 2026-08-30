//! Policy types and builders

use crate::error::PolicyViolation;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Access level for action authorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    /// No special access
    #[default]
    User,
    /// Elevated access
    Operator,
    /// Administrative access
    Admin,
    /// System-level access
    System,
}

impl std::fmt::Display for AccessLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Operator => write!(f, "operator"),
            Self::Admin => write!(f, "admin"),
            Self::System => write!(f, "system"),
        }
    }
}

/// Pattern for matching action names
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionPattern {
    /// Exact match
    Exact(String),
    /// Prefix match
    Prefix(String),
    /// Suffix match
    Suffix(String),
    /// Regex match
    Regex {
        /// The regex pattern source
        pattern: String,
        /// Compiled pattern, built on construction and not serialized
        #[serde(skip)]
        #[serde(default)]
        regex: Option<::regex::Regex>,
    },
}

impl ActionPattern {
    /// Create an exact match pattern
    pub fn exact(name: impl Into<String>) -> Self {
        Self::Exact(name.into())
    }

    /// Create a prefix match pattern
    pub fn prefix(prefix: impl Into<String>) -> Self {
        Self::Prefix(prefix.into())
    }

    /// Create a suffix match pattern
    pub fn suffix(suffix: impl Into<String>) -> Self {
        Self::Suffix(suffix.into())
    }

    /// Create a regex match pattern
    ///
    /// # Errors
    ///
    /// Returns `regex::Error` if `pattern` is not a valid regular expression.
    pub fn regex(pattern: impl Into<String>) -> Result<Self, ::regex::Error> {
        let pattern_str = pattern.into();
        let regex = Some(::regex::Regex::new(&pattern_str)?);
        Ok(Self::Regex {
            pattern: pattern_str,
            regex,
        })
    }

    /// Check if this pattern matches the given action name
    #[must_use]
    pub fn matches(&self, action_name: &str) -> bool {
        match self {
            Self::Exact(name) => action_name == name,
            Self::Prefix(prefix) => action_name.starts_with(prefix),
            Self::Suffix(suffix) => action_name.ends_with(suffix),
            Self::Regex { regex, .. } => regex
                .as_ref()
                .is_some_and(|r| r.is_match(action_name)),
        }
    }
}

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Maximum number of actions allowed in the window
    pub max_count: u32,
    /// Time window for the rate limit
    pub window: Duration,
}

impl RateLimit {
    /// Create a new rate limit
    #[must_use]
    pub fn new(max_count: u32, window: Duration) -> Self {
        Self { max_count, window }
    }
}

/// Contract requirement for an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractRequirement {
    /// The action name this requirement applies to
    pub action_name: String,
    /// Whether the contract is required (true) or optional (false)
    pub required: bool,
}

/// Result of policy evaluation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Action is allowed by this policy
    Allowed,
    /// Action is denied by this policy
    Denied(PolicyViolation),
}

impl PolicyDecision {
    /// Returns true if the action is allowed
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }

    /// Returns the violation if denied
    #[must_use]
    pub fn violation(&self) -> Option<&PolicyViolation> {
        match self {
            Self::Allowed => None,
            Self::Denied(v) => Some(v),
        }
    }
}

impl std::fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allowed => write!(f, "Allowed"),
            Self::Denied(v) => write!(f, "Denied: {v}"),
        }
    }
}

/// Policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    /// Default rate limit window if not specified
    #[serde(default = "default_rate_limit_window")]
    pub default_rate_limit_window: Duration,
    /// Maximum number of rate limit buckets to track
    #[serde(default = "default_max_buckets")]
    pub max_buckets: usize,
    /// Whether to enforce strict contract validation
    #[serde(default = "default_strict_validation")]
    pub strict_contract_validation: bool,
}

fn default_rate_limit_window() -> Duration {
    Duration::from_secs(60)
}

fn default_max_buckets() -> usize {
    10_000
}

fn default_strict_validation() -> bool {
    true
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            default_rate_limit_window: Duration::from_secs(60),
            max_buckets: 10_000,
            strict_contract_validation: true,
        }
    }
}

/// Internal tracking for rate limits
#[derive(Debug, Clone)]
pub struct RateLimitBucket {
    /// Current count in the window
    pub count: u32,
    /// When the window started
    pub window_start: Instant,
    /// The rate limit configuration
    pub limit: RateLimit,
}

impl RateLimitBucket {
    /// Check if a new action is allowed and increment if so
    pub fn check_and_increment(&mut self) -> bool {
        self.reset_if_expired();
        if self.count < self.limit.max_count {
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Reset the bucket if the window has expired
    fn reset_if_expired(&mut self) {
        if self.window_start.elapsed() >= self.limit.window {
            self.count = 0;
            self.window_start = Instant::now();
        }
    }
}

/// Rate limit tracking by action name
#[derive(Debug, Clone)]
pub struct RateLimitTracker {
    /// Buckets indexed by action name
    buckets: HashMap<String, RateLimitBucket>,
}

impl RateLimitTracker {
    /// Create a new tracker
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Get or create a bucket for an action
    fn get_or_create_bucket(
        &mut self,
        action_name: &str,
        limit: &RateLimit,
    ) -> &mut RateLimitBucket {
        self.buckets
            .entry(action_name.to_string())
            .or_insert_with(|| RateLimitBucket {
                count: 0,
                window_start: Instant::now(),
                limit: limit.clone(),
            })
    }

    /// Check if an action is allowed under rate limits and increment if so
    pub fn check_rate_limit(&mut self, action_name: &str, limit: &RateLimit) -> bool {
        let bucket = self.get_or_create_bucket(action_name, limit);
        bucket.check_and_increment()
    }

    /// Get the current bucket for an action name
    pub fn get_bucket(&self, action_name: &str) -> Option<&RateLimitBucket> {
        self.buckets.get(action_name)
    }

    /// Clean up expired buckets
    pub fn cleanup_expired(&mut self) {
        self.buckets
            .retain(|_, bucket| bucket.window_start.elapsed() < bucket.limit.window * 2);
    }
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate limit tracking by idempotency key
#[derive(Debug, Clone)]
pub struct IdempotencyRateLimitTracker {
    /// Buckets indexed by idempotency key
    buckets: HashMap<String, RateLimitBucket>,
}

impl IdempotencyRateLimitTracker {
    /// Create a new tracker
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Check if an action with the given idempotency key is allowed
    pub fn check_rate_limit(&mut self, key: &str, action_name: &str, limit: &RateLimit) -> bool {
        // For per-key limits, we use the action name as part of the key
        // so different actions with same key have separate limits
        let effective_key = format!("{action_name}:{key}");
        let bucket = self
            .buckets
            .entry(effective_key)
            .or_insert_with(|| RateLimitBucket {
                count: 0,
                window_start: Instant::now(),
                limit: limit.clone(),
            });

        bucket.check_and_increment()
    }

    /// Clean up expired buckets
    pub fn cleanup_expired(&mut self) {
        self.buckets
            .retain(|_, bucket| bucket.window_start.elapsed() < bucket.limit.window * 2);
    }
}

impl Default for IdempotencyRateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Unique policy name
    pub name: String,
    /// Description of what this policy controls
    #[serde(default)]
    pub description: String,
    /// Allowed action patterns
    #[serde(default)]
    pub allowed_actions: Vec<ActionPattern>,
    /// Explicitly blocked action names
    #[serde(default)]
    pub blocked_actions: Vec<String>,
    /// Rate limits per action name
    #[serde(default)]
    pub rate_limits: HashMap<String, RateLimit>,
    /// Rate limits per idempotency key
    #[serde(default)]
    pub rate_limits_per_key: HashMap<String, RateLimit>,
    /// Required contract patterns
    #[serde(default)]
    pub contract_requirements: Vec<ContractRequirement>,
    /// Access level requirements per action
    #[serde(default)]
    pub access_requirements: HashMap<String, AccessLevel>,
    /// Whether this policy is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Policy {
    /// Create a new policy with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            allowed_actions: Vec::new(),
            blocked_actions: Vec::new(),
            rate_limits: HashMap::new(),
            rate_limits_per_key: HashMap::new(),
            contract_requirements: Vec::new(),
            access_requirements: HashMap::new(),
            enabled: true,
        }
    }

    /// Add an allowed action name
    #[must_use]
    pub fn allow_action_name(mut self, name: impl Into<String>) -> Self {
        self.allowed_actions.push(ActionPattern::Exact(name.into()));
        self
    }

    /// Add an allowed action pattern
    #[must_use]
    pub fn allow_action_pattern(mut self, pattern: ActionPattern) -> Self {
        self.allowed_actions.push(pattern);
        self
    }

    /// Add a blocked action name
    #[must_use]
    pub fn block_action_name(mut self, name: impl Into<String>) -> Self {
        self.blocked_actions.push(name.into());
        self
    }

    /// Add a rate limit for an action
    #[must_use]
    pub fn with_rate_limit(
        mut self,
        action_name: impl Into<String>,
        max: u32,
        window: Duration,
    ) -> Self {
        self.rate_limits
            .insert(action_name.into(), RateLimit::new(max, window));
        self
    }

    /// Add a rate limit per idempotency key for an action
    #[must_use]
    pub fn with_rate_limit_per_key(
        mut self,
        action_name: impl Into<String>,
        max: u32,
        window: Duration,
    ) -> Self {
        self.rate_limits_per_key
            .insert(action_name.into(), RateLimit::new(max, window));
        self
    }

    /// Require a contract for an action
    #[must_use]
    pub fn require_contract_for_action(mut self, action_name: impl Into<String>) -> Self {
        self.contract_requirements.push(ContractRequirement {
            action_name: action_name.into(),
            required: true,
        });
        self
    }

    /// Require a specific access level for an action
    #[must_use]
    pub fn require_access_level(
        mut self,
        action_name: impl Into<String>,
        level: AccessLevel,
    ) -> Self {
        self.access_requirements.insert(action_name.into(), level);
        self
    }

    /// Set the policy description
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Check if the policy is enabled
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Check if an action name matches any allowed pattern
    #[must_use]
    pub fn is_action_allowed(&self, action_name: &str) -> bool {
        // Empty action name is never allowed
        if action_name.is_empty() {
            return false;
        }

        // Blocked actions take precedence
        if self.blocked_actions.contains(&action_name.to_string()) {
            return false;
        }

        // If no allowed patterns defined, allow by default
        if self.allowed_actions.is_empty() {
            return true;
        }

        // Check if any pattern matches
        self.allowed_actions
            .iter()
            .any(|pattern| pattern.matches(action_name))
    }

    /// Get the rate limit for an action if defined
    #[must_use]
    pub fn get_rate_limit(&self, action_name: &str) -> Option<&RateLimit> {
        self.rate_limits.get(action_name)
    }

    /// Get the per-key rate limit for an action if defined
    #[must_use]
    pub fn get_rate_limit_per_key(&self, action_name: &str) -> Option<&RateLimit> {
        self.rate_limits_per_key.get(action_name)
    }

    /// Check if a contract is required for an action
    #[must_use]
    pub fn is_contract_required(&self, action_name: &str) -> bool {
        self.contract_requirements
            .iter()
            .any(|req| req.action_name == action_name && req.required)
    }

    /// Get the required access level for an action
    #[must_use]
    pub fn get_required_access_level(&self, action_name: &str) -> Option<AccessLevel> {
        self.access_requirements.get(action_name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_pattern_exact() {
        let pattern = ActionPattern::exact("create_user");
        assert!(pattern.matches("create_user"));
        assert!(!pattern.matches("update_user"));
        assert!(!pattern.matches("create"));
    }

    #[test]
    fn test_action_pattern_prefix() {
        let pattern = ActionPattern::prefix("create_");
        assert!(pattern.matches("create_user"));
        assert!(pattern.matches("create_order"));
        assert!(!pattern.matches("update_user"));
    }

    #[test]
    fn test_action_pattern_suffix() {
        let pattern = ActionPattern::suffix("_user");
        assert!(pattern.matches("create_user"));
        assert!(pattern.matches("update_user"));
        assert!(!pattern.matches("createorder"));
    }

    #[test]
    fn test_action_pattern_regex() {
        let pattern = ActionPattern::regex("^delete_.+$").unwrap();
        assert!(pattern.matches("delete_user"));
        assert!(pattern.matches("delete_order"));
        assert!(!pattern.matches("delete_"));
        assert!(!pattern.matches("update_user"));
    }

    #[test]
    fn test_policy_allow_by_default() {
        let policy = Policy::new("empty_policy");
        // Empty allowed list means allow all
        assert!(policy.is_action_allowed("any_action"));
    }

    #[test]
    fn test_policy_blocked_takes_precedence() {
        let policy = Policy::new("blocked_policy")
            .allow_action_name("create_user")
            .block_action_name("create_user");

        // Even though create_user is allowed, blocking takes precedence
        assert!(!policy.is_action_allowed("create_user"));
    }

    #[test]
    fn test_policy_empty_name_not_allowed() {
        let policy = Policy::new("test").allow_action_name("create_user");
        assert!(!policy.is_action_allowed(""));
    }

    #[test]
    fn test_rate_limit_bucket() {
        let limit = RateLimit::new(2, Duration::from_secs(60));
        let mut bucket = RateLimitBucket {
            count: 0,
            window_start: Instant::now(),
            limit,
        };

        assert!(bucket.check_and_increment()); // count = 1
        assert!(bucket.check_and_increment()); // count = 2
        assert!(!bucket.check_and_increment()); // count = 2, denied
    }

    #[test]
    fn test_policy_decision_display() {
        let allowed = PolicyDecision::Allowed;
        assert_eq!(format!("{allowed}"), "Allowed");

        let denied = PolicyDecision::Denied(PolicyViolation::EmptyActionName);
        assert_eq!(format!("{denied}"), "Denied: Action name cannot be empty");
    }
}
