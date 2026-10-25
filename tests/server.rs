use ruvane::{
    Document, Method, Response, RouteError, Router, Server, ServerConfig, ShutdownHandle,
    StatusCode, Tag, element,
};
#[path = "../examples/support/blog.rs"]
mod blog;
#[path = "../examples/support/server_tasks.rs"]
mod server_tasks;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct Running {
    address: SocketAddr,
    shutdown: ShutdownHandle,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl Running {
    fn start(router: Router, config: ServerConfig) -> Self {
        let server =
            Server::bind_with_config("127.0.0.1:0".parse().unwrap(), router, config).unwrap();
        let address = server.local_addr().unwrap();
        let shutdown = server.shutdown_handle();
        let thread = Some(thread::spawn(move || server.run()));
        Self {
            address,
            shutdown,
            thread,
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(2)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
    }

    fn request(&self, method: &str, target: &str) -> Vec<u8> {
        self.raw(
            format!(
                "{method} {target} HTTP/1.1\r\nHost: {}\r\n\r\n",
                self.address
            )
            .as_bytes(),
        )
    }

    fn raw(&self, input: &[u8]) -> Vec<u8> {
        let mut stream = self.connect();
        stream.write_all(input).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        read_closed(&mut stream)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.shutdown();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .expect("server thread")
                .expect("server result");
        }
    }
}

fn read_closed(stream: &mut TcpStream) -> Vec<u8> {
    let mut output = Vec::new();
    // Unread pipelined data can cause a reset; keep any response already received.
    match stream.read_to_end(&mut output) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
        Err(error) => panic!("connection did not close: {error}"),
    }
    output
}

fn parts(output: &[u8]) -> (&str, &[u8]) {
    let end = output
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("complete headers");
    let headers = std::str::from_utf8(&output[..end]).unwrap();
    // Check CSP on every response, including errors.
    let policies: Vec<_> = headers
        .lines()
        .filter_map(|line| line.strip_prefix("Content-Security-Policy: "))
        .collect();
    assert_eq!(
        policies,
        [
            "default-src 'none'; script-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
        ]
    );
    (headers, &output[end + 4..])
}

fn status(output: &[u8]) -> u16 {
    parts(output).0.split(' ').nth(1).unwrap().parse().unwrap()
}

fn content_length(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse()
        .unwrap()
}

fn counted_router() -> (Router, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut router = Router::new();
    router
        .get("/", move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Response::text("ok\n")
        })
        .unwrap();
    (router, calls)
}

fn capture_pattern(count: usize) -> String {
    use std::fmt::Write as _;
    let mut pattern = String::new();
    for index in 0..count {
        write!(&mut pattern, "/{{p{index}}}").unwrap();
    }
    pattern
}

