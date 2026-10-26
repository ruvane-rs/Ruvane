use ruvane::{
    ActionData, ActionValue, App, AppError, BrowserTag, ClientPageData, ClientRuntime, Component,
    ComponentError, ComponentProps, InitialState, InternalUrl, MediaType, NavigationMatch,
    NavigationPage, PageError, PageMode, PageRoute, Server, StatusCode, View,
};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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
        if data.len() != 1 {
            return Err(ComponentError::InvalidProps);
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
    match (props.component(), props.state()) {
        (1 | 2, InitialState::Number(_)) => Ok(Box::new(Model {
            error: false,
            value: String::new(),
        })),
        (4, InitialState::Number(0 | 408 | 400 | 404 | 500)) | (5, InitialState::Number(0)) => {
            Ok(Box::new(Model {
                error: true,
                value: "Local boundary".into(),
            }))
        }
        (3, InitialState::Number(400 | 404)) => Ok(Box::new(Model {
            error: true,
            value: "Error page".into(),
        })),
        _ => Err(ComponentError::InvalidProps),
    }
}
fn data(value: &str) -> ActionData {
    let mut data = ActionData::default();
    data.insert("value", ActionValue::Text(value.into()))
        .unwrap();
    data
}
fn zero(_: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(0))
}
fn id(route: &NavigationMatch) -> Result<InitialState, PageError> {
    let raw = route.param("id").unwrap();
    let id = raw.parse::<u32>().map_err(|_| PageError::BadRequest)?;
    if id == 0 || id.to_string() != raw {
        return Err(PageError::BadRequest);
    }
    Ok(InitialState::Number(id))
}
fn app(loads: Arc<AtomicUsize>) -> App {
    let mut app = App::new();
    app.components(factory, 3).unwrap();
    app.client_page(PageRoute::new("home", "/").unwrap(), 1, zero, move |_| {
        loads.fetch_add(1, Ordering::SeqCst);
        Ok(ClientPageData::new("<Home> 🦀", data("<script> & Rust")))
    })
    .unwrap();
    app.client_page(
        PageRoute::new("item", "/items/{id}").unwrap(),
        2,
        id,
        |ctx| {
            if ctx.param("id") == Some("99") {
                return Err(PageError::NotFound);
            }
            let query = ctx.request().unwrap().query_params().get("q").unwrap_or("");
            Ok(ClientPageData::new("Item", data(query)))
        },
    )
    .unwrap();
    app
}
fn runtime() -> ClientRuntime {
    ClientRuntime::new(b"\0asm\x01\0\0\0", 123).unwrap()
}
fn initial_snapshot(html: &str) -> ruvane::NavigationSnapshot {
    let hex = html
        .split("<div hidden id=\"ruvane-initial\">")
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap();
    assert!(
        hex.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    ruvane::NavigationSnapshot::decode(&bytes).unwrap()
}
#[test]
fn development_is_explicit_and_native_pages_keep_their_script_free_policy() {
    let path = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("ruvane-client-dev-{}", std::process::id()));
    std::fs::write(&path, "RVD1\n9\n1\nready\n123\n\n").unwrap();
    let mut candidate = app(Arc::new(AtomicUsize::new(0)));
    candidate
        .client_runtime(runtime().development(ruvane::Development::new(&path, 9).unwrap()))
        .unwrap();
    let server = Running::start(candidate);
    let (headers, body) = server.get("GET", "/", false);
    let html = String::from_utf8(body).unwrap();
    assert_eq!(html.matches("id=\"ruvane-development\"").count(), 1);
    assert!(html.contains("data-session=\"9\" data-revision=\"123\""));
    assert!(headers.contains(ruvane::INTERACTIVE_CONTENT_SECURITY_POLICY));
    assert_eq!(initial_snapshot(&html).revision(), 123);
    let (headers, body) = server.get("GET", "/?native=yes", false);
    assert!(headers.contains(ruvane::CONTENT_SECURITY_POLICY));
    assert!(!String::from_utf8(body).unwrap().contains("<script"));
    drop(server);
    std::fs::remove_file(path).unwrap();
    let production = Running::new(app(Arc::new(AtomicUsize::new(0))));
    assert!(
        !String::from_utf8(production.get("GET", "/", false).1)
            .unwrap()
            .contains("ruvane-development")
    );
    assert!(
        production
            .get("GET", "/_ruvane/dev/status", false)
            .0
            .starts_with("HTTP/1.1 404")
    );
}
#[test]
fn ssr_carries_the_exact_initial_seed_without_executable_markup_or_an_extra_loader_call() {
    let loads = Arc::new(AtomicUsize::new(0));
    let mut app = app(Arc::clone(&loads));
    app.client_page_cache("home", ruvane::NavigationCachePolicy::public(30).unwrap())
        .unwrap();
    let server = Running::new(app);
    let (headers, bytes) = server.get("GET", "/?q=%3Cscript%3E", false);
    let html = String::from_utf8(bytes).unwrap();
    let snapshot = initial_snapshot(&html);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot.revision(), 123);
    assert_eq!(snapshot.page().url().as_str(), "/?q=%3Cscript%3E");
    assert_eq!(
        snapshot.page().data().text("value"),
        Some("<script> & Rust")
    );
    assert!(headers.contains("Cache-Control: no-store") && headers.contains("script-src 'self'"));
    assert!(html.contains("&lt;script&gt; &amp; Rust"));
    assert!(html.contains("data-ruvane-page=\"\" tabindex=\"-1\""));
    assert_eq!(html.matches("id=\"ruvane-initial\"").count(), 1);
    assert!(!html.contains("<script>"));
    let (_, native) = server.get("GET", "/?native=yes", false);
    assert!(
        !String::from_utf8(native)
            .unwrap()
            .contains("id=\"ruvane-initial\"")
    );
    let (_, typed) = server.get("GET", "/", true);
    assert!(typed.starts_with(b"RVP1"));
}
#[test]
fn initial_snapshots_keep_ssr_layouts_and_scoped_errors() {
    let server = Running::new(boundary_app());
    for (url, status) in [("/items/1", 200), ("/items/99", 404)] {
        let (headers, bytes) = server.get("GET", url, false);
        assert!(headers.starts_with(&format!("HTTP/1.1 {status}")));
        let snapshot = initial_snapshot(&String::from_utf8(bytes).unwrap());
        assert_eq!(snapshot.page().status().as_u16(), status);
        assert!(!snapshot.page().layouts().is_empty());
    }
}
#[test]
fn declarations_are_shared_without_running_loaders_and_runtime_is_explicit() {
    let loads = Arc::new(AtomicUsize::new(0));
    let mut app = app(loads.clone());
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    assert_eq!(app.router(None).err(), Some(AppError::InvalidPage));
    let packet = |url, component, value| {
        NavigationPage::new(
            InternalUrl::from_target(url).unwrap(),
            "Page",
            ComponentProps::new(component, InitialState::Number(value)).unwrap(),
            data("x"),
            StatusCode::Ok,
        )
        .unwrap()
    };
    assert!(app.validate_navigation(&packet("/", 1, 0)));
    assert!(app.validate_navigation(&packet("/items/42?q=a+b", 2, 42)));
    assert!(!app.validate_navigation(&packet("/items/042", 2, 42)));
    assert!(!app.validate_navigation(&packet("/", 2, 0)));
    assert!(!app.validate_navigation(&packet("/unknown", 1, 0)));
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    assert_eq!(
        app.server_only_routes().collect::<Vec<_>>(),
        ["home", "item"]
    );
    assert!(
        app.client_page(
            PageRoute::new("home", "/other").unwrap(),
            2,
            zero,
            |_| unreachable!()
        )
        .is_err()
    );
    assert_eq!(app.url("home", &[]).unwrap().as_str(), "/");
    app.client_runtime(runtime()).unwrap();
    assert!(app.client_runtime(runtime()).is_err());
}
#[test]
fn client_assets_share_route_budgets_and_named_url_ownership() {
    let mut app = app(Arc::new(AtomicUsize::new(0)));
    app.client_runtime(runtime()).unwrap();
    assert!(
        app.asset(
            "/ruvane-runtime/000000000000007b/app.wasm",
            b"x",
            MediaType::WebAssembly
        )
        .is_err()
    );
    app.page(
        PageRoute::new("wild", "/ruvane-runtime/{rev}/{file}").unwrap(),
        None,
        PageMode::ServerOnly(ruvane::QueryMode::Uri),
        |_| Ok(ruvane::Page::new("Page", "body")),
    )
    .unwrap();
    assert!(
        app.url("wild", &[("rev", "000000000000007b"), ("file", "app.wasm")])
            .is_err()
    );
    assert!(ClientRuntime::new(b"not Wasm", 0).is_err());
    assert!(ClientRuntime::new(&[0; 1], 0).is_err());
    assert_eq!(
        ruvane::application_revision("18446744073709551615"),
        u64::MAX
    );
}
struct Running {
    address: std::net::SocketAddr,
    stop: ruvane::ShutdownHandle,
    thread: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn new(mut app: App) -> Self {
        app.client_runtime(runtime()).unwrap();
        Self::start(app)
    }
    fn start(app: App) -> Self {
        let server =
            Server::bind("127.0.0.1:0".parse().unwrap(), app.router(None).unwrap()).unwrap();
        let address = server.local_addr().unwrap();
        let stop = server.shutdown_handle();
        Self {
            address,
            stop,
            thread: Some(std::thread::spawn(move || server.run())),
        }
    }
    fn get(&self, method: &str, target: &str, typed: bool) -> (String, Vec<u8>) {
        self.get_revision(method, target, typed, None)
    }
    fn get_revision(
        &self,
        method: &str,
        target: &str,
        typed: bool,
        revision: Option<&str>,
    ) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: {}\r\n{}{}\r\n",
            self.address,
            if typed {
                "Accept: application/vnd.ruvane.page\r\n"
            } else {
                ""
            },
            revision
                .map(|value| format!("Ruvane-Application-Revision: {value}\r\n"))
                .unwrap_or_default()
        )
        .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8(bytes[..end].to_vec()).unwrap(),
            bytes[end + 4..].to_vec(),
        )
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
#[test]
fn app_serves_ssr_and_rvp_from_the_same_loader_with_native_error_statuses() {
    let loads = Arc::new(AtomicUsize::new(0));
    let server = Running::new(app(loads.clone()));
    let (headers, html) = server.get("GET", "/", false);
    let html = String::from_utf8(html).unwrap();
    assert!(
        headers.starts_with("HTTP/1.1 200")
            && headers.contains("script-src 'self' 'wasm-unsafe-eval'")
    );
    assert!(html.contains("&lt;Home&gt; 🦀") && html.contains("&lt;script&gt; &amp; Rust"));
    assert!(
        html.contains("data-ruvane-revision=\"123\"")
            && html.contains("/ruvane-runtime/000000000000007b/application.mjs")
    );
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    let (headers, bytes) = server.get("GET", "/", true);
    assert!(headers.contains("Content-Type: application/vnd.ruvane.page"));
    let page = NavigationPage::decode(&bytes).unwrap();
    assert_eq!(page.title(), "<Home> 🦀");
    assert_eq!(page.data(), &data("<script> & Rust"));
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    for (url, expected) in [
        ("/items/42?q=a+b", 200),
        ("/items/99", 404),
        ("/items/01", 400),
        ("/unknown", 404),
    ] {
        let (headers, bytes) = server.get("GET", url, true);
        assert!(headers.starts_with(&format!("HTTP/1.1 {expected}")));
        let page = NavigationPage::decode(&bytes).unwrap();
        assert_eq!(page.url().as_str(), url);
        if expected == 200 {
            assert_eq!(page.data().text("value"), Some("a+b"));
        } else {
            assert_eq!(page.props().component(), 3);
            assert!(page.data().is_empty());
        }
        let (headers, body) = server.get("HEAD", url, true);
        assert!(headers.starts_with(&format!("HTTP/1.1 {expected}")));
        assert!(headers.contains(&format!("Content-Length: {}", bytes.len())) && body.is_empty());
    }
    let (headers, html) = server.get("GET", "/?native=yes", false);
    assert!(headers.contains("script-src 'none'"));
    assert!(
        !String::from_utf8(html)
            .unwrap()
            .contains("/application.mjs")
    );
    let (headers, bytes) = server.get("GET", "/ruvane-runtime/000000000000007b/app.wasm", false);
    assert!(headers.contains("Content-Type: application/wasm"));
    assert_eq!(bytes, b"\0asm\x01\0\0\0");
}
#[test]
fn invalid_component_seed_is_a_server_error_before_any_packet_or_page_is_served() {
    let mut app = App::new();
    app.components(factory, 3).unwrap();
    app.client_page(PageRoute::new("bad", "/").unwrap(), 1, zero, |_| {
        Ok(ClientPageData::new("Bad", ActionData::default()))
    })
    .unwrap();
    let server = Running::new(app);
    for typed in [true, false] {
        let (headers, body) = server.get("GET", "/", typed);
        assert!(headers.starts_with("HTTP/1.1 500") && headers.contains("script-src 'none'"));
        assert!(!body.starts_with(b"RVP1"));
    }
}

