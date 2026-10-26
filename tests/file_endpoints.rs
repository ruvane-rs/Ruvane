use ruvane::{
    ActionData, ActionReply, App, FileEndpoint, FilePage, FileRouteRegistry, InternalUrl, Page,
    PageMode, QueryMode, Response, Server, SiteOrigin,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Running {
    address: SocketAddr,
    stop: ruvane::ShutdownHandle,
    thread: Option<std::thread::JoinHandle<std::io::Result<()>>>,
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
            thread: Some(std::thread::spawn(move || server.run())),
        }
    }
    fn request(&self, method: &str, path: &str, headers: &str, body: &str) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n{body}",
            self.address
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
    fn headers(&self, body: &str) -> String {
        format!(
            "Origin: http://{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
            self.address,
            body.len()
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
fn endpoints_run_only_on_requests_with_head_parameters_and_query_modes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut app = App::new();
    let mut registry = FileRouteRegistry::new(&mut app);
    registry
        .endpoint(
            "api/[id]/route.rs",
            "/api/{id}",
            FileEndpoint::new().get(move |request| {
                counted.fetch_add(1, Ordering::Relaxed);
                Response::text(format!(
                    "{}|{}",
                    request.param("id").unwrap(),
                    request.query_params().get("q").unwrap_or("")
                ))
            }),
        )
        .unwrap();
    registry
        .endpoint(
            "search/route.rs",
            "/search",
            FileEndpoint::new().get_form(|request| {
                Response::text(request.query_params().get("q").unwrap_or("").to_owned())
            }),
        )
        .unwrap();
    registry.finish().unwrap();
    assert!(matches!(
        app.static_site(&SiteOrigin::parse("https://example.com").unwrap()),
        Err(ruvane::AppError::Export(ruvane::ExportError::EmptySite))
    ));
    assert_eq!(
        app.server_only_routes().collect::<Vec<_>>(),
        ["/api/{id}", "/search"]
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let server = Running::new(app);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let (headers, bytes) = server.request("GET", "/api/a%20b?q=x+y", "", "");
    assert!(headers.starts_with("HTTP/1.1 200") && headers.contains("Content-Type: text/plain"));
    assert_eq!(bytes, b"a b|x+y");
    let (headers, bytes) = server.request("HEAD", "/api/a%20b?q=x+y", "", "");
    assert!(
        headers.starts_with("HTTP/1.1 200")
            && headers.contains("Content-Length: 7")
            && bytes.is_empty()
    );
    assert_eq!(server.request("GET", "/search?q=x+y", "", "").1, b"x y");
    assert!(
        server
            .request("GET", "/search?q=one&q=two", "", "")
            .0
            .starts_with("HTTP/1.1 400")
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn post_endpoints_share_page_urls_and_keep_origin_mime_body_and_method_checks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut app = App::new();
    let mut registry = FileRouteRegistry::new(&mut app);
    registry
        .page(
            "page.rs",
            "/form",
            None,
            FilePage::server("form", PageMode::ServerOnly(QueryMode::Uri), |_| {
                Ok(Page::new("Form page", "Ready"))
            }),
        )
        .unwrap();
    registry
        .endpoint(
            "route.rs",
            "/form",
            FileEndpoint::new().post_form(move |request| {
                counted.fetch_add(1, Ordering::Relaxed);
                assert_eq!(request.form_data().unwrap().get("name"), Some("Rust"));
                Response::see_other(InternalUrl::new("/form").unwrap())
            }),
        )
        .unwrap();
    registry
        .endpoint(
            "actions/[id]/route.rs",
            "/actions/{id}",
            FileEndpoint::new().post_action(|request| {
                let mut data = ActionData::default();
                data.insert(
                    "id",
                    ruvane::ActionValue::Text(request.param("id").unwrap().into()),
                )?;
                Ok(ActionReply::success(data))
            }),
        )
        .unwrap();
    registry.finish().unwrap();
    let server = Running::new(app);
    assert!(
        server
            .request("GET", "/form", "", "")
            .0
            .starts_with("HTTP/1.1 200")
    );
    let body = "name=Rust";
    let headers = server.headers(body);
    for (headers, status, body) in [
        (
            headers.replace(&format!("Origin: http://{}\r\n", server.address), ""),
            403,
            body,
        ),
        (
            headers.replace(
                &format!("http://{}", server.address),
                "http://other.example",
            ),
            403,
            body,
        ),
        (
            format!("{headers}Sec-Fetch-Site: cross-site\r\n"),
            403,
            body,
        ),
        (
            headers.replace("application/x-www-form-urlencoded", "text/plain"),
            415,
            body,
        ),
        (
            headers.replace(
                "Content-Length: 9",
                &format!("Content-Length: {}", ruvane::MAX_POST_BODY_BYTES + 1),
            ),
            413,
            "",
        ),
    ] {
        assert!(
            server
                .request("POST", "/form", &headers, body)
                .0
                .starts_with(&format!("HTTP/1.1 {status}"))
        );
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let (headers, _) = server.request("POST", "/form", &headers, body);
    assert!(headers.starts_with("HTTP/1.1 303") && headers.contains("Location: /form"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        server
            .request("GET", "/actions/7", "", "")
            .0
            .starts_with("HTTP/1.1 405")
    );
    let (headers, bytes) = server.request("POST", "/actions/7", &server.headers(body), body);
    assert!(
        headers.starts_with("HTTP/1.1 200")
            && headers.contains("application/vnd.ruvane.action")
            && bytes.starts_with(b"RVA1")
    );
}

#[test]
fn typed_endpoint_revision_checks_precede_handlers_and_panics_stay_generic() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut app = App::new();
    app.components(|_| Err(ruvane::ComponentError::InvalidProps), 1)
        .unwrap();
    app.client_runtime(ruvane::ClientRuntime::new(b"\0asm\x01\0\0\0", 42).unwrap())
        .unwrap();
    let mut registry = FileRouteRegistry::new(&mut app);
    registry
        .endpoint(
            "action/route.rs",
            "/action",
            FileEndpoint::new().post_action(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                Ok(ActionReply::success(ActionData::default()))
            }),
        )
        .unwrap();
    registry
        .endpoint(
            "panic/route.rs",
            "/panic",
            FileEndpoint::new().get(|_| panic!("private handler diagnostic")),
        )
        .unwrap();
    registry.finish().unwrap();
    let server = Running::new(app);
    let headers = server.headers("");
    for revision in ["", "Ruvane-Application-Revision: 41\r\n"] {
        assert!(
            server
                .request("POST", "/action", &format!("{headers}{revision}"), "")
                .0
                .starts_with("HTTP/1.1 409")
        );
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(
        server
            .request(
                "POST",
                "/action",
                &format!("{headers}Ruvane-Application-Revision: 42\r\n"),
                ""
            )
            .0
            .starts_with("HTTP/1.1 200")
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let (headers, bytes) = server.request("GET", "/panic", "", "");
    assert!(
        headers.starts_with("HTTP/1.1 500")
            && !String::from_utf8_lossy(&bytes).contains("private handler")
    );
}

#[test]
fn invalid_method_sets_and_get_page_conflicts_poison_the_batch() {
    for declaration in [
        FileEndpoint::new(),
        FileEndpoint::new()
            .get(|_| Response::text("a"))
            .get_form(|_| Response::text("b")),
        FileEndpoint::new()
            .post_form(|_| Response::text("a"))
            .post_action(|_| Ok(ActionReply::success(ActionData::default()))),
    ] {
        let mut app = App::new();
        let mut registry = FileRouteRegistry::new(&mut app);
        assert_eq!(
            registry
                .endpoint("api/route.rs", "/api", declaration)
                .unwrap_err()
                .path(),
            "api/route.rs"
        );
        assert!(registry.finish().is_err());
        assert_eq!(app.server_only_routes().count(), 0);
    }
    let mut app = App::new();
    let mut registry = FileRouteRegistry::new(&mut app);
    registry
        .endpoint(
            "api/[id]/route.rs",
            "/api/{id}",
            FileEndpoint::new().get(|_| Response::text("first")),
        )
        .unwrap();
    let error = registry
        .endpoint(
            "api/[slug]/route.rs",
            "/api/{slug}",
            FileEndpoint::new().get(|_| Response::text("second")),
        )
        .unwrap_err();
    assert!(error.to_string().contains("api/[slug]/route.rs: ambiguous"));
    assert!(registry.finish().is_err());
    assert_eq!(app.server_only_routes().count(), 0);
    for pattern in ["/items/{id}", "/items/fixed"] {
        let mut app = App::new();
        let mut registry = FileRouteRegistry::new(&mut app);
        registry
            .page(
                "items/[id]/page.rs",
                "/items/{id}",
                None,
                FilePage::server("item", PageMode::ServerOnly(QueryMode::Uri), |_| {
                    Ok(Page::new("Item", "Item"))
                }),
            )
            .unwrap();
        let error = registry
            .endpoint(
                "items/fixed/route.rs",
                pattern,
                FileEndpoint::new().get(|_| Response::text("conflict")),
            )
            .unwrap_err();
        assert!(error.to_string().contains("overlaps"));
        assert!(registry.finish().is_err());
        assert!(app.url("item", &[("id", "1")]).is_err());
    }
}

#[test]
fn native_fixed_endpoints_win_over_dynamic_endpoints_and_reserved_captures_never_execute() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut app = App::new();
    let mut registry = FileRouteRegistry::new(&mut app);
    registry
        .endpoint(
            "[name]/route.rs",
            "/{name}",
            FileEndpoint::new()
                .get(move |_| {
                    counted.fetch_add(1, Ordering::Relaxed);
                    Response::text("dynamic")
                })
                .post_form(|_| Response::text("post")),
        )
        .unwrap();
    registry
        .endpoint(
            "fixed/route.rs",
            "/fixed",
            FileEndpoint::new().get(|_| Response::text("fixed")),
        )
        .unwrap();
    registry.finish().unwrap();
    let server = Running::new(app);
    assert_eq!(server.request("GET", "/fixed", "", "").1, b"fixed");
    for path in ["/_ruvane", "/ruvane-runtime"] {
        assert!(
            server
                .request("GET", path, "", "")
                .0
                .starts_with("HTTP/1.1 404")
        );
        assert!(
            server
                .request("POST", path, &server.headers(""), "")
                .0
                .starts_with("HTTP/1.1 404")
        );
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(server.request("GET", "/hello", "", "").1, b"dynamic");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn endpoints_share_the_application_route_budget_and_reject_reserved_literals() {
    let mut app = App::new();
    let mut registry = FileRouteRegistry::new(&mut app);
    for index in 0..64 {
        registry
            .endpoint(
                "route.rs",
                &format!("/r{index}"),
                FileEndpoint::new()
                    .get(|_| Response::text("ok"))
                    .post_form(|_| Response::text("ok")),
            )
            .unwrap();
    }
    assert!(
        registry
            .endpoint(
                "overflow/route.rs",
                "/overflow",
                FileEndpoint::new().get(|_| Response::text("no"))
            )
            .is_err()
    );
    assert!(registry.finish().is_err());
    assert_eq!(app.server_only_routes().count(), 0);
    for path in ["/_ruvane/status", "/ruvane-runtime/app"] {
        let mut registry = FileRouteRegistry::new(&mut app);
        assert!(
            registry
                .endpoint(
                    "route.rs",
                    path,
                    FileEndpoint::new().post_form(|_| Response::text("no"))
                )
                .is_err()
        );
        assert!(registry.finish().is_err());
    }
}
