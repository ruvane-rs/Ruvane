use ruvane::{
    App, FileEndpoint, FileRouteRegistry, GuardDecision, JsonValue, Request, Response, RouteGuard,
    Server,
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

struct Running {
    address: SocketAddr,
    stop: ruvane::ShutdownHandle,
    thread: Option<thread::JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn new(app: App) -> Self {
        Self::configured(app, ruvane::ServerConfig::default())
    }
    fn configured(app: App, config: ruvane::ServerConfig) -> Self {
        let server = Server::bind_with_config(
            "127.0.0.1:0".parse().unwrap(),
            app.router(None).unwrap(),
            config,
        )
        .unwrap();
        let address = server.local_addr().unwrap();
        let stop = server.shutdown_handle();
        Self {
            address,
            stop,
            thread: Some(thread::spawn(move || server.run())),
        }
    }
    fn raw(&self, method: &str, path: &str, headers: &str, body: &[u8]) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            self.address
        )
        .unwrap();
        if let Err(error) = stream.write_all(body) {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ));
        }
        let mut bytes = Vec::new();
        if let Err(error) = stream.read_to_end(&mut bytes) {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        let end = bytes
            .windows(4)
            .position(|word| word == b"\r\n\r\n")
            .unwrap();
        (
            String::from_utf8(bytes[..end].to_vec()).unwrap(),
            bytes[end + 4..].to_vec(),
        )
    }
    fn post(&self, path: &str, mime: &str, extra: &str, body: &[u8]) -> (String, Vec<u8>) {
        self.raw(
            "POST",
            path,
            &format!(
                "Origin: http://{}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{extra}",
                self.address,
                body.len()
            ),
            body,
        )
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn status(headers: &str) -> u16 {
    headers.split_whitespace().nth(1).unwrap().parse().unwrap()
}
fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers
        .split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}
fn register(
    app: &mut App,
    source: &str,
    pattern: &str,
    endpoint: FileEndpoint,
) -> Result<(), ruvane::FileRouteError> {
    let mut registry = FileRouteRegistry::new(app);
    registry.endpoint(source, pattern, endpoint)?;
    registry.finish()
}
fn response(request: &Request) -> Response {
    Response::json(request.json().unwrap())
}

