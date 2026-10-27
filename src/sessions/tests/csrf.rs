use super::*;

fn post(cookie: &Cookie, header: Option<&str>, body: &str) -> Request {
    let header = header.map_or_else(String::new, |value| {
        format!("Ruvane-Csrf-Token: {value}\r\n")
    });
    let mut request = crate::http::parse(
        format!(
            "POST /private HTTP/1.1\r\nHost: localhost\r\nCookie: {}={}\r\n{header}\r\n",
            cookie.name(),
            cookie.value()
        )
        .as_bytes(),
        8192,
        32,
    )
    .unwrap();
    request.form = Some(crate::FormData::parse(body.as_bytes()).unwrap());
    request
}
fn issued(store: &SessionStore) -> (Cookie, Session, CsrfToken) {
    let cookie = store.create(data("demo")).unwrap();
    let session = handle(store, &cookie);
    let token = session.csrf_token().unwrap().unwrap();
    (cookie, session, token)
}
#[test]
fn claims_are_bound_to_cookie_and_generation() {
    let store = store();
    let (cookie, session, token) = issued(&store);
    let (other, _, other_token) = issued(&store);
    assert_ne!(token.as_str(), cookie.value());
    assert_ne!(token, other_token);
    assert_eq!(session.csrf_token().unwrap(), Some(token.clone()));
    for _ in 0..3 {
        assert!(
            store
                .verify_csrf(&post(&cookie, Some(token.as_str()), "mode=save"))
                .unwrap()
        );
    }
    for claim in [
        None,
        Some("forged"),
        Some(other_token.as_str()),
        Some(cookie.value()),
    ] {
        assert!(
            !store
                .verify_csrf(&post(&cookie, claim, "mode=save"))
                .unwrap()
        );
    }
    assert!(
        !store
            .verify_csrf(&post(&other, Some(token.as_str()), ""))
            .unwrap()
    );
    assert!(!store.verify_csrf(&request(Some(&cookie))).unwrap());
    let next = store.rotate(&session).unwrap().unwrap();
    let fresh = handle(&store, &next).csrf_token().unwrap().unwrap();
    assert_ne!(fresh, token);
    assert_eq!(session.csrf_token().unwrap(), None);
    assert!(
        !store
            .verify_csrf(&post(&cookie, Some(token.as_str()), ""))
            .unwrap()
    );
    assert!(
        !store
            .verify_csrf(&post(&next, Some(token.as_str()), ""))
            .unwrap()
    );
    assert!(
        store
            .verify_csrf(&post(&next, Some(fresh.as_str()), ""))
            .unwrap()
    );
    assert!(store.revoke(&session).unwrap());
    assert!(
        !store
            .verify_csrf(&post(&next, Some(fresh.as_str()), ""))
            .unwrap()
    );
    assert_eq!(store.inner.state.lock().unwrap().csrf_index.len(), 1);
}
#[test]
fn native_claims_are_separate_and_ambiguous_sources_fail_closed() {
    let store = store();
    let (cookie, _, token) = issued(&store);
    let body = format!("mode=save&_ruvane_csrf={}", token.as_str());
    let request = post(&cookie, None, &body);
    assert!(store.verify_csrf(&request).unwrap());
    let form = request.form_data().unwrap();
    assert!(form.has_only(&["mode"]));
    assert_eq!(form.len(), 1);
    assert_eq!(form.get(crate::CSRF_FORM_FIELD), None);
    assert!(form.encode().is_err());
    let both = post(&cookie, Some(token.as_str()), &body);
    assert_eq!(both.csrf_token(), Err(crate::CsrfError::Ambiguous));
    assert!(!store.verify_csrf(&both).unwrap());
    for value in ["", "forged", &"A".repeat(64)] {
        assert!(
            !store
                .verify_csrf(&post(&cookie, None, &format!("_ruvane_csrf={value}")))
                .unwrap()
        );
    }
    let duplicate = format!("_ruvane_csrf={0}&%5Fruvane_csrf={0}", token.as_str());
    assert_eq!(
        crate::FormData::parse(duplicate.as_bytes()),
        Err(crate::FormError::DuplicateName)
    );
    for value in [&request, &both] {
        assert!(!format!("{value:?}").contains(token.as_str()));
    }
}
#[test]
fn native_forms_and_responses_redact_and_keep_token_out_of_business_fields() {
    let store = store();
    let (_, _, token) = issued(&store);
    let form = crate::PostForm::new(crate::InternalUrl::new("/save").unwrap())
        .unwrap()
        .csrf(token.clone())
        .unwrap()
        .hidden("mode", "save")
        .unwrap();
    assert!(!format!("{form:?}").contains(token.as_str()));
    let html = form.render();
    assert!(html.as_str().contains(&format!(
        "name=\"_ruvane_csrf\" value=\"{}\"",
        token.as_str()
    )));
    assert!(
        crate::FormData::default()
            .field(crate::CSRF_FORM_FIELD, token.as_str())
            .is_err()
    );
    assert!(
        crate::PostForm::new(crate::InternalUrl::new("/save").unwrap())
            .unwrap()
            .hidden(crate::CSRF_FORM_FIELD, token.as_str())
            .is_err()
    );
    let fields = (0..crate::MAX_QUERY_PAIRS)
        .map(|n| format!("n{n}=x"))
        .collect::<Vec<_>>()
        .join("&");
    let parsed =
        crate::FormData::parse(format!("{fields}&_ruvane_csrf={}", token.as_str()).as_bytes())
            .unwrap();
    assert_eq!(parsed.len(), crate::MAX_QUERY_PAIRS);
    for token in [None, Some(token)] {
        let response = Response::csrf_token(token.clone());
        assert_eq!(response.cache_control(), "no-store");
        assert!(response.body().is_empty());
        let headers = response.wire_headers("GET, HEAD");
        assert!(headers.contains("Content-Length: 0\r\n"));
        assert!(headers.contains(crate::CSRF_MEDIA_TYPE));
        if let Some(token) = token {
            assert!(headers.contains(token.as_str()));
            assert!(!format!("{response:?}").contains(token.as_str()));
        } else {
            assert!(!headers.contains(crate::CSRF_HEADER));
        }
    }
}
#[test]
fn failed_entropy_keeps_current_generation_and_rejects_cross_credential_collisions() {
    let store = fake(
        SessionLimits::default(),
        [
            Ok([1; 32]),
            Ok([1; 32]),
            Ok([0; 32]),
            Ok([2; 32]),
            Ok([2; 32]),
            Ok([3; 32]),
            Err(SessionError::EntropyUnavailable),
        ],
    );
    let (cookie, session, token) = issued(&store);
    assert_eq!(token, CsrfToken::from_bytes(&[2; 32]));
    assert_eq!(
        store.rotate(&session),
        Err(SessionError::EntropyUnavailable)
    );
    assert!(
        store
            .verify_csrf(&post(&cookie, Some(token.as_str()), ""))
            .unwrap()
    );
    assert_eq!(handle(&store, &cookie).csrf_token().unwrap(), Some(token));
    let store = fake(
        SessionLimits::default(),
        [
            Ok([1; 32]),
            Err(SessionError::EntropyUnavailable),
            Ok([2; 32]),
        ],
    );
    let cookie = store.create(data("demo")).unwrap();
    let session = handle(&store, &cookie);
    assert_eq!(session.csrf_token(), Err(SessionError::EntropyUnavailable));
    assert!(session.data().unwrap().is_some());
    assert!(session.csrf_token().unwrap().is_some());
}
#[test]
fn rejected_claims_do_not_extend_idle_and_clear_removes_the_index() {
    let limits = SessionLimits::new(4, 1024, 256, 2, 5).unwrap();
    let store = fake(limits, (1..20).map(|n| Ok([n; 32])));
    let now = Instant::now();
    at(&store, now);
    let (cookie, session, token) = issued(&store);
    at(&store, now + Duration::from_secs(1));
    assert!(
        !store
            .verify_csrf(&post(&cookie, Some(&"f".repeat(64)), ""))
            .unwrap()
    );
    assert!(session.csrf_token().unwrap().is_some());
    at(&store, now + Duration::from_secs(2));
    assert!(
        !store
            .verify_csrf(&post(&cookie, Some(token.as_str()), ""))
            .unwrap()
    );
    assert_eq!(session.csrf_token().unwrap(), None);
    assert!(store.inner.state.lock().unwrap().csrf_index.is_empty());
    let (_, _, _) = issued(&store);
    assert_eq!(store.clear().unwrap(), 1);
    assert!(store.inner.state.lock().unwrap().csrf_index.is_empty());
}
#[test]
fn valid_claims_refresh_idle_without_extending_absolute_expiry() {
    let limits = SessionLimits::new(4, 1024, 256, 2, 5).unwrap();
    let store = fake(limits, (1..20).map(|n| Ok([n; 32])));
    let now = Instant::now();
    at(&store, now);
    let (cookie, _, token) = issued(&store);
    for n in 1..5 {
        at(&store, now + Duration::from_secs(n));
        assert!(
            store
                .verify_csrf(&post(&cookie, Some(token.as_str()), ""))
                .unwrap()
        );
    }
    at(&store, now + Duration::from_secs(5));
    assert!(
        !store
            .verify_csrf(&post(&cookie, Some(token.as_str()), ""))
            .unwrap()
    );
}