#[test]
fn stale_browser_revision_is_rejected_before_the_get_loader_runs() {
    let loads = Arc::new(AtomicUsize::new(0));
    let server = Running::new(app(loads.clone()));
    let (headers, body) = server.get_revision("GET", "/", true, Some("122"));
    assert!(headers.starts_with("HTTP/1.1 409") && headers.contains("script-src 'none'"));
    assert!(!body.starts_with(b"RVP1"));
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let (headers, body) = server.get_revision("GET", "/", true, Some("123"));
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(NavigationPage::decode(&body).is_ok());
    assert_eq!(loads.load(Ordering::SeqCst), 1);
}

fn extra_get(server: &Running, target: &str, extra: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(server.address).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: {}\r\nAccept: application/vnd.ruvane.page\r\n{extra}\r\n",
        server.address
    )
    .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (
        String::from_utf8(bytes[..end].to_vec()).unwrap(),
        bytes[end + 4..].to_vec(),
    )
}
#[test]
fn public_cache_policy_applies_only_to_explicit_routes_and_uncredentialed_successes() {
    let mut app = app(Arc::new(AtomicUsize::new(0)));
    assert!(app.prefetch_link("home", &[], "Home").is_err());
    app.client_page_cache("home", ruvane::NavigationCachePolicy::public(30).unwrap())
        .unwrap();
    assert!(
        app.prefetch_link("home", &[], "Home")
            .unwrap()
            .as_str()
            .contains("data-ruvane-prefetch")
    );
    assert!(
        app.client_page_cache(
            "unknown",
            ruvane::NavigationCachePolicy::public(30).unwrap()
        )
        .is_err()
    );
    let server = Running::new(app);
    let (headers, _) = server.get("GET", "/", true);
    assert!(headers.contains("Cache-Control: public, max-age=30, must-revalidate"));
    assert!(headers.contains("Vary: Accept, Ruvane-Application-Revision"));
    let (headers, _) = server.get("GET", "/", false);
    assert!(headers.contains("Cache-Control: no-store"));
    assert!(headers.contains("Vary: Accept, Ruvane-Application-Revision"));
    for extra in ["Cookie: session=fixture\r\n", "Authorization: fixture\r\n"] {
        let (headers, _) = extra_get(&server, "/", extra);
        assert!(headers.contains("Cache-Control: no-store"));
    }
    for path in ["/unknown", "/items/99", "/items/01"] {
        let (headers, _) = server.get("GET", path, true);
        assert!(headers.contains("Cache-Control: no-store"));
    }
}
#[test]
fn loaders_can_tighten_public_routes_to_no_store() {
    let mut app = App::new();
    app.components(factory, 3).unwrap();
    app.client_page(PageRoute::new("private", "/").unwrap(), 1, zero, |_| {
        Ok(ClientPageData::new("Private", data("value")).no_store())
    })
    .unwrap();
    app.client_page_cache(
        "private",
        ruvane::NavigationCachePolicy::public(30).unwrap(),
    )
    .unwrap();
    let server = Running::new(app);
    assert!(
        server
            .get("GET", "/", true)
            .0
            .contains("Cache-Control: no-store")
    );
}

