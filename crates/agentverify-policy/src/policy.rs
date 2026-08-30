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
///
/// Serializes as a tagged enum, e.g. `{"type": "prefix", "value": "update_"}`.
/// Deserialization rebuilds the compiled regex from its source pattern, so a
/// round-tripped policy keeps matching.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionPattern {
    /// Exact match
    Exact {
        /// The exact action name
        value: String,
    },
    /// Prefix match
    Prefix {
        /// The action-name prefix
        value: String,
    },
    /// Suffix match
    Suffix {
        /// The action-name suffix
        value: String,
    },
    /// Regex match
    Regex {
        /// The regex pattern source
        pattern: String,
        /// Compiled pattern, built on construction and rebuilt on
        /// deserialization; not serialized
        #[serde(skip)]
        regex: Option<::regex::Regex>,
    },
}

impl<'de> Deserialize<'de> for ActionPattern {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Wire {
            Exact { value: String },
            Prefix { value: String },
            Suffix { value: String },
            Regex { pattern: String },
        }
        match Wire::deserialize(deserializer)? {
            Wire::Exact { value } => Ok(Self::Exact { value }),
            Wire::Prefix { value } => Ok(Self::Prefix { value }),
            Wire::Suffix { value } => Ok(Self::Suffix { value }),
            Wire::Regex { pattern } => {
                let regex = ::regex::Regex::new(&pattern).map_err(serde::de::Error::custom)?;
                Ok(Self::Regex {
                    pattern,
                    regex: Some(regex),
                })
            }
        }
    }
}

impl ActionPattern {
    /// Create an exact match pattern
    pub fn exact(name: impl Into<String>) -> Self {
        Self::Exact { value: name.into() }
    }

    /// Create a prefix match pattern
    pub fn prefix(prefix: impl Into<String>) -> Self {
        Self::Prefix {
            value: prefix.into(),
        }
    }

