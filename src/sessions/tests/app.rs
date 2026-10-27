use super::*;
use crate::{
    ActionData, ActionReply, App, FileEndpoint, Page, PageMode, PageRoute, QueryMode, Response,
    StatusCode,
};
use std::sync::atomic::{AtomicUsize, Ordering};
fn with_cookie(path: &str, cookie: Option<&Cookie>) -> Request {
    let header = cookie.map_or_else(String::new, |cookie| {
        format!("Cookie: {}={}\r\n", cookie.name(), cookie.value())
    });
    crate::http::parse(
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{header}\r\n").as_bytes(),
        8192,
        32,
    )
    .unwrap()
}
#[test]
fn guards_stop_loaders_and_unknown_descendants_and_keep_private_data_per_session() {
    let store = store();
    let a = store.create(data("USER_A")).unwrap();
    let b = store.create(data("USER_B")).unwrap();
    let loads = Arc::new(AtomicUsize::new(0));
    let calls = loads.clone();
    let sessions = store.clone();
    let mut app = App::new();
    app.page(
        PageRoute::new("private", "/private").unwrap(),
        None,
        PageMode::ServerOnly(QueryMode::Uri),
        move |context| {
            calls.fetch_add(1, Ordering::SeqCst);
            let request = context.request().unwrap();
            assert!(request.cache_bypass());
            let data = sessions
                .lookup(request)
                .unwrap()
                .unwrap()
                .data()
                .unwrap()
                .unwrap();
            Ok(Page::new(
                "Private",
                crate::text(data.get("user_id").unwrap()),
            ))
        },
    )
    .unwrap();
    app.guard_branch("/private", store.guard()).unwrap();
    let router = app.router(None).unwrap();
    for path in ["/private", "/private/missing"] {
        let response = router.dispatch(with_cookie(path, None));
        assert_eq!(response.status(), StatusCode::Forbidden);
        assert_eq!(response.cache_control(), "no-store");
    }
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    for (cookie, own, other) in [(&a, "USER_A", "USER_B"), (&b, "USER_B", "USER_A")] {
        let response = router.dispatch(with_cookie("/private", Some(cookie)));
        assert_eq!(response.status(), StatusCode::Ok);
        let body = response.body_text().unwrap();
        assert!(body.contains(own));
        assert!(!body.contains(other));
        assert!(!body.contains(cookie.value()));
        assert_eq!(response.cache_control(), "no-store");
    }
    let session = handle(&store, &a);
    let rotated = store.rotate(&session).unwrap().unwrap();
    assert_eq!(
        router.dispatch(with_cookie("/private", Some(&a))).status(),
        StatusCode::Forbidden
    );
    assert!(store.revoke(&session).unwrap());
    assert_eq!(
        router
            .dispatch(with_cookie("/private", Some(&rotated)))
            .status(),
        StatusCode::Forbidden
    );
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}
#[test]
fn action_preflight_revision_and_expired_session_checks_precede_mutation() {
    let store = fake(
        SessionLimits::new(2, 512, 128, 10, 30).unwrap(),
        [Ok([1; 32])],
    );
    let base = Instant::now();
    at(&store, base);
    let cookie = store.create(data("user")).unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let calls = writes.clone();
    let mut app = App::new();
    app.file_endpoint(
        "/private/action",
        FileEndpoint::new()
            .post_action_response(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Response::action(&ActionReply::success(ActionData::default()))
            })
            .guard(store.guard()),
    )
    .unwrap();
    let mut router = app.router(None).unwrap();
    router.application_revision(123);
    for (origin, revision, expected) in [
        ("http://foreign.example", "123", StatusCode::Forbidden),
        ("http://localhost", "122", StatusCode::Conflict),
        ("http://localhost", "123", StatusCode::Ok),
    ] {
        let raw = format!(
            "POST /private/action HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\nRuvane-Application-Revision: {revision}\r\nCookie: {}={}\r\n\r\n",
            cookie.name(),
            cookie.value()
        );
        let mut request = crate::http::parse(raw.as_bytes(), 8192, 32).unwrap();
        match router.prepare_mutation(&request, 1024) {
            Err(status) => assert_eq!(status, expected),
            Ok(_) => {
                request.form = Some(crate::FormData::default());
                assert_eq!(router.dispatch(request).status(), expected);
            }
        }
    }
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    at(&store, base + Duration::from_secs(10));
    let raw = format!(
        "POST /private/action HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\nRuvane-Application-Revision: 123\r\nCookie: {}={}\r\n\r\n",
        cookie.name(),
        cookie.value()
    );
    let mut request = crate::http::parse(raw.as_bytes(), 8192, 32).unwrap();
    router.prepare_mutation(&request, 1024).unwrap();
    request.form = Some(crate::FormData::default());
    assert_eq!(router.dispatch(request).status(), StatusCode::Forbidden);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}
#[test]
fn unavailable_session_state_returns_503_without_entering_a_page() {
    let store = store();
    let cookie = store.create(data("user")).unwrap();
    let mut app = App::new();
    app.page(
        PageRoute::new("private", "/private").unwrap(),
        None,
        PageMode::ServerOnly(QueryMode::Uri),
        |_| panic!("private loader ran"),
    )
    .unwrap();
    app.guard(store.guard()).unwrap();
    let router = app.router(None).unwrap();
    let inner = store.inner.clone();
    let _ = thread::spawn(move || {
        let _state = inner.state.lock().unwrap();
        panic!("poison fixture");
    })
    .join();
    let response = router.dispatch(with_cookie("/private", Some(&cookie)));
    assert_eq!(response.status(), StatusCode::ServiceUnavailable);
    assert_eq!(response.cache_control(), "no-store");
    assert!(!response.body_text().unwrap().contains(cookie.value()));
}