#[test]
fn json_posts_are_explicit_and_get_head_keep_their_existing_read_only_contract() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut app = App::new();
    register(
        &mut app,
        "api/[id]/route.rs",
        "/api/{id}",
        FileEndpoint::new()
            .get(|request| {
                assert!(request.json().is_none());
                Response::json(&JsonValue::string(request.param("id").unwrap()).unwrap())
            })
            .post_json(move |request| {
                assert!(request.form_data().is_none());
                assert_eq!(request.param("id"), Some("rust"));
                assert_eq!(request.query_params().get("mode"), Some("json"));
                counted.fetch_add(1, Ordering::SeqCst);
                response(request)
            }),
    )
    .unwrap();
    register(
        &mut app,
        "form/route.rs",
        "/form",
        FileEndpoint::new().post_form(|request| {
            assert!(request.json().is_none());
            Response::text("form")
        }),
    )
    .unwrap();
    let server = Running::new(app);
    let source = "{\"text\":\"Rust 🦀\\n\",\"n\":18446744073709551615}";
    for mime in ["application/json", "Application/JSON; charset=\"UTF-8\""] {
        let (headers, body) = server.post("/api/rust?mode=json", mime, "", source.as_bytes());
        assert_eq!(status(&headers), 200);
        assert_eq!(header(&headers, "Content-Type"), Some("application/json"));
        assert_eq!(header(&headers, "Cache-Control"), Some("no-store"));
        assert_eq!(header(&headers, "X-Content-Type-Options"), Some("nosniff"));
        assert!(headers.contains("script-src 'none'"));
        assert_eq!(
            JsonValue::parse(&body).unwrap(),
            JsonValue::parse(source.as_bytes()).unwrap()
        );
    }
    for method in ["GET", "HEAD"] {
        let (headers, body) = server.raw(method, "/api/rust", "", b"");
        assert_eq!(status(&headers), 200);
        assert_eq!(header(&headers, "Content-Length"), Some("6"));
        if method == "HEAD" {
            assert!(body.is_empty());
        } else {
            assert_eq!(body, b"\"rust\"");
        }
    }
    assert_eq!(
        status(
            &server
                .post(
                    "/api/rust",
                    "application/x-www-form-urlencoded",
                    "",
                    b"title=Rust"
                )
                .0
        ),
        415
    );
    assert_eq!(
        status(&server.post("/form", "application/json", "", b"{}").0),
        415
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[test]
fn malformed_and_oversized_json_never_run_guards_or_handlers_and_errors_hide_the_body() {
    let guards = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(AtomicUsize::new(0));
    let counted = guards.clone();
    let called = writes.clone();
    let mut app = App::new();
    register(
        &mut app,
        "json/route.rs",
        "/json",
        FileEndpoint::new()
            .post_json(move |request| {
                called.fetch_add(1, Ordering::SeqCst);
                response(request)
            })
            .guard(RouteGuard::new(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                GuardDecision::Allow
            })),
    )
    .unwrap();
    let server = Running::new(app);
    for body in [
        b"".as_slice(),
        b"{\"secret\":\"PRIVATE_JSON_FIXTURE\",}",
        b"{\"x\":1,\"\\u0078\":2}",
        b"\"\\uD800\"",
        b"\"\xff\"",
        b"[0,]",
        b"null true",
        b"\xef\xbb\xbf{}",
    ] {
        let (headers, error) = server.post("/json", "application/json", "", body);
        assert_eq!(status(&headers), 400);
        assert!(!String::from_utf8_lossy(&error).contains("PRIVATE_JSON_FIXTURE"));
        assert_eq!(header(&headers, "Cache-Control"), Some("no-store"));
    }
    for body in [
        format!("\"{}\"", "x".repeat(ruvane::MAX_JSON_STRING_BYTES + 1)),
        format!(
            "{}0{}",
            "[".repeat(ruvane::MAX_JSON_DEPTH + 1),
            "]".repeat(ruvane::MAX_JSON_DEPTH + 1)
        ),
        format!("[{}]", vec!["0"; ruvane::MAX_JSON_ITEMS + 1].join(",")),
        " ".repeat(ruvane::MAX_JSON_BYTES + 1),
    ] {
        assert_eq!(
            status(
                &server
                    .post("/json", "application/json", "", body.as_bytes())
                    .0
            ),
            413
        );
    }
    for mime in [
        "text/json",
        "application/json; charset=ascii",
        "application/json; charset=utf-8; charset=utf-8",
        "application/json; version=1",
    ] {
        assert_eq!(status(&server.post("/json", mime, "", b"{}").0), 415);
    }
    assert_eq!(
        status(
            &server
                .post(
                    "/json",
                    "application/json",
                    "Content-Encoding: identity\r\n",
                    b"{}"
                )
                .0
        ),
        415
    );
    assert_eq!(
        status(
            &server
                .raw(
                    "POST",
                    "/json",
                    &format!(
                        "Origin: http://{}\r\nContent-Type: application/json\r\n",
                        server.address
                    ),
                    b""
                )
                .0
        ),
        411
    );
    assert_eq!(status(&server.raw("POST","/json","Origin: http://foreign.example\r\nContent-Type: application/json\r\nContent-Length: 2\r\n",b"{}").0),403);
    assert_eq!(guards.load(Ordering::SeqCst), 0);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        status(&server.post("/json", "application/json", "", b"{}").0),
        200
    );
    assert_eq!(guards.load(Ordering::SeqCst), 1);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}
