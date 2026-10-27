use crate::sessions::RateSubject;
use crate::{GuardDecision, PageRoute, RetryAfter, RouteGuard, Session, SessionStore};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
    time::Instant,
};

/// Maximum retained quota buckets.
pub const MAX_RATE_LIMIT_ENTRIES: usize = 4096;
/// Maximum aggregate UTF-8 bytes in retained route patterns.
pub const MAX_RATE_LIMIT_SCOPE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum burst capacity and requests replenished per period.
pub const MAX_RATE_LIMIT_REQUESTS: u32 = 1_000_000;
/// Maximum refill period in seconds.
pub const MAX_RATE_LIMIT_PERIOD_SECONDS: u32 = crate::MAX_RETRY_AFTER_SECONDS;

/// A quota error without route, cookie or session values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitError {
    /// Entry or route-byte limits are outside the supported bounds.
    InvalidLimits,
    /// Capacity or refill period is outside the supported bounds.
    InvalidPolicy,
    /// The scope is not a supported route pattern.
    InvalidScope,
    /// A native session handle is stale, revoked, expired or detached.
    InvalidSession,
    /// Retaining another bucket would exceed a memory budget.
    Full,
    /// A limiter or session store lock is unavailable.
    Unavailable,
}
impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "invalid rate limit budgets",
            Self::InvalidPolicy => "invalid rate limit policy",
            Self::InvalidScope => "invalid rate limit scope",
            Self::InvalidSession => "invalid rate limit session",
            Self::Full => "rate limiter capacity exhausted",
            Self::Unavailable => "rate limiter unavailable",
        })
    }
}
impl std::error::Error for RateLimitError {}

/// Token-bucket capacity and steady refill rate. Each scope starts with a full burst.
#[derive(Clone, Copy, Debug)]
pub struct RateLimitPolicy {
    requests: u32,
    period: u32,
}
impl RateLimitPolicy {
    /// Allows a burst of `requests`, replenishing that many tokens per period.
    pub fn new(requests: u32, period_seconds: u32) -> Result<Self, RateLimitError> {
        if !(1..=MAX_RATE_LIMIT_REQUESTS).contains(&requests)
            || !(1..=MAX_RATE_LIMIT_PERIOD_SECONDS).contains(&period_seconds)
        {
            return Err(RateLimitError::InvalidPolicy);
        }
        Ok(Self {
            requests,
            period: period_seconds,
        })
    }
    fn cost(self) -> u128 {
        u128::from(self.period) * 1_000_000_000
    }
    fn full(self) -> u128 {
        self.cost() * u128::from(self.requests)
    }
}

/// Bounds retained bucket count and logical route-pattern bytes.
#[derive(Clone, Copy, Debug)]
pub struct RateLimitLimits {
    entries: usize,
    scope_bytes: usize,
}
impl RateLimitLimits {
    /// Sets positive entry and route-byte budgets. Live debt is never evicted for space.
    pub fn new(entries: usize, scope_bytes: usize) -> Result<Self, RateLimitError> {
        if !(1..=MAX_RATE_LIMIT_ENTRIES).contains(&entries)
            || !(1..=MAX_RATE_LIMIT_SCOPE_BYTES).contains(&scope_bytes)
        {
            return Err(RateLimitError::InvalidLimits);
        }
        Ok(Self {
            entries,
            scope_bytes,
        })
    }
}
impl Default for RateLimitLimits {
    fn default() -> Self {
        Self {
            entries: 1024,
            scope_bytes: 256 * 1024,
        }
    }
}

/// A consumed request allowance or an explicit wait before another attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitDecision {
    /// One request token was consumed.
    Allowed,
    /// No token was consumed; the delay is rounded up to whole seconds.
    Limited(RetryAfter),
}
/// Aggregate counters. Debug and statistics contain no scope or session identifiers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RateLimitStats {
    /// Buckets with outstanding refill debt.
    pub entries: usize,
    /// Logical UTF-8 route-pattern bytes, excluding fixed bucket overhead.
    pub scope_bytes: usize,
    /// Allowed checks, saturating at u64::MAX.
    pub allowed: u64,
    /// Quota denials, saturating at u64::MAX.
    pub limited: u64,
    /// New buckets refused by a capacity budget, saturating at u64::MAX.
    pub capacity_rejections: u64,
}

#[derive(Clone, Eq, PartialEq, Hash)]
enum Scope {
    Route(String),
    Unmatched,
    Anonymous,
}
impl Scope {
    fn bytes(&self) -> usize {
        match self {
            Self::Route(value) => value.len(),
            Self::Unmatched | Self::Anonymous => 0,
        }
    }
}
#[derive(Clone, Eq, PartialEq, Hash)]
struct Key {
    scope: Scope,
    subject: Option<RateSubject>,
}
struct Bucket {
    credits: u128,
    updated: Instant,
}
impl Bucket {
    fn balance(&self, now: Instant, policy: RateLimitPolicy) -> u128 {
        let elapsed = now
            .saturating_duration_since(self.updated)
            .as_nanos()
            .min(policy.cost());
        (self.credits + elapsed * u128::from(policy.requests)).min(policy.full())
    }
}
struct State {
    buckets: HashMap<Key, Bucket>,
    stats: RateLimitStats,
    #[cfg(test)]
    now: Option<Instant>,
}
impl State {
    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(now) = self.now {
            return now;
        }
        Instant::now()
    }
    fn prune(&mut self, now: Instant, policy: RateLimitPolicy) {
        self.buckets
            .retain(|_, bucket| bucket.balance(now, policy) < policy.full());
        self.stats.entries = self.buckets.len();
        self.stats.scope_bytes = self.buckets.keys().map(|key| key.scope.bytes()).sum();
    }
}
struct Inner {
    policy: RateLimitPolicy,
    limits: RateLimitLimits,
    state: Mutex<State>,
}