fn nested() -> App {
    let mut app = app(Arc::new(AtomicUsize::new(0)));
    app.client_layout(PageRoute::new("root", "/").unwrap(), None, 1, zero, |_| {
        Ok(ruvane::ClientLayoutData::new(data("Root layout")))
    })
    .unwrap();
    app.client_layout(
        PageRoute::new("item_context", "/items/{id}").unwrap(),
        Some("root"),
        2,
        id,
        |_| Ok(ruvane::ClientLayoutData::new(data("Item layout"))),
    )
    .unwrap();
    app.client_page_layout("home", Some("root")).unwrap();
    app.client_page_layout("item", Some("item_context"))
        .unwrap();
    app
}
#[test]
fn nested_layouts_share_canonical_contexts_and_reject_forged_or_missing_branches() {
    let application = nested();
    let server = Running::new(application.clone());
    let (_, body) = server.get("GET", "/items/7?q=hello", true);
    let page = NavigationPage::decode(&body).unwrap();
    assert!(application.validate_navigation(&page));
    assert_eq!(
        page.layouts().iter().map(|l| l.name()).collect::<Vec<_>>(),
        vec!["root", "item_context"]
    );
    assert_eq!(page.layouts()[1].key().as_str(), "/items/7");
    assert_eq!(page.layouts()[1].props().state(), InitialState::Number(7));
    assert!(!application.validate_navigation(&page.clone().with_layouts(vec![]).unwrap()));
    let mut chain = page.layouts().to_vec();
    chain[1] = ruvane::NavigationLayout::new(
        "item_context",
        InternalUrl::new("/items/8").unwrap(),
        chain[1].props(),
        chain[1].data().clone(),
    )
    .unwrap();
    assert!(!application.validate_navigation(&page.clone().with_layouts(chain).unwrap()));
    let (headers, html) = server.get("GET", "/items/7?native=yes", false);
    assert!(headers.contains("script-src 'none'"));
    let html = String::from_utf8(html).unwrap();
    assert!(html.find("Root layout").unwrap() < html.find("Item layout").unwrap());
    assert_eq!(html.matches("data-ruvane-child-outlet").count(), 2);
    let (_, body) = server.get("GET", "/items/99", true);
    assert!(NavigationPage::decode(&body).unwrap().layouts().is_empty());
}
#[test]
fn every_layout_must_opt_in_and_the_strictest_cache_policy_wins() {
    let mut app = nested();
    app.client_page_cache("item", ruvane::NavigationCachePolicy::public(30).unwrap())
        .unwrap();
    app.client_layout_cache("root", ruvane::NavigationCachePolicy::public(20).unwrap())
        .unwrap();
    assert!(app.prefetch_link("item", &[("id", "7")], "Item").is_err());
    assert!(
        Running::new(app.clone())
            .get("GET", "/items/7", true)
            .0
            .contains("Cache-Control: no-store")
    );
    app.client_layout_cache(
        "item_context",
        ruvane::NavigationCachePolicy::public(10).unwrap(),
    )
    .unwrap();
    assert!(app.prefetch_link("item", &[("id", "7")], "Item").is_ok());
    let server = Running::new(app);
    assert!(server.get("GET", "/items/7", true).0.contains("max-age=10"));
    assert!(
        extra_get(&server, "/items/7", "Cookie: fixture=1\r\n")
            .0
            .contains("Cache-Control: no-store")
    );
}
#[test]
fn client_layout_registration_is_bounded_and_rejects_unrelated_patterns_atomically() {
    let mut app = app(Arc::new(AtomicUsize::new(0)));
    for n in 0..6 {
        let parent = if n == 0 {
            None
        } else {
            Some(format!("l{}", n - 1))
        };
        app.client_layout(
            PageRoute::new(&format!("l{n}"), "/").unwrap(),
            parent.as_deref(),
            1,
            zero,
            |_| Ok(ruvane::ClientLayoutData::new(data("Layout"))),
        )
        .unwrap();
    }
    assert_eq!(
        app.client_layout(
            PageRoute::new("deep", "/").unwrap(),
            Some("l5"),
            1,
            zero,
            |_| Ok(ruvane::ClientLayoutData::new(data("L")))
        ),
        Err(AppError::LayoutDepth)
    );
    app.client_layout(
        PageRoute::new("other", "/other").unwrap(),
        None,
        1,
        zero,
        |_| Ok(ruvane::ClientLayoutData::new(data("L"))),
    )
    .unwrap();
    assert!(app.client_page_layout("item", Some("other")).is_err());
    assert!(
        app.client_layout(
            PageRoute::new("bad", "/items").unwrap(),
            Some("other"),
            1,
            zero,
            |_| Ok(ruvane::ClientLayoutData::new(data("L")))
        )
        .is_err()
    );
    assert!(app.client_page_layout("item", Some("deep")).is_err());
    app.client_page_layout("item", Some("l5")).unwrap();
    let (_, body) = Running::new(app).get("GET", "/items/1", true);
    assert_eq!(NavigationPage::decode(&body).unwrap().layouts().len(), 6);
}
#[test]
fn invalid_layout_seeds_and_conditional_no_store_apply_to_the_entire_response() {
    let mut app = nested();
    app.client_layout(PageRoute::new("bad", "/").unwrap(), None, 1, zero, |_| {
        Ok(ruvane::ClientLayoutData::new(ActionData::default()))
    })
    .unwrap();
    app.client_page_layout("home", Some("bad")).unwrap();
    assert!(
        Running::new(app)
            .get("GET", "/", true)
            .0
            .starts_with("HTTP/1.1 500")
    );
    let mut app = nested();
    app.client_layout(
        PageRoute::new("conditional", "/").unwrap(),
        None,
        1,
        zero,
        |_| Ok(ruvane::ClientLayoutData::new(data("Private")).no_store()),
    )
    .unwrap();
    app.client_page_layout("home", Some("conditional")).unwrap();
    app.client_page_cache("home", ruvane::NavigationCachePolicy::public(30).unwrap())
        .unwrap();
    app.client_layout_cache(
        "conditional",
        ruvane::NavigationCachePolicy::public(30).unwrap(),
    )
    .unwrap();
    assert!(
        Running::new(app)
            .get("GET", "/", true)
            .0
            .contains("Cache-Control: no-store")
    );
}