#[test]
fn json_forms_and_actions_cannot_claim_the_same_post_slot() {
    for endpoint in [
        FileEndpoint::new().post_json(response).post_json(response),
        FileEndpoint::new()
            .post_json(response)
            .post_form(|_| Response::text("form")),
        FileEndpoint::new()
            .post_action(|_| Ok(ruvane::ActionReply::success(Default::default())))
            .post_json(response),
    ] {
        let mut app = App::new();
        assert!(register(&mut app, "json/route.rs", "/json", endpoint).is_err());
    }
    let mut router = ruvane::Router::new();
    router.post_json("/json", response).unwrap();
    assert!(
        router
            .post_form("/json", |_| Response::text("form"))
            .is_err()
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
fn json_session_csrf_and_quota_guards_reject_before_mutation_without_body_token_fallback() {
    use ruvane::{
        RateLimitLimits, RateLimitPolicy, RateLimiter, SessionCookie, SessionData, SessionLimits,
        SessionStore,
    };
    let store = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("fixture_json").unwrap(),
    )
    .unwrap();
    let cookie = store.create(SessionData::default()).unwrap();
    let limiter = RateLimiter::new(
        RateLimitPolicy::new(1, 60).unwrap(),
        RateLimitLimits::default(),
    );
    let writes = Arc::new(AtomicUsize::new(0));
    let counted = writes.clone();
    let mut app = App::new();
    register(
        &mut app,
        "json/route.rs",
        "/json",
        FileEndpoint::new()
            .post_json(move |request| {
                counted.fetch_add(1, Ordering::SeqCst);
                response(request)
            })
            .guard(store.csrf_guard())
            .guard(limiter.session_guard(store.clone())),
    )
    .unwrap();
    register(
        &mut app,
        "csrf/route.rs",
        "/csrf",
        FileEndpoint::new().get(move |request| store.csrf_response(request)),
    )
    .unwrap();
    let server = Running::new(app);
    let credentials = format!("Cookie: {}={}\r\n", cookie.name(), cookie.value());
    let (headers, body) = server.raw("GET", "/csrf", &credentials, b"");
    assert!(body.is_empty());
    let token = header(&headers, "Ruvane-Csrf-Token").unwrap();
    for (extra, body) in [
        (String::new(), "{}".into()),
        (credentials.clone(), "{}".into()),
        (
            credentials.clone(),
            format!("{{\"_ruvane_csrf\":\"{token}\"}}"),
        ),
        (
            format!("{credentials}Ruvane-Csrf-Token: forged\r\n"),
            "{}".into(),
        ),
    ] {
        assert_eq!(
            status(
                &server
                    .post("/json", "application/json", &extra, body.as_bytes())
                    .0
            ),
            403
        );
    }
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(limiter.stats().unwrap().allowed, 0);
    let extra = format!("{credentials}Ruvane-Csrf-Token: {token}\r\n");
    assert_eq!(
        status(&server.post("/json", "application/json", &extra, b"{}").0),
        200
    );
    let (headers, _) = server.post("/json", "application/json", &extra, b"{}");
    assert_eq!(status(&headers), 429);
    assert!(header(&headers, "Retry-After").is_some());
    assert_eq!(
        header(&headers, "Ruvane-Action-Rejected"),
        Some("rate-limit")
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

#[test]
fn every_registered_method_contributes_exactly_to_allow() {
    let mut app = App::new();
    for mask in 1..32 {
        let mut endpoint = FileEndpoint::new();
        if mask & 1 != 0 {
            endpoint = endpoint.get(|_| Response::text("read"));
        }
        if mask & 2 != 0 {
            endpoint = endpoint.post_json(response);
        }
        if mask & 4 != 0 {
            endpoint = endpoint.put_json(response);
        }
        if mask & 8 != 0 {
            endpoint = endpoint.patch_json(response);
        }
        if mask & 16 != 0 {
            endpoint = endpoint.delete(|_| Response::text("deleted"));
        }
        register(
            &mut app,
            &format!("m{mask}/route.rs"),
            &format!("/m{mask}"),
            endpoint,
        )
        .unwrap();
    }
    let server = Running::new(app);
    for mask in 1..32 {
        let (headers, _) = server.raw("OPTIONS", &format!("/m{mask}"), "", b"");
        assert_eq!(status(&headers), 405);
        let mut names = Vec::new();
        if mask & 1 != 0 {
            names.extend(["GET", "HEAD"]);
        }
        for (bit, name) in [(2, "POST"), (4, "PUT"), (8, "PATCH"), (16, "DELETE")] {
            if mask & bit != 0 {
                names.push(name);
            }
        }
        assert_eq!(header(&headers, "Allow"), Some(names.join(", ").as_str()));
    }
    let (headers, _) = server.raw("OPTIONS", "/missing", "", b"");
    assert_eq!(status(&headers), 405);
    assert_eq!(header(&headers, "Allow"), Some(""));
    for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"] {
        assert_eq!(status(&server.raw(method, "/missing", "", b"").0), 404);
    }
}

#[test]
fn put_patch_and_bodyless_delete_validate_before_guards() {
    let guards = Arc::new(AtomicUsize::new(0));
    let counted = guards.clone();
    let mut app = App::new();
    register(
        &mut app,
        "resource/[id]/route.rs",
        "/resource/{id}",
        FileEndpoint::new()
            .put_json(response)
            .patch_json(response)
            .delete(|request| {
                assert!(request.json().is_none() && request.form_data().is_none());
                assert_eq!(request.param("id"), Some("rust"));
                Response::text("deleted")
            })
            .guard(RouteGuard::new(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                GuardDecision::Allow
            })),
    )
    .unwrap();
    let server = Running::new(app);
    let origin = format!("Origin: http://{}\r\n", server.address);
    for method in ["PUT", "PATCH"] {
        for (extra, body, expected) in [
            ("Content-Type: application/json\r\n", &b""[..], 411),
            ("Content-Length: 2\r\n", &b"{}"[..], 415),
            (
                "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 2\r\n",
                &b"{}"[..],
                415,
            ),
            (
                "Content-Type: application/json; charset=ascii\r\nContent-Length: 2\r\n",
                &b"{}"[..],
                415,
            ),
            (
                "Content-Type: application/json\r\nContent-Encoding: identity\r\nContent-Length: 2\r\n",
                &b"{}"[..],
                415,
            ),
            (
                "Content-Type: application/json\r\nContent-Length: 1\r\n",
                &b"{"[..],
                400,
            ),
            (
                "Content-Type: application/json\r\nContent-Length: 65537\r\n",
                &b""[..],
                413,
            ),
            (
                "Content-Type: application/json\r\nContent-Length: 0\r\n",
                &b""[..],
                400,
            ),
            (
                "Content-Type: application/json\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n",
                &b"{}"[..],
                400,
            ),
        ] {
            assert_eq!(
                status(
                    &server
                        .raw(method, "/resource/rust", &format!("{origin}{extra}"), body)
                        .0
                ),
                expected,
                "{method} {extra}"
            );
        }
        for body in [b"{\"x\":1,\"x\":2}".to_vec(), b"[".repeat(17)] {
            let headers = format!(
                "{origin}Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            );
            let expected = if body[0] == b'[' { 413 } else { 400 };
            assert_eq!(
                status(&server.raw(method, "/resource/rust", &headers, &body).0),
                expected
            );
        }
    }
    for (extra, expected) in [
        ("Content-Length: 1\r\n", 413),
        ("Content-Type: application/json\r\n", 415),
        ("Content-Encoding: identity\r\n", 415),
        ("Transfer-Encoding: chunked\r\n", 501),
        ("Content-Length: 0\r\nContent-Length: 0\r\n", 400),
    ] {
        assert_eq!(
            status(
                &server
                    .raw("DELETE", "/resource/rust", &format!("{origin}{extra}"), b"")
                    .0
            ),
            expected
        );
    }
    for method in ["PUT", "PATCH", "DELETE"] {
        let body_headers = if method == "DELETE" {
            ""
        } else {
            "Content-Type: application/json\r\nContent-Length: 2\r\n"
        };
        let body = if method == "DELETE" {
            &b""[..]
        } else {
            &b"{}"[..]
        };
        for source in ["", "Origin: null\r\n", "Origin: http://foreign.example\r\n"] {
            assert_eq!(
                status(
                    &server
                        .raw(
                            method,
                            "/resource/rust",
                            &format!("{source}{body_headers}"),
                            body
                        )
                        .0
                ),
                403
            );
        }
        assert_eq!(
            status(
                &server
                    .raw(
                        method,
                        "/resource/rust",
                        &format!("{origin}{body_headers}Sec-Fetch-Site: cross-site\r\n"),
                        body
                    )
                    .0
            ),
            403
        );
    }
    assert_eq!(guards.load(Ordering::SeqCst), 0);
    for method in ["PUT", "PATCH"] {
        let headers = format!(
            "{origin}Content-Type: Application/JSON; charset=\"UTF-8\"\r\nContent-Length: 2\r\n"
        );
        assert_eq!(
            server.raw(method, "/resource/rust", &headers, b"{}").1,
            b"{}"
        );
    }
    for length in ["", "Content-Length: 0\r\n"] {
        assert_eq!(
            server
                .raw(
                    "DELETE",
                    "/resource/rust",
                    &format!("{origin}{length}"),
                    b""
                )
                .1,
            b"deleted"
        );
    }
    assert_eq!(guards.load(Ordering::SeqCst), 4);
    let headers = format!("{origin}Content-Type: application/json\r\nContent-Length: 2\r\n");
    assert_eq!(
        status(&server.raw("PUT", "/resource/%2F", &headers, b"{}").0),
        400
    );
    assert_eq!(guards.load(Ordering::SeqCst), 4);
}

#[test]
fn mutation_slots_reject_duplicates_and_ambiguity_atomically() {
    for endpoint in [
        FileEndpoint::new().put_json(response).put_json(response),
        FileEndpoint::new()
            .patch_json(response)
            .patch_json(response),
        FileEndpoint::new().delete(response).delete(response),
    ] {
        let mut app = App::new();
        assert!(register(&mut app, "bad/route.rs", "/bad", endpoint).is_err());
        register(
            &mut app,
            "bad/route.rs",
            "/bad",
            FileEndpoint::new().delete(|_| Response::text("ok")),
        )
        .unwrap();
    }
    for method in [
        ruvane::Method::Put,
        ruvane::Method::Patch,
        ruvane::Method::Delete,
    ] {
        let mut router = ruvane::Router::new();
        let add = |router: &mut ruvane::Router, path| match method {
            ruvane::Method::Put => router.put_json(path, response),
            ruvane::Method::Patch => router.patch_json(path, response),
            _ => router.delete(path, response),
        };
        add(&mut router, "/a/{id}").unwrap();
        assert_eq!(
            add(&mut router, "/a/{id}"),
            Err(ruvane::RouteError::DuplicatePath)
        );
        assert_eq!(
            add(&mut router, "/{kind}/x"),
            Err(ruvane::RouteError::AmbiguousRoute)
        );
        add(&mut router, "/a/x").unwrap();
        router.post_json("/a/{id}", response).unwrap();
    }
}

#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[test]
fn every_mutation_requires_csrf_and_shares_its_session_quota_across_methods() {
    use ruvane::{
        RateLimitLimits, RateLimitPolicy, RateLimiter, SessionCookie, SessionData, SessionLimits,
        SessionStore,
    };
    let store = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("methods_test").unwrap(),
    )
    .unwrap();
    let a = store.create(SessionData::default()).unwrap();
    let b = store.create(SessionData::default()).unwrap();
    let quota = RateLimiter::new(
        RateLimitPolicy::new(1, 60).unwrap(),
        RateLimitLimits::default(),
    );
    let writes = Arc::new(AtomicUsize::new(0));
    let counted = writes.clone();
    let handler = move |_: &Request| {
        counted.fetch_add(1, Ordering::SeqCst);
        Response::json(&true.into())
    };
    let mut app = App::new();
    register(
        &mut app,
        "api/route.rs",
        "/api",
        FileEndpoint::new()
            .get(|_| Response::text("read"))
            .post_json(handler.clone())
            .put_json(handler.clone())
            .patch_json(handler.clone())
            .delete(handler)
            .guard(store.csrf_guard())
            .guard(quota.session_guard(store.clone())),
    )
    .unwrap();
    let csrf = store.clone();
    register(
        &mut app,
        "csrf/route.rs",
        "/csrf",
        FileEndpoint::new().get(move |r| csrf.csrf_response(r)),
    )
    .unwrap();
    let server = Running::new(app);
    let cookie = |c: &ruvane::Cookie| format!("Cookie: {}={}\r\n", c.name(), c.value());
    let token = |c: &ruvane::Cookie| {
        header(
            &server.raw("GET", "/csrf", &cookie(c), b"").0,
            "Ruvane-Csrf-Token",
        )
        .unwrap()
        .to_owned()
    };
    let ta = token(&a);
    let tb = token(&b);
    let send = |method: &str, credentials: &str, claim: Option<&str>| {
        let body = if method == "DELETE" { "" } else { "{}" };
        let mime = if method == "DELETE" {
            String::new()
        } else {
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            )
        };
        let claim = claim.map_or(String::new(), |t| format!("Ruvane-Csrf-Token: {t}\r\n"));
        server.raw(
            method,
            "/api",
            &format!(
                "Origin: http://{}\r\n{mime}{credentials}{claim}",
                server.address
            ),
            body.as_bytes(),
        )
    };
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        for (credentials, claim) in [
            (String::new(), Some(ta.as_str())),
            (cookie(&a), None),
            (cookie(&a), Some("forged")),
            (cookie(&a), Some(tb.as_str())),
        ] {
            assert_eq!(
                status(&send(method, &credentials, claim).0),
                403,
                "{method}"
            );
        }
    }
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(quota.stats().unwrap().allowed, 0);
    assert_eq!(status(&send("PUT", &cookie(&a), Some(&ta)).0), 200);
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let (headers, _) = send(method, &cookie(&a), Some(&ta));
        assert_eq!(status(&headers), 429, "{method}");
        assert!(header(&headers, "Retry-After").is_some());
        assert_eq!(
            header(&headers, "Ruvane-Action-Rejected"),
            Some("rate-limit")
        );
    }
    assert_eq!(status(&server.raw("GET", "/api", &cookie(&a), b"").0), 429);
    assert_eq!(status(&send("DELETE", &cookie(&b), Some(&tb)).0), 200);
    assert_eq!(writes.load(Ordering::SeqCst), 2);
}