    /// Create a suffix match pattern
    pub fn suffix(suffix: impl Into<String>) -> Self {
        Self::Suffix {
            value: suffix.into(),
        }
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
            Self::Exact { value } => action_name == value,
            Self::Prefix { value } => action_name.starts_with(value),
            Self::Suffix { value } => action_name.ends_with(value),
            Self::Regex { regex, .. } => regex.as_ref().is_some_and(|r| r.is_match(action_name)),
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
        self.allowed_actions.push(ActionPattern::exact(name));
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

    #[test]
    fn access_level_display_covers_every_level() {
        assert_eq!(AccessLevel::User.to_string(), "user");
        assert_eq!(AccessLevel::Operator.to_string(), "operator");
        assert_eq!(AccessLevel::Admin.to_string(), "admin");
        assert_eq!(AccessLevel::System.to_string(), "system");
    }

    #[test]
    fn access_level_is_ordered_from_user_to_system() {
        // `evaluate_with_access` relies on this ordering to decide whether the
        // caller's level satisfies the policy requirement.
        let levels = [
            AccessLevel::User,
            AccessLevel::Operator,
            AccessLevel::Admin,
            AccessLevel::System,
        ];
        for (lower, higher) in levels.iter().zip(levels.iter().skip(1)) {
            assert!(lower < higher);
            assert!(higher > lower);
            assert_ne!(lower, higher);
        }
        assert_eq!(levels.iter().min(), Some(&AccessLevel::User));
        assert_eq!(levels.iter().max(), Some(&AccessLevel::System));
    }

    #[test]
    fn access_level_default_is_user() {
        assert_eq!(AccessLevel::default(), AccessLevel::User);
    }

    #[test]
    fn policy_decision_accessors_report_outcome() {
        let allowed = PolicyDecision::Allowed;
        assert!(allowed.is_allowed());
        assert!(allowed.violation().is_none());

        let violation = PolicyViolation::ActionBlocked("drop_table".to_string());
        let denied = PolicyDecision::Denied(violation.clone());
        assert!(!denied.is_allowed());
        assert_eq!(denied.violation(), Some(&violation));
    }

    #[test]
    fn policy_config_serde_supplies_defaults_for_missing_fields() {
        let config: PolicyConfig = serde_json::from_str(
            r#"{"default_rate_limit_window": {"secs": 60, "nanos": 0}, "max_buckets": 7}"#,
        )
        .expect("partial config should deserialize");
        assert_eq!(config.max_buckets, 7);
        assert!(config.strict_contract_validation);

        let config: PolicyConfig = serde_json::from_str(
            r#"{"default_rate_limit_window": {"secs": 60, "nanos": 0},
                "strict_contract_validation": false}"#,
        )
        .expect("partial config should deserialize");
        assert_eq!(config.max_buckets, 10_000);
        assert!(!config.strict_contract_validation);

        let config: PolicyConfig =
            serde_json::from_str(r#"{"max_buckets": 1, "strict_contract_validation": true}"#)
                .expect("partial config should deserialize");
        assert_eq!(config.default_rate_limit_window, Duration::from_secs(60));
    }

    #[test]
    fn policy_config_default_matches_documented_values() {
        let config = PolicyConfig::default();
        assert_eq!(config.default_rate_limit_window, Duration::from_secs(60));
        assert_eq!(config.max_buckets, 10_000);
        assert!(config.strict_contract_validation);
    }

    #[test]
    fn policy_serde_defaults_enable_policy() {
        let policy: Policy = serde_json::from_str(r#"{"name": "minimal"}"#)
            .expect("policy with only a name should deserialize");
        assert_eq!(policy.name, "minimal");
        // Serde defaults: enabled, permissive pattern list, no limits.
        assert!(policy.enabled);
        assert!(policy.is_enabled());
        assert!(policy.description.is_empty());
        assert!(policy.allowed_actions.is_empty());
        assert!(policy.blocked_actions.is_empty());
        assert!(policy.rate_limits.is_empty());
        assert!(policy.rate_limits_per_key.is_empty());
        assert!(policy.contract_requirements.is_empty());
        assert!(policy.access_requirements.is_empty());
        // With no allow list the policy defaults to allowing any action.
        assert!(policy.is_action_allowed("anything"));
    }

    #[test]
    fn policy_serde_roundtrips_builder_state() {
        let policy = Policy::new("builder_policy")
            .with_description("guards destructive actions")
            .allow_action_pattern(ActionPattern::regex("^delete_.+$").expect("valid regex"))
            .block_action_name("purge_all")
            .with_rate_limit("create_user", 5, Duration::from_secs(30))
            .with_rate_limit_per_key("create_user", 1, Duration::from_secs(10))
            .require_contract_for_action("create_user")
            .require_access_level("delete_", AccessLevel::Operator);

        let json = serde_json::to_string(&policy).expect("policy should serialize");
        let parsed: Policy = serde_json::from_str(&json).expect("policy should deserialize");

        assert_eq!(parsed.name, "builder_policy");
        assert_eq!(parsed.description, "guards destructive actions");
        assert!(parsed.enabled);
        assert_eq!(
            parsed.get_rate_limit("create_user").map(|l| l.max_count),
            Some(5)
        );
        assert_eq!(
            parsed
                .get_rate_limit_per_key("create_user")
                .map(|l| l.window),
            Some(Duration::from_secs(10))
        );
        assert!(parsed.is_contract_required("create_user"));
        assert!(!parsed.is_contract_required("delete_user"));
        assert_eq!(
            parsed.get_required_access_level("delete_"),
            Some(AccessLevel::Operator)
        );
        assert_eq!(parsed.get_required_access_level("unknown"), None);
        // `purge_all` is blocked, so it is denied even with an empty allow list.
        assert!(!parsed.is_action_allowed("purge_all"));
    }

    #[test]
    fn rate_limit_tracker_default_creates_empty_tracker() {
        let tracker = RateLimitTracker::default();
        assert!(tracker.get_bucket("nope").is_none());
    }

    #[test]
    fn rate_limit_tracker_cleanup_expired_removes_only_stale_buckets() {
        let fresh_limit = RateLimit::new(10, Duration::from_secs(120));
        let stale_limit = RateLimit::new(10, Duration::from_millis(1));
        let mut tracker = RateLimitTracker::new();

        assert!(tracker.check_rate_limit("fresh", &fresh_limit));
        assert!(tracker.check_rate_limit("stale", &stale_limit));

        // Age the "stale" bucket far past twice its window, which is the
        // retention threshold used by `cleanup_expired`.
        let aged_start = Instant::now()
            .checked_sub(Duration::from_secs(30))
            .expect("30s back from now is representable");
        tracker.buckets.get_mut("stale").unwrap().window_start = aged_start;

        tracker.cleanup_expired();

        assert!(tracker.get_bucket("fresh").is_some());
        assert!(tracker.get_bucket("stale").is_none());
    }

    #[test]
    fn rate_limit_bucket_resets_after_window_expires() {
        let limit = RateLimit::new(1, Duration::from_secs(60));
        let expired_start = Instant::now()
            .checked_sub(Duration::from_secs(120))
            .expect("120s back from now is representable");
        let mut bucket = RateLimitBucket {
            count: 1,
            window_start: expired_start,
            limit,
        };

        // The expired window resets the count rather than denying the action.
        assert!(bucket.check_and_increment());
        assert_eq!(bucket.count, 1);
    }

    #[test]
    fn idempotency_tracker_default_creates_empty_tracker() {
        let mut tracker = IdempotencyRateLimitTracker::default();
        let limit = RateLimit::new(1, Duration::from_secs(60));

        // Keys are scoped per action name, so the same key on a different
        // action draws from a separate bucket.
        assert!(tracker.check_rate_limit("key-1", "action_a", &limit));
        assert!(!tracker.check_rate_limit("key-1", "action_a", &limit));
        assert!(tracker.check_rate_limit("key-1", "action_b", &limit));
    }

    #[test]
    fn idempotency_tracker_cleanup_expired_prunes_stale_buckets() {
        let limit = RateLimit::new(1, Duration::from_secs(60));
        let mut tracker = IdempotencyRateLimitTracker::new();

        assert!(tracker.check_rate_limit("key-1", "action_a", &limit));
        assert!(!tracker.check_rate_limit("key-1", "action_a", &limit));

        tracker.cleanup_expired();

        // Live buckets survive cleanup, so the limit still holds.
        assert!(!tracker.check_rate_limit("key-1", "action_a", &limit));
    }

    #[test]
    fn action_pattern_regex_roundtrip_rebuilds_compiled_regex() {
        let pattern = ActionPattern::regex("^delete_.+$").expect("valid regex");
        let json = serde_json::to_string(&pattern).expect("regex patterns serialize");
        assert_eq!(json, r#"{"type":"regex","pattern":"^delete_.+$"}"#);

        let parsed: ActionPattern =
            serde_json::from_str(&json).expect("regex patterns deserialize");
        // Deserialization rebuilds the compiled matcher from the pattern
        // source, so the round-tripped policy still matches.
        assert!(parsed.matches("delete_user"));
    }

    #[test]
    fn action_pattern_roundtrip_preserves_every_variant() {
        let patterns = [
            ActionPattern::exact("create_user"),
            ActionPattern::prefix("delete_"),
            ActionPattern::suffix("_admin"),
            ActionPattern::regex("^refund_\\d+$").expect("valid regex"),
        ];
        for pattern in patterns {
            let json = serde_json::to_string(&pattern).expect("patterns serialize");
            let parsed: ActionPattern = serde_json::from_str(&json).expect("patterns deserialize");
            // Round-trip equality via behaviour: the parsed pattern matches
            // exactly the same action names as the original.
            for name in ["create_user", "delete_1", "x_admin", "refund_42", "other"] {
                assert_eq!(
                    pattern.matches(name),
                    parsed.matches(name),
                    "round-trip changed matching for {name}: {json}"
                );
            }
        }
    }

    #[test]
    fn action_pattern_deserialize_rejects_invalid_regex_source() {
        let err =
            serde_json::from_str::<ActionPattern>(r#"{"type":"regex","pattern":"a(unclosed"}"#)
                .expect_err("invalid regex source must be rejected");
        assert!(
            err.to_string().contains("regex parse error"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn action_pattern_regex_rejects_invalid_source() {
        assert!(ActionPattern::regex("a(unclosed").is_err());
    }

    #[test]
    fn policy_is_enabled_reflects_field() {
        let mut policy = Policy::new("toggled");
        assert!(policy.is_enabled());
        policy.enabled = false;
        assert!(!policy.is_enabled());
    }
}
