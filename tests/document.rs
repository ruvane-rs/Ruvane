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
    if (4..=6).contains(&props.component()) {
        return Ok(Box::new(BadBoundary(props.component())));
    }
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
        .hydrate_shell(q.get("shell") == Some("yes"))
        .language("en\"<")
        .limits(limits)
        .unwrap())
}
fn app(loads: Arc<AtomicUsize>, gate: Arc<AtomicBool>) -> App {
    app_shell(loads, gate, |child| {
        element(Tag::Main)
            .child("Shell")
            .child(child)
            .child("Footer")
            .render()
    })
}
fn app_shell(loads: Arc<AtomicUsize>, gate: Arc<AtomicBool>, shell: fn(Html) -> Html) -> App {
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
                Some("bad") => return Err(PageError::BadRequest),
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
                let query = ctx.request().unwrap().query_params();
                if query.get("fail") == Some(name) {
                    return Err(PageError::InternalServerError);
                }
                if name == "inner" {
                    match query.get("fail") {
                        Some("inner-bad") => return Err(PageError::BadRequest),
                        Some("inner-missing") => return Err(PageError::NotFound),
                        Some("inner-panic") => panic!("private layout diagnostic"),
                        Some("inner-schema") => {
                            return Ok(ClientLayoutData::new(ActionData::default()));
                        }
                        _ => {}
                    }
                }
                let mut seed = data(label);
                if query.get("fail") == Some("record") {
                    for index in 0..38 {
                        seed.insert(
                            &format!("field{index}"),
                            ActionValue::Text("x".repeat(1000)),
                        )
                        .unwrap();
                    }
                }
                Ok(ClientLayoutData::new(seed))
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
            .shell(shell),
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

struct BadBoundary(u32);
impl Component for BadBoundary {
    fn page_data(&mut self, _: ActionData) -> Result<(), ComponentError> {
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        if self.0 == 5 {
            panic!("private boundary diagnostic");
        }
        let mut view = View::new();
        if self.0 == 4 {
            view.button(0, "Unsafe boundary event", 1, false)?;
        } else {
            view.leaf(0, BrowserTag::Heading, "Invalid boundary")?;
        }
        Ok(view)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Ok(())
    }
    fn take_action(&mut self) -> Option<ActionRequest> {
        (self.0 == 6).then(|| {
            ActionRequest::new(
                1,
                InternalUrl::new("/actions/private").unwrap(),
                FormData::default(),
            )
            .unwrap()
        })
    }
}
fn page_boundary(app: &mut App) {
    app.client_page_boundary("page", Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
}
#[test]
fn streamed_leaf_boundaries_keep_all_validated_parents_and_logical_statuses() {
    let (mut app, loads, _) = fixture();
    page_boundary(&mut app);
    let server = Running::new(&app, ServerConfig::default());
    for mode in ["", "&hydrate=yes"] {
        for (fail, status) in [
            ("page", 404),
            ("bad", 400),
            ("panic", 500),
            ("schema", 500),
            ("title", 500),
        ] {
            let before = loads.load(Ordering::SeqCst);
            let target = format!("/work/page?fail={fail}{mode}");
            let (h, body, complete) = server.get(&target);
            assert!(
                h.starts_with("HTTP/1.1 200") && h.contains("Cache-Control: no-store") && complete
            );
            let packet = snapshot(&body);
            let packet = packet.page();
            assert_eq!(packet.status().as_u16(), status);
            assert_eq!(packet.failed_segment(), Some(2));
            assert_eq!(packet.layouts().len(), 2);
            assert_eq!(packet.props().component(), 3);
            assert_eq!(packet.props().state(), InitialState::Number(status as u32));
            assert!(packet.data().is_empty());
            assert_eq!(packet.title(), "Report <&🦀");
            assert_eq!(packet.url().as_str(), target);
            assert_eq!(loads.load(Ordering::SeqCst) - before, 3);
            assert!(
                body.contains("data-ruvane-page")
                    && body.contains("ruvane-complete")
                    && !body.contains("data-ruvane-stream-error")
                    && !body.contains("private")
            );
        }
    }
}
#[test]
fn a_typed_error_finishes_the_same_document_after_the_blocked_leaf_fails() {
    let (mut app, loads, gate) = fixture();
    page_boundary(&mut app);
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect(
        "GET",
        "/work/page?hydrate=yes&pause=yes&fail=page",
        "text/html",
    );
    headers(&mut reader);
    let prefix = chunk(&mut reader).unwrap();
    let root = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    let inner = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    assert!(String::from_utf8(prefix).unwrap().contains("async src="));
    assert_eq!(layout_snapshot(&root).page().layouts().len(), 1);
    assert_eq!(layout_snapshot(&inner).page().layouts().len(), 2);
    gate.store(true, Ordering::Release);
    let (rest, complete) = chunks(&mut reader);
    assert!(complete);
    let rest = String::from_utf8(rest).unwrap();
    let final_page = snapshot(&rest);
    assert_eq!(final_page.page().status(), StatusCode::NotFound);
    assert_eq!(final_page.page().layouts()[1].name(), "inner");
    assert_eq!(loads.load(Ordering::SeqCst), 3);
}
#[test]
fn the_innermost_layout_child_boundary_can_recover_a_leaf_error() {
    let (mut app, _, _) = fixture();
    app.client_layout_boundary("inner", Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes&fail=page");
    assert!(complete);
    assert_eq!(snapshot(&body).page().failed_segment(), Some(2));
}
fn retired_snapshot(body: &str) -> LayoutSnapshot {
    let hex = body
        .split("<div hidden id=\"ruvane-retired\">")
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).unwrap())
        .collect::<Vec<_>>();
    LayoutSnapshot::decode(&bytes).unwrap()
}
#[test]
fn an_outer_boundary_closes_emitted_descendants_before_the_error_host() {
    let (mut app, loads, _) = fixture();
    app.client_layout_boundary("root", Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    let server = Running::new(&app, ServerConfig::default());
    for mode in ["", "&hydrate=yes"] {
        for (fail, status) in [
            ("page", 404),
            ("bad", 400),
            ("panic", 500),
            ("schema", 500),
            ("title", 500),
        ] {
            let before = loads.load(Ordering::SeqCst);
            let (_, body, complete) = server.get(&format!("/work/page?fail={fail}{mode}"));
            assert!(
                complete && !body.contains("data-ruvane-stream-error") && !body.contains("private")
            );
            let final_page = snapshot(&body);
            let prefix = retired_snapshot(&body);
            assert_eq!(final_page.page().failed_segment(), Some(2));
            assert_eq!(final_page.page().layouts().len(), 1);
            assert_eq!(final_page.page().status().as_u16(), status);
            assert_eq!(prefix.page().layouts().len(), 2);
            assert_eq!(prefix.page().pending_layouts(), Some(2));
            assert_eq!(prefix.page().layouts()[1].name(), "inner");
            assert_eq!(
                prefix.page().layouts()[0].name(),
                final_page.page().layouts()[0].name()
            );
            assert_eq!(prefix.page().title(), final_page.page().title());
            assert_eq!(prefix.page().url(), final_page.page().url());
            assert_eq!(loads.load(Ordering::SeqCst) - before, 3);
            assert!(body.contains(
                "data-ruvane-child-outlet=\"\"></div></section><div data-ruvane-page=\"\""
            ));
            assert!(body.find("ruvane-retired").unwrap() < body.find("ruvane-initial").unwrap());
        }
    }
}
#[test]
fn an_outer_boundary_can_retire_a_prefix_before_a_deeper_layout_fails() {
    let (mut app, loads, _) = fixture();
    app.client_layout(
        PageRoute::new("third", "/work").unwrap(),
        Some("inner"),
        1,
        state,
        |_| Err(PageError::NotFound),
    )
    .unwrap();
    app.client_page_layout("page", Some("third")).unwrap();
    app.client_layout_boundary("root", Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes");
    assert!(complete);
    let final_page = snapshot(&body);
    let prefix = retired_snapshot(&body);
    assert_eq!(final_page.page().failed_segment(), Some(2));
    assert_eq!(final_page.page().layouts().len(), 1);
    assert_eq!(prefix.page().layouts().len(), 2);
    assert_eq!(prefix.page().pending_layouts(), Some(3));
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    assert!(!body.contains("data-ruvane-layout=\"third\""));
}
#[test]
fn the_nearest_parent_boundary_keeps_its_own_emitted_ancestors() {
    let (mut app, _, _) = fixture();
    app.client_layout(
        PageRoute::new("third", "/work").unwrap(),
        Some("inner"),
        1,
        state,
        |_| Ok(ClientLayoutData::new(data("Third"))),
    )
    .unwrap();
    app.client_page_layout("page", Some("third")).unwrap();
    for name in ["root", "inner"] {
        app.client_layout_boundary(name, Some(ClientBoundary::new(3, 3).unwrap()))
            .unwrap();
    }
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes&fail=page");
    assert!(complete);
    assert_eq!(snapshot(&body).page().layouts().len(), 2);
    assert_eq!(snapshot(&body).page().failed_segment(), Some(3));
    assert_eq!(retired_snapshot(&body).page().layouts().len(), 3);
}
#[test]
fn layout_loader_errors_cannot_be_relabelled_as_leaf_errors() {
    let (mut app, _, _) = fixture();
    page_boundary(&mut app);
    let server = Running::new(&app, ServerConfig::default());
    for fail in ["root", "inner"] {
        let (_, body, complete) = server.get(&format!("/work/page?hydrate=yes&fail={fail}"));
        assert!(
            complete
                && body.contains("data-ruvane-stream-error")
                && !body.contains("ruvane-initial")
        );
    }
}
#[test]
fn a_parent_child_boundary_recovers_an_unemitted_layout() {
    let (mut app, loads, _) = fixture();
    app.client_layout_boundary("root", Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    page_boundary(&mut app);
    let server = Running::new(&app, ServerConfig::default());
    for mode in ["", "&hydrate=yes"] {
        for (fail, status) in [
            ("inner", 500),
            ("inner-bad", 400),
            ("inner-missing", 404),
            ("inner-panic", 500),
            ("inner-schema", 500),
            ("record", 500),
        ] {
            let before = loads.load(Ordering::SeqCst);
            let (headers, body, complete) = server.get(&format!("/work/page?fail={fail}{mode}"));
            assert!(
                complete
                    && headers.starts_with("HTTP/1.1 200")
                    && headers.contains("Cache-Control: no-store")
            );
            let snapshot = snapshot(&body);
            let page = snapshot.page();
            assert_eq!(page.failed_segment(), Some(1));
            assert_eq!(page.layouts().len(), 1);
            assert_eq!(page.layouts()[0].name(), "root");
            assert_eq!(page.status().as_u16(), status);
            assert_eq!(page.title(), "Report <&🦀");
            assert_eq!(loads.load(Ordering::SeqCst) - before, 2);
            assert!(
                body.contains("data-ruvane-page")
                    && body.contains("ruvane-complete")
                    && !body.contains("data-ruvane-layout=\"inner\"")
                    && !body.contains("data-ruvane-stream-error")
                    && !body.contains("private")
            );
        }
    }
}
#[test]
fn a_layout_cannot_catch_its_own_failure() {
    let (mut app, _, _) = fixture();
    for name in ["root", "inner"] {
        app.client_layout_boundary(name, Some(ClientBoundary::new(3, 3).unwrap()))
            .unwrap();
    }
    let server = Running::new(&app, ServerConfig::default());
    let (_, root, complete) = server.get("/work/page?hydrate=yes&fail=root");
    assert!(
        complete && root.contains("data-ruvane-stream-error") && !root.contains("ruvane-initial")
    );
    let (_, inner, complete) = server.get("/work/page?hydrate=yes&fail=inner");
    assert!(complete);
    assert_eq!(snapshot(&inner).page().layouts().len(), 1);
}
#[test]
fn invalid_error_components_fall_back_without_diagnostics_or_initial_actions() {
    for component in 4..=6 {
        let (mut app, _, _) = fixture();
        app.client_page_boundary("page", Some(ClientBoundary::new(3, component).unwrap()))
            .unwrap();
        app.client_layout_boundary("root", Some(ClientBoundary::new(3, component).unwrap()))
            .unwrap();
        let server = Running::new(&app, ServerConfig::default());
        for fail in ["page", "inner"] {
            let (_, body, complete) = server.get(&format!("/work/page?hydrate=yes&fail={fail}"));
            assert!(
                complete
                    && body.contains("data-ruvane-stream-error")
                    && !body.contains("ruvane-initial")
                    && !body.contains("private")
            );
        }
        assert!(server.get("/work/page").2);
    }
}
#[test]
fn a_page_boundary_recovers_without_any_layouts() {
    let (mut app, _, _) = fixture();
    page_boundary(&mut app);
    app.client_page_layout("page", None).unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes&fail=bad");
    assert!(complete);
    let packet = snapshot(&body);
    assert_eq!(packet.page().failed_segment(), Some(0));
    assert!(packet.page().layouts().is_empty());
    assert_eq!(packet.page().status(), StatusCode::BadRequest);
}
#[test]
fn leaf_boundaries_do_not_bypass_deadlines_or_transport_limits() {
    let (mut app, _, _) = fixture();
    page_boundary(&mut app);
    let server = Running::new(&app, ServerConfig::default());
    for query in [
        "limit=deadline&pause=yes&hydrate=yes",
        "limit=chunks&fail=page&hydrate=yes",
        "limit=bytes&fail=page",
    ] {
        let (_, body, complete) = server.get(&format!("/work/page?{query}"));
        assert!(!complete && !body.contains("ruvane-complete"));
    }
    assert!(server.get("/work/page").2);
}

#[test]
fn app_boundary_registration_is_explicit_and_removable() {
    let boundary = ClientBoundary::new(3, 3).unwrap();
    assert!(App::new().client_boundary(Some(boundary)).is_err());
    let (mut app, loads, _) = fixture();
    let packet = NavigationPage::new(
        InternalUrl::new("/work/page").unwrap(),
        "Error",
        ComponentProps::new(3, InitialState::Number(500)).unwrap(),
        ActionData::default(),
        StatusCode::InternalServerError,
    )
    .unwrap()
    .with_failure(0, Vec::new())
    .unwrap();
    assert!(!app.validate_navigation(&packet));
    app.client_boundary(Some(boundary)).unwrap();
    assert!(app.validate_navigation(&packet));
    app.client_boundary(None).unwrap();
    assert!(!app.validate_navigation(&packet));
    assert_eq!(loads.load(Ordering::SeqCst), 0);
}
#[test]
fn app_boundaries_recover_root_and_unhandled_branch_errors_in_both_document_profiles() {
    let (mut app, loads, _) = fixture();
    app.client_boundary(Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    let server = Running::new(&app, ServerConfig::default());
    for mode in ["", "&hydrate=yes"] {
        for (fail, failed, status) in [
            ("root", 0, 500),
            ("inner", 1, 500),
            ("inner-bad", 1, 400),
            ("inner-missing", 1, 404),
            ("inner-schema", 1, 500),
            ("inner-panic", 1, 500),
            ("page", 2, 404),
            ("panic", 2, 500),
            ("record", 1, 500),
        ] {
            let before = loads.load(Ordering::SeqCst);
            let (_, body, complete) = server.get(&format!("/work/page?fail={fail}{mode}"));
            assert!(
                complete && !body.contains("private") && !body.contains("data-ruvane-stream-error")
            );
            let final_page = snapshot(&body);
            let page = final_page.page();
            assert!(page.layouts().is_empty());
            assert_eq!(page.failed_segment(), Some(failed));
            assert_eq!(page.status().as_u16(), status);
            assert_eq!(page.title(), "Report <&🦀");
            assert!(body.contains("data-rvn-kind=\"app\""));
            assert_eq!(loads.load(Ordering::SeqCst) - before, failed + 1);
            if failed == 0 {
                assert!(
                    !body.contains("ruvane-retired")
                        && !body.contains("data-ruvane-layout=\"root\"")
                );
            } else {
                let prefix = retired_snapshot(&body);
                assert_eq!(prefix.page().layouts().len(), failed);
                assert_eq!(prefix.page().pending_layouts(), Some(2));
            }
        }
    }
    let (headers, body, complete) = server.get("/work/page?native=yes&fail=root");
    assert!(
        complete
            && headers.starts_with("HTTP/1.1 500")
            && headers.contains("script-src 'none'")
            && !body.contains("ruvane-initial")
    );
}
#[test]
fn local_boundaries_keep_precedence_over_the_app_boundary() {
    let (mut app, _, _) = fixture();
    app.client_boundary(Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    app.client_layout_boundary("root", Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    let forged = NavigationPage::new(
        InternalUrl::new("/work/page").unwrap(),
        "Error",
        ComponentProps::new(3, InitialState::Number(500)).unwrap(),
        ActionData::default(),
        StatusCode::InternalServerError,
    )
    .unwrap()
    .with_failure(2, Vec::new())
    .unwrap();
    assert!(!app.validate_navigation(&forged));
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes&fail=page");
    assert!(complete);
    assert_eq!(snapshot(&body).page().layouts().len(), 1);
    assert_eq!(snapshot(&body).page().failed_segment(), Some(2));
}
#[test]
fn invalid_app_error_components_use_the_minimal_fallback() {
    for component in 4..=6 {
        let (mut app, _, _) = fixture();
        app.client_boundary(Some(ClientBoundary::new(3, component).unwrap()))
            .unwrap();
        let server = Running::new(&app, ServerConfig::default());
        let (_, body, complete) = server.get("/work/page?hydrate=yes&fail=root");
        assert!(
            complete
                && body.contains("data-ruvane-stream-error")
                && !body.contains("ruvane-initial")
                && !body.contains("private")
        );
        let (_, body, complete) = server.get("/work/page?fail=root");
        assert!(!complete && !body.contains("ruvane-initial") && !body.contains("private"));
        let (headers, body, complete) = server.get("/work/page?native=yes&fail=root");
        assert!(
            complete
                && headers.starts_with("HTTP/1.1 500")
                && !body.contains("ruvane-initial")
                && !body.contains("private")
        );
    }
}
#[test]
fn app_boundaries_also_cover_pages_without_layouts() {
    let (mut app, _, _) = fixture();
    app.client_boundary(Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    app.client_page_layout("page", None).unwrap();
    let server = Running::new(&app, ServerConfig::default());
    let (_, body, complete) = server.get("/work/page?hydrate=yes&fail=bad");
    assert!(complete);
    let page = snapshot(&body);
    assert_eq!(page.page().failed_segment(), Some(0));
    assert!(page.page().layouts().is_empty());
    assert!(!body.contains("ruvane-retired"));
}
#[test]
fn app_boundaries_do_not_bypass_transport_limits_or_cancellation() {
    let (mut app, _, _) = fixture();
    app.client_boundary(Some(ClientBoundary::new(3, 3).unwrap()))
        .unwrap();
    let server = Running::new(&app, ServerConfig::default());
    for query in [
        "limit=deadline&pause=yes&hydrate=yes",
        "limit=chunks&fail=page&hydrate=yes",
    ] {
        let (_, body, complete) = server.get(&format!("/work/page?{query}"));
        assert!(!complete && !body.contains("ruvane-complete"));
    }
    assert!(server.get("/work/page").2);
}

fn shell_view(name: &str) -> Html {
    let mut view = View::new();
    view.leaf(0, BrowserTag::Heading, "Shell <&🦀").unwrap();
    view.shell_island(
        name,
        ComponentProps::new(3, InitialState::Number(0)).unwrap(),
    )
    .unwrap()
}
fn marked_shell(child: Html) -> Html {
    element(Tag::Main)
        .child(shell_view("header"))
        .child(child)
        .child(shell_view("footer"))
        .render()
}
#[test]
fn shell_islands_have_bounded_names_and_complete_view_markers() {
    let props = ComponentProps::new(1, InitialState::Number(7)).unwrap();
    let mut view = View::new();
    let label = view.element(0, BrowserTag::Label).unwrap();
    view.leaf(label, BrowserTag::Span, "Draft").unwrap();
    view.input(label, 1, "Draft <&🦀", 1, false).unwrap();
    for name in [
        "",
        "1header",
        "a-b",
        "header,footer",
        "<script>",
        "é",
        &"a".repeat(33),
    ] {
        assert_eq!(
            view.shell_island(name, props),
            Err(ViewError::InvalidIdentifier)
        );
    }
    for name in ["header", "_draft2", &"a".repeat(32)] {
        let html = view.shell_island(name, props).unwrap().into_string();
        assert!(html.starts_with(&format!(
            "<div data-ruvane-shell=\"{name}\"><div data-ruvane-version="
        )));
        assert!(html.ends_with(&format!(
            "</div></div><br hidden data-ruvane-shell-ready=\"{name}\">"
        )));
        assert!(html.contains("disabled") && html.contains("Draft &lt;&amp;🦀"));
    }
}
#[test]
fn shell_only_hydration_streams_complete_islands_before_the_leaf() {
    let loads = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(false));
    let app = app_shell(loads.clone(), gate.clone(), marked_shell);
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("GET", "/work/page?shell=yes&pause=yes", "text/html");
    assert!(headers(&mut reader).contains("chunked"));
    let prefix = String::from_utf8(chunk(&mut reader).unwrap()).unwrap();
    assert!(
        prefix.contains("data-ruvane-document=\"segments\"")
            && prefix.contains("data-ruvane-shells=\"header,footer\"")
    );
    assert!(
        prefix.contains("data-ruvane-layout-hydration=\"off\"")
            && prefix.contains("<script type=\"module\" async src=")
    );
    assert!(
        prefix.contains("data-ruvane-shell-ready=\"header\"")
            && !prefix.contains("data-ruvane-shell-ready=\"footer\"")
    );
    gate.store(true, Ordering::Release);
    let (body, complete) = chunks(&mut reader);
    assert!(complete);
    let body = String::from_utf8(body).unwrap();
    assert!(
        !body.contains("data-ruvane-segment")
            && body.contains("data-ruvane-shell-ready=\"footer\"")
    );
    assert_eq!(snapshot(&body).page().layouts().len(), 2);
    assert_eq!(loads.load(Ordering::SeqCst), 3);
}
#[test]
fn shell_and_layout_hydration_are_independent_opt_ins() {
    let loads = Arc::new(AtomicUsize::new(0));
    let app = app_shell(loads, Arc::new(AtomicBool::new(true)), marked_shell);
    let server = Running::new(&app, ServerConfig::default());
    for query in ["shell=yes", "shell=yes&hydrate=yes", "hydrate=yes", ""] {
        let (h, body, complete) = server.get(&format!("/work/page?{query}"));
        assert!(h.starts_with("HTTP/1.1 200") && complete);
        assert_eq!(
            body.contains("data-ruvane-shells="),
            query.contains("shell=yes")
        );
        assert_eq!(
            body.contains("data-ruvane-segment="),
            query.contains("hydrate=yes")
        );
        assert_eq!(
            body.contains("data-ruvane-layout-hydration=\"off\""),
            query == "shell=yes"
        );
    }
    let (h, body, _) = server.get("/work/page?shell=yes&native=yes");
    assert!(h.contains("script-src 'none'") && !body.contains("data-ruvane-shells="));
}
#[test]
fn duplicate_and_excess_shell_islands_fail_before_deferred_loading() {
    let duplicate: fn(Html) -> Html =
        |child| fragment([shell_view("same"), child, shell_view("same")]);
    let excess: fn(Html) -> Html = |child| {
        let mut parts = vec![child];
        for n in 0..9 {
            parts.push(shell_view(&format!("item{n}")));
        }
        fragment(parts)
    };
    for shell in [duplicate, excess] {
        let loads = Arc::new(AtomicUsize::new(0));
        let app = app_shell(loads.clone(), Arc::new(AtomicBool::new(true)), shell);
        let server = Running::new(&app, ServerConfig::default());
        let (h, _, _) = server.get("/work/page?shell=yes");
        assert!(h.starts_with("HTTP/1.1 500") && !h.contains("chunked"));
        assert_eq!(loads.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn empty_shell_opt_in_preserves_lazy_head_requests() {
    let (app, loads, _) = fixture();
    let server = Running::new(&app, ServerConfig::default());
    let mut reader = server.connect("HEAD", "/work/page?shell=yes", "text/html");
    assert!(headers(&mut reader).contains("chunked"));
    let mut body = Vec::new();
    reader.read_to_end(&mut body).unwrap();
    assert!(body.is_empty());
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let (_, body, complete) = server.get("/work/page?shell=yes");
    assert!(complete && body.contains("data-ruvane-shells=\"\""));
}
#[test]
fn early_shell_metadata_cannot_bypass_prefix_and_stream_budgets() {
    let loads = Arc::new(AtomicUsize::new(0));
    let app = app_shell(loads.clone(), Arc::new(AtomicBool::new(true)), marked_shell);
    let server = Running::new(&app, ServerConfig::default());
    let (h, _, _) = server.get("/work/page?shell=yes&limit=prefix");
    assert!(h.starts_with("HTTP/1.1 500") && !h.contains("chunked"));
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let (h, body, complete) = server.get("/work/page?shell=yes&limit=chunks");
    assert!(h.starts_with("HTTP/1.1 200") && !complete && !body.contains("ruvane-complete"));
}