#[test]
fn new_method_slots_count_toward_the_application_route_budget() {
    let mut app = App::new();
    for n in 0..25 {
        register(
            &mut app,
            &format!("b{n}/route.rs"),
            &format!("/b{n}"),
            FileEndpoint::new()
                .get(response)
                .post_json(response)
                .put_json(response)
                .patch_json(response)
                .delete(response),
        )
        .unwrap();
    }
    register(
        &mut app,
        "last/route.rs",
        "/last",
        FileEndpoint::new()
            .put_json(response)
            .patch_json(response)
            .delete(response),
    )
    .unwrap();
    assert!(
        register(
            &mut app,
            "overflow/route.rs",
            "/overflow",
            FileEndpoint::new().get(response)
        )
        .is_err()
    );
    assert!(app.router(None).is_ok());
}

#[test]
fn put_and_patch_keep_configured_body_limits_and_absolute_receive_deadlines() {
    for method in ["PUT", "PATCH"] {
        let mut app = App::new();
        register(
            &mut app,
            "body/route.rs",
            "/body",
            FileEndpoint::new().put_json(response).patch_json(response),
        )
        .unwrap();
        let server = Running::configured(
            app,
            ruvane::ServerConfig {
                max_request_body_bytes: 8,
                request_timeout: Duration::from_millis(200),
                ..Default::default()
            },
        );
        let headers = |length| {
            format!(
                "{method} /body HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\r\n",
                server.address, server.address
            )
        };
        for (finish, expected) in [(true, 400), (false, 408)] {
            let mut stream = TcpStream::connect(server.address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.write_all(headers(2).as_bytes()).unwrap();
            stream.write_all(b"{").unwrap();
            if finish {
                stream.shutdown(std::net::Shutdown::Write).unwrap();
            }
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            assert_eq!(status(std::str::from_utf8(&bytes).unwrap()), expected);
        }
        let extra = format!(
            "Origin: http://{}\r\nContent-Type: application/json\r\nContent-Length: 9\r\n",
            server.address
        );
        assert_eq!(status(&server.raw(method, "/body", &extra, b"").0), 413);
        let mut stream = TcpStream::connect(server.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(headers(2).as_bytes()).unwrap();
        stream.write_all(b"{").unwrap();
        stream.write_all(b"}").unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(status(std::str::from_utf8(&bytes).unwrap()), 200);
    }
}
