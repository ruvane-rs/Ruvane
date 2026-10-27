use super::*;
mod csrf;
mod rate_limit;
use std::{collections::VecDeque, sync::Barrier, thread};
fn data(user: &str) -> SessionData {
    let mut data = SessionData::default();
    data.insert("user_id", user).unwrap();
    data
}
fn request(cookie: Option<&Cookie>) -> Request {
    let header = cookie.map_or_else(String::new, |cookie| {
        format!("Cookie: {}={}\r\n", cookie.name(), cookie.value())
    });
    crate::http::parse(
        format!("GET /private HTTP/1.1\r\nHost: localhost\r\n{header}\r\n").as_bytes(),
        8192,
        32,
    )
    .unwrap()
}
fn fake(
    limits: SessionLimits,
    values: impl IntoIterator<Item = Result<[u8; 32], SessionError>>,
) -> SessionStore {
    SessionStore::from_entropy(
        limits,
        SessionCookie::local_http("fixture_session").unwrap(),
        Entropy::Scripted(VecDeque::from_iter(values)),
    )
}
fn store() -> SessionStore {
    fake(SessionLimits::default(), (1..=200).map(|n| Ok([n; 32])))
}
fn at(store: &SessionStore, now: Instant) {
    store.inner.state.lock().unwrap().now = Some(now);
}
fn handle(store: &SessionStore, cookie: &Cookie) -> Session {
    store.lookup(&request(Some(cookie))).unwrap().unwrap()
}
#[test]
fn payload_fields_are_bounded_atomic_and_redacted() {
    let mut payload = data("PRIVATE_USER_FIXTURE");
    let before = payload.clone();
    for (name, value) in [
        ("user_id", "other"),
        ("bad name", "x"),
        ("value", "line\nbreak"),
        ("value", "\0"),
    ] {
        assert_eq!(payload.insert(name, value), Err(SessionError::InvalidData));
        assert_eq!(payload, before);
    }
    assert!(
        payload
            .insert("value", &"x".repeat(MAX_SESSION_VALUE_BYTES + 1))
            .is_err()
    );
    assert_eq!(payload.get("user_id"), Some("PRIVATE_USER_FIXTURE"));
    assert!(!format!("{payload:?}").contains("PRIVATE_USER_FIXTURE"));
    let mut count = SessionData::default();
    for n in 0..MAX_SESSION_FIELDS {
        count.insert(&format!("n{n}"), "").unwrap();
    }
    assert!(count.insert("extra", "").is_err());
    let mut bytes = SessionData::default();
    for n in 0..3 {
        bytes.insert(&format!("n{n}"), &"x".repeat(1024)).unwrap();
    }
    let before = bytes.clone();
    assert!(bytes.insert("n3", &"x".repeat(1024)).is_err());
    assert_eq!(bytes, before);
}
#[test]
fn limit_and_cookie_policies_fail_before_store_creation() {
    for (entries, bytes, data, idle, absolute) in [
        (0, 1, 1, 1, 1),
        (1, 0, 1, 1, 1),
        (1, 1, 0, 1, 1),
        (1, 1, 1, 0, 1),
        (1, 1, 1, 2, 1),
        (MAX_SESSION_ENTRIES + 1, 1, 1, 1, 1),
        (1, MAX_SESSION_STORE_BYTES + 1, 1, 1, 1),
        (1, 1, MAX_SESSION_DATA_BYTES + 1, 1, 1),
        (1, 1, 1, 1, MAX_SESSION_LIFETIME_SECONDS + 1),
    ] {
        assert!(SessionLimits::new(entries, bytes, data, idle, absolute).is_err());
    }
    assert!(SessionCookie::local_http("__Host-session").is_err());
    assert!(
        SessionCookie::secure("__Host-session")
            .unwrap()
            .path("/private")
            .is_err()
    );
    assert!(
        SessionCookie::local_http("session")
            .unwrap()
            .same_site(SameSite::None)
            .is_err()
    );
    assert!(
        SessionCookie::secure("session")
            .unwrap()
            .path("/;Secure")
            .is_err()
    );
    let policy = SessionCookie::secure("__Host-session").unwrap();
    let cookie = policy.issue(&[1; 32], 30).unwrap();
    assert!(cookie.is_secure() && cookie.is_http_only());
    assert_eq!(cookie.same_site_policy(), SameSite::Strict);
    assert_eq!(cookie.cookie_path(), "/");
}
#[test]
fn valid_cookie_resolves_only_in_its_store_and_never_from_other_inputs() {
    let store = store();
    let cookie = store.create(data("user-a")).unwrap();
    let session = handle(&store, &cookie);
    assert_eq!(
        session.data().unwrap().unwrap().get("user_id"),
        Some("user-a")
    );
    assert!(store.lookup(&request(None)).unwrap().is_none());
    let raw = format!(
        "GET /private?fixture_session={} HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\n\r\n",
        cookie.value(),
        cookie.value()
    );
    assert!(
        store
            .lookup(&crate::http::parse(raw.as_bytes(), 8192, 32).unwrap())
            .unwrap()
            .is_none()
    );
    let other = fake(SessionLimits::default(), [Ok([2; 32])]);
    assert!(other.lookup(&request(Some(&cookie))).unwrap().is_none());
    assert_eq!(
        other.rotate(&session).unwrap_err(),
        SessionError::InvalidHandle
    );
    assert_eq!(other.revoke(&session), Err(SessionError::InvalidHandle));
}
#[test]
fn malformed_unknown_and_uppercase_tokens_do_not_extend_idle_time() {
    let store = store();
    let base = Instant::now();
    at(&store, base);
    let cookie = store.create(data("user")).unwrap();
    let limits = store.inner.limits;
    for value in [
        "".into(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        "%30".repeat(32),
        "f".repeat(64),
    ] {
        let bad = Cookie::local_http(cookie.name(), &value).unwrap();
        assert!(store.lookup(&request(Some(&bad))).unwrap().is_none());
    }
    at(&store, base + Duration::from_secs(limits.idle.into()));
    assert!(store.lookup(&request(Some(&cookie))).unwrap().is_none());
    assert_eq!(store.stats().unwrap().entries, 0);
}
#[test]
fn idle_touch_never_extends_the_absolute_deadline() {
    let store = fake(
        SessionLimits::new(4, 512, 128, 10, 30).unwrap(),
        [Ok([1; 32])],
    );
    let base = Instant::now();
    at(&store, base);
    let cookie = store.create(data("user")).unwrap();
    for offset in [9, 18, 27] {
        at(&store, base + Duration::from_secs(offset));
        assert!(store.lookup(&request(Some(&cookie))).unwrap().is_some());
    }
    at(&store, base + Duration::from_secs(30));
    assert!(store.lookup(&request(Some(&cookie))).unwrap().is_none());
    assert_eq!(store.stats().unwrap().data_bytes, 0);
}
#[test]
fn data_handles_do_not_keep_idle_sessions_or_dropped_stores_alive() {
    let store = fake(
        SessionLimits::new(2, 512, 128, 10, 30).unwrap(),
        [Ok([1; 32])],
    );
    let base = Instant::now();
    at(&store, base);
    let cookie = store.create(data("user")).unwrap();
    let session = handle(&store, &cookie);
    at(&store, base + Duration::from_secs(9));
    assert!(session.data().unwrap().is_some());
    at(&store, base + Duration::from_secs(10));
    assert!(session.data().unwrap().is_none());
    drop(store);
    assert!(session.data().unwrap().is_none());
}
#[test]
fn rotation_retires_old_generations_and_preserves_absolute_expiration() {
    let store = fake(
        SessionLimits::new(2, 512, 128, 20, 30).unwrap(),
        [Ok([1; 32]), Ok([2; 32])],
    );
    let base = Instant::now();
    at(&store, base);
    let original = store.create(data("user")).unwrap();
    let session = handle(&store, &original);
    at(&store, base + Duration::from_secs(11));
    let rotated = store.rotate(&session).unwrap().unwrap();
    assert_eq!(rotated.age_seconds(), Some(19));
    assert!(original.value() != rotated.value());
    assert!(store.lookup(&request(Some(&original))).unwrap().is_none());
    assert!(session.data().unwrap().is_none());
    assert!(store.rotate(&session).unwrap().is_none());
    assert!(handle(&store, &rotated).data().unwrap().is_some());
    at(&store, base + Duration::from_secs(30));
    assert!(store.lookup(&request(Some(&rotated))).unwrap().is_none());
}
#[test]
fn native_handles_can_revoke_a_family_after_rotation_but_retired_cookies_cannot_resolve() {
    let store = store();
    let original = store.create(data("user")).unwrap();
    let session = handle(&store, &original);
    let next = store.rotate(&session).unwrap().unwrap();
    assert!(store.lookup(&request(Some(&original))).unwrap().is_none());
    assert!(store.revoke(&session).unwrap());
    assert!(!store.revoke(&session).unwrap());
    assert!(store.lookup(&request(Some(&next))).unwrap().is_none());
    assert!(store.rotate(&session).unwrap().is_none());
}
#[test]
fn data_changes_rotate_atomically_and_capacity_failures_keep_the_old_session() {
    let store = fake(
        SessionLimits::new(4, 40, 40, 10, 30).unwrap(),
        [Ok([1; 32]), Ok([2; 32])],
    );
    let original = store.create(data("a")).unwrap();
    let session = handle(&store, &original);
    let too_large = data(&"v".repeat(30));
    assert_eq!(
        store.rotate_with_data(&session, too_large).unwrap_err(),
        SessionError::InvalidData
    );
    assert!(session.data().unwrap().is_some());
    let rotated = store
        .rotate_with_data(&session, data("admin"))
        .unwrap()
        .unwrap();
    assert_eq!(
        handle(&store, &rotated)
            .data()
            .unwrap()
            .unwrap()
            .get("user_id"),
        Some("admin")
    );
    assert!(session.data().unwrap().is_none());
    assert_eq!(store.stats().unwrap().data_bytes, data("admin").bytes());
}
#[test]
fn store_capacity_never_evicts_live_sessions_and_recovers_after_expiry_or_revocation() {
    let payload = data("a");
    let bytes = payload.bytes();
    let store = fake(
        SessionLimits::new(2, bytes * 2, 128, 10, 30).unwrap(),
        (1..=8).map(|n| Ok([n; 32])),
    );
    let base = Instant::now();
    at(&store, base);
    let first = store.create(payload.clone()).unwrap();
    let second = store.create(payload.clone()).unwrap();
    assert_eq!(
        store.create(payload.clone()).unwrap_err(),
        SessionError::Full
    );
    assert!(store.lookup(&request(Some(&first))).unwrap().is_some());
    assert!(store.lookup(&request(Some(&second))).unwrap().is_some());
    store.revoke(&handle(&store, &first)).unwrap();
    let _ = store.create(payload.clone()).unwrap();
    assert_eq!(store.stats().unwrap().entries, 2);
    at(&store, base + Duration::from_secs(10));
    let _ = store.create(payload).unwrap();
    assert_eq!(store.stats().unwrap().entries, 1);
}
#[test]
fn aggregate_budget_failures_are_atomic_even_when_entry_space_remains() {
    let payload = data("a");
    let bytes = payload.bytes();
    let store = fake(
        SessionLimits::new(4, bytes * 2, 128, 10, 30).unwrap(),
        [Ok([1; 32]), Ok([2; 32]), Ok([3; 32])],
    );
    let cookie = store.create(payload.clone()).unwrap();
    let _ = store.create(payload.clone()).unwrap();
    assert_eq!(store.create(payload).unwrap_err(), SessionError::Full);
    let before = store.stats().unwrap();
    assert_eq!(
        store
            .rotate_with_data(&handle(&store, &cookie), data("longer"))
            .unwrap_err(),
        SessionError::Full
    );
    assert_eq!(store.stats().unwrap(), before);
    assert!(store.lookup(&request(Some(&cookie))).unwrap().is_some());
}
#[test]
fn clear_invalidates_every_handle_and_does_not_reuse_family_numbers() {
    let store = store();
    let first = store.create(SessionData::default()).unwrap();
    let session = handle(&store, &first);
    assert_eq!(store.clear().unwrap(), 1);
    let cookie = store.create(SessionData::default()).unwrap();
    let fresh = handle(&store, &cookie);
    assert_ne!(fresh.family, session.family);
    assert!(!store.revoke(&session).unwrap());
    assert!(fresh.data().unwrap().is_some());
    assert!(session.data().unwrap().is_none());
}
#[test]
fn entropy_failures_and_repeated_live_identifiers_do_not_remove_current_credentials() {
    let store = fake(
        SessionLimits::default(),
        [Ok([1; 32]), Err(SessionError::EntropyUnavailable)],
    );
    let cookie = store.create(data("user")).unwrap();
    let session = handle(&store, &cookie);
    assert_eq!(
        store.rotate(&session).unwrap_err(),
        SessionError::EntropyUnavailable
    );
    assert!(session.data().unwrap().is_some());
    let store = fake(
        SessionLimits::default(),
        [
            Ok([1; 32]),
            Ok([1; 32]),
            Ok([1; 32]),
            Ok([1; 32]),
            Ok([1; 32]),
        ],
    );
    let cookie = store.create(data("user")).unwrap();
    assert_eq!(
        store.rotate(&handle(&store, &cookie)).unwrap_err(),
        SessionError::EntropyUnavailable
    );
    assert!(store.lookup(&request(Some(&cookie))).unwrap().is_some());
    let zeros = fake(SessionLimits::default(), [Ok([0; 32]); 4]);
    assert_eq!(
        zeros.create(data("user")).unwrap_err(),
        SessionError::EntropyUnavailable
    );
    assert_eq!(zeros.stats().unwrap().entries, 0);
}
#[test]
fn concurrent_rotation_has_one_winner_and_old_native_handles_still_revoke_it() {
    let store = store();
    let cookie = store.create(data("user")).unwrap();
    let session = handle(&store, &cookie);
    let barrier = Arc::new(Barrier::new(9));
    let jobs = (0..8)
        .map(|_| {
            let (store, session, barrier) = (store.clone(), session.clone(), barrier.clone());
            thread::spawn(move || {
                barrier.wait();
                store.rotate(&session).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let winners = jobs
        .into_iter()
        .filter_map(|job| job.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    assert_eq!(store.stats().unwrap().entries, 1);
    assert!(store.revoke(&session).unwrap());
    assert!(store.lookup(&request(Some(&winners[0]))).unwrap().is_none());
}
#[test]
fn concurrent_revoke_and_rotation_cannot_resurrect_a_revoked_family() {
    for _ in 0..32 {
        let store = store();
        let cookie = store.create(data("user")).unwrap();
        let session = handle(&store, &cookie);
        let barrier = Arc::new(Barrier::new(3));
        let rotate = {
            let (store, session, barrier) = (store.clone(), session.clone(), barrier.clone());
            thread::spawn(move || {
                barrier.wait();
                store.rotate(&session).unwrap()
            })
        };
        let revoke = {
            let (store, session, barrier) = (store.clone(), session.clone(), barrier.clone());
            thread::spawn(move || {
                barrier.wait();
                store.revoke(&session).unwrap()
            })
        };
        barrier.wait();
        let next = rotate.join().unwrap();
        assert!(revoke.join().unwrap());
        assert!(store.lookup(&request(Some(&cookie))).unwrap().is_none());
        if let Some(next) = next {
            assert!(store.lookup(&request(Some(&next))).unwrap().is_none());
        }
        assert_eq!(store.stats().unwrap().entries, 0);
    }
}
#[test]
fn poisoned_state_fails_closed_and_debug_never_exposes_credentials() {
    let store = store();
    let cookie = store.create(data("SERVER_DATA_SECRET")).unwrap();
    let session = handle(&store, &cookie);
    for debug in [
        format!("{store:?}"),
        format!("{session:?}"),
        format!("{:?}", session.data().unwrap().unwrap()),
        format!("{cookie:?}"),
    ] {
        assert!(!debug.contains(cookie.value()));
        assert!(!debug.contains("SERVER_DATA_SECRET"));
    }
    let inner = store.inner.clone();
    assert!(
        thread::spawn(move || {
            let _lock = inner.state.lock().unwrap();
            panic!("session poison fixture");
        })
        .join()
        .is_err()
    );
    assert_eq!(
        store.lookup(&request(Some(&cookie))).unwrap_err(),
        SessionError::Unavailable
    );
    assert_eq!(session.data().unwrap_err(), SessionError::Unavailable);
    assert_eq!(
        store.create(data("user")).unwrap_err(),
        SessionError::Unavailable
    );
}
#[test]
fn removal_keeps_scope_flags_and_does_not_revoke_implicitly() {
    let store = SessionStore::from_entropy(
        SessionLimits::default(),
        SessionCookie::secure("session")
            .unwrap()
            .path("/private")
            .unwrap()
            .same_site(SameSite::Lax)
            .unwrap(),
        Entropy::Scripted([Ok([1; 32])].into()),
    );
    let cookie = store.create(data("user")).unwrap();
    let removal = store.removal_cookie();
    assert!(removal.is_secure() && removal.is_http_only());
    assert_eq!(removal.cookie_path(), "/private");
    assert_eq!(removal.age_seconds(), Some(0));
    assert_eq!(removal.same_site_policy(), SameSite::Lax);
    assert_eq!(removal.value(), "");
    assert!(store.lookup(&request(Some(&cookie))).unwrap().is_some());
}
#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[test]
fn real_os_provider_issues_fresh_full_length_identifiers() {
    let store = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::secure("session").unwrap(),
    )
    .unwrap();
    let mut values = std::collections::HashSet::new();
    for _ in 0..64 {
        let cookie = store.create(data("user")).unwrap();
        assert_eq!(cookie.value().len(), 64);
        assert!(
            cookie
                .value()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert!(values.insert(cookie.value().to_owned()));
        assert!(store.lookup(&request(Some(&cookie))).unwrap().is_some());
    }
}

mod app;
