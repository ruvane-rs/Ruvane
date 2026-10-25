use ruvane::{
    Document, Html, HtmlStream, Response, Router, Server, ServerConfig, ShutdownHandle,
    StreamContext, StreamError, StreamLimits, Tag, element,
};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct Running {
    address: SocketAddr,
    stop: ShutdownHandle,
    thread: Option<JoinHandle<std::io::Result<()>>>,
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
    fn connect(&self, method: &str) -> TcpStream {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "{method} /stream HTTP/1.1\r\nHost: {}\r\n\r\n",
            self.address
        )
        .unwrap();
        stream
    }
    fn request(&self, method: &str) -> Vec<u8> {
        let mut stream = self.connect(method);
        stream.shutdown(Shutdown::Write).unwrap();
        read_closed(&mut stream)
    }
    fn healthy(&self) {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "GET /health HTTP/1.1\r\nHost: {}\r\n\r\n",
            self.address
        )
        .unwrap();
        assert!(
            String::from_utf8(read_closed(&mut stream))
                .unwrap()
                .ends_with("healthy")
        );
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap().unwrap();
        }
    }
}
fn router(response: Response) -> Router {
    let mut router = Router::new();
    router.get("/stream", move |_| response.clone()).unwrap();
    router
        .get("/health", |_| Response::text("healthy"))
        .unwrap();
    router
}
fn document() -> Document {
    Document::new("<Streaming> 🦀", element(Tag::H1).child("Initial shell"))
}
fn read_closed(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        Err(error) => panic!("read: {error}"),
    }
    bytes
}
fn parts(bytes: &[u8]) -> (&str, &[u8]) {
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    (
        std::str::from_utf8(&bytes[..end]).unwrap(),
        &bytes[end + 4..],
    )
}
fn decode(bytes: &[u8]) -> (Vec<u8>, bool, usize) {
    let (headers, mut wire) = parts(bytes);
    assert!(headers.contains("Transfer-Encoding: chunked"));
    assert!(!headers.contains("Content-Length:"));
    let mut output = Vec::new();
    let mut chunks = 0;
    loop {
        let Some(end) = wire.windows(2).position(|part| part == b"\r\n") else {
            return (output, false, chunks);
        };
        let size = usize::from_str_radix(std::str::from_utf8(&wire[..end]).unwrap(), 16).unwrap();
        wire = &wire[end + 2..];
        if size == 0 {
            return (output, wire == b"\r\n", chunks);
        }
        if wire.len() < size + 2 {
            return (output, false, chunks);
        }
        assert_eq!(&wire[size..size + 2], b"\r\n");
        output.extend_from_slice(&wire[..size]);
        wire = &wire[size + 2..];
        chunks += 1;
    }
}
type ProducerFn = fn(&StreamContext) -> Result<Option<Html>, StreamError>;
fn end(_: &StreamContext) -> Result<Option<Html>, StreamError> {
    Ok(None)
}
#[test]
fn prefix_arrives_while_deferred_production_is_waiting_and_html_stays_escaped() {
    let release = Arc::new(AtomicBool::new(false));
    let factory = Arc::new(AtomicUsize::new(0));
    let gate = Arc::clone(&release);
    let calls = Arc::clone(&factory);
    let response = Response::stream_html(HtmlStream::new(document(), move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        let gate = Arc::clone(&gate);
        let mut sent = false;
        Ok(move |context: &StreamContext| {
            if sent {
                return Ok(None);
            }
            sent = true;
            while !gate.load(Ordering::Acquire) {
                context.wait(Duration::from_millis(2))?;
            }
            Ok(Some(
                element(Tag::P)
                    .child("Deferred <script> & 🦀\r\n0\r\n\r\nHTTP/1.1")
                    .render(),
            ))
        })
    }));
    assert!(response.is_streaming() && response.buffered_body().is_none());
    assert_eq!(factory.load(Ordering::SeqCst), 0);
    let server = Running::new(router(response), ServerConfig::default());
    let mut stream = server.connect("GET");
    let mut initial = Vec::new();
    while !initial.windows(13).any(|part| part == b"Initial shell") {
        let mut part = [0; 64];
        let count = stream.read(&mut part).unwrap();
        assert!(count > 0);
        initial.extend_from_slice(&part[..count]);
    }
    assert!(!initial.windows(8).any(|part| part == b"Deferred"));
    assert_eq!(factory.load(Ordering::SeqCst), 1);
    release.store(true, Ordering::Release);
    initial.extend(read_closed(&mut stream));
    let (body, complete, chunks) = decode(&initial);
    assert!(complete);
    assert_eq!(chunks, 3);
    let text = String::from_utf8(body).unwrap();
    assert!(text.starts_with("<!doctype html>") && text.ends_with("</body></html>"));
    assert!(
        text.contains("&lt;Streaming&gt;") && text.contains("Deferred &lt;script&gt; &amp; 🦀")
    );
    assert!(!text.contains("<script>"));
    let headers = parts(&initial).0;
    assert!(
        headers.contains("Cache-Control: no-store")
            && headers.contains("script-src 'none'")
            && headers.contains("Connection: close")
    );
}
#[test]
fn head_does_not_initialize_or_poll_and_cloned_plans_create_independent_producers() {
    let factories = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&factories);
    let response = Response::stream_html(HtmlStream::new(document(), move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        let mut sent = false;
        Ok(move |_: &StreamContext| {
            if sent {
                Ok(None)
            } else {
                sent = true;
                Ok(Some(element(Tag::P).child("One fragment").render()))
            }
        })
    }));
    let clone = response.clone();
    assert_eq!(response.body().as_ptr(), clone.body().as_ptr());
    let server = Running::new(router(clone), ServerConfig::default());
    let head = server.request("HEAD");
    let (headers, body) = parts(&head);
    assert!(body.is_empty());
    assert!(headers.contains("Transfer-Encoding: chunked") && !headers.contains("Content-Length:"));
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    let first = server.request("GET");
    let second = server.request("GET");
    assert_eq!(parts(&first).0, headers);
    assert_eq!(decode(&first).0, decode(&second).0);
    assert!(decode(&first).1 && decode(&second).1);
    assert_eq!(factories.load(Ordering::SeqCst), 2);
}
#[test]
fn invalid_prefix_or_tight_server_budget_fails_before_factory_or_headers_commit() {
    for (chunk_bytes, server_bytes) in [(1, 1024), (16384, 64)] {
        let factory = Arc::new(AtomicBool::new(false));
        let called = Arc::clone(&factory);
        let plan = HtmlStream::new(document(), move |_| {
            called.store(true, Ordering::SeqCst);
            Ok(end as fn(&StreamContext) -> Result<Option<Html>, StreamError>)
        })
        .limits(StreamLimits {
            chunk_bytes,
            ..StreamLimits::default()
        })
        .unwrap();
        let server = Running::new(
            router(Response::stream_html(plan)),
            ServerConfig {
                max_response_body_bytes: server_bytes,
                ..ServerConfig::default()
            },
        );
        for method in ["GET", "HEAD"] {
            let wire = server.request(method);
            let (headers, body) = parts(&wire);
            assert!(headers.starts_with("HTTP/1.1 500") && !headers.contains("Transfer-Encoding"));
            if method == "HEAD" {
                assert!(body.is_empty());
            }
        }
        assert!(!factory.load(Ordering::SeqCst));
        server.healthy();
    }
}
#[test]
fn factory_errors_and_panics_are_generic_500_and_workers_survive() {
    for panic in [false, true] {
        let response = Response::stream_html(HtmlStream::new(
            document(),
            move |_| -> Result<ProducerFn, StreamError> {
                if panic {
                    panic!("private factory diagnostic");
                }
                Err(StreamError::Producer)
            },
        ));
        let server = Running::new(
            router(response),
            ServerConfig {
                workers: 1,
                ..ServerConfig::default()
            },
        );
        let wire = server.request("GET");
        let (headers, body) = parts(&wire);
        assert!(headers.starts_with("HTTP/1.1 500") && headers.contains("Content-Length:"));
        assert_eq!(body, b"Internal Server Error\n");
        assert!(!wire.windows(7).any(|part| part == b"private"));
        server.healthy();
    }
}
#[test]
fn late_errors_and_panics_leave_incomplete_framing_without_a_second_status() {
    for panic in [false, true] {
        let response = Response::stream_html(HtmlStream::new(document(), move |_| {
            Ok(
                move |_: &StreamContext| -> Result<Option<Html>, StreamError> {
                    if panic {
                        panic!("private late diagnostic");
                    }
                    Err(StreamError::Producer)
                },
            )
        }));
        let server = Running::new(
            router(response),
            ServerConfig {
                workers: 1,
                ..ServerConfig::default()
            },
        );
        let wire = server.request("GET");
        let (body, complete, _) = decode(&wire);
        assert!(!complete);
        assert!(parts(&wire).0.starts_with("HTTP/1.1 200"));
        assert_eq!(
            wire.windows(9).filter(|part| *part == b"HTTP/1.1 ").count(),
            1
        );
        assert!(!String::from_utf8_lossy(&body).contains("private"));
        assert!(!body.ends_with(b"</body></html>"));
        server.healthy();
    }
}
#[test]
fn chunk_total_and_empty_yield_limits_stop_before_successful_completion() {
    let prefix = document().render().as_str().len();
    for (limits, value) in [
        (
            StreamLimits {
                chunk_bytes: 512,
                ..StreamLimits::default()
            },
            "x".repeat(600),
        ),
        (
            StreamLimits {
                body_bytes: prefix + 5,
                ..StreamLimits::default()
            },
            "x".repeat(100),
        ),
        (
            StreamLimits {
                chunks: 4,
                ..StreamLimits::default()
            },
            String::new(),
        ),
    ] {
        let count = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&count);
        let response = Response::stream_html(
            HtmlStream::new(document(), move |_| {
                let value = value.clone();
                let calls = Arc::clone(&calls);
                Ok(move |_: &StreamContext| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(ruvane::text(&value)))
                })
            })
            .limits(limits)
            .unwrap(),
        );
        let server = Running::new(router(response), ServerConfig::default());
        let wire = server.request("GET");
        let (_, complete, chunks) = decode(&wire);
        assert!(!complete);
        assert_eq!(chunks, 1);
        assert!(count.load(Ordering::SeqCst) <= 3);
        server.healthy();
    }
}
#[test]
fn an_empty_fragment_is_not_a_terminal_chunk() {
    let response = Response::stream_html(HtmlStream::new(document(), |_| {
        let mut step = 0;
        Ok(move |_: &StreamContext| {
            step += 1;
            match step {
                1 => Ok(Some(Html::default())),
                2 => Ok(Some(ruvane::text("after empty"))),
                _ => Ok(None),
            }
        })
    }));
    let server = Running::new(router(response), ServerConfig::default());
    let (body, complete, chunks) = decode(&server.request("GET"));
    assert!(complete);
    assert_eq!(chunks, 3);
    assert!(String::from_utf8(body).unwrap().contains("after empty"));
}
struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
#[test]
fn the_server_deadline_tightens_producer_waits_and_shutdown_is_cooperative() {
    for shutdown in [false, true] {
        let entered = Arc::new(AtomicBool::new(false));
        let started = Arc::clone(&entered);
        let dropped = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&dropped);
        let response = Response::stream_html(HtmlStream::new(document(), move |_| {
            let marker = Dropped(Arc::clone(&ended));
            let started = Arc::clone(&started);
            Ok(move |context: &StreamContext| {
                let _ = &marker;
                started.store(true, Ordering::Release);
                context.wait(Duration::from_secs(5))?;
                Ok(None)
            })
        }));
        let server = Running::new(
            router(response),
            ServerConfig {
                write_timeout: if shutdown {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(70)
                },
                ..ServerConfig::default()
            },
        );
        let mut stream = server.connect("GET");
        let end = Instant::now() + Duration::from_secs(2);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(2));
        }
        let start = Instant::now();
        if shutdown {
            server.stop.shutdown();
        }
        let wire = read_closed(&mut stream);
        assert!(!decode(&wire).1);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(dropped.load(Ordering::Acquire));
    }
}
#[test]
fn disconnects_release_production_and_slow_readers_do_not_create_an_unbounded_queue() {
    for disconnect in [false, true] {
        let dropped = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&dropped);
        let count = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&count);
        let plan = HtmlStream::new(document(), move |_| {
            let marker = Dropped(Arc::clone(&ended));
            let calls = Arc::clone(&calls);
            Ok(move |context: &StreamContext| {
                let _ = &marker;
                context.check()?;
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(ruvane::text("x".repeat(60000))))
            })
        })
        .limits(StreamLimits {
            chunk_bytes: 65536,
            body_bytes: 64 * 1024 * 1024,
            chunks: 1024,
            duration: Duration::from_secs(1),
        })
        .unwrap();
        let server = Running::new(
            router(Response::stream_html(plan)),
            ServerConfig {
                max_response_body_bytes: 64 * 1024 * 1024,
                write_timeout: Duration::from_millis(100),
                ..ServerConfig::default()
            },
        );
        let stream = server.connect("GET");
        if disconnect {
            stream.shutdown(Shutdown::Both).unwrap();
        }
        let end = Instant::now() + Duration::from_secs(2);
        while !dropped.load(Ordering::Acquire) {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(count.load(Ordering::SeqCst) < 1023);
        drop(stream);
        server.healthy();
    }
}
#[test]
fn producer_drop_panics_cannot_mark_completion_or_kill_the_worker() {
    struct PanicDrop;
    impl Drop for PanicDrop {
        fn drop(&mut self) {
            panic!("private drop diagnostic");
        }
    }
    let response = Response::stream_html(HtmlStream::new(document(), |_| {
        let marker = PanicDrop;
        Ok(move |_: &StreamContext| {
            let _ = &marker;
            Ok(None)
        })
    }));
    let server = Running::new(
        router(response),
        ServerConfig {
            workers: 1,
            ..ServerConfig::default()
        },
    );
    let wire = server.request("GET");
    let (body, complete, _) = decode(&wire);
    assert!(!complete);
    assert!(!body.ends_with(b"</body></html>"));
    server.healthy();
}
#[test]
fn streaming_limits_are_validated_and_post_cannot_start_a_stream() {
    for limits in [
        StreamLimits {
            chunk_bytes: 0,
            ..StreamLimits::default()
        },
        StreamLimits {
            chunk_bytes: 65537,
            ..StreamLimits::default()
        },
        StreamLimits {
            body_bytes: 0,
            ..StreamLimits::default()
        },
        StreamLimits {
            chunks: 1,
            ..StreamLimits::default()
        },
        StreamLimits {
            chunks: 1025,
            ..StreamLimits::default()
        },
        StreamLimits {
            duration: Duration::ZERO,
            ..StreamLimits::default()
        },
        StreamLimits {
            duration: Duration::from_secs(61),
            ..StreamLimits::default()
        },
    ] {
        assert!(limits.validate().is_err());
    }
    let factory = Arc::new(AtomicBool::new(false));
    let called = Arc::clone(&factory);
    let response = Response::stream_html(HtmlStream::new(document(), move |_| {
        called.store(true, Ordering::SeqCst);
        Ok(end as ProducerFn)
    }));
    let mut router = Router::new();
    router
        .post_form("/stream", move |_| response.clone())
        .unwrap();
    let server = Running::new(router, ServerConfig::default());
    let mut stream = TcpStream::connect(server.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream,"POST /stream HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\n\r\n",server.address,server.address).unwrap();
    let wire = read_closed(&mut stream);
    assert!(parts(&wire).0.starts_with("HTTP/1.1 500"));
    assert!(!factory.load(Ordering::SeqCst));
}