#[test]
fn serves_rendered_html_utf8_lengths_and_fixed_headers() {
    let mut router = Router::new();
    router
        .get("/", |_| {
            Response::html(Document::new("Rust", element(Tag::P).child("<script> & 🦀")).render())
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let output = server.request("GET", "/");
    let (headers, body) = parts(&output);
    assert_eq!(status(&output), 200);
    assert_eq!(content_length(headers), body.len());
    assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
    assert!(headers.contains("Connection: close"));
    assert!(headers.contains("X-Content-Type-Options: nosniff"));
    assert!(headers.contains("Cache-Control: no-store"));
    let html = std::str::from_utf8(body).unwrap();
    assert!(html.contains("&lt;script&gt; &amp; 🦀"));
    assert!(!html.contains("<script>"));
}

#[test]
fn head_has_get_headers_but_no_body_including_errors() {
    let mut router = Router::new();
    router
        .get("/", |request| {
            assert!(matches!(request.method(), Method::Get | Method::Head));
            Response::text("Hello 🦀")
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let get = server.request("GET", "/");
    let head = server.request("HEAD", "/");
    assert_eq!(parts(&get).0, parts(&head).0);
    assert!(parts(&head).1.is_empty());
    let missing = server.request("HEAD", "/missing");
    assert_eq!(status(&missing), 404);
    assert!(parts(&missing).1.is_empty());
    let malformed = server.raw(b"HEAD / HTTP/1.1\r\nHost: invalid\r\n\r\n");
    assert_eq!(status(&malformed), 400);
    assert!(parts(&malformed).1.is_empty());
}

#[test]
fn routes_are_exact_queries_are_raw_and_paths_are_not_decoded() {
    let mut router = Router::new();
    router
        .get("/hello", |request| {
            Response::text(request.query().unwrap_or("none"))
        })
        .unwrap();
    router.get("/health", |_| Response::text("ok\n")).unwrap();
    let server = Running::start(router, ServerConfig::default());
    assert_eq!(
        parts(&server.request("GET", "/hello?x=%3Cscript%3E")).1,
        b"x=%3Cscript%3E"
    );
    assert_eq!(parts(&server.request("GET", "/health")).1, b"ok\n");
    for path in ["/Hello", "/hello/", "/%68ello", "/missing"] {
        assert_eq!(status(&server.request("GET", path)), 404, "{path}");
    }
}

#[test]
fn invalid_and_duplicate_routes_fail_before_serving() {
    let mut router = Router::new();
    for path in [
        "",
        "relative",
        "//host",
        "/../",
        "/./x",
        "/a?query",
        "/#fragment",
        "/a\\b",
        "/%xx",
    ] {
        assert_eq!(
            router.get(path, |_| Response::text("unreachable")),
            Err(RouteError::InvalidPath),
            "{path}"
        );
    }
    router.get("/", |_| Response::text("first")).unwrap();
    assert_eq!(
        router.get("/", |_| Response::text("second")),
        Err(RouteError::DuplicatePath)
    );
}

#[test]
fn method_errors_and_unsupported_features_never_dispatch() {
    let (router, calls) = counted_router();
    let server = Running::start(router, ServerConfig::default());
    for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE"] {
        let output = server.request(method, "/");
        assert_eq!(status(&output), 405);
        assert!(parts(&output).0.contains("Allow: GET, HEAD"));
    }
    assert_eq!(status(&server.request("UNKNOWN", "/")), 501);
    let cases = [
        ("Transfer-Encoding: chunked\r\n", 501),
        ("Content-Length: 1\r\n", 413),
        ("Expect: 100-continue\r\n", 417),
        ("Upgrade: websocket\r\n", 501),
    ];
    for (headers, expected) in cases {
        let input = format!(
            "GET / HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            server.address
        );
        assert_eq!(status(&server.raw(input.as_bytes())), expected);
    }
    assert_eq!(
        status(
            &server.raw(format!("GET / HTTP/1.0\r\nHost: {}\r\n\r\n", server.address).as_bytes())
        ),
        505
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn ambiguous_lengths_duplicate_headers_and_controls_never_dispatch() {
    let (router, calls) = counted_router();
    let server = Running::start(router, ServerConfig::default());
    for extra in [
        "Content-Length: 0\r\nContent-Length: 1\r\n",
        "Content-Length: 0\r\ncontent-length: 0\r\n",
        "Content-Length: 0, 0\r\n",
        "Content-Length: -1\r\n",
        "Content-Length: 9999999999999999999999999999\r\n",
        "Transfer-Encoding: chunked\r\nContent-Length: 0\r\n",
        "Host: evil.test\r\n",
        "X-Test: a\r\nx-test: b\r\n",
        "X-Test : value\r\n",
        " folded: value\r\n",
        "X-Test: a\nb\r\n",
        "X-Test: \0\r\n",
    ] {
        let input = format!("GET / HTTP/1.1\r\nHost: {}\r\n{extra}\r\n", server.address);
        assert_eq!(status(&server.raw(input.as_bytes())), 400, "{extra:?}");
    }
    for input in [
        b"GET / HTTP/1.1\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\nHost: localhost\n\n",
        b"GET / HTTP/1.1\r\nHost: ",
    ] {
        assert_eq!(status(&server.raw(input)), 400);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn host_must_match_loopback_and_actual_port() {
    let (router, calls) = counted_router();
    let server = Running::start(router, ServerConfig::default());
    for authority in [
        "evil.test".to_owned(),
        format!("evil.test:{}", server.address.port()),
        "localhost:0".to_owned(),
        format!("user@localhost:{}", server.address.port()),
    ] {
        assert_eq!(
            status(&server.raw(format!("GET / HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())),
            400
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        status(
            &server.raw(
                format!(
                    "GET / HTTP/1.1\r\nHost: LOCALHOST:{}\r\n\r\n",
                    server.address.port()
                )
                .as_bytes()
            )
        ),
        200
    );
}

#[test]
fn first_request_only_even_when_more_are_pipelined() {
    let (router, calls) = counted_router();
    let server = Running::start(router, ServerConfig::default());
    let request = format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", server.address);
    let output = server.raw(request.repeat(2).as_bytes());
    assert_eq!(status(&output), 200);
    assert_eq!(
        output
            .windows(8)
            .filter(|bytes| *bytes == b"HTTP/1.1")
            .count(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn reads_fragmented_requests_including_split_crlf_delimiters() {
    let (router, calls) = counted_router();
    let server = Running::start(router, ServerConfig::default());
    let mut stream = server.connect();
    for part in [
        b"GET / HTTP/1.1\r".as_slice(),
        b"\nHost: ",
        server.address.to_string().as_bytes(),
        b"\r",
        b"\n\r",
        b"\n",
    ] {
        stream.write_all(part).unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(status(&read_closed(&mut stream)), 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn header_byte_count_and_request_line_limits_are_enforced() {
    let (router, calls) = counted_router();
    let config = ServerConfig {
        max_header_bytes: 128,
        max_request_line_bytes: 40,
        max_headers: 2,
        ..ServerConfig::default()
    };
    let server = Running::start(router, config);
    let line = format!("GET /{} HTTP/1.1\r\n", "x".repeat(50));
    assert_eq!(status(&server.raw(line.as_bytes())), 414);
    let oversized = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nX: {}",
        server.address,
        "a".repeat(200)
    );
    assert_eq!(status(&server.raw(oversized.as_bytes())), 431);
    let excess_fields = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nX: a\r\nY: b\r\n\r\n",
        server.address
    );
    assert_eq!(status(&server.raw(excess_fields.as_bytes())), 431);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // The delimiter may end exactly at the byte budget.
    let prefix = format!("GET / HTTP/1.1\r\nHost: {}\r\nX: ", server.address);
    let exact = format!("{prefix}{}\r\n\r\n", "x".repeat(128 - prefix.len() - 4));
    assert_eq!(exact.len(), 128);
    assert_eq!(status(&server.raw(exact.as_bytes())), 200);
}

#[test]
fn idle_and_trickling_clients_hit_a_total_request_deadline() {
    let (router, calls) = counted_router();
    let server = Running::start(
        router,
        ServerConfig {
            request_timeout: Duration::from_millis(250),
            ..ServerConfig::default()
        },
    );
    let mut idle = server.connect();
    let started = Instant::now();
    assert_eq!(status(&read_closed(&mut idle)), 408);
    assert!(started.elapsed() < Duration::from_secs(2));

    let mut stream = server.connect();
    let mut writer = stream.try_clone().unwrap();
    let started = Instant::now();
    let writer_thread = thread::spawn(move || {
        if writer.write_all(b"GET / HTTP/1.1\r\nHost: ").is_err() {
            return;
        }
        for _ in 0..100 {
            if writer.write_all(b"x").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
    });
    assert_eq!(status(&read_closed(&mut stream)), 408);
    assert!(started.elapsed() < Duration::from_secs(2));
    writer_thread.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn handler_panics_and_oversized_output_are_generic_and_workers_survive() {
    let mut router = Router::new();
    router
        .get("/panic", |_| panic!("private failure details"))
        .unwrap();
    router
        .get("/large", |_| Response::text("x".repeat(65)))
        .unwrap();
    router
        .get("/", |_| Response::text("still serving"))
        .unwrap();
    let server = Running::start(
        router,
        ServerConfig {
            workers: 1,
            max_response_body_bytes: 64,
            ..ServerConfig::default()
        },
    );
    for path in ["/panic", "/large"] {
        let output = server.request("GET", path);
        assert_eq!(status(&output), 500);
        assert_eq!(parts(&output).1, b"Internal Server Error\n");
        assert!(!String::from_utf8_lossy(&output).contains("private failure"));
        assert_eq!(status(&server.request("GET", "/")), 200);
    }
}

#[test]
fn response_body_cannot_inject_headers_or_change_the_wire_length() {
    let mut router = Router::new();
    let payload = "\r\nSet-Cookie: injected=1\r\nHTTP/1.1 200 OK\r\n🦀";
    router
        .get("/", move |_| {
            Response::text(payload).with_status(StatusCode::NotFound)
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let output = server.request("GET", "/");
    let (headers, body) = parts(&output);
    assert_eq!(status(&output), 404);
    assert!(!headers.contains("Set-Cookie"));
    assert_eq!(body, payload.as_bytes());
    assert_eq!(content_length(headers), payload.len());
}

#[test]
fn rejects_nonloopback_bindings_and_invalid_resource_budgets() {
    for address in ["0.0.0.0:0", "[::]:0", "192.0.2.1:0"] {
        assert_eq!(
            Server::bind(address.parse().unwrap(), Router::new())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    let invalid = [
        ServerConfig {
            workers: 0,
            ..ServerConfig::default()
        },
        ServerConfig {
            workers: 65,
            ..ServerConfig::default()
        },
        ServerConfig {
            pending_connections: 0,
            ..ServerConfig::default()
        },
        ServerConfig {
            pending_connections: 1025,
            ..ServerConfig::default()
        },
        ServerConfig {
            max_header_bytes: 0,
            ..ServerConfig::default()
        },
        ServerConfig {
            max_header_bytes: usize::MAX,
            ..ServerConfig::default()
        },
        ServerConfig {
            max_request_line_bytes: usize::MAX,
            ..ServerConfig::default()
        },
        ServerConfig {
            max_headers: 0,
            ..ServerConfig::default()
        },
        ServerConfig {
            max_response_body_bytes: 0,
            ..ServerConfig::default()
        },
        ServerConfig {
            request_timeout: Duration::ZERO,
            ..ServerConfig::default()
        },
        ServerConfig {
            write_timeout: Duration::MAX,
            ..ServerConfig::default()
        },
    ];
    for config in invalid {
        assert_eq!(
            Server::bind_with_config("127.0.0.1:0".parse().unwrap(), Router::new(), config)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn bounded_queue_closes_excess_connections_and_shutdown_drains_workers() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = std::sync::Mutex::new(release_rx);
    let mut router = Router::new();
    router
        .get("/", move |_| {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            Response::text("done")
        })
        .unwrap();
    let server = Running::start(
        router,
        ServerConfig {
            workers: 1,
            pending_connections: 1,
            ..ServerConfig::default()
        },
    );
    let mut active = server.connect();
    active
        .write_all(format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", server.address).as_bytes())
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // Fill the only queue slot, then check that excess connections close promptly.
    let mut queued = server.connect();
    let mut excess: Vec<_> = (0..4).map(|_| server.connect()).collect();
    for stream in &mut excess {
        assert!(read_closed(stream).is_empty());
    }
    server.shutdown.shutdown();
    let started = Instant::now();
    assert!(read_closed(&mut queued).is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
    release_tx.send(()).unwrap();
    let _ = read_closed(&mut active);
}

#[test]
fn shutdown_interrupts_idle_socket_io_without_waiting_for_request_timeout() {
    let (router, _) = counted_router();
    let server = Running::start(
        router,
        ServerConfig {
            request_timeout: Duration::from_secs(60),
            ..ServerConfig::default()
        },
    );
    let mut stream = server.connect();
    stream.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let started = Instant::now();
    server.shutdown.shutdown();
    assert!(read_closed(&mut stream).is_empty());
    drop(server);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn shutdown_before_run_releases_the_listening_socket() {
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), Router::new()).unwrap();
    let address = server.local_addr().unwrap();
    server.shutdown_handle().shutdown();
    server.run().unwrap();
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err());
}

#[test]
fn unread_large_responses_cannot_hold_the_only_worker_indefinitely() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let mut router = Router::new();
    router
        .get("/large", move |_| {
            entered_tx.send(()).unwrap();
            Response::text("x".repeat(32 * 1024 * 1024))
        })
        .unwrap();
    router.get("/", |_| Response::text("available")).unwrap();
    let server = Running::start(
        router,
        ServerConfig {
            workers: 1,
            max_response_body_bytes: 32 * 1024 * 1024,
            write_timeout: Duration::from_millis(150),
            ..ServerConfig::default()
        },
    );
    let mut unread = server.connect();
    unread
        .write_all(format!("GET /large HTTP/1.1\r\nHost: {}\r\n\r\n", server.address).as_bytes())
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    assert_eq!(status(&server.request("GET", "/")), 200);
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(unread);
}

#[test]
fn connections_waiting_in_the_queue_do_not_receive_a_fresh_read_budget() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = std::sync::Mutex::new(release_rx);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut router = Router::new();
    router
        .get("/block", move |_| {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            Response::text("done")
        })
        .unwrap();
    router
        .get("/", move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Response::text("should not run")
        })
        .unwrap();
    let server = Running::start(
        router,
        ServerConfig {
            workers: 1,
            request_timeout: Duration::from_millis(200),
            ..ServerConfig::default()
        },
    );
    let mut active = server.connect();
    active
        .write_all(format!("GET /block HTTP/1.1\r\nHost: {}\r\n\r\n", server.address).as_bytes())
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut queued = server.connect();
    queued
        .write_all(format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", server.address).as_bytes())
        .unwrap();
    thread::sleep(Duration::from_millis(400));
    release_tx.send(()).unwrap();
    assert_eq!(status(&read_closed(&mut active)), 200);
    assert_eq!(status(&read_closed(&mut queued)), 408);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn fixed_routes_win_in_both_registration_orders_and_keep_raw_matching() {
    for fixed_first in [true, false] {
        let mut router = Router::new();
        if fixed_first {
            router
                .get("/articles/new", |_| Response::text("fixed"))
                .unwrap();
        }
        router
            .get("/articles/{slug}", |request| {
                Response::text(request.param("slug").unwrap())
            })
            .unwrap();
        if !fixed_first {
            router
                .get("/articles/new", |_| Response::text("fixed"))
                .unwrap();
        }
        router
            .get("/articles/%FF", |_| Response::text("raw fixed"))
            .unwrap();
        let server = Running::start(router, ServerConfig::default());
        assert_eq!(parts(&server.request("GET", "/articles/new")).1, b"fixed");
        assert_eq!(parts(&server.request("GET", "/articles/%6Eew")).1, b"new");
        assert_eq!(
            parts(&server.request("GET", "/articles/%FF")).1,
            b"raw fixed"
        );
        assert_eq!(
            parts(&server.request("GET", "/articles/hello?x=1")).1,
            b"hello"
        );
        assert_eq!(status(&server.request("GET", "/articles/")), 404);
        assert_eq!(status(&server.request("GET", "/articles/a/b")), 404);
    }
}

#[test]
fn rejects_dynamic_ambiguity_and_invalid_parameter_patterns() {
    for (a, b) in [
        ("/a/{x}", "/a/{y}"),
        ("/{x}/new", "/a/{y}"),
        ("/a/{x}/{y}", "/a/b/{z}"),
    ] {
        for (first, second) in [(a, b), (b, a)] {
            let mut router = Router::new();
            router.get(first, |_| Response::text("first")).unwrap();
            assert_eq!(
                router.get(second, |_| Response::text("second")),
                Err(RouteError::AmbiguousRoute)
            );
        }
    }
    let mut router = Router::new();
    for pattern in [
        "/{id}/{id}",
        "/{}",
        "/{1id}",
        "/{a-b}",
        "/{id",
        "/id}",
        "/a{id}",
        "/{id?}",
        "/{id*}",
        "/{id}//x",
    ] {
        assert_eq!(
            router.get(pattern, |_| Response::text("bad")),
            Err(RouteError::InvalidPath),
            "{pattern}"
        );
    }
    let excessive = capture_pattern(ruvane::MAX_PATH_PARAMS + 1);
    assert_eq!(
        router.get(excessive, |_| Response::text("bad")),
        Err(RouteError::InvalidPath)
    );
    router.get("/a/{id}", |_| Response::text("a")).unwrap();
    router.get("/b/{id}", |_| Response::text("b")).unwrap();
    assert_eq!(
        router.get("/a/{id}", |_| Response::text("again")),
        Err(RouteError::DuplicatePath)
    );
}

#[test]
fn captures_multiple_segments_and_distinguishes_typed_parameter_errors() {
    let mut router = Router::new();
    router
        .get("/users/{user}/posts/{post}", |request| {
            assert_eq!(
                request.param_as::<u32>("absent"),
                Err(ruvane::ParamError::Missing)
            );
            match request.param_as::<u32>("post") {
                Ok(id) => Response::text(format!("{}:{id}", request.param("user").unwrap())),
                Err(_) => Response::text("invalid id").with_status(StatusCode::BadRequest),
            }
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    assert_eq!(
        parts(&server.request("GET", "/users/%F0%9F%A6%80/posts/42")).1,
        "🦀:42".as_bytes()
    );
    for path in ["/users/a/posts/no", "/users/a/posts/4294967296"] {
        assert_eq!(status(&server.request("GET", path)), 400);
    }
    let head = server.request("HEAD", "/users/a/posts/42");
    assert_eq!(status(&head), 200);
    assert_eq!(content_length(parts(&head).0), 4);
    assert!(parts(&head).1.is_empty());
}

#[test]
fn invalid_dynamic_values_are_rejected_before_the_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut router = Router::new();
    router
        .get("/value/{value}", move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Response::text("should not run")
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for value in [
        "a%2fb",
        "%5Cevil",
        "%2e",
        ".%2E",
        "%FF",
        "%C0%AF",
        "%ED%A0%80",
        "%00",
        "%0d%0a",
        "%7F",
        "%C2%85",
    ] {
        let output = server.request("GET", &format!("/value/{value}"));
        assert_eq!(status(&output), 400, "{value}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn decoded_path_values_are_bounded_by_utf8_bytes_and_never_decoded_twice() {
    let mut router = Router::new();
    router
        .get("/value/{value}", |request| {
            Response::text(request.param("value").unwrap())
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    assert_eq!(parts(&server.request("GET", "/value/%252F")).1, b"%2F");
    let maximum = "%CE%BB".repeat(ruvane::MAX_COMPONENT_BYTES / 2);
    let output = server.request("GET", &format!("/value/{maximum}"));
    assert_eq!(status(&output), 200);
    assert_eq!(parts(&output).1.len(), ruvane::MAX_COMPONENT_BYTES);
    assert_eq!(
        status(&server.request("GET", &format!("/value/{maximum}%CE%BB"))),
        400
    );
}

#[test]
fn queries_are_validated_before_dispatch_and_preserve_the_raw_accessor() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut router = Router::new();
    router
        .get("/", move |request| {
            counted.fetch_add(1, Ordering::SeqCst);
            Response::text(format!(
                "{}|{}",
                request.query().unwrap_or(""),
                request.query_params().get("q").unwrap_or("")
            ))
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for query in [
        "a=1&%61=2",
        "a=1&a=2",
        "=empty",
        "a=1&",
        "a=1&&b=2",
        "q=%FF",
        "q=%00",
        "q=%0a",
    ] {
        assert_eq!(
            status(&server.request("GET", &format!("/?{query}"))),
            400,
            "{query}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        parts(&server.request("GET", "/?q=a%26b%3Dc+%2526")).1,
        b"q=a%26b%3Dc+%2526|a&b=c+%26"
    );
}

#[test]
fn query_resource_limits_reject_without_dispatch() {
    let (router, calls) = counted_router();
    let server = Running::start(
        router,
        ServerConfig {
            max_request_line_bytes: 8192,
            ..ServerConfig::default()
        },
    );
    let fields = (0..=ruvane::MAX_QUERY_PAIRS)
        .map(|i| format!("k{i}=v"))
        .collect::<Vec<_>>()
        .join("&");
    for query in [
        fields,
        format!("q={}", "a".repeat(ruvane::MAX_COMPONENT_BYTES + 1)),
        "a".repeat(ruvane::MAX_INTERNAL_URL_BYTES + 1),
    ] {
        assert_eq!(status(&server.request("GET", &format!("/?{query}"))), 400);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn blog_has_pagination_unicode_articles_static_precedence_and_escaped_text() {
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    for path in [
        "/",
        "/articles",
        "/articles?page=2",
        "/articles?search=Rust&page=2",
        "/articles/about",
        "/articles/summer-%F0%9F%A6%80",
    ] {
        assert_eq!(status(&server.request("GET", path)), 200, "{path}");
    }
    let second = server.request("GET", "/articles?page=2");
    let second_html = std::str::from_utf8(parts(&second).1).unwrap();
    assert!(second_html.contains("href=\"/articles/summer-%F0%9F%A6%80\""));
    assert!(second_html.contains("Rendering &lt;text&gt; safely"));
    assert!(second_html.contains("Previous page"));
    let article = server.request("GET", "/articles/escaping");
    let html = std::str::from_utf8(parts(&article).1).unwrap();
    assert!(html.contains("&lt;script&gt;alert(&#39;hello&#39;)&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
    let reflected = server.request("GET", "/articles?search=%3Cscript%3E");
    assert!(
        std::str::from_utf8(parts(&reflected).1)
            .unwrap()
            .contains("Search: &lt;script&gt;")
    );
    for path in [
        "/articles?page=0",
        "/articles?page=oops",
        "/articles?page=99999999999999999999999999999",
    ] {
        assert_eq!(status(&server.request("GET", path)), 400, "{path}");
    }
    for path in ["/articles?page=99", "/articles/not-found"] {
        assert_eq!(status(&server.request("GET", path)), 404);
    }
}

#[test]
fn generated_blog_links_can_be_followed_as_internal_routes() {
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    for path in [
        "/",
        "/articles",
        "/articles?page=2",
        "/articles?page=2&search=Rust",
    ] {
        let output = server.request("GET", path);
        let html = std::str::from_utf8(parts(&output).1).unwrap();
        let mut found = 0;
        for following in html.split("href=\"").skip(1) {
            let target = following.split('"').next().unwrap().replace("&amp;", "&");
            assert!(target.starts_with('/') && !target.starts_with("//"));
            assert_eq!(status(&server.request("GET", &target)), 200, "{target}");
            found += 1;
        }
        assert!(found >= 3);
    }
}

#[test]
fn dynamic_capture_name_and_count_boundaries_are_exact() {
    let mut router = Router::new();
    let long_name = "x".repeat(64);
    router
        .get(format!("/long/{{{long_name}}}"), move |request| {
            Response::text(request.param(&long_name).unwrap())
        })
        .unwrap();
    let too_long = format!("/bad/{{{}}}", "x".repeat(65));
    assert_eq!(
        router.get(too_long, |_| Response::text("bad")),
        Err(RouteError::InvalidPath)
    );
    let pattern = capture_pattern(ruvane::MAX_PATH_PARAMS);
    router
        .get(pattern, |request| {
            assert_eq!(request.param("p0"), Some("x"));
            assert_eq!(request.param("p15"), Some("x"));
            Response::text("all captures")
        })
        .unwrap();
    router
        .get("/tail/{value}/", |request| {
            Response::text(request.param("value").unwrap())
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    assert_eq!(parts(&server.request("GET", "/long/value")).1, b"value");
    let target = "/x".repeat(ruvane::MAX_PATH_PARAMS);
    assert_eq!(status(&server.request("GET", &target)), 200);
    assert_eq!(parts(&server.request("GET", "/tail/one/")).1, b"one");
    assert_eq!(status(&server.request("GET", "/tail/one")), 404);
    let invalid_head = server.request("HEAD", "/tail/%FF/");
    assert_eq!(status(&invalid_head), 400);
    assert!(parts(&invalid_head).1.is_empty());
}

#[test]
fn arbitrary_binary_bodies_keep_wire_bytes_and_head_lengths() {
    let payload: Vec<u8> = (0..=255).collect();
    let expected = payload.clone();
    let mut router = Router::new();
    router
        .get("/bytes", move |_| {
            Response::bytes(payload.clone(), ruvane::MediaType::OctetStream)
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let get = server.request("GET", "/bytes");
    let head = server.request("HEAD", "/bytes");
    assert_eq!(parts(&get).1, expected);
    assert_eq!(parts(&get).0, parts(&head).0);
    assert!(parts(&head).1.is_empty());
    assert_eq!(content_length(parts(&get).0), 256);
    assert!(
        parts(&get)
            .0
            .contains("Content-Type: application/octet-stream")
    );
    assert!(parts(&get).0.contains("X-Content-Type-Options: nosniff"));
}

#[test]
fn embedded_assets_have_explicit_mime_types_and_fixed_route_priority() {
    let mut router = Router::new();
    router
        .get("/assets/{name}", |_| Response::text("dynamic"))
        .unwrap();
    router
        .asset(
            "/assets/style.png",
            b"body { color: #123; }",
            ruvane::MediaType::Css,
        )
        .unwrap();
    router
        .asset("/assets/empty.css", b"", ruvane::MediaType::Css)
        .unwrap();
    router
        .asset(
            "/assets/data",
            b"\0\xff\r\nHTTP/1.1 200 OK\r\nInjected: value",
            ruvane::MediaType::OctetStream,
        )
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let css = server.request("GET", "/assets/style.png?version=1");
    assert_eq!(parts(&css).1, b"body { color: #123; }");
    assert!(
        parts(&css)
            .0
            .contains("Content-Type: text/css; charset=utf-8")
    );
    assert!(parts(&css).0.contains("Cache-Control: no-store"));
    assert_eq!(
        parts(&css).0,
        parts(&server.request("HEAD", "/assets/style.png")).0
    );
    let empty = server.request("GET", "/assets/empty.css");
    assert_eq!(status(&empty), 200);
    assert_eq!(content_length(parts(&empty).0), 0);
    let binary = server.request("GET", "/assets/data");
    assert_eq!(
        parts(&binary).1,
        b"\0\xff\r\nHTTP/1.1 200 OK\r\nInjected: value"
    );
    assert!(!parts(&binary).0.contains("Injected:"));
    assert_eq!(parts(&server.request("GET", "/assets/other")).1, b"dynamic");
}

#[test]
fn undeclared_asset_paths_never_map_to_files_or_encoded_aliases() {
    let mut router = Router::new();
    router
        .asset("/assets/site.css", b"body {}", ruvane::MediaType::Css)
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for target in [
        "/assets/unknown",
        "/Cargo.toml",
        "/assets/%73ite.css",
        "/assets/%2e%2e/Cargo.toml",
        "/assets/site.css/extra",
    ] {
        assert_eq!(status(&server.request("GET", target)), 404, "{target}");
    }
    for target in ["/assets/../Cargo.toml", "/assets/site.css?v=1&%76=2"] {
        assert_eq!(status(&server.request("GET", target)), 400, "{target}");
    }
    let post = server.request("POST", "/assets/site.css");
    assert_eq!(status(&post), 405);
    assert!(parts(&post).0.contains("Allow: GET, HEAD"));
}

#[test]
fn response_budget_applies_equally_to_owned_bytes_and_embedded_assets() {
    let mut router = Router::new();
    router
        .get("/owned", |_| {
            Response::bytes(vec![0xff; 65], ruvane::MediaType::Png)
        })
        .unwrap();
    router
        .asset("/embedded", &[0xff; 65], ruvane::MediaType::Png)
        .unwrap();
    router
        .asset("/exact", &[0xff; 64], ruvane::MediaType::Png)
        .unwrap();
    let server = Running::start(
        router,
        ServerConfig {
            max_response_body_bytes: 64,
            ..ServerConfig::default()
        },
    );
    for path in ["/owned", "/embedded"] {
        let output = server.request("GET", path);
        assert_eq!(status(&output), 500);
        assert_eq!(parts(&output).1, b"Internal Server Error\n");
        assert!(
            parts(&output)
                .0
                .contains("Content-Type: text/plain; charset=utf-8")
        );
        let head = server.request("HEAD", path);
        assert_eq!(status(&head), 500);
        assert!(parts(&head).1.is_empty());
    }
    assert_eq!(parts(&server.request("GET", "/exact")).1, &[0xff; 64]);
}

#[test]
fn styled_blog_serves_its_css_and_original_png_assets() {
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    let page = server.request("GET", "/articles?page=2");
    let html = std::str::from_utf8(parts(&page).1).unwrap();
    assert!(html.contains("<link rel=\"stylesheet\" href=\"/assets/site.css\">"));
    assert!(html.contains("<img src=\"/assets/mark.png\" alt=\"\" width=\"96\" height=\"96\">"));
    assert!(html.contains("class=\"post-grid\""));
    let css = server.request("GET", "/assets/site.css");
    assert_eq!(parts(&css).1, include_bytes!("../examples/assets/site.css"));
    assert!(
        parts(&css)
            .0
            .contains("Content-Type: text/css; charset=utf-8")
    );
    assert!(
        std::str::from_utf8(parts(&css).1)
            .unwrap()
            .contains("@media (max-width: 640px)")
    );
    for (path, expected) in [
        (
            "/assets/mark.png",
            include_bytes!("../examples/assets/mark.png").as_slice(),
        ),
        (
            "/assets/geometry.png",
            include_bytes!("../examples/assets/geometry.png").as_slice(),
        ),
        (
            "/assets/landscape.png",
            include_bytes!("../examples/assets/landscape.png").as_slice(),
        ),
    ] {
        let get = server.request("GET", path);
        let head = server.request("HEAD", path);
        assert_eq!(status(&get), 200);
        assert_eq!(parts(&get).1, expected);
        assert_eq!(content_length(parts(&get).0), expected.len());
        assert!(parts(&get).0.contains("Content-Type: image/png"));
        assert_eq!(parts(&get).0, parts(&head).0);
        assert!(parts(&head).1.is_empty());
    }
}

#[test]
fn query_mode_is_fixed_by_registration_and_raw_query_remains_available() {
    use ruvane::QueryMode;
    let mut router = Router::new();
    router
        .get("/uri", |request| {
            assert_eq!(request.query_mode(), QueryMode::Uri);
            Response::text(format!(
                "{}|{}",
                request.query().unwrap(),
                request.query_params().get("q").unwrap()
            ))
        })
        .unwrap();
    router
        .get_form("/form", |request| {
            assert_eq!(request.query_mode(), QueryMode::Form);
            Response::text(format!(
                "{}|{}",
                request.query().unwrap(),
                request.query_params().get("q").unwrap()
            ))
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for fields in ["", "&_mode=form", "&_mode=uri&_form=true"] {
        let query = format!("q=Rust+%2B+HTML%252B{fields}");
        assert_eq!(
            parts(&server.request("GET", &format!("/uri?{query}"))).1,
            format!("{query}|Rust+++HTML%2B").as_bytes()
        );
        assert_eq!(
            parts(&server.request("GET", &format!("/form?{query}"))).1,
            format!("{query}|Rust + HTML%2B").as_bytes()
        );
    }
}

#[test]
fn selected_form_mode_does_not_preparse_keys_using_uri_rules() {
    let mut router = Router::new();
    router
        .get_form("/form", |request| {
            assert_eq!(request.query_params().get("a b"), Some("1"));
            assert_eq!(request.query_params().get("a+b"), Some("2"));
            Response::text("distinct")
        })
        .unwrap();
    router.get("/uri", |_| Response::text("distinct")).unwrap();
    let server = Running::start(router, ServerConfig::default());
    assert_eq!(
        parts(&server.request("GET", "/form?a+b=1&a%2Bb=2")).1,
        b"distinct"
    );
    assert_eq!(status(&server.request("GET", "/uri?a+b=1&a%2Bb=2")), 400);
    assert_eq!(status(&server.request("GET", "/uri?a+b=1&a%20b=2")), 200);
    assert_eq!(status(&server.request("GET", "/form?a+b=1&a%20b=2")), 400);
}

#[test]
fn invalid_form_queries_and_framing_never_reach_the_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut router = Router::new();
    router
        .get_form("/form", move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Response::text("unreachable")
        })
        .unwrap();
    let server = Running::start(
        router,
        ServerConfig {
            max_request_line_bytes: 8192,
            ..ServerConfig::default()
        },
    );
    let mut queries = [
        "q=1&q=2",
        "a+b=1&a%20b=2",
        "q=%FF",
        "q=%00",
        "q=%0d%0a",
        "q=%C2%85",
        "q=%",
        "q=1&",
        "=empty",
    ]
    .map(String::from)
    .to_vec();
    queries.push(format!("q={}", "+".repeat(ruvane::MAX_COMPONENT_BYTES + 1)));
    queries.push(
        (0..=ruvane::MAX_QUERY_PAIRS)
            .map(|i| format!("k{i}=v"))
            .collect::<Vec<_>>()
            .join("&"),
    );
    queries.push("x".repeat(ruvane::MAX_INTERNAL_URL_BYTES + 1));
    for query in queries {
        let output = server.request("GET", &format!("/form?{query}"));
        assert_eq!(status(&output), 400, "{query}");
        assert_eq!(parts(&output).1, b"Bad Request\n");
    }
    for (headers, expected) in [
        ("Content-Length: 0\r\nContent-Length: 0\r\n", 400),
        ("Content-Length: 1\r\n", 413),
        ("Transfer-Encoding: chunked\r\n", 501),
        ("Host: evil.test\r\n", 400),
    ] {
        let raw = format!(
            "GET /form?q=Rust+HTML HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            server.address
        );
        assert_eq!(status(&server.raw(raw.as_bytes())), expected);
    }
    assert_eq!(status(&server.request("POST", "/form?q=Rust+HTML")), 405);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn form_routes_share_duplicate_and_ambiguity_checks_with_uri_routes() {
    for form_first in [true, false] {
        let mut router = Router::new();
        if form_first {
            router
                .get_form("/same", |_| Response::text("first"))
                .unwrap();
            assert_eq!(
                router.get("/same", |_| Response::text("second")),
                Err(RouteError::DuplicatePath)
            );
            router
                .get_form("/a/{id}", |_| Response::text("first"))
                .unwrap();
            assert_eq!(
                router.get("/{area}/new", |_| Response::text("second")),
                Err(RouteError::AmbiguousRoute)
            );
        } else {
            router.get("/same", |_| Response::text("first")).unwrap();
            assert_eq!(
                router.get_form("/same", |_| Response::text("second")),
                Err(RouteError::DuplicatePath)
            );
            router.get("/a/{id}", |_| Response::text("first")).unwrap();
            assert_eq!(
                router.get_form("/{area}/new", |_| Response::text("second")),
                Err(RouteError::AmbiguousRoute)
            );
        }
        assert_eq!(
            router.get_form("/bad?query", |_| Response::text("bad")),
            Err(RouteError::InvalidPath)
        );
    }
}

#[test]
fn raw_route_precedence_selects_mode_before_decoding_and_captures_keep_plus() {
    use ruvane::QueryMode;
    for fixed_first in [true, false] {
        let mut router = Router::new();
        if fixed_first {
            router
                .get("/items/new", |request| {
                    assert_eq!(request.query_mode(), QueryMode::Uri);
                    Response::text(request.query_params().get("q").unwrap())
                })
                .unwrap();
        }
        router
            .get_form("/items/{slug}", |request| {
                assert_eq!(request.query_mode(), QueryMode::Form);
                Response::text(format!(
                    "{}|{}",
                    request.param("slug").unwrap(),
                    request.query_params().get("q").unwrap()
                ))
            })
            .unwrap();
        if !fixed_first {
            router
                .get("/items/new", |request| {
                    assert_eq!(request.query_mode(), QueryMode::Uri);
                    Response::text(request.query_params().get("q").unwrap())
                })
                .unwrap();
        }
        let server = Running::start(router, ServerConfig::default());
        assert_eq!(parts(&server.request("GET", "/items/new?q=a+b")).1, b"a+b");
        assert_eq!(
            parts(&server.request("GET", "/items/%6Eew?q=a+b")).1,
            b"new|a b"
        );
        assert_eq!(
            parts(&server.request("GET", "/items/a+b?q=a+b")).1,
            b"a+b|a b"
        );
        assert_eq!(status(&server.request("GET", "/items/%2F?q=a+b")), 400);
    }
}

#[test]
fn unknown_paths_and_assets_use_uri_query_rules() {
    let mut router = Router::new();
    router
        .asset("/assets/a.css", b"body{}", ruvane::MediaType::Css)
        .unwrap();
    router
        .get_form("/search", |_| Response::text("form"))
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    assert_eq!(
        status(&server.request("GET", "/missing?a+b=1&a%20b=2")),
        404
    );
    assert_eq!(
        status(&server.request("GET", "/missing?a+b=1&a%2Bb=2")),
        400
    );
    assert_eq!(
        status(&server.request("GET", "/assets/a.css?a+b=1&a%20b=2")),
        200
    );
    assert_eq!(
        status(&server.request("GET", "/assets/a.css?a+b=1&a%2Bb=2")),
        400
    );
}

#[test]
fn form_routes_support_head_for_success_and_invalid_queries() {
    let mut router = Router::new();
    router
        .get_form("/form", |request| {
            Response::text(request.query_params().get("q").unwrap())
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for (target, expected) in [("/form?q=Rust+%2B+HTML", 200), ("/form?q=1&q=2", 400)] {
        let get = server.request("GET", target);
        let head = server.request("HEAD", target);
        assert_eq!(status(&get), expected);
        assert_eq!(parts(&get).0, parts(&head).0);
        assert!(parts(&head).1.is_empty());
    }
}

#[test]
fn blog_native_search_retains_values_resets_page_and_preserves_pagination() {
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    let output = server.request("GET", "/articles?search=Rust+%2B+HTML&page=1");
    assert_eq!(status(&output), 200);
    let html = std::str::from_utf8(parts(&output).1).unwrap();
    assert!(html.contains("<form method=\"get\" action=\"/articles\" accept-charset=\"UTF-8\">"));
    assert!(html.contains(
        "<span>Search articles</span><input type=\"search\" name=\"search\" value=\"Rust + HTML\""
    ));
    assert!(html.contains("<input type=\"hidden\" name=\"page\" value=\"1\">"));
    assert!(html.contains("href=\"/articles\">Clear search</a>"));
    assert!(html.contains("Search: Rust + HTML"));
    assert!(html.contains("No articles found."));
    let output = server.request("GET", "/articles?search=Rust&page=1");
    let html = std::str::from_utf8(parts(&output).1).unwrap();
    assert!(html.contains("href=\"/articles?page=2&amp;search=Rust\">Next page</a>"));
    let output = server.request("GET", "/articles?page=2&search=Rust");
    assert_eq!(status(&output), 200);
    let html = std::str::from_utf8(parts(&output).1).unwrap();
    assert!(html.contains("href=\"/articles?page=1&amp;search=Rust\">Previous page</a>"));
    assert!(html.contains("name=\"page\" value=\"1\""));
    let output = server.request("GET", "/articles?search=summer+in+Rust");
    let html = std::str::from_utf8(parts(&output).1).unwrap();
    assert!(html.contains("value=\"summer in Rust\""));
    assert!(html.contains("Summer in Rust"));
}

#[test]
fn blog_validation_preserves_escaped_search_without_reflecting_invalid_page() {
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    for page in [
        "oops",
        "0",
        "18446744073709551615",
        "99999999999999999999999999",
    ] {
        let target = format!("/articles?search=%22%3E%3Cscript%3E%26%27&page={page}");
        let output = server.request("GET", &target);
        assert_eq!(status(&output), 400);
        let html = std::str::from_utf8(parts(&output).1).unwrap();
        assert!(html.contains("value=\"&quot;&gt;&lt;script&gt;&amp;&#39;\""));
        assert!(html.contains("That page number is not valid."));
        assert!(!html.contains("<script>"));
        assert!(!html.contains(&format!("value=\"{page}\"")));
    }
}

#[test]
fn csp_is_fixed_for_html_assets_errors_and_head_despite_request_fields() {
    assert_eq!(
        ruvane::CONTENT_SECURITY_POLICY,
        "default-src 'none'; script-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
    );
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    for (path, expected) in [
        ("/", 200),
        ("/assets/site.css", 200),
        ("/assets/mark.png", 200),
        ("/missing", 404),
        ("/articles?search=a&%73earch=b", 400),
    ] {
        for method in ["GET", "HEAD"] {
            let output = server.request(method, path);
            assert_eq!(status(&output), expected);
            if method == "HEAD" {
                assert!(parts(&output).1.is_empty());
            }
        }
    }
    let input = format!(
        "GET /?csp=unsafe-inline HTTP/1.1\r\nHost: {}\r\nContent-Security-Policy: default-src *\r\n\r\n",
        server.address
    );
    assert_eq!(status(&server.raw(input.as_bytes())), 200);
}

#[test]
fn configured_blog_metadata_is_specific_and_pagination_does_not_collapse() {
    let origin = ruvane::SiteOrigin::parse("https://JOURNAL.EXAMPLE:443/").unwrap();
    let server = Running::start(
        blog::router_with_origin(origin).unwrap(),
        ServerConfig::default(),
    );
    for (path, canonical, description) in [
        ("/", "/", "Notes on Rust, thoughtful interfaces"),
        ("/articles", "/articles", "Notes on Rust, typed interfaces"),
        (
            "/articles?page=1",
            "/articles",
            "Notes on Rust, typed interfaces",
        ),
        (
            "/articles?page=2",
            "/articles?page=2",
            "Notes on Rust, typed interfaces",
        ),
        (
            "/articles?page=3",
            "/articles?page=3",
            "Notes on Rust, typed interfaces",
        ),
        ("/articles/about", "/articles/about", "A small publication"),
        (
            "/articles/summer-%F0%9F%A6%80",
            "/articles/summer-%F0%9F%A6%80",
            "UTF-8 slugs travel",
        ),
        (
            "/articles/%74yped-urls?tracking=ignored",
            "/articles/typed-urls",
            "Path segments and query values",
        ),
    ] {
        let output = server.request("GET", path);
        assert_eq!(status(&output), 200);
        let html = std::str::from_utf8(parts(&output).1).unwrap();
        assert!(
            html.contains(&format!(
                "<link rel=\"canonical\" href=\"https://journal.example{canonical}\">"
            )),
            "{path}"
        );
        assert!(
            html.contains(&format!(
                "<meta name=\"description\" content=\"{description}"
            )),
            "{path}"
        );
        assert!(html.contains("<meta name=\"robots\" content=\"index, follow\">"));
        assert!(html.contains("<link rel=\"icon\" type=\"image/png\" href=\"/assets/mark.png\">"));
        assert_eq!(html.matches("name=\"viewport\"").count(), 1);
        assert_eq!(html.matches("rel=\"canonical\"").count(), 1);
    }
}

#[test]
fn blog_search_and_errors_have_no_canonical_or_reflected_metadata() {
    let origin = ruvane::SiteOrigin::parse("https://journal.example").unwrap();
    let server = Running::start(
        blog::router_with_origin(origin).unwrap(),
        ServerConfig::default(),
    );
    for (target, expected) in [
        ("/articles?search=%22%3E%3Cscript%3E&page=1", 200),
        ("/articles?page=0&search=private-search", 400),
        ("/articles/not-found", 404),
        ("/articles?page=99", 404),
    ] {
        let output = server.request("GET", target);
        assert_eq!(status(&output), expected);
        let html = std::str::from_utf8(parts(&output).1).unwrap();
        let head = html.split_once("</head>").unwrap().0;
        assert!(head.contains("name=\"robots\" content=\"noindex, follow\""));
        assert!(!head.contains("rel=\"canonical\""));
        assert!(!head.contains("private-search"));
        assert!(!head.contains("<script>"));
        assert!(!head.contains("&lt;script&gt;"));
    }
}

#[test]
fn unconfigured_local_blog_does_not_invent_a_public_origin() {
    let server = Running::start(blog::router().unwrap(), ServerConfig::default());
    for target in ["/", "/articles?page=2", "/articles/typed-urls"] {
        let output = server.request("GET", target);
        let html = std::str::from_utf8(parts(&output).1).unwrap();
        assert!(html.contains("name=\"robots\" content=\"noindex, follow\""));
        assert!(!html.contains("rel=\"canonical\""));
    }
}

#[test]
fn configured_canonicals_ignore_forwarding_and_host_aliases() {
    let origin = ruvane::SiteOrigin::parse("https://journal.example").unwrap();
    let server = Running::start(
        blog::router_with_origin(origin).unwrap(),
        ServerConfig::default(),
    );
    for authority in [
        server.address.to_string(),
        format!("localhost:{}", server.address.port()),
    ] {
        let input = format!(
            "GET /articles/typed-urls HTTP/1.1\r\nHost: {authority}\r\nForwarded: host=evil.test;proto=http\r\nX-Forwarded-Host: evil.test\r\nX-Forwarded-Proto: javascript\r\n\r\n"
        );
        let output = server.raw(input.as_bytes());
        assert_eq!(status(&output), 200);
        let html = std::str::from_utf8(parts(&output).1).unwrap();
        assert!(html.contains("href=\"https://journal.example/articles/typed-urls\""));
        assert!(!html.contains("evil.test"));
        assert!(!html.contains("javascript"));
    }
    let invalid = server.raw(b"GET / HTTP/1.1\r\nHost: evil.test\r\n\r\n");
    assert_eq!(status(&invalid), 400);
    assert!(
        !parts(&invalid)
            .1
            .windows(b"evil.test".len())
            .any(|part| part == b"evil.test")
    );
}

#[test]
fn static_preview_serves_only_declared_bytes_with_head_csp_and_query_validation() {
    use ruvane::{MediaType, StaticPath, StaticSite};
    let mut site = StaticSite::new();
    site.page(
        StaticPath::new("/hello/").unwrap(),
        Document::new("Static", "λ🦀").render(),
    )
    .unwrap();
    site.asset(
        StaticPath::new("/data.bin").unwrap(),
        [0, 255, 13, 10],
        MediaType::OctetStream,
    )
    .unwrap();
    let router = site.preview_router().unwrap();
    site.page(
        StaticPath::new("/later/").unwrap(),
        Document::new("Later", "").render(),
    )
    .unwrap();
    drop(site);
    let server = Running::start(router, ServerConfig::default());
    let get = server.request("GET", "/hello/");
    assert_eq!(status(&get), 200);
    assert!(std::str::from_utf8(parts(&get).1).unwrap().contains("λ🦀"));
    assert_eq!(
        parts(&server.request("GET", "/hello/?page=2")).1,
        parts(&get).1
    );
    let head = server.request("HEAD", "/hello/");
    assert_eq!(parts(&head).0, parts(&get).0);
    assert!(parts(&head).1.is_empty());
    assert_eq!(
        parts(&server.request("GET", "/data.bin")).1,
        [0, 255, 13, 10]
    );
    for path in [
        "/hello",
        "/hello/index.html",
        "/%68ello/",
        "/%2e%2e/",
        "/later/",
        "/_ruvane/manifest.tsv",
        "/_ruvane/headers.txt",
        "/_ruvane/complete",
    ] {
        assert_eq!(status(&server.request("GET", path)), 404, "{path}");
    }
    assert_eq!(status(&server.request("GET", "/hello/?a=1&%61=2")), 400);
    assert_eq!(status(&server.request("GET", "/hello/?q=%FF")), 400);
    assert_eq!(status(&server.request("GET", "/../outside")), 400);
    assert_eq!(status(&server.request("POST", "/hello/")), 405);
}

#[test]
fn exported_blog_preview_has_complete_internal_links_assets_and_canonicals() {
    use std::collections::BTreeSet;
    let site =
        blog::static_site(ruvane::SiteOrigin::parse("https://journal.example").unwrap()).unwrap();
    assert_eq!(site.len(), 14);
    let manifest = site.manifest();
    let rows: Vec<_> = manifest
        .lines()
        .skip(2)
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect();
    let declared: BTreeSet<_> = rows.iter().map(|row| row[1]).collect();
    let server = Running::start(site.preview_router().unwrap(), ServerConfig::default());
    let mut links = BTreeSet::new();
    for row in rows {
        let output = server.request("GET", row[1]);
        assert_eq!(status(&output), 200, "{}", row[1]);
        let (headers, body) = parts(&output);
        assert!(headers.contains(&format!("Content-Type: {}", row[3])));
        assert_eq!(body.len(), row[4].parse::<usize>().unwrap());
        let head = server.request("HEAD", row[1]);
        assert_eq!(headers, parts(&head).0);
        assert!(parts(&head).1.is_empty());
        if row[0] == "page" {
            let html = std::str::from_utf8(body).unwrap();
            assert!(html.contains(&format!(
                "rel=\"canonical\" href=\"https://journal.example{}\"",
                row[1]
            )));
            assert!(!html.contains("<form"));
            assert!(!html.contains("<script>"));
            for attribute in [" href=\"", " src=\""] {
                for tail in html.split(attribute).skip(1) {
                    let value = tail.split('"').next().unwrap();
                    if value.starts_with('/') {
                        links.insert(value.to_owned());
                    }
                }
            }
        }
    }
    for link in links {
        assert!(
            declared.contains(link.as_str()),
            "unexported internal link: {link}"
        );
        assert!(!link.contains('?'));
    }
    let page2 = server.request("GET", "/articles/page/2/");
    let html = std::str::from_utf8(parts(&page2).1).unwrap();
    assert!(html.contains("Summer in Rust"));
    assert!(html.contains("href=\"/articles/summer-rust/\""));
    assert!(html.contains("href=\"/articles/\">Previous page"));
    assert!(html.contains("href=\"/articles/page/3/\">Next page"));
    assert_eq!(
        parts(&server.request("GET", "/assets/mark.png")).1,
        include_bytes!("../examples/assets/mark.png")
    );
}

#[test]
fn static_shared_preview_responses_obey_the_server_output_budget() {
    let mut site = ruvane::StaticSite::new();
    site.page(
        ruvane::StaticPath::new("/").unwrap(),
        Document::new("Large", "x".repeat(256)).render(),
    )
    .unwrap();
    let server = Running::start(
        site.preview_router().unwrap(),
        ServerConfig {
            max_response_body_bytes: 64,
            ..ServerConfig::default()
        },
    );
    let get = server.request("GET", "/");
    let head = server.request("HEAD", "/");
    assert_eq!(status(&get), 500);
    assert_eq!(parts(&get).1, b"Internal Server Error\n");
    assert_eq!(parts(&head).0, parts(&get).0);
    assert!(parts(&head).1.is_empty());
}

#[test]
fn existing_export_preview_uses_disk_snapshot_and_never_serves_extra_files() {
    use ruvane::{StaticPath, StaticSite};
    use std::fs;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let base = fs::canonicalize(std::env::temp_dir()).unwrap();
    let root = loop {
        let candidate = base.join(format!(
            "ruvane-loaded-preview-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("test directory: {error}"),
        }
    };
    let directory = root.join("site");
    let mut site = StaticSite::new();
    site.page(
        StaticPath::new("/").unwrap(),
        Document::new("Snapshot", "old").render(),
    )
    .unwrap();
    site.export_to(&directory).unwrap();
    let html = fs::read_to_string(directory.join("index.html"))
        .unwrap()
        .replace(">old<", ">new<");
    fs::write(directory.join("index.html"), html).unwrap();
    fs::write(directory.join("extra.txt"), b"private extra").unwrap();
    let router = StaticSite::preview_export(&directory).unwrap();
    fs::remove_dir_all(root).unwrap();
    let server = Running::start(router, ServerConfig::default());
    let get = server.request("GET", "/");
    assert_eq!(status(&get), 200);
    assert!(
        std::str::from_utf8(parts(&get).1)
            .unwrap()
            .contains(">new<")
    );
    let head = server.request("HEAD", "/");
    assert_eq!(parts(&head).0, parts(&get).0);
    assert!(parts(&head).1.is_empty());
    for path in ["/extra.txt", "/_ruvane/manifest.tsv", "/index.html"] {
        assert_eq!(status(&server.request("GET", path)), 404);
    }
}

#[test]
fn blog_common_application_pages_match_export_and_keep_legacy_server_routes() {
    let origin = ruvane::SiteOrigin::parse("https://journal.example").unwrap();
    let app = blog::application().unwrap();
    assert_eq!(
        app.server_only_routes().collect::<Vec<_>>(),
        vec!["search", "legacy-article", "legacy-about"]
    );
    let server = Running::start(
        app.router(Some(origin.clone())).unwrap(),
        ServerConfig::default(),
    );
    let site = app.static_site(&origin).unwrap();
    let preview = Running::start(site.preview_router().unwrap(), ServerConfig::default());
    for row in site.manifest().lines().skip(2) {
        let path = row.split('\t').nth(1).unwrap();
        let live = server.request("GET", path);
        let static_page = preview.request("GET", path);
        assert_eq!(parts(&live), parts(&static_page), "{path}");
        if row.starts_with("page\t") {
            let body = std::str::from_utf8(parts(&live).1).unwrap();
            assert_eq!(body.matches("<!doctype html>").count(), 1);
            if path.starts_with("/articles/") {
                assert!(body.contains("class=\"journal-layout\""));
            }
        }
    }
    assert_eq!(status(&server.request("GET", "/articles?search=Rust")), 200);
    assert_eq!(
        status(&server.request("GET", "/articles/summer-%F0%9F%A6%80")),
        200
    );
    assert_eq!(status(&preview.request("GET", "/articles")), 404);
}

fn post_wire(server: &Running, target: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "POST {target} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
        server.address
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    server.raw(&bytes)
}

fn post_headers(server: &Running, length: usize) -> String {
    format!(
        "Origin: http://{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {length}\r\n",
        server.address
    )
}

fn post(server: &Running, target: &str, body: &[u8]) -> Vec<u8> {
    post_wire(server, target, &post_headers(server, body.len()), body)
}

fn counted_posts() -> (Router, Arc<AtomicUsize>) {
    let mut router = Router::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let current = Arc::clone(&calls);
    router
        .post_form("/form/{id}", move |request| {
            assert_eq!(request.method(), Method::Post);
            current.fetch_add(1, Ordering::SeqCst);
            let form = request.form_data().unwrap();
            Response::text(format!(
                "{}|{}|{}|{}",
                request.param("id").unwrap(),
                request.query_params().get("q").unwrap_or(""),
                form.get("q").unwrap_or(""),
                form.get("notes").unwrap_or("")
            ))
        })
        .unwrap();
    (router, calls)
}

#[test]
fn post_routes_are_explicit_method_scoped_and_keep_query_and_body_separate() {
    let (mut router, calls) = counted_posts();
    router
        .get("/form/{id}", |request| {
            assert!(request.form_data().is_none());
            Response::text("read only")
        })
        .unwrap();
    assert_eq!(
        router.post_form("/form/{other}", |_| Response::text("bad")),
        Err(RouteError::AmbiguousRoute)
    );
    assert_eq!(
        router.post_form("/form/{id}", |_| Response::text("bad")),
        Err(RouteError::DuplicatePath)
    );
    router
        .post_form("/form/new", |_| Response::text("fixed action"))
        .unwrap();
    router
        .get("/only-get", |_| Response::text("read only"))
        .unwrap();
    router
        .asset("/form/new", b"asset", ruvane::MediaType::PlainText)
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let response = post(
        &server,
        "/form/42?q=URL+plus",
        b"q=Rust+%2B+HTML&notes=one%0D%0Atwo",
    );
    assert_eq!(status(&response), 200);
    assert_eq!(parts(&response).1, b"42|URL+plus|Rust + HTML|one\ntwo");
    assert_eq!(parts(&server.request("GET", "/form/42")).1, b"read only");
    assert_eq!(parts(&server.request("GET", "/form/new")).1, b"asset");
    assert_eq!(parts(&post(&server, "/form/new", b"")).1, b"fixed action");
    assert_eq!(status(&post(&server, "/only-get", b"")), 405);
    assert_eq!(status(&post(&server, "/missing", b"")), 404);
    assert_eq!(status(&post(&server, "/form/%2F", b"q=1")), 400);
    assert_eq!(status(&post(&server, "/form/1?q=%FF", b"q=1")), 400);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let denied = server.request("PUT", "/form/42");
    assert!(
        parts(&denied)
            .0
            .lines()
            .any(|line| line == "Allow: GET, HEAD, POST")
    );
}

#[test]
fn post_requires_same_origin_and_refuses_missing_null_and_forged_sources() {
    let (router, calls) = counted_posts();
    let server = Running::start(router, ServerConfig::default());
    let prefix = "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\n";
    for source in [
        "".into(),
        "Origin: null\r\n".into(),
        "Origin: http://evil.example\r\n".into(),
        format!("Origin: http://{}/\r\n", server.address),
        format!("Origin: http://localhost:{}\r\n", server.address.port()),
        format!("Origin: http://{} http://evil.example\r\n", server.address),
        format!("Origin: https://{}\r\n", server.address),
        format!("Referer: http://{}/tasks\r\n", server.address),
        format!(
            "Origin: http://evil.example\r\nX-Forwarded-Host: {}\r\nForwarded: host={}\r\n",
            server.address, server.address
        ),
    ] {
        assert_eq!(
            status(&post_wire(
                &server,
                "/form/1",
                &(prefix.to_owned() + &source),
                b""
            )),
            403,
            "{source}"
        );
    }
    for site in [
        "cross-site",
        "same-site",
        "none",
        "",
        "Same-Origin",
        "same-origin, cross-site",
    ] {
        let headers = format!("{}Sec-Fetch-Site: {site}\r\n", post_headers(&server, 0));
        assert_eq!(
            status(&post_wire(&server, "/form/1", &headers, b"")),
            403,
            "{site}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(status(&post(&server, "/form/1", b"")), 200);
    assert_eq!(
        status(&post_wire(
            &server,
            "/form/1",
            &(post_headers(&server, 0) + "Sec-Fetch-Site: same-origin\r\n"),
            b""
        )),
        200
    );
    let raw = format!(
        "POST /form/1 HTTP/1.1\r\nHost: evil.example:{}\r\nOrigin: http://evil.example:{}\r\nContent-Length: 0\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\n",
        server.address.port(),
        server.address.port()
    );
    assert_eq!(status(&server.raw(raw.as_bytes())), 400);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let head = server.request("HEAD", "/form/1");
    assert_eq!(status(&head), 405);
    assert!(parts(&head).0.lines().any(|line| line == "Allow: POST"));
    assert!(parts(&head).1.is_empty());
}

#[test]
fn post_media_and_length_checks_run_before_body_reads_or_handlers() {
    let (router, calls) = counted_posts();
    let server = Running::start(router, ServerConfig::default());
    let origin = format!("Origin: http://{}\r\n", server.address);
    for content_type in [
        "",
        "Content-Type: text/plain\r\n",
        "Content-Type: application/json\r\n",
        "Content-Type: multipart/form-data; boundary=x\r\n",
        "Content-Type: application/x-www-form-urlencoded; charset=latin1\r\n",
        "Content-Type: application/x-www-form-urlencoded; charset=UTF-8; charset=UTF-8\r\n",
        "Content-Type: application/x-www-form-urlencoded;\r\n",
    ] {
        assert_eq!(
            status(&post_wire(
                &server,
                "/form/1",
                &format!("{origin}{content_type}Content-Length: 0\r\n"),
                b""
            )),
            415
        );
    }
    assert_eq!(
        status(&post_wire(
            &server,
            "/form/1",
            &(post_headers(&server, 0) + "Content-Encoding: gzip\r\n"),
            b""
        )),
        415
    );
    let form = format!("{origin}Content-Type: application/x-www-form-urlencoded\r\n");
    assert_eq!(status(&post_wire(&server, "/form/1", &form, b"")), 411);
    for (headers, expected) in [
        ("Content-Length: 65537\r\n", 413),
        ("Content-Length: 18446744073709551615\r\n", 413),
        ("Content-Length: 18446744073709551616\r\n", 400),
        ("Content-Length: +0\r\n", 400),
        ("Content-Length: 0, 0\r\n", 400),
        ("Content-Length: 0\r\ncontent-length: 0\r\n", 400),
        ("Transfer-Encoding: chunked\r\n", 501),
        ("Transfer-Encoding: chunked\r\nContent-Length: 0\r\n", 400),
        ("Content-Length: 0\r\nExpect: 100-continue\r\n", 417),
    ] {
        assert_eq!(
            status(&post_wire(
                &server,
                "/form/1",
                &(form.clone() + headers),
                b""
            )),
            expected,
            "{headers}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for content_type in [
        "application/x-www-form-urlencoded",
        "Application/X-WWW-Form-Urlencoded; Charset=UTF-8",
        "application/x-www-form-urlencoded;charset=\"utf-8\"",
    ] {
        assert_eq!(
            status(&post_wire(
                &server,
                "/form/1",
                &format!("{origin}Content-Type: {content_type}\r\nContent-Length: 0\r\n"),
                b""
            )),
            200
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn malformed_post_data_and_truncated_bodies_never_reach_an_action() {
    let (router, calls) = counted_posts();
    let server = Running::start(router, ServerConfig::default());
    for body in [
        b"q=%FF".as_slice(),
        b"q=1&%71=2",
        b"q=%00",
        b"q[]=1",
        b"q=1&",
        b"bare",
        b"q=\xff",
    ] {
        assert_eq!(status(&post(&server, "/form/1", body)), 400, "{body:?}");
    }
    assert_eq!(
        status(&post(
            &server,
            "/form/1",
            format!("q={}", "x".repeat(1025)).as_bytes()
        )),
        413
    );
    assert_eq!(
        status(&post_wire(
            &server,
            "/form/1",
            &post_headers(&server, 8),
            b"q=1"
        )),
        400
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let bytes = format!(
        "POST /form/1 HTTP/1.1\r\nHost: {}\r\n{}\r\nq=%F0%9F%A6%80",
        server.address,
        post_headers(&server, 14)
    )
    .into_bytes();
    let mut stream = server.connect();
    for chunk in bytes.chunks(3) {
        stream.write_all(chunk).unwrap();
    }
    stream.shutdown(Shutdown::Write).unwrap();
    assert_eq!(status(&read_closed(&mut stream)), 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn post_body_budget_and_deadline_include_headers_and_slow_input() {
    let (router, calls) = counted_posts();
    let server = Running::start(
        router,
        ServerConfig {
            max_request_body_bytes: 4,
            ..ServerConfig::default()
        },
    );
    assert_eq!(status(&post(&server, "/form/1", b"q=12")), 200);
    assert_eq!(status(&post(&server, "/form/1", b"q=123")), 413);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (router, calls) = counted_posts();
    let server = Running::start(
        router,
        ServerConfig {
            request_timeout: Duration::from_millis(220),
            ..ServerConfig::default()
        },
    );
    let mut stream = server.connect();
    stream
        .write_all(format!("POST /form/1 HTTP/1.1\r\nHost: {}\r\n", server.address).as_bytes())
        .unwrap();
    thread::sleep(Duration::from_millis(130));
    stream
        .write_all(format!("{}\r\nq", post_headers(&server, 8)).as_bytes())
        .unwrap();
    let before = Instant::now();
    let output = read_closed(&mut stream);
    assert_eq!(status(&output), 408);
    assert!(before.elapsed() < Duration::from_millis(210));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        Server::bind_with_config(
            "127.0.0.1:0".parse().unwrap(),
            Router::new(),
            ServerConfig {
                max_request_body_bytes: 65537,
                ..ServerConfig::default()
            }
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn post_shutdown_interrupts_an_incomplete_body_without_dispatch() {
    let (router, calls) = counted_posts();
    let mut server = Running::start(
        router,
        ServerConfig {
            workers: 1,
            request_timeout: Duration::from_secs(60),
            ..ServerConfig::default()
        },
    );
    let mut stream = server.connect();
    stream
        .write_all(
            format!(
                "POST /form/1 HTTP/1.1\r\nHost: {}\r\n{}\r\nq",
                server.address,
                post_headers(&server, 8)
            )
            .as_bytes(),
        )
        .unwrap();
    thread::sleep(Duration::from_millis(30));
    let before = Instant::now();
    server.shutdown.shutdown();
    server.thread.take().unwrap().join().unwrap().unwrap();
    assert!(before.elapsed() < Duration::from_secs(2));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn internal_redirects_emit_a_single_safe_location_and_head_omits_the_body() {
    let mut router = Router::new();
    router
        .get("/redirect", |_| {
            Response::see_other(
                ruvane::InternalUrl::new("/tasks")
                    .unwrap()
                    .query("q", "<script>\" & Rust")
                    .unwrap(),
            )
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let response = server.request("GET", "/redirect");
    let (headers, body) = parts(&response);
    assert_eq!(status(&response), 303);
    assert!(body.is_empty());
    assert_eq!(content_length(headers), 0);
    assert!(
        headers
            .lines()
            .any(|line| line == "Location: /tasks?q=%3Cscript%3E%22%20%26%20Rust")
    );
    assert_eq!(headers.matches("Location:").count(), 1);
    assert_eq!(parts(&server.request("HEAD", "/redirect")).0, headers);
    let redirect = Response::see_other(ruvane::InternalUrl::new("/tasks").unwrap());
    assert_eq!(redirect.location().unwrap().as_str(), "/tasks");
    assert!(
        redirect
            .with_status(StatusCode::BadRequest)
            .location()
            .is_none()
    );
    for url in [
        "//evil.example",
        "https://evil.example",
        "/%0D%0ALocation:evil",
    ] {
        assert!(ruvane::InternalUrl::new(url).is_err());
    }
}

#[test]
fn server_task_forms_validate_escape_and_keep_state_across_reads() {
    let server = Running::start(server_tasks::router().unwrap(), ServerConfig::default());
    let initial = server.request("GET", "/tasks");
    assert_eq!(status(&initial), 200);
    let html = std::str::from_utf8(parts(&initial).1).unwrap();
    assert!(html.contains("method=\"post\"") && !html.contains("<script"));
    let invalid = post(&server, "/tasks", b"title=&notes=Kept+notes");
    assert_eq!(status(&invalid), 400);
    assert!(
        std::str::from_utf8(parts(&invalid).1)
            .unwrap()
            .contains("Kept notes</textarea>")
    );
    assert!(
        std::str::from_utf8(parts(&invalid).1)
            .unwrap()
            .contains("Enter a task name.")
    );
    for body in [b"title=one%0Atwo".as_slice(), b"title=valid&extra=bad"] {
        assert_eq!(status(&post(&server, "/tasks", body)), 400);
    }
    let response = post(
        &server,
        "/tasks",
        b"title=%3Cscript%3Ex%3C%2Fscript%3E&notes=one%0D%0Atwo",
    );
    assert_eq!(status(&response), 303);
    assert!(
        parts(&response)
            .0
            .lines()
            .any(|line| line == "Location: /tasks")
    );
    for _ in 0..3 {
        let response = server.request("GET", "/tasks");
        let html = std::str::from_utf8(parts(&response).1).unwrap();
        assert!(html.contains("Tasks (1)"));
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("one\ntwo"));
    }
    assert_eq!(status(&post(&server, "/tasks/1/status", b"done=on")), 303);
    let checked = server.request("GET", "/tasks");
    assert!(
        std::str::from_utf8(parts(&checked).1)
            .unwrap()
            .contains("value=\"on\" checked>")
    );
    assert_eq!(status(&post(&server, "/tasks/1/status", b"")), 303);
    let unchecked = server.request("GET", "/tasks");
    assert!(
        !std::str::from_utf8(parts(&unchecked).1)
            .unwrap()
            .contains(" checked>")
    );
    assert_eq!(
        status(&post(&server, "/tasks/1/status", b"done=false")),
        400
    );
    assert_eq!(
        status(&post(&server, "/tasks/1/remove", b"unexpected=1")),
        400
    );
    assert_eq!(status(&post(&server, "/tasks/1/remove", b"")), 303);
    assert_eq!(status(&post(&server, "/tasks/1/remove", b"")), 404);
    let empty = server.request("GET", "/tasks");
    assert!(
        std::str::from_utf8(parts(&empty).1)
            .unwrap()
            .contains("No tasks yet.")
    );
}

#[test]
fn server_task_store_is_bounded_and_handles_concurrent_submissions() {
    let server = Arc::new(Running::start(
        server_tasks::router().unwrap(),
        ServerConfig::default(),
    ));
    let mut threads = Vec::new();
    for n in 0..32 {
        let server = Arc::clone(&server);
        threads.push(thread::spawn(move || {
            post(&server, "/tasks", format!("title=Task+{n}").as_bytes())
        }));
    }
    for thread in threads {
        assert_eq!(status(&thread.join().unwrap()), 303);
    }
    let full = server.request("GET", "/tasks");
    assert!(
        std::str::from_utf8(parts(&full).1)
            .unwrap()
            .contains("Tasks (32)")
    );
    let overflow = post(&server, "/tasks", b"title=Overflow");
    assert_eq!(status(&overflow), 400);
    assert!(
        std::str::from_utf8(parts(&overflow).1)
            .unwrap()
            .contains("Task limit reached.")
    );
    for id in 1..=32 {
        assert_eq!(
            status(&post(&server, &format!("/tasks/{id}/remove"), b"")),
            303
        );
    }
    let empty = server.request("GET", "/tasks");
    assert!(
        std::str::from_utf8(parts(&empty).1)
            .unwrap()
            .contains("Tasks (0)")
    );
}

#[test]
fn typed_actions_share_origin_validation_and_the_native_task_store() {
    use ruvane::{ACTION_MEDIA_TYPE, ActionReply};
    let server = Running::start(server_tasks::router().unwrap(), ServerConfig::default());
    let invalid = post(&server, "/actions/tasks/create", b"title=&notes=kept");
    assert_eq!(status(&invalid), 400);
    assert!(
        parts(&invalid)
            .0
            .lines()
            .any(|line| line == format!("Content-Type: {ACTION_MEDIA_TYPE}"))
    );
    let reply = ActionReply::decode(parts(&invalid).1).unwrap();
    assert!(reply.is_validation());
    assert_eq!(reply.data().text("field"), Some("title"));
    let forbidden = post_wire(
        &server,
        "/actions/tasks/create",
        "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 8\r\n",
        b"title=no",
    );
    assert_eq!(status(&forbidden), 403);
    let created = post(
        &server,
        "/actions/tasks/create",
        b"title=From+Wasm&notes=%3Cscript%3E%0D%0A%F0%9F%A6%80",
    );
    assert_eq!(status(&created), 200);
    let reply = ActionReply::decode(parts(&created).1).unwrap();
    assert!(!reply.is_validation());
    assert_eq!(reply.data().number("count"), Some(1));
    assert_eq!(reply.data().text("notes_0"), Some("<script>\n🦀"));
    let page = server.request("GET", "/tasks");
    let html = std::str::from_utf8(parts(&page).1).unwrap();
    assert!(html.contains("From Wasm") && html.contains("&lt;script&gt;"));
    let native = post(&server, "/tasks/1/status", b"done=on");
    assert_eq!(status(&native), 303);
    let list = post(&server, "/actions/tasks/list", b"");
    assert_eq!(
        ActionReply::decode(parts(&list).1)
            .unwrap()
            .data()
            .boolean("done_0"),
        Some(true)
    );
    let removed = post(&server, "/actions/tasks/remove", b"id=1");
    assert_eq!(
        ActionReply::decode(parts(&removed).1)
            .unwrap()
            .data()
            .number("count"),
        Some(0)
    );
    let error = post(&server, "/actions/tasks/remove", b"id=1");
    assert_eq!(status(&error), 400);
    assert!(
        ActionReply::decode(parts(&error).1)
            .unwrap()
            .is_validation()
    );
}
