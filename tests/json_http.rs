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
        let server =
            Server::bind("127.0.0.1:0".parse().unwrap(), app.router(None).unwrap()).unwrap();
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