fn boundary_app() -> App {
    let mut app = nested();
    app.client_page(
        PageRoute::new("failure", "/items/{id}/failure").unwrap(),
        2,
        id,
        |context| match context.request().unwrap().query_params().get("mode") {
            Some("panic") => panic!("private diagnostic"),
            Some("schema") => Ok(ClientPageData::new("Bad seed", ActionData::default())),
            Some("bad") => Err(PageError::BadRequest),
            Some("missing") => Err(PageError::NotFound),
            _ => Err(PageError::InternalServerError),
        },
    )
    .unwrap();
    app.client_page_layout("failure", Some("item_context"))
        .unwrap();
    app.client_layout_boundary("root", Some(ruvane::ClientBoundary::new(5, 4).unwrap()))
        .unwrap();
    app
}
#[test]
fn local_errors_keep_only_validated_ancestors_with_real_status_and_no_store() {
    let app = boundary_app();
    let server = Running::new(app.clone());
    for (mode, status) in [
        ("error", 500),
        ("panic", 500),
        ("schema", 500),
        ("bad", 400),
        ("missing", 404),
    ] {
        let (headers, body) = server.get("GET", &format!("/items/7/failure?mode={mode}"), true);
        assert!(headers.starts_with(&format!("HTTP/1.1 {status}")));
        assert!(headers.contains("Cache-Control: no-store"));
        let packet = NavigationPage::decode(&body).unwrap();
        assert_eq!(packet.failed_segment(), Some(2));
        assert_eq!(packet.layouts().len(), 1);
        assert_eq!(packet.layouts()[0].name(), "root");
        assert!(app.validate_navigation(&packet));
        assert!(!String::from_utf8_lossy(&body).contains("private diagnostic"));
    }
    let (headers, body) = server.get("GET", "/items/7/failure?native=yes", false);
    assert!(headers.starts_with("HTTP/1.1 500") && headers.contains("script-src 'none'"));
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Root layout") && html.contains("Local boundary"));
    assert!(!html.contains("Item layout"));
}
#[test]
fn page_boundary_overrides_ancestors_and_forged_failure_scopes_are_rejected() {
    let mut app = boundary_app();
    let boundary = ruvane::ClientBoundary::new(5, 4).unwrap();
    app.client_page_boundary("failure", Some(boundary)).unwrap();
    let (_, body) = Running::new(app.clone()).get("GET", "/items/7/failure", true);
    let packet = NavigationPage::decode(&body).unwrap();
    assert_eq!(packet.layouts().len(), 2);
    assert!(app.validate_navigation(&packet));
    let forged = packet
        .clone()
        .with_failure(3, packet.layouts().to_vec())
        .unwrap();
    assert!(!app.validate_navigation(&forged));
    let wrong = NavigationPage::new(
        packet.url().clone(),
        packet.title(),
        ComponentProps::new(3, InitialState::Number(500)).unwrap(),
        ActionData::default(),
        StatusCode::InternalServerError,
    )
    .unwrap()
    .with_failure(2, packet.layouts().to_vec())
    .unwrap();
    assert!(!app.validate_navigation(&wrong));
    let mut forged = packet.layouts().to_vec();
    forged[1] = ruvane::NavigationLayout::new(
        "item_context",
        InternalUrl::new("/items/8").unwrap(),
        forged[1].props(),
        forged[1].data().clone(),
    )
    .unwrap();
    assert!(!app.validate_navigation(&packet.with_failure(2, forged).unwrap()));
    assert!(app.client_page_boundary("unknown", Some(boundary)).is_err());
    assert!(
        app.client_layout_boundary("unknown", Some(boundary))
            .is_err()
    );
    assert!(ruvane::ClientBoundary::new(0, 4).is_err());
}
#[test]
fn a_layout_failure_uses_its_parent_boundary_and_an_invalid_boundary_fails_closed() {
    let mut app = boundary_app();
    app.client_layout(
        PageRoute::new("broken", "/items/{id}").unwrap(),
        Some("root"),
        2,
        id,
        |_| Err(PageError::InternalServerError),
    )
    .unwrap();
    app.client_layout_boundary("broken", Some(ruvane::ClientBoundary::new(5, 4).unwrap()))
        .unwrap();
    app.client_page_layout("item", Some("broken")).unwrap();
    let (_, body) = Running::new(app.clone()).get("GET", "/items/7", true);
    let packet = NavigationPage::decode(&body).unwrap();
    assert_eq!(packet.failed_segment(), Some(1));
    assert_eq!(packet.layouts().len(), 1);
    assert!(app.validate_navigation(&packet));
    app.client_layout_boundary("root", Some(ruvane::ClientBoundary::new(5, 999).unwrap()))
        .unwrap();
    let (headers, body) = Running::new(app).get("GET", "/items/7", true);
    assert!(headers.starts_with("HTTP/1.1 500"));
    assert!(NavigationPage::decode(&body).is_err());
}
#[test]
fn public_boundary_manifest_declares_routes_without_running_other_loaders() {
    let mut app = boundary_app();
    app.client_page(
        PageRoute::new("unused", "/unused").unwrap(),
        1,
        zero,
        |_| panic!("must not run"),
    )
    .unwrap();
    let (_, body) = Running::new(app).get("GET", "/", false);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("id=\"ruvane-boundaries\""));
    assert!(html.contains("data-rvn-name=\"failure\""));
    assert!(html.contains("data-rvn-chain=\"root,item_context\""));
    assert!(html.contains("data-rvn-loading=\"5\""));
}
