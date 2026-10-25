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
