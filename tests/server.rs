use ruvane::{
    Document, Method, Response, RouteError, Router, Server, ServerConfig, ShutdownHandle,
    StatusCode, Tag, element,
};
#[path = "../examples/support/blog.rs"]
mod blog;
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
    // A reset is permitted when closing a socket containing unread pipelined
    // data. Preserve any response already received before that reset.
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
    (
        std::str::from_utf8(&output[..end]).unwrap(),
        &output[end + 4..],
    )
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
            Response::html(Document::new("Été", element(Tag::P).child("<script> & 🦀")).render())
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
            Response::text("Bonjour 🦀")
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
    // The first idle socket occupies the sole queue slot. A batch of additional
    // sockets must be closed promptly while the handler is still blocked.
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
        parts(&server.request("GET", "/users/%C3%A9t%C3%A9/posts/42")).1,
        "été:42".as_bytes()
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
    let maximum = "%C3%A9".repeat(ruvane::MAX_COMPONENT_BYTES / 2);
    let output = server.request("GET", &format!("/value/{maximum}"));
    assert_eq!(status(&output), 200);
    assert_eq!(parts(&output).1.len(), ruvane::MAX_COMPONENT_BYTES);
    assert_eq!(
        status(&server.request("GET", &format!("/value/{maximum}%C3%A9"))),
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
        "/articles/%C3%A9t%C3%A9-rust",
    ] {
        assert_eq!(status(&server.request("GET", path)), 200, "{path}");
    }
    let second = server.request("GET", "/articles?page=2");
    let second_html = std::str::from_utf8(parts(&second).1).unwrap();
    assert!(second_html.contains("href=\"/articles/%C3%A9t%C3%A9-rust\""));
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
