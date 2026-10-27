use super::*;
use crate::{
    Cookie, GuardContext, Request, Response, SessionCookie, SessionData, SessionLimits, StatusCode,
};
use std::{sync::Barrier, thread, time::Duration};

fn make_limiter(requests: u32, period: u32) -> RateLimiter {
    RateLimiter::new(
        RateLimitPolicy::new(requests, period).unwrap(),
        RateLimitLimits::default(),
    )
}
fn at(limiter: &RateLimiter, now: Instant) {
    limiter.inner.state.lock().unwrap().now = Some(now);
}
fn limited(seconds: u32) -> RateLimitDecision {
    RateLimitDecision::Limited(RetryAfter::new(seconds).unwrap())
}
fn request(path: &str, cookie: Option<&Cookie>, extra: &str) -> Request {
    let cookie = cookie.map_or_else(String::new, |cookie| {
        format!("Cookie: {}={}\r\n", cookie.name(), cookie.value())
    });
    crate::http::parse(
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{cookie}{extra}\r\n").as_bytes(),
        8192,
        32,
    )
    .unwrap()
}
fn run_guard(guard: &RouteGuard, request: &Request, pattern: Option<&str>) -> GuardDecision {
    (guard.0)(&GuardContext {
        request,
        pattern,
        page: None,
        private: false,
    })
}
#[test]
fn policy_limits_retry_deltas_and_http_headers_are_bounded() {
    for (requests, period) in [
        (0, 1),
        (1, 0),
        (MAX_RATE_LIMIT_REQUESTS + 1, 1),
        (1, MAX_RATE_LIMIT_PERIOD_SECONDS + 1),
    ] {
        assert!(RateLimitPolicy::new(requests, period).is_err());
    }
    for (entries, bytes) in [
        (0, 1),
        (1, 0),
        (MAX_RATE_LIMIT_ENTRIES + 1, 1),
        (1, MAX_RATE_LIMIT_SCOPE_BYTES + 1),
    ] {
        assert!(RateLimitLimits::new(entries, bytes).is_err());
    }
    for seconds in [0, crate::MAX_RETRY_AFTER_SECONDS + 1, u32::MAX] {
        assert_eq!(RetryAfter::new(seconds), None);
    }
    let retry = RetryAfter::new(3).unwrap();
    let plain = Response::too_many_requests(retry);
    assert_eq!(plain.status(), StatusCode::TooManyRequests);
    assert_eq!(plain.cache_control(), "no-store");
    assert_eq!(plain.retry_after(), Some(retry));
    let headers = plain.wire_headers("POST");
    assert!(headers.contains("HTTP/1.1 429 Too Many Requests\r\n"));
    assert!(headers.contains("Retry-After: 3\r\n"));
    assert!(!headers.contains("Ruvane-Action-Rejected:"));
    assert!(
        plain
            .clone()
            .guard_rate_limited(retry, true)
            .wire_headers("POST")
            .contains("Ruvane-Action-Rejected: rate-limit\r\n")
    );
    let changed = plain.with_status(StatusCode::Ok);
    assert_eq!(changed.retry_after(), None);
    assert!(!changed.wire_headers("GET").contains("Retry-After:"));
}
#[test]
fn refill_keeps_fractional_credit_and_denials_do_not_postpone_recovery() {
    let limiter = make_limiter(2, 3);
    let now = Instant::now();
    at(&limiter, now);
    for _ in 0..2 {
        assert_eq!(limiter.check("/save"), Ok(RateLimitDecision::Allowed));
    }
    assert_eq!(limiter.check("/save"), Ok(limited(2)));
    at(&limiter, now + Duration::from_secs(1));
    assert_eq!(limiter.check("/save"), Ok(limited(1)));
    at(&limiter, now + Duration::from_millis(1499));
    assert_eq!(limiter.check("/save"), Ok(limited(1)));
    at(&limiter, now + Duration::from_millis(1500));
    assert_eq!(limiter.check("/save"), Ok(RateLimitDecision::Allowed));
    at(&limiter, now + Duration::from_secs(3));
    assert_eq!(limiter.check("/save"), Ok(RateLimitDecision::Allowed));
    assert_eq!(limiter.stats().unwrap().entries, 1);
    at(&limiter, now + Duration::from_secs(6));
    assert_eq!(limiter.stats().unwrap().entries, 0);
    assert_eq!(limiter.stats().unwrap().allowed, 4);
    assert_eq!(limiter.stats().unwrap().limited, 3);
}
#[test]
fn backwards_clock_samples_never_mint_extra_credit() {
    let limiter = make_limiter(1, 10);
    let now = Instant::now();
    at(&limiter, now);
    assert_eq!(limiter.check("/save"), Ok(RateLimitDecision::Allowed));
    at(&limiter, now + Duration::from_secs(5));
    assert_eq!(limiter.check("/save"), Ok(limited(5)));
    at(&limiter, now + Duration::from_secs(3));
    assert_eq!(limiter.check("/save"), Ok(limited(5)));
    at(&limiter, now + Duration::from_secs(9));
    assert_eq!(limiter.check("/save"), Ok(limited(1)));
    at(&limiter, now + Duration::from_secs(10));
    assert_eq!(limiter.check("/save"), Ok(RateLimitDecision::Allowed));
}
#[test]
fn capacity_never_evicts_active_debt_and_releases_only_full_buckets() {
    let limiter = RateLimiter::new(
        RateLimitPolicy::new(1, 10).unwrap(),
        RateLimitLimits::new(1, 2).unwrap(),
    );
    let now = Instant::now();
    at(&limiter, now);
    assert_eq!(limiter.check("/a"), Ok(RateLimitDecision::Allowed));
    assert_eq!(limiter.check("/b"), Err(RateLimitError::Full));
    assert_eq!(limiter.check("/a"), Ok(limited(10)));
    at(&limiter, now + Duration::from_secs(9));
    assert_eq!(limiter.check("/b"), Err(RateLimitError::Full));
    at(&limiter, now + Duration::from_secs(10));
    assert_eq!(limiter.check("/b"), Ok(RateLimitDecision::Allowed));
    let stats = limiter.stats().unwrap();
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.scope_bytes, 2);
    assert_eq!(stats.capacity_rejections, 2);
    assert_eq!(limiter.clear().unwrap(), 1);
    assert_eq!(limiter.stats().unwrap().entries, 0);
    assert_eq!(limiter.stats().unwrap().allowed, 2);
    let byte_limited = RateLimiter::new(
        RateLimitPolicy::new(1, 10).unwrap(),
        RateLimitLimits::new(10, 1).unwrap(),
    );
    assert_eq!(byte_limited.check("/a"), Err(RateLimitError::Full));
    assert_eq!(byte_limited.stats().unwrap().entries, 0);
}
#[test]
fn scopes_use_patterns_and_unmatched_requests_share_a_bucket() {
    let limiter = make_limiter(1, 60);
    let guard = limiter.guard();
    assert_eq!(
        run_guard(&guard, &request("/items/a", None, ""), Some("/items/{id}")),
        GuardDecision::Allow
    );
    assert!(matches!(
        run_guard(
            &guard,
            &request("/items/b?x=1", None, ""),
            Some("/items/{id}")
        ),
        GuardDecision::RateLimited(_)
    ));
    assert_eq!(
        run_guard(&guard, &request("/unknown-a", None, ""), None),
        GuardDecision::Allow
    );
    assert!(matches!(
        run_guard(&guard, &request("/unknown-b", None, ""), None),
        GuardDecision::RateLimited(_)
    ));
    assert_eq!(limiter.stats().unwrap().entries, 2);
    let before = limiter.stats().unwrap();
    for scope in ["external", "//host/path", "/bad?x=1", "/bad#x", "/../bad"] {
        assert_eq!(limiter.check(scope), Err(RateLimitError::InvalidScope));
    }
    assert_eq!(limiter.stats().unwrap(), before);
}
#[test]
fn concurrent_clones_consume_one_shared_allowance() {
    let limiter = make_limiter(16, 86400);
    let barrier = Arc::new(Barrier::new(16));
    let workers = (0..16)
        .map(|_| {
            let limiter = limiter.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                (0..50)
                    .filter(|_| limiter.check("/save") == Ok(RateLimitDecision::Allowed))
                    .count()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum::<usize>(),
        16
    );
    let stats = limiter.stats().unwrap();
    assert_eq!(stats.allowed, 16);
    assert_eq!(stats.limited, 784);
    assert_eq!(stats.entries, 1);
}
#[test]
fn maximum_arithmetic_and_counters_remain_bounded() {
    let limiter = make_limiter(1, MAX_RATE_LIMIT_PERIOD_SECONDS);
    let now = Instant::now();
    at(&limiter, now);
    assert_eq!(limiter.check("/"), Ok(RateLimitDecision::Allowed));
    assert_eq!(
        limiter.check("/"),
        Ok(limited(MAX_RATE_LIMIT_PERIOD_SECONDS))
    );
    let large = make_limiter(MAX_RATE_LIMIT_REQUESTS, MAX_RATE_LIMIT_PERIOD_SECONDS);
    at(&large, now);
    assert_eq!(large.check("/"), Ok(RateLimitDecision::Allowed));
    large.inner.state.lock().unwrap().stats.allowed = u64::MAX;
    at(&large, now + Duration::from_secs(u32::MAX.into()));
    assert_eq!(large.check("/"), Ok(RateLimitDecision::Allowed));
    assert_eq!(large.stats().unwrap().allowed, u64::MAX);
}
#[test]
fn poisoned_state_fails_closed_without_a_guard_allowance() {
    let limiter = make_limiter(1, 60);
    let _ = std::panic::catch_unwind(|| {
        let _lock = limiter.inner.state.lock().unwrap();
        panic!("fixture poison");
    });
    assert_eq!(limiter.check("/"), Err(RateLimitError::Unavailable));
    assert_eq!(limiter.stats(), Err(RateLimitError::Unavailable));
    assert_eq!(
        run_guard(&limiter.guard(), &request("/", None, ""), Some("/")),
        GuardDecision::Unavailable
    );
}
#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[test]
fn live_session_families_keep_debt_across_rotation_and_store_boundaries() {
    let store = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("fixture_session").unwrap(),
    )
    .unwrap();
    let limiter = make_limiter(1, 60);
    let cookie = store.create(SessionData::default()).unwrap();
    let session = store
        .lookup(&request("/", Some(&cookie), ""))
        .unwrap()
        .unwrap();
    assert_eq!(
        limiter.check_session("/save", &session),
        Ok(RateLimitDecision::Allowed)
    );
    let next = store.rotate(&session).unwrap().unwrap();
    assert_eq!(
        limiter.check_session("/save", &session),
        Err(RateLimitError::InvalidSession)
    );
    let fresh = store
        .lookup(&request("/", Some(&next), ""))
        .unwrap()
        .unwrap();
    assert!(matches!(
        limiter.check_session("/save", &fresh),
        Ok(RateLimitDecision::Limited(_))
    ));
    let other = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("fixture_session").unwrap(),
    )
    .unwrap();
    let cookie = other.create(SessionData::default()).unwrap();
    let other_session = other
        .lookup(&request("/", Some(&cookie), ""))
        .unwrap()
        .unwrap();
    assert_eq!(
        limiter.check_session("/save", &other_session),
        Ok(RateLimitDecision::Allowed)
    );
    assert!(store.revoke(&session).unwrap());
    assert_eq!(
        limiter.check_session("/save", &fresh),
        Err(RateLimitError::InvalidSession)
    );
    assert!(!format!("{limiter:?}").contains(cookie.value()));
    assert_eq!(limiter.stats().unwrap().entries, 2);
    let guard = limiter.session_guard(store);
    for (n, value) in ["forged", "unknown", "other"].iter().enumerate() {
        let cookie = Cookie::local_http("fixture_session", value).unwrap();
        let path = format!("/anonymous-{n}");
        let decision = run_guard(
            &guard,
            &request(
                &path,
                Some(&cookie),
                &format!("X-Forwarded-For: 192.0.2.{n}\r\n"),
            ),
            Some(&path),
        );
        if n == 0 {
            assert_eq!(decision, GuardDecision::Allow);
        } else {
            assert!(matches!(decision, GuardDecision::RateLimited(_)));
        }
    }
}
