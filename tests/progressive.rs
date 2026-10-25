use ruvane::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct Model {
    error: bool,
    value: String,
}
impl Component for Model {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if self.error {
            return if data.is_empty() {
                Ok(())
            } else {
                Err(ComponentError::InvalidProps)
            };
        }
        self.value = data
            .text("value")
            .ok_or(ComponentError::InvalidProps)?
            .into();
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        view.leaf(0, BrowserTag::Heading, &self.value)?;
        Ok(view)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
}
fn factory(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    if !(1..=5).contains(&props.component()) {
        return Err(ComponentError::InvalidProps);
    }
    Ok(Box::new(Model {
        error: props.component() >= 3,
        value: "Boundary".into(),
    }))
}
fn data(label: &str) -> ActionData {
    let mut data = ActionData::default();
    data.insert("value", ActionValue::Text(label.into()))
        .unwrap();
    data
}
fn zero(_: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(0))
}
fn app(loads: Arc<AtomicUsize>, gate: Arc<AtomicBool>) -> App {
    let mut app = App::new();
    app.components(factory, 3).unwrap();
    let count = Arc::clone(&loads);
    app.client_page(
        PageRoute::new("project", "/tasks/project").unwrap(),
        2,
        zero,
        move |ctx| {
            count.fetch_add(1, Ordering::SeqCst);
            let query = ctx.request().unwrap().query_params();
            if query.get("pause") == Some("page") {
                while !gate.load(Ordering::Acquire) {
                    ctx.stream_context()
                        .unwrap()
                        .wait(Duration::from_millis(2))
                        .map_err(|_| PageError::InternalServerError)?;
                }
            }
            match query.get("fail") {
                Some("page") => return Err(PageError::InternalServerError),
                Some("panic") => panic!("private stream diagnostic"),
                _ => {}
            }
            Ok(ClientPageData::new(
                "Report",
                if query.get("fail") == Some("schema") {
                    ActionData::default()
                } else {
                    data("<script>🦀")
                },
            )
            .no_store())
        },
    )
    .unwrap();
    let count = Arc::clone(&loads);
    app.client_layout(
        PageRoute::new("root", "/tasks").unwrap(),
        None,
        1,
        zero,
        move |ctx| {
            count.fetch_add(1, Ordering::SeqCst);
            if ctx.request().unwrap().query_params().get("fail") == Some("root") {
                return Err(PageError::InternalServerError);
            }
            Ok(ClientLayoutData::new(data("Root")))
        },
    )
    .unwrap();
    app.client_layout(
        PageRoute::new("context", "/tasks/project").unwrap(),
        Some("root"),
        1,
        zero,
        move |ctx| {
            loads.fetch_add(1, Ordering::SeqCst);
            if ctx.request().unwrap().query_params().get("fail") == Some("layout") {
                return Err(PageError::InternalServerError);
            }
            Ok(ClientLayoutData::new(data("Context")))
        },
    )
    .unwrap();
    app.client_page_layout("project", Some("context")).unwrap();
    let boundary = ClientBoundary::new(5, 4).unwrap();
    app.client_layout_boundary("root", Some(boundary)).unwrap();
    app.client_page_boundary("project", Some(boundary)).unwrap();
    app.client_page_streaming("project", true).unwrap();
    app.client_runtime(ClientRuntime::new(b"\0asm\x01\0\0\0", 42).unwrap())
        .unwrap();
    app
}
struct Running {
    address: SocketAddr,
    stop: ShutdownHandle,
    thread: Option<JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn new(app: &App, config: ServerConfig) -> Self {
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
    fn connect(
        &self,
        target: &str,
        method: &str,
        accept: &str,
        revision: Option<&str>,
        token: Option<&str>,
    ) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nAccept: {accept}\r\n",
            self.address
        )
        .unwrap();
        if let Some(value) = revision {
            write!(stream, "Ruvane-Application-Revision: {value}\r\n").unwrap();
        }
        if let Some(value) = token {
            write!(stream, "Ruvane-Stream-Token: {value}\r\n").unwrap();
        }
        write!(stream, "\r\n").unwrap();
        BufReader::new(stream)
    }
    fn get(&self, target: &str) -> (String, Vec<u8>, bool) {
        let mut reader = self.connect(
            target,
            "GET",
            NAVIGATION_STREAM_MEDIA_TYPE,
            Some("42"),
            Some("7"),
        );
        let headers = headers(&mut reader);
        let (body, complete) = chunks(&mut reader);
        (headers, body, complete)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn headers(reader: &mut impl BufRead) -> String {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        headers.push_str(&line);
        if line == "\r\n" {
            return headers;
        }
    }
}
fn chunk(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap() == 0 {
        return None;
    }
    let len = usize::from_str_radix(line.trim(), 16).unwrap();
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    let mut crlf = [0; 2];
    reader.read_exact(&mut crlf).unwrap();
    assert_eq!(&crlf, b"\r\n");
    Some(body)
}
fn chunks(reader: &mut impl BufRead) -> (Vec<u8>, bool) {
    let mut body = Vec::new();
    loop {
        match chunk(reader) {
            None => return (body, false),
            Some(bytes) if bytes.is_empty() => return (body, true),
            Some(bytes) => body.extend(bytes),
        }
    }
}
fn packets(body: &[u8], complete: bool) -> Vec<NavigationPage> {
    assert!(body.starts_with(b"RVS1"));
    assert_eq!(u64::from_le_bytes(body[4..12].try_into().unwrap()), 42);
    assert_eq!(u64::from_le_bytes(body[12..20].try_into().unwrap()), 7);
    let mut rest = &body[20..];
    let mut packets = Vec::new();
    while !rest.is_empty() {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        rest = &rest[4..];
        if len == 0 {
            assert!(rest.is_empty() && complete);
            return packets;
        }
        packets.push(NavigationPage::decode(&rest[..len]).unwrap());
        rest = &rest[len..];
    }
    assert!(!complete);
    packets
}
#[test]
fn typed_layouts_arrive_before_the_leaf_loader_finishes() {
    let loads = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(false));
    let app = app(Arc::clone(&loads), Arc::clone(&gate));
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect(
        "/tasks/project?pause=page",
        "GET",
        NAVIGATION_STREAM_MEDIA_TYPE,
        Some("42"),
        Some("7"),
    );
    let head = headers(&mut reader);
    assert!(
        head.starts_with("HTTP/1.1 200")
            && head.contains("Transfer-Encoding: chunked")
            && !head.contains("Content-Length:")
    );
    assert!(head.contains("Cache-Control: no-store"));
    let mut initial = chunk(&mut reader).unwrap();
    initial.extend(chunk(&mut reader).unwrap());
    initial.extend(chunk(&mut reader).unwrap());
    let end = Instant::now() + Duration::from_secs(2);
    while loads.load(Ordering::SeqCst) < 3 {
        assert!(Instant::now() < end);
        thread::yield_now();
    }
    let prefix = packets(&initial, false);
    assert_eq!(
        prefix.iter().map(|p| p.layouts().len()).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(
        prefix
            .iter()
            .all(|p| p.pending_layouts() == Some(2) && app.validate_navigation(p))
    );
    gate.store(true, Ordering::Release);
    let (tail, complete) = chunks(&mut reader);
    initial.extend(tail);
    assert!(complete);
    let all = packets(&initial, true);
    let final_page = all.last().unwrap();
    assert_eq!(final_page.pending_layouts(), None);
    assert_eq!(final_page.data().text("value"), Some("<script>🦀"));
    assert!(app.validate_navigation(final_page));
}
#[test]
fn head_identity_checks_and_opt_in_never_run_deferred_loaders() {
    let loads = Arc::new(AtomicUsize::new(0));
    let mut app = app(Arc::clone(&loads), Arc::new(AtomicBool::new(true)));
    let server = Running::new(&app, ServerConfig::default());
    for (method, revision, token, status) in [
        ("HEAD", Some("42"), Some("7"), 200),
        ("GET", None, Some("7"), 409),
        ("GET", Some("1"), Some("7"), 409),
        ("GET", Some("42"), None, 400),
        ("GET", Some("42"), Some("01"), 400),
        ("GET", Some("42"), Some("18446744073709551616"), 400),
    ] {
        let mut reader = server.connect(
            "/tasks/project",
            method,
            NAVIGATION_STREAM_MEDIA_TYPE,
            revision,
            token,
        );
        let head = headers(&mut reader);
        assert!(head.starts_with(&format!("HTTP/1.1 {status}")));
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        if method == "HEAD" {
            assert!(body.is_empty());
        }
    }
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    drop(server);
    app.client_page_streaming("project", false).unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect(
        "/tasks/project",
        "GET",
        NAVIGATION_STREAM_MEDIA_TYPE,
        None,
        None,
    );
    let head = headers(&mut reader);
    assert!(head.contains("Content-Length:") && head.contains(NAVIGATION_MEDIA_TYPE));
    assert!(!head.contains("Transfer-Encoding:"));
}
#[test]
fn pending_packets_are_bounded_and_cannot_forge_a_layout_or_mount_a_leaf() {
    let app = app(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(true)),
    );
    let page = NavigationPage::new(
        InternalUrl::new("/tasks/project").unwrap(),
        "Loading",
        ComponentProps::new(2, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap();
    let pending = page.clone().with_pending_layouts(2, vec![]).unwrap();
    assert!(app.validate_navigation(&pending));
    let bytes = pending.encode();
    assert!(bytes.starts_with(b"RVP4"));
    for n in 0..bytes.len() {
        assert!(NavigationPage::decode(&bytes[..n]).is_err());
    }
    let mut bad = bytes.clone();
    bad.push(0);
    assert!(NavigationPage::decode(&bad).is_err());
    assert!(!app.validate_navigation(&page.clone().with_pending_layouts(1, vec![]).unwrap()));
    assert!(page.clone().with_pending_layouts(7, vec![]).is_err());
    let forged = NavigationLayout::new(
        "wrong",
        InternalUrl::new("/tasks").unwrap(),
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        data("Root"),
    )
    .unwrap();
    assert!(!app.validate_navigation(&page.with_pending_layouts(2, vec![forged]).unwrap()));
    assert_eq!(
        Response::navigation_with_cache(&pending, NavigationCachePolicy::public(30).unwrap())
            .status(),
        StatusCode::InternalServerError
    );
}
#[test]
fn scoped_errors_complete_but_an_uncaught_root_failure_stays_incomplete() {
    let app = app(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(true)),
    );
    let server = Running::new(&app, ServerConfig::default());
    for (failure, depth, failed) in [
        ("layout", 1, 1),
        ("page", 2, 2),
        ("panic", 2, 2),
        ("schema", 2, 2),
    ] {
        let (head, body, complete) = server.get(&format!("/tasks/project?fail={failure}"));
        assert!(head.starts_with("HTTP/1.1 200") && complete);
        let packets = packets(&body, true);
        let error = packets.last().unwrap();
        assert_eq!(error.failed_segment(), Some(failed));
        assert_eq!(error.layouts().len(), depth);
        assert_eq!(error.status(), StatusCode::InternalServerError);
        assert!(app.validate_navigation(error));
        assert!(!body.windows(7).any(|s| s == b"private"));
    }
    let (_, body, complete) = server.get("/tasks/project?fail=root");
    assert!(!complete);
    assert!(
        packets(&body, false)
            .iter()
            .all(|p| p.pending_layouts().is_some())
    );
}
#[test]
fn deadlines_and_shutdown_interrupt_cooperative_loading_without_completion() {
    for shutdown in [false, true] {
        let loads = Arc::new(AtomicUsize::new(0));
        let app = app(Arc::clone(&loads), Arc::new(AtomicBool::new(false)));
        let server = Running::new(
            &app,
            ServerConfig {
                write_timeout: Duration::from_millis(if shutdown { 5000 } else { 80 }),
                ..ServerConfig::default()
            },
        );
        let mut reader = server.connect(
            "/tasks/project?pause=page",
            "GET",
            NAVIGATION_STREAM_MEDIA_TYPE,
            Some("42"),
            Some("7"),
        );
        headers(&mut reader);
        let end = Instant::now() + Duration::from_secs(2);
        while loads.load(Ordering::SeqCst) < 3 {
            assert!(Instant::now() < end);
            thread::yield_now();
        }
        if shutdown {
            server.stop.shutdown();
        }
        let (body, complete) = chunks(&mut reader);
        assert!(!complete);
        assert!(
            packets(&body, false)
                .iter()
                .all(|p| p.pending_layouts().is_some())
        );
    }
}
#[test]
fn native_ssr_keeps_real_statuses_and_server_byte_caps_truncate_streams() {
    let app = app(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(true)),
    );
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect(
        "/tasks/project?native=yes&fail=page",
        "GET",
        "text/html",
        None,
        None,
    );
    let head = headers(&mut reader);
    assert!(head.starts_with("HTTP/1.1 500") && head.contains("script-src 'none'"));
    drop(server);
    let server = Running::new(
        &app,
        ServerConfig {
            max_response_body_bytes: 400,
            ..ServerConfig::default()
        },
    );
    let (_, body, complete) = server.get("/tasks/project");
    assert!(!complete && body.len() <= 400);
    assert!(
        packets(&body, false)
            .iter()
            .all(|p| p.pending_layouts().is_some())
    );
}

#[test]
fn a_page_without_layouts_streams_a_manifest_then_a_complete_leaf() {
    let mut app = app(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicBool::new(true)),
    );
    app.client_page_layout("project", None).unwrap();
    assert!(app.client_page_streaming("missing", true).is_err());
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/tasks/project");
    assert!(complete);
    let all = packets(&body, true);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].pending_layouts(), Some(0));
    assert_eq!(all[1].pending_layouts(), None);
    assert!(all.iter().all(|packet| app.validate_navigation(packet)));
    assert!(all[1].encode().starts_with(b"RVP1"));
}