/// Bounded native token buckets. Clones share state; restarts reset quotas.
/// This limits requests reaching a guard, not socket traffic or handler execution time.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Inner>,
}
impl fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimiter")
            .field("policy", &self.inner.policy)
            .field("limits", &self.inner.limits)
            .finish_non_exhaustive()
    }
}
impl RateLimiter {
    /// Creates an empty limiter without workers, timers or external storage.
    pub fn new(policy: RateLimitPolicy, limits: RateLimitLimits) -> Self {
        Self {
            inner: Arc::new(Inner {
                policy,
                limits,
                state: Mutex::new(State {
                    buckets: HashMap::new(),
                    stats: RateLimitStats::default(),
                    #[cfg(test)]
                    now: None,
                }),
            }),
        }
    }
    /// Consumes the shared quota for a trusted static route pattern.
    /// Do not use raw paths, query values or credentials as scopes.
    pub fn check(&self, scope: &str) -> Result<RateLimitDecision, RateLimitError> {
        self.check_key(Self::scope(scope)?, None)
    }
    /// Consumes a live session family's quota, shared across identifier rotations.
    /// This rechecks the handle without extending its idle lifetime or authenticating a user.
    pub fn check_session(
        &self,
        scope: &str,
        session: &Session,
    ) -> Result<RateLimitDecision, RateLimitError> {
        let scope = Self::scope(scope)?;
        let subject = session
            .rate_subject()
            .map_err(|_| RateLimitError::Unavailable)?
            .ok_or(RateLimitError::InvalidSession)?;
        self.check_key(scope, Some(subject))
    }
    fn scope(scope: &str) -> Result<Scope, RateLimitError> {
        PageRoute::new("quota", scope).map_err(|_| RateLimitError::InvalidScope)?;
        Ok(Scope::Route(scope.into()))
    }
    fn check_key(
        &self,
        scope: Scope,
        subject: Option<RateSubject>,
    ) -> Result<RateLimitDecision, RateLimitError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| RateLimitError::Unavailable)?;
        let now = state.now();
        let policy = self.inner.policy;
        state.prune(now, policy);
        let key = Key { scope, subject };
        if !state.buckets.contains_key(&key) {
            if state.buckets.len() == self.inner.limits.entries
                || state.stats.scope_bytes + key.scope.bytes() > self.inner.limits.scope_bytes
            {
                state.stats.capacity_rejections = state.stats.capacity_rejections.saturating_add(1);
                return Err(RateLimitError::Full);
            }
            state.stats.scope_bytes += key.scope.bytes();
            state.buckets.insert(
                key.clone(),
                Bucket {
                    credits: policy.full(),
                    updated: now,
                },
            );
            state.stats.entries += 1;
        }
        let bucket = state
            .buckets
            .get_mut(&key)
            .ok_or(RateLimitError::Unavailable)?;
        bucket.credits = bucket.balance(now, policy);
        bucket.updated = bucket.updated.max(now);
        if bucket.credits >= policy.cost() {
            bucket.credits -= policy.cost();
            state.stats.allowed = state.stats.allowed.saturating_add(1);
            Ok(RateLimitDecision::Allowed)
        } else {
            let nanos = (policy.cost() - bucket.credits).div_ceil(u128::from(policy.requests));
            let seconds = nanos.div_ceil(1_000_000_000) as u32;
            let retry = RetryAfter::new(seconds).ok_or(RateLimitError::Unavailable)?;
            state.stats.limited = state.stats.limited.saturating_add(1);
            Ok(RateLimitDecision::Limited(retry))
        }
    }
    /// Limits all callers together per selected route pattern, including GET/HEAD and POST.
    /// All unmatched requests share one additional scope. Assets outside App guards are unaffected.
    pub fn guard(&self) -> RouteGuard {
        self.make_guard(None)
    }
    /// Limits each live family per route; anonymous or invalid identifiers share one global quota.
    /// Session resolution does not refresh idle time. Add authorization and CSRF guards separately.
    pub fn session_guard(&self, store: SessionStore) -> RouteGuard {
        self.make_guard(Some(store))
    }
    fn make_guard(&self, store: Option<SessionStore>) -> RouteGuard {
        let limiter = self.clone();
        RouteGuard::new(move |context| {
            let subject = match store
                .as_ref()
                .map(|store| store.rate_subject(context.request()))
                .transpose()
            {
                Ok(subject) => subject.flatten(),
                Err(_) => return GuardDecision::Unavailable,
            };
            let scope = if store.is_some() && subject.is_none() {
                Scope::Anonymous
            } else {
                context
                    .pattern()
                    .map_or(Scope::Unmatched, |pattern| Scope::Route(pattern.into()))
            };
            let private = subject.is_some();
            match limiter.check_key(scope, subject) {
                Ok(RateLimitDecision::Allowed) => {
                    if private {
                        GuardDecision::Private
                    } else {
                        GuardDecision::Allow
                    }
                }
                Ok(RateLimitDecision::Limited(retry)) => GuardDecision::RateLimited(retry),
                Err(_) => GuardDecision::Unavailable,
            }
        })
    }
    /// Drops fully refilled buckets and returns aggregate counters.
    pub fn stats(&self) -> Result<RateLimitStats, RateLimitError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| RateLimitError::Unavailable)?;
        let now = state.now();
        state.prune(now, self.inner.policy);
        Ok(state.stats)
    }
    /// Explicitly resets all quotas while preserving aggregate counters.
    pub fn clear(&self) -> Result<usize, RateLimitError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| RateLimitError::Unavailable)?;
        let count = state.buckets.len();
        state.buckets.clear();
        state.stats.entries = 0;
        state.stats.scope_bytes = 0;
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
