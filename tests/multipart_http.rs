#![cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
use ruvane::*;
use std::{
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
struct Running {
    address: SocketAddr,
    stop: ShutdownHandle,
    thread: Option<thread::JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn new(router: Router, config: ServerConfig) -> Self {
        let server =
            Server::bind_with_config("127.0.0.1:0".parse().unwrap(), router, config).unwrap();
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
        let _ = stream.write_all(body);
        let _ = stream.shutdown(Shutdown::Write);
        read(&mut stream)
    }
    fn post(&self, path: &str, extra: &str, body: &[u8]) -> (String, Vec<u8>) {
        self.raw("POST",path,&format!("Origin: http://{}\r\nContent-Type: multipart/form-data; boundary=test\r\nContent-Length: {}\r\n{extra}",self.address,body.len()),body)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn read(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    if let Err(error) = stream.read_to_end(&mut bytes) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    let end = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    (
        String::from_utf8(bytes[..end].to_vec()).unwrap(),
        bytes[end + 4..].to_vec(),
    )
}
fn status(reply: &(String, Vec<u8>)) -> u16 {
    reply.0.split_whitespace().nth(1).unwrap().parse().unwrap()
}
fn header<'a>(reply: &'a (String, Vec<u8>), name: &str) -> Option<&'a str> {
    reply.0.lines().skip(1).find_map(|line| {
        line.split_once(':')
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    })
}
fn body(token: Option<&str>, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(token) = token {
        out.extend(
            format!(
                "--test\r\nContent-Disposition: form-data; name=\"_ruvane_csrf\"\r\n\r\n{token}\r\n"
            )
            .as_bytes(),
        );
    }
    out.extend(b"--test\r\nContent-Disposition: form-data; name=\"caption\"\r\n\r\nRust + %41\r\n");
    out.extend(format!("--test\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    out.extend(bytes);
    out.extend(b"\r\n--test--\r\n");
    out
}
struct Fixture {
    server: Running,
    cookie: String,
    token: String,
    calls: Arc<AtomicUsize>,
    guards: Arc<AtomicUsize>,
    seen: Arc<Mutex<Option<UploadedFile>>>,
}
impl Fixture {
    fn new(burst: u32) -> Self {
        let store = SessionStore::new(
            SessionLimits::default(),
            SessionCookie::local_http("multipart_test").unwrap(),
        )
        .unwrap();
        let cookie = store.create(SessionData::default()).unwrap();
        let cookie = format!("{}={}", cookie.name(), cookie.value());
        let calls = Arc::new(AtomicUsize::new(0));
        let guards = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(None));
        let quota = RateLimiter::new(
            RateLimitPolicy::new(burst, 60).unwrap(),
            RateLimitLimits::default(),
        );
        let mut app = App::new();
        let mut routes = FileRouteRegistry::new(&mut app);
        let csrf = store.clone();
        routes
            .endpoint(
                "token/route.rs",
                "/token",
                FileEndpoint::new().get(move |request| csrf.csrf_response(request)),
            )
            .unwrap();
        let (called, guarded, observed) = (calls.clone(), guards.clone(), seen.clone());
        routes
            .endpoint(
                "uploads/route.rs",
                "/uploads",
                FileEndpoint::new()
                    .get(|request| {
                        assert!(request.multipart().is_none());
                        Response::text("Read")
                    })
                    .post_multipart(move |request| {
                        assert!(request.form_data().is_none() && request.json().is_none());
                        let data = request.multipart().unwrap();
                        assert_eq!(data.fields().get("caption"), Some("Rust + %41"));
                        assert_eq!(data.fields().len(), 1);
                        *observed.lock().unwrap() = data.file().cloned();
                        called.fetch_add(1, Ordering::SeqCst);
                        Response::text("Stored")
                    })
                    .guard(RouteGuard::new(move |context| {
                        if context.request().method() == Method::Post {
                            assert!(context.request().multipart().is_some());
                            guarded.fetch_add(1, Ordering::SeqCst);
                        }
                        GuardDecision::Private
                    }))
                    .guard(store.csrf_guard())
                    .guard(quota.session_guard(store)),
            )
            .unwrap();
        routes
            .endpoint(
                "form/route.rs",
                "/form",
                FileEndpoint::new().post_form(|_| Response::text("form")),
            )
            .unwrap();
        routes
            .endpoint(
                "json/route.rs",
                "/json",
                FileEndpoint::new().post_json(|_| Response::text("json")),
            )
            .unwrap();
        routes.finish().unwrap();
        let server = Running::new(app.router(None).unwrap(), ServerConfig::default());
        let response = server.raw("GET", "/token", &format!("Cookie: {cookie}\r\n"), b"");
        assert_eq!(status(&response), 200);
        let token = header(&response, CSRF_HEADER).unwrap().to_owned();
        Self {
            server,
            cookie,
            token,
            calls,
            guards,
            seen,
        }
    }
    fn cookie(&self) -> String {
        format!("Cookie: {}\r\n", self.cookie)
    }
}
#[test]
fn native_hidden_and_header_csrf_preserve_exact_binary_without_mixing_profiles() {
    let f = Fixture::new(10);
    let binary = b"\0\xff\r\n--testX\r\n\x80";
    for token in [true, false] {
        let bytes = body(token.then_some(f.token.as_str()), "Rust 🦀.bin", binary);
        let extra = format!(
            "{}{}",
            f.cookie(),
            if token {
                String::new()
            } else {
                format!("{CSRF_HEADER}: {}\r\n", f.token)
            }
        );
        let result = f.server.post("/uploads?caption=query", &extra, &bytes);
        assert_eq!(status(&result), 200);
        assert_eq!(header(&result, "cache-control"), Some("no-store"));
        let seen = f.seen.lock().unwrap();
        assert_eq!(seen.as_ref().unwrap().bytes(), binary);
        assert_eq!(seen.as_ref().unwrap().file_name(), "Rust 🦀.bin");
    }
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
}
#[test]
fn missing_invalid_ambiguous_and_query_only_csrf_skip_the_upload_handler() {
    let f = Fixture::new(20);
    for (cookie, token, extra, path) in [
        (false, Some(f.token.as_str()), String::new(), "/uploads"),
        (true, None, String::new(), "/uploads"),
        (true, Some("bad"), String::new(), "/uploads"),
        (
            true,
            Some(f.token.as_str()),
            format!("{CSRF_HEADER}: {}\r\n", f.token),
            "/uploads",
        ),
        (true, None, String::new(), "/uploads?_ruvane_csrf=ignored"),
    ] {
        let extra = format!(
            "{}{}",
            if cookie { f.cookie() } else { String::new() },
            extra
        );
        assert_eq!(
            status(&f.server.post(path, &extra, &body(token, "x.bin", b"x"))),
            403
        );
    }
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(f.seen.lock().unwrap().is_none());
}
#[test]
fn malformed_multipart_and_budget_failures_precede_guards_and_handlers() {
    let f = Fixture::new(20);
    for (bytes, expected) in [
        (body(Some(&f.token), "../secret", b"x"), 400),
        (
            body(Some(&f.token), "x.bin", &vec![0; MAX_UPLOAD_BYTES + 1]),
            413,
        ),
        (
            b"--test\r\nContent-Disposition: form-data; name=x\r\n\r\ntruncated".to_vec(),
            400,
        ),
        (
            body(Some(&f.token), "x.bin", b"x")
                .into_iter()
                .chain(b"epilogue".iter().copied())
                .collect(),
            400,
        ),
    ] {
        assert_eq!(
            status(&f.server.post("/uploads", &f.cookie(), &bytes)),
            expected
        );
    }
    let mut duplicate = body(Some(&f.token), "x.bin", b"x");
    let close = duplicate.len() - 10;
    duplicate.splice(
        close..close,
        b"--test\r\nContent-Disposition: form-data; name=attachment\r\n\r\nshadow\r\n"
            .iter()
            .copied(),
    );
    assert_eq!(
        status(&f.server.post("/uploads", &f.cookie(), &duplicate)),
        400
    );
    assert_eq!(f.guards.load(Ordering::SeqCst), 0);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn native_quotas_refuse_before_file_storage_and_keep_retry_metadata() {
    let f = Fixture::new(2);
    let bytes = body(Some(&f.token), "x.bin", b"x");
    for expected in [200, 200, 429] {
        let result = f.server.post("/uploads", &f.cookie(), &bytes);
        assert_eq!(status(&result), expected);
        if expected == 429 {
            assert_eq!(
                header(&result, "ruvane-action-rejected"),
                Some("rate-limit")
            );
            assert!(
                header(&result, "retry-after")
                    .unwrap()
                    .parse::<u32>()
                    .unwrap()
                    > 0
            );
        }
    }
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
}
#[test]
fn route_media_length_origin_revision_and_method_checks_cannot_switch_profiles() {
    let f = Fixture::new(30);
    let server = &f.server;
    let bytes = body(Some(&f.token), "x.bin", b"x");
    for path in ["/form", "/json"] {
        assert_eq!(status(&server.post(path, &f.cookie(), &bytes)), 415);
    }
    let base = format!(
        "Cookie: {}\r\nOrigin: http://{}\r\n",
        f.cookie, server.address
    );
    for (headers, expected) in [
        (
            format!("{base}Content-Type: application/json\r\nContent-Length: 0\r\n"),
            415,
        ),
        (
            format!("{base}Content-Type: multipart/form-data; boundary=test\r\n"),
            411,
        ),
        (
            format!(
                "{base}Content-Type: multipart/form-data; boundary=test\r\nContent-Length: 65537\r\n"
            ),
            413,
        ),
        (
            format!(
                "{base}Content-Type: multipart/form-data; boundary=test; boundary=test\r\nContent-Length: 1\r\n"
            ),
            415,
        ),
        (
            "Content-Type: multipart/form-data; boundary=test\r\nContent-Length: 1\r\n".to_string(),
            403,
        ),
        (
            format!(
                "{base}Content-Type: multipart/form-data; boundary=test\r\nContent-Length: 0\r\nContent-Encoding: gzip\r\n"
            ),
            415,
        ),
    ] {
        assert_eq!(
            status(&server.raw("POST", "/uploads", &headers, b"")),
            expected
        );
    }
    let foreign=server.raw("POST","/uploads",&format!("Origin: http://foreign.example\r\nContent-Type: multipart/form-data; boundary=test\r\nContent-Length: {}\r\n",bytes.len()),b"");
    assert_eq!(status(&foreign), 403);
    let invalid=server.raw("POST","/uploads",&format!("{base}Content-Type: multipart/form-data; boundary=test\r\nContent-Length: {}\r\nContent-Length: {}\r\n",bytes.len(),bytes.len()),b"");
    assert_eq!(status(&invalid), 400);
    let wrong = server.raw(
        "PUT",
        "/uploads",
        &format!("{base}Content-Type: multipart/form-data; boundary=test\r\nContent-Length: 0\r\n"),
        b"",
    );
    assert_eq!(status(&wrong), 405);
    assert_eq!(header(&wrong, "allow"), Some("GET, HEAD, POST"));
    assert_eq!(f.guards.load(Ordering::SeqCst), 0);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn incomplete_body_and_request_deadline_never_run_the_multipart_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut router = Router::new();
    router
        .post_multipart("/upload", move |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Response::text("unexpected")
        })
        .unwrap();
    let server = Running::new(
        router,
        ServerConfig {
            request_timeout: Duration::from_millis(120),
            ..ServerConfig::default()
        },
    );
    let headers = format!(
        "Origin: http://{}\r\nContent-Type: multipart/form-data; boundary=test\r\nContent-Length: 400\r\n",
        server.address
    );
    assert_eq!(
        status(&server.raw("POST", "/upload", &headers, b"--test\r\n")),
        400
    );
    let mut stream = TcpStream::connect(server.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "POST /upload HTTP/1.1\r\nHost: {}\r\n{headers}\r\n--test\r\n",
        server.address
    )
    .unwrap();
    assert_eq!(status(&read(&mut stream)), 408);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn multipart_registration_shares_the_post_namespace_and_failure_is_atomic() {
    let mut router = Router::new();
    router
        .post_form("/same", |_| Response::text("form"))
        .unwrap();
    assert!(
        router
            .post_multipart("/same", |_| Response::text("multipart"))
            .is_err()
    );
    for endpoint in [
        FileEndpoint::new()
            .post_form(|_| Response::text("form"))
            .post_multipart(|_| Response::text("multipart")),
        FileEndpoint::new()
            .post_multipart(|_| Response::text("multipart"))
            .post_json(|_| Response::text("json")),
    ] {
        let mut app = App::new();
        let mut routes = FileRouteRegistry::new(&mut app);
        assert!(routes.endpoint("same/route.rs", "/same", endpoint).is_err());
        assert!(routes.finish().is_err());
        assert_eq!(app.server_only_routes().count(), 0);
        let mut routes = FileRouteRegistry::new(&mut app);
        routes
            .endpoint(
                "same/route.rs",
                "/same",
                FileEndpoint::new().post_multipart(|_| Response::text("good")),
            )
            .unwrap();
        routes.finish().unwrap();
    }
}

#[path = "../examples/support/multipart_demo.rs"]
mod demo;
#[test]
fn demo_requires_a_session_validates_fields_and_stops_at_eight_memory_entries() {
    let server = Running::new(
        demo::app().unwrap().router(None).unwrap(),
        ServerConfig::default(),
    );
    let anonymous = server.raw("GET", "/", "", b"");
    assert_eq!(status(&anonymous), 200);
    assert!(!String::from_utf8_lossy(&anonymous.1).contains("type=\"file\""));
    assert_eq!(
        status(&server.post("/upload", "", &body(None, "x.bin", b"x"))),
        403
    );
    let opened=server.raw("POST","/demo-session",&format!("Origin: http://{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\n",server.address),b"");
    assert_eq!(status(&opened), 303);
    let set = header(&opened, "set-cookie").unwrap();
    assert!(set.contains("HttpOnly"));
    let cookie = set.split(';').next().unwrap().to_owned();
    let extra = format!("Cookie: {cookie}\r\n");
    let page = server.raw("GET", "/", &extra, b"");
    assert_eq!(header(&page, "cache-control"), Some("no-store"));
    assert!(
        header(&page, "content-security-policy")
            .unwrap()
            .contains("script-src 'none'")
    );
    let html = String::from_utf8(page.1).unwrap();
    let token = html
        .split("name=\"_ruvane_csrf\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(html.contains("enctype=\"multipart/form-data\""));
    let missing = server.post("/upload", &extra, &body(Some(token), "", b""));
    assert_eq!(status(&missing), 400);
    assert!(String::from_utf8_lossy(&missing.1).contains("Select one file."));
    let malformed = body(Some(token), "x.bin", b"x");
    let text = String::from_utf8(malformed)
        .unwrap()
        .replace("Rust + %41", "bad\r\ncaption");
    assert_eq!(
        status(&server.post("/upload", &extra, text.as_bytes())),
        400
    );
    let response = server.post(
        "/upload",
        &extra,
        &body(Some(token), "<script>.bin", b"\xff\0"),
    );
    assert_eq!(status(&response), 303);
    for _ in 0..7 {
        assert_eq!(
            status(&server.post(
                "/upload",
                &extra,
                &body(Some(token), "x.bin", &vec![0; MAX_UPLOAD_BYTES])
            )),
            303
        );
    }
    assert_eq!(
        status(&server.post("/upload", &extra, &body(Some(token), "overflow.bin", b"x"))),
        503
    );
    let page = server.raw("GET", "/", &extra, b"");
    let html = String::from_utf8(page.1).unwrap();
    assert!(html.contains("Stored files: 8 / 8"));
    assert!(html.contains("&lt;script&gt;.bin"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("32768 bytes"));
    assert!(!html.contains("overflow.bin"));
    let data = format!("_ruvane_csrf={token}");
    let closed=server.raw("POST","/logout",&format!("{extra}Origin: http://{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",server.address,data.len()),data.as_bytes());
    assert_eq!(status(&closed), 303);
    assert_eq!(
        status(&server.post("/upload", &extra, &body(Some(token), "x.bin", b"x"))),
        403
    );
}
