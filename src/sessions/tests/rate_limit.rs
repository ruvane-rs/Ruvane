use super::*;
use crate::{
    GuardContext, RateLimitDecision, RateLimitError, RateLimitLimits, RateLimitPolicy, RateLimiter,
};

#[test]
fn rate_subject_resolution_never_extends_idle_lifetime() {
    let store = fake(
        SessionLimits::new(4, 1024, 256, 2, 5).unwrap(),
        (1..20).map(|n| Ok([n; 32])),
    );
    let now = Instant::now();
    at(&store, now);
    let cookie = store.create(data("demo")).unwrap();
    let session = handle(&store, &cookie);
    let limiter = RateLimiter::new(
        RateLimitPolicy::new(100, 1).unwrap(),
        RateLimitLimits::default(),
    );
    let request = request(Some(&cookie));
    let guard = limiter.session_guard(store.clone());
    let context = GuardContext {
        request: &request,
        pattern: Some("/private"),
        page: None,
        private: false,
    };
    at(&store, now + Duration::from_secs(1));
    assert_eq!((guard.0)(&context), GuardDecision::Private);
    assert_eq!(
        limiter.check_session("/private", &session),
        Ok(RateLimitDecision::Allowed)
    );
    at(&store, now + Duration::from_secs(2));
    assert!(store.rate_subject(&request).unwrap().is_none());
    assert_eq!(
        limiter.check_session("/private", &session),
        Err(RateLimitError::InvalidSession)
    );
    assert_eq!((guard.0)(&context), GuardDecision::Allow);
}
