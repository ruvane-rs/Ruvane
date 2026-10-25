use ruvane::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct Model(String);
impl Component for Model {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if self.0 == "Error" && data.is_empty() {
            return Ok(());
        }
        self.0 = data
            .text("value")
            .ok_or(ComponentError::InvalidProps)?
            .into();
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        view.leaf(0, BrowserTag::Heading, &self.0)?;
        Ok(view)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
}
fn factory(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    Ok(Box::new(Model(if props.component() == 3 {
        "Error".into()
    } else {
        String::new()
    })))
}
fn data(value: &str) -> ActionData {
    let mut data = ActionData::default();
    data.insert("value", ActionValue::Text(value.into()))
        .unwrap();
    data
}
fn state(route: &NavigationMatch) -> Result<InitialState, PageError> {
    if route.query().get("invalid") == Some("yes") {
        return Err(PageError::BadRequest);
    }
    Ok(InitialState::Number(0))
}
fn head(ctx: &PageContext<'_>) -> Result<ClientDocument, PageError> {
    let q = ctx.request().unwrap().query_params();
    match q.get("head") {
        Some("400") => return Err(PageError::BadRequest),
        Some("404") => return Err(PageError::NotFound),
        Some("panic") => panic!("private head diagnostic"),
        _ => {}
    }
    let mut limits = StreamLimits::default();
    match q.get("limit") {
        Some("prefix") => limits.chunk_bytes = 50,
        Some("bytes") => limits.body_bytes = 900,
        Some("chunks") => limits.chunks = 2,
        Some("split") => limits.chunk_bytes = 600,
        Some("deadline") => limits.duration = Duration::from_millis(60),
        _ => {}
    }
    Ok(ClientDocument::new("Report <&🦀")
        .hydrate_layouts(q.get("hydrate") == Some("yes"))
        .language("en\"<")
        .limits(limits)
        .unwrap())
}
fn app(loads: Arc<AtomicUsize>, gate: Arc<AtomicBool>) -> App {
    let mut app = App::new();
    app.components(factory, 3).unwrap();
    let count = loads.clone();
    app.client_page(
        PageRoute::new("page", "/work/page").unwrap(),
        2,
        state,
        move |ctx| {
            count.fetch_add(1, Ordering::SeqCst);
            let q = ctx.request().unwrap().query_params();
            if q.get("pause") == Some("yes") {
                while !gate.load(Ordering::Acquire) {
                    ctx.stream_context()
                        .unwrap()
                        .wait(Duration::from_millis(2))
                        .map_err(|_| PageError::InternalServerError)?;
                }
            }
            match q.get("fail") {
                Some("page") => return Err(PageError::NotFound),
                Some("panic") => panic!("private loader diagnostic"),
                _ => {}
            }
            let mut seed = if q.get("fail") == Some("schema") {
                ActionData::default()
            } else {
                data("Leaf <script>🦀")
            };
            if q.get("large") == Some("yes") {
                for index in 0..50 {
                    seed.insert(
                        &format!("field{index}"),
                        ActionValue::Text("x".repeat(1000)),
                    )
                    .unwrap();
                }
            }
            Ok(ClientPageData::new(
                if q.get("fail") == Some("title") {
                    "Other"
                } else {
                    "Report <&🦀"
                },
                seed,
            )
            .fallback(element(Tag::P).child("Native fallback")))
        },
    )
    .unwrap();
    for (name, parent, label) in [
        ("root", None, "Root <&"),
        ("inner", Some("root"), "Inner 🦀"),
    ] {
        let count = loads.clone();
        app.client_layout(
            PageRoute::new(name, "/work").unwrap(),
            parent,
            1,
            state,
            move |ctx| {
                count.fetch_add(1, Ordering::SeqCst);
                if ctx.request().unwrap().query_params().get("fail") == Some(name) {
                    return Err(PageError::InternalServerError);
                }
                Ok(ClientLayoutData::new(data(label)))
            },
        )
        .unwrap();
    }
    app.client_page_layout("page", Some("inner")).unwrap();
    app.client_page_document_streaming("page", Some(head))
        .unwrap();
    app.client_runtime(
        ClientRuntime::new(b"\0asm\x01\0\0\0", 42)
            .unwrap()
            .shell(|child| {
                element(Tag::Main)
                    .child("Shell")
                    .child(child)
                    .child("Footer")
                    .render()
            }),
    )
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
    fn connect(&self, method: &str, target: &str, accept: &str) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nAccept: {accept}\r\n\r\n",
            self.address
        )
        .unwrap();
        BufReader::new(stream)
    }
    fn get(&self, target: &str) -> (String, String, bool) {
        let mut reader = self.connect("GET", target, "text/html");
        let headers = headers(&mut reader);
        if !headers.contains("Transfer-Encoding: chunked") {
            let mut body = String::new();
            reader.read_to_string(&mut body).unwrap();
            return (headers, body, true);
        }
        let (body, complete) = chunks(&mut reader);
        (headers, String::from_utf8(body).unwrap(), complete)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn headers(reader: &mut impl BufRead) -> String {
    let mut out = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        out.push_str(&line);
        if line == "\r\n" {
            return out;
        }
    }
}
fn chunk(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap() == 0 {
        return None;
    }
    let len = usize::from_str_radix(line.trim(), 16).unwrap();
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).unwrap();
    let mut end = [0; 2];
    reader.read_exact(&mut end).unwrap();
    assert_eq!(&end, b"\r\n");
    Some(bytes)
}
fn chunks(reader: &mut impl BufRead) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    loop {
        match chunk(reader) {
            None => return (bytes, false),
            Some(b) if b.is_empty() => return (bytes, true),
            Some(b) => bytes.extend(b),
        }
    }
}
fn snapshot(body: &str) -> NavigationSnapshot {
    let hex = body
        .split("<div hidden id=\"ruvane-initial\">")
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).unwrap())
        .collect::<Vec<_>>();
    NavigationSnapshot::decode(&bytes).unwrap()
}
fn fixture() -> (App, Arc<AtomicUsize>, Arc<AtomicBool>) {
    let loads = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(false));
    (app(loads.clone(), gate.clone()), loads, gate)
}
#[test]
fn initial_shell_and_nested_layouts_arrive_before_the_blocked_leaf() {
    let (app, loads, gate) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("GET", "/work/page?pause=yes", "text/html");
    let h = headers(&mut reader);
    assert!(
        h.starts_with("HTTP/1.1 200")
            && h.contains("Transfer-Encoding: chunked")
            && !h.contains("Content-Length")
    );
    assert!(
        h.contains("Cache-Control: no-store")
            && h.contains("Vary: Accept")
            && h.contains(INTERACTIVE_CONTENT_SECURITY_POLICY)
    );
    let prefix = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    assert!(
        prefix.contains("<title>Report &lt;&amp;🦀</title>")
            && prefix.contains("<main>Shell")
            && !prefix.contains("ruvane-initial")
    );
    let root = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    let inner = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    assert!(root.contains("data-ruvane-layout=\"root\"") && root.contains("Root &lt;&amp;"));
    assert!(inner.contains("Inner 🦀"));
    let until = Instant::now() + Duration::from_secs(2);
    while loads.load(Ordering::SeqCst) != 3 {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    gate.store(true, Ordering::Release);
    let (rest, complete) = chunks(&mut reader);
    assert!(complete);
    let body = format!("{prefix}{root}{inner}{}", String::from_utf8(rest).unwrap());
    assert!(
        body.contains("Leaf &lt;script&gt;🦀")
            && body.contains("Native fallback")
            && body.ends_with("</body></html>")
    );
    assert_eq!(loads.load(Ordering::SeqCst), 3);
    let seed = snapshot(&body);
    assert_eq!(seed.revision(), 42);
    assert_eq!(seed.page().layouts().len(), 2);
    assert_eq!(seed.page().data().text("value"), Some("Leaf <script>🦀"));
    assert!(body.find("ruvane-initial").unwrap() < body.find("ruvane-complete").unwrap());
    assert!(body.find("ruvane-complete").unwrap() < body.find("application.mjs").unwrap());
}
#[test]
fn head_and_route_validation_run_without_deferred_loaders() {
    let (app, loads, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("HEAD", "/work/page", "text/html");
    let h = headers(&mut reader);
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    assert!(h.contains("Transfer-Encoding: chunked") && rest.is_empty());
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    for (query, status) in [
        ("head=400", 400),
        ("head=404", 404),
        ("head=panic", 500),
        ("invalid=yes", 400),
        ("limit=prefix", 500),
    ] {
        let (h, body, _) = server.get(&format!("/work/page?{query}"));
        assert!(h.starts_with(&format!("HTTP/1.1 {status}")));
        assert!(!h.contains("chunked"));
        assert!(!body.contains("private"));
        assert_eq!(loads.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn native_and_typed_navigation_keep_buffered_responses() {
    let (app, loads, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let (h, body, _) = server.get("/work/page?native=yes");
    assert!(!h.contains("chunked") && h.contains(CONTENT_SECURITY_POLICY));
    assert!(!body.contains("ruvane-initial") && !body.contains("application.mjs"));
    let mut reader = server.connect("GET", "/work/page", NAVIGATION_MEDIA_TYPE);
    let h = headers(&mut reader);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert!(!h.contains("chunked"));
    assert_eq!(NavigationPage::decode(&bytes).unwrap().layouts().len(), 2);
    assert_eq!(loads.load(Ordering::SeqCst), 6);
}
#[test]
fn late_failures_leave_an_incomplete_document_without_activation_or_diagnostics() {
    let (app, _, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    for fail in ["root", "inner", "page", "panic", "schema", "title"] {
        let (h, body, complete) = server.get(&format!("/work/page?fail={fail}"));
        assert!(h.starts_with("HTTP/1.1 200"));
        assert!(!complete);
        for forbidden in [
            "ruvane-initial",
            "ruvane-complete",
            "application.mjs",
            "private",
            "</body></html>",
        ] {
            assert!(!body.contains(forbidden), "{fail}: {forbidden}");
        }
    }
    assert!(server.get("/work/page").2);
}
#[test]
fn byte_yield_and_deadline_budgets_stop_before_completion() {
    let (app, _, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    for query in ["limit=bytes", "limit=chunks", "limit=deadline&pause=yes"] {
        let (h, body, complete) = server.get(&format!("/work/page?{query}"));
        assert!(h.contains("chunked"));
        assert!(
            !complete && !body.contains("ruvane-complete") && !body.contains("application.mjs")
        );
    }
    assert!(server.get("/work/page").2);
}
#[test]
fn shutdown_cancels_a_waiting_initial_loader() {
    let (app, loads, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("GET", "/work/page?pause=yes", "text/html");
    headers(&mut reader);
    let until = Instant::now() + Duration::from_secs(2);
    while loads.load(Ordering::SeqCst) < 3 {
        assert!(Instant::now() < until);
        thread::yield_now();
    }
    server.stop.shutdown();
    let (body, complete) = chunks(&mut reader);
    assert!(!complete && !String::from_utf8(body).unwrap().contains("ruvane-complete"));
}
#[test]
fn response_fragments_split_without_changing_the_snapshot() {
    let (app, _, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?limit=split");
    assert!(complete);
    assert_eq!(
        snapshot(&body).page().data().text("value"),
        Some("Leaf <script>🦀")
    );
}
#[test]
fn the_streamed_branch_matches_the_buffered_html() {
    let (mut app, _, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let (_, streamed, complete) = server.get("/work/page");
    assert!(complete);
    drop(server);
    app.client_page_document_streaming("page", None).unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let (_, buffered, _) = server.get("/work/page");
    let branch = |s: &str| {
        s.split("<main>")
            .nth(1)
            .unwrap()
            .split("</main>")
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(branch(&streamed), branch(&buffered));
    assert_eq!(snapshot(&streamed).encode(), snapshot(&buffered).encode());
}
#[test]
fn missing_or_duplicate_shell_slots_fail_before_loading() {
    for shell in [(|_: Html| text("Missing")) as fn(Html) -> Html, |slot| {
        fragment([slot.clone(), slot])
    }] {
        let loads = Arc::new(AtomicUsize::new(0));
        let mut invalid = App::new();
        invalid.components(factory, 3).unwrap();
        let count = loads.clone();
        invalid
            .client_page(
                PageRoute::new("page", "/work/page").unwrap(),
                2,
                state,
                move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(ClientPageData::new("Report <&🦀", data("Leaf")))
                },
            )
            .unwrap();
        invalid
            .client_page_document_streaming("page", Some(head))
            .unwrap();
        invalid
            .client_runtime(
                ClientRuntime::new(b"\0asm\x01\0\0\0", 42)
                    .unwrap()
                    .shell(shell),
            )
            .unwrap();
        let server = Running::new(&invalid, ServerConfig::default());
        let (h, _, _) = server.get("/work/page");
        assert!(h.starts_with("HTTP/1.1 500") && !h.contains("chunked"));
        assert_eq!(loads.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn large_hex_snapshots_are_split_into_bounded_chunks() {
    let (app, _, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("GET", "/work/page?large=yes", "text/html");
    headers(&mut reader);
    let mut body = Vec::new();
    let mut count = 0;
    loop {
        let chunk = chunk(&mut reader).expect("complete response");
        if chunk.is_empty() {
            break;
        }
        count += 1;
        assert!(chunk.len() <= StreamLimits::default().chunk_bytes);
        body.extend(chunk);
    }
    assert!(body.len() > 65536 && count > 6);
    let snapshot = snapshot(&String::from_utf8(body).unwrap());
    assert_eq!(snapshot.page().data().text("field49").unwrap().len(), 1000);
    assert!(snapshot.encode().len() <= MAX_NAVIGATION_SNAPSHOT_BYTES);
}
#[test]
fn server_byte_caps_withhold_the_completion_suffix_of_a_large_snapshot() {
    let (app, _, _) = fixture();
    let server = Running::new(
        &app,
        ServerConfig {
            max_response_body_bytes: 65536,
            ..ServerConfig::default()
        },
    );
    let (headers, body, complete) = server.get("/work/page?large=yes");
    assert!(headers.starts_with("HTTP/1.1 200") && !complete);
    assert!(
        body.len() <= 65536
            && !body.contains("ruvane-complete")
            && !body.contains("application.mjs")
    );
    assert!(server.get("/work/page").2);
}
#[test]
fn typed_metadata_is_present_and_escaped_in_the_initial_head() {
    let (mut app, _, _) = fixture();
    app.client_page_document_streaming(
        "page",
        Some(|ctx| {
            Ok(head(ctx)?.metadata(Metadata::new().description("Report <script> & 🦀").unwrap()))
        }),
    )
    .unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("GET", "/work/page", "text/html");
    headers(&mut reader);
    let prefix = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    assert!(
        prefix.contains("<meta name=\"description\" content=\"Report &lt;script&gt; &amp; 🦀\">")
            && !prefix.contains("<script>")
    );
    assert!(chunks(&mut reader).1);
}

fn layout_snapshot(body: &str) -> LayoutSnapshot {
    let hex = body
        .split("<div hidden data-ruvane-segment=\"\">")
        .nth(1)
        .unwrap()
        .split("<br")
        .next()
        .unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).unwrap())
        .collect::<Vec<_>>();
    LayoutSnapshot::decode(&bytes).unwrap()
}
#[test]
fn hydration_records_arrive_root_first_with_an_early_external_bootstrap() {
    let (app, loads, gate) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("GET", "/work/page?hydrate=yes&pause=yes", "text/html");
    let h = headers(&mut reader);
    assert!(
        h.contains(INTERACTIVE_CONTENT_SECURITY_POLICY) && h.contains("Cache-Control: no-store")
    );
    let prefix = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    assert!(
        prefix.contains("data-ruvane-document=\"segments\"")
            && prefix.contains("data-ruvane-layouts=\"2\"")
            && prefix.contains("<script type=\"module\" async src=")
    );
    let root = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    let inner = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    for (index, record) in [&root, &inner].iter().enumerate() {
        let record = layout_snapshot(record);
        assert_eq!(record.revision(), 42);
        assert_eq!(record.page().layouts().len(), index + 1);
        assert_eq!(record.page().pending_layouts(), Some(2));
        assert_eq!(
            record.page().url().as_str(),
            "/work/page?hydrate=yes&pause=yes"
        );
    }
    assert!(!inner.contains("ruvane-initial") && !inner.contains("ruvane-complete"));
    gate.store(true, Ordering::Release);
    let (rest, complete) = chunks(&mut reader);
    assert!(complete);
    let rest = String::from_utf8(rest).unwrap();
    assert!(!rest.contains("application.mjs"));
    assert!(rest.contains("ruvane-complete"));
    assert_eq!(snapshot(&rest).page().layouts().len(), 2);
    assert_eq!(loads.load(Ordering::SeqCst), 3);
}
#[test]
fn early_hydration_head_stays_lazy_and_native_rendering_stays_buffered() {
    let (app, loads, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("HEAD", "/work/page?hydrate=yes", "text/html");
    assert!(headers(&mut reader).contains("chunked"));
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert!(bytes.is_empty());
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let (h, body, _) = server.get("/work/page?hydrate=yes&native=yes");
    assert!(
        !h.contains("chunked")
            && !body.contains("data-ruvane-segment")
            && !body.contains("application.mjs")
    );
}
#[test]
fn early_hydration_supports_a_page_without_layouts() {
    let (mut app, _, _) = fixture();
    app.client_page_layout("page", None).unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes");
    assert!(
        complete
            && body.contains("data-ruvane-layouts=\"0\"")
            && !body.contains("data-ruvane-segment")
    );
    assert!(snapshot(&body).page().layouts().is_empty());
}
#[test]
fn known_early_hydration_load_errors_finish_with_abort_html() {
    let (app, _, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    for fail in ["root", "inner", "page", "panic", "schema", "title"] {
        let (h, body, complete) = server.get(&format!("/work/page?hydrate=yes&fail={fail}"));
        assert!(h.starts_with("HTTP/1.1 200") && complete);
        assert!(
            body.contains("async src=")
                && body.contains("data-ruvane-stream-error")
                && body.contains("ruvane-complete")
                && !body.contains("ruvane-initial")
                && !body.contains("private")
        );
    }
}
