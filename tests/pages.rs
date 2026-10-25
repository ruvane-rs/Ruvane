use ruvane::{
    App, AppError, Attribute, CONTENT_SECURITY_POLICY, ExportError, Html, InternalUrl,
    MAX_APP_LAYOUTS, MAX_APP_ROUTES, MAX_LAYOUT_DEPTH, MAX_PAGE_NAME_BYTES, MAX_PAGE_SEGMENTS,
    MAX_PAGE_STYLESHEETS, MAX_STATIC_ENTRIES, MAX_STATIC_FILE_BYTES, MediaType, Metadata, Page,
    PageError, PageMode, PageParams, PageRoute, QueryMode, Robots, Server, ServerConfig,
    ShutdownHandle, SiteOrigin, StatusCode, Tag, element,
};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn route(name: &str, path: &str) -> PageRoute {
    PageRoute::new(name, path).unwrap()
}
fn origin() -> SiteOrigin {
    SiteOrigin::parse("https://site.example").unwrap()
}
fn hello(_: &ruvane::PageContext<'_>) -> Result<Page, PageError> {
    Ok(Page::new("Hello", "body"))
}
fn params(value: &str) -> PageParams {
    PageParams::new(&[("slug", value)]).unwrap()
}

struct Running {
    address: SocketAddr,
    shutdown: ShutdownHandle,
    thread: Option<JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn start(app: &App) -> Self {
        Self::router(app.router(Some(origin())).unwrap(), ServerConfig::default())
    }
    fn router(router: ruvane::Router, config: ServerConfig) -> Self {
        let server =
            Server::bind_with_config("127.0.0.1:0".parse().unwrap(), router, config).unwrap();
        let address = server.local_addr().unwrap();
        let shutdown = server.shutdown_handle();
        Self {
            address,
            shutdown,
            thread: Some(thread::spawn(move || server.run())),
        }
    }
    fn request(&self, method: &str, path: &str) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(2)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nX-Forwarded-Host: evil.example\r\n\r\n",
            self.address
        )
        .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
        let headers = String::from_utf8(bytes[..end + 4].to_vec()).unwrap();
        assert!(headers.contains(&format!(
            "Content-Security-Policy: {CONTENT_SECURITY_POLICY}\r\n"
        )));
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, headers, bytes[end + 4..].to_vec())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

#[test]
fn named_routes_encode_data_once_and_require_exact_parameter_names() {
    let article = route("article", "/articles/{slug}/{id}/");
    assert_eq!(
        article
            .url(&[("id", "42"), ("slug", "été & %2F")])
            .unwrap()
            .as_str(),
        "/articles/%C3%A9t%C3%A9%20%26%20%252F/42/"
    );
    for values in [
        vec![],
        vec![("slug", "x")],
        vec![("slug", "x"), ("slug", "y")],
        vec![("slug", "x"), ("other", "42")],
        vec![("slug", "x"), ("id", "42"), ("extra", "1")],
    ] {
        assert!(article.url(&values).is_err());
    }
    for value in ["", ".", "..", "a/b", "a\\b", "\0", "a\nb"] {
        assert!(article.url(&[("id", "42"), ("slug", value)]).is_err());
        assert!(PageParams::new(&[("slug", value)]).is_err());
    }
    let escaped = route("value", "/{slug}/")
        .url(&[("slug", "\"><script>?a=1#x")])
        .unwrap();
    assert!(!escaped.as_str().contains(['<', '>', '"', '?', '#']));
}

#[test]
fn route_and_capture_names_patterns_and_segment_budgets_are_checked() {
    for name in ["", "_name", "1name", "a/b", "a\nb", "é", "a b"] {
        assert!(PageRoute::new(name, "/").is_err());
    }
    assert!(PageRoute::new(&"a".repeat(MAX_PAGE_NAME_BYTES), "/").is_ok());
    assert!(PageRoute::new(&"a".repeat(MAX_PAGE_NAME_BYTES + 1), "/").is_err());
    for path in [
        "relative",
        "//host/",
        "/a//b/",
        "/{x}/{x}/",
        "/{bad-name}/",
        "/../",
        "/%FF/",
        "/%2F/",
        "/x?y=1",
        "/x#f",
        "/partial{x}/",
    ] {
        assert!(PageRoute::new("page", path).is_err(), "{path}");
    }
    assert!(PageRoute::new("page", &format!("{}/", "/a".repeat(MAX_PAGE_SEGMENTS))).is_ok());
    assert!(PageRoute::new("page", &format!("{}/", "/a".repeat(MAX_PAGE_SEGMENTS + 1))).is_err());
    let name = "x".repeat(64);
    let capture = route("page", &format!("/{{{name}}}/"));
    let set = PageParams::new(&[(&name, "x")]).unwrap();
    let mut app = App::new();
    app.page(capture, None, PageMode::Static(vec![set]), hello)
        .unwrap();
    let underscore = route("under", "/{_value}/");
    assert!(underscore.url(&[("_value", "x")]).is_ok());
    assert!(PageParams::new(&[("_value", "x")]).is_ok());
}

#[test]
fn parameter_byte_and_url_byte_limits_are_enforced_before_registration() {
    let page = route("page", "/{slug}/");
    assert!(
        page.url(&[("slug", &"x".repeat(ruvane::MAX_COMPONENT_BYTES))])
            .is_ok()
    );
    assert!(
        page.url(&[("slug", &"x".repeat(ruvane::MAX_COMPONENT_BYTES + 1))])
            .is_err()
    );
    let page = route("page", "/{a}/{b}/");
    let value = " ".repeat(ruvane::MAX_COMPONENT_BYTES);
    assert!(page.url(&[("a", &value), ("b", &value)]).is_err());
    assert!(PageParams::new(&[("slug", "x"), ("slug", "y")]).is_err());
}

#[test]
fn registration_is_atomic_and_rejects_duplicate_names_paths_and_ambiguity() {
    let mut app = App::new();
    app.page(
        route("article", "/article/{slug}/"),
        None,
        PageMode::Static(vec![params("one")]),
        hello,
    )
    .unwrap();
    assert_eq!(
        app.page(
            route("article", "/another/"),
            None,
            PageMode::fixed(),
            hello
        ),
        Err(AppError::DuplicateName)
    );
    assert!(
        app.page(
            route("other", "/article/{id}/"),
            None,
            PageMode::ServerOnly(QueryMode::Uri),
            hello
        )
        .is_err()
    );
    assert!(
        app.page(
            route("shadow", "/article/one/"),
            None,
            PageMode::fixed(),
            hello
        )
        .is_err()
    );
    assert_eq!(app.url("shadow", &[]), Err(AppError::UnknownRoute));
    assert_eq!(
        app.url("article", &[("slug", "one")]).unwrap().as_str(),
        "/article/one/"
    );
    assert_eq!(app.static_site(&origin()).unwrap().len(), 1);
    app.page(
        route("reserved", "/article/reserved/"),
        None,
        PageMode::ServerOnly(QueryMode::Uri),
        hello,
    )
    .unwrap();
    assert_eq!(
        app.url("article", &[("slug", "reserved")]),
        Err(AppError::RouteConflict)
    );
    assert_eq!(
        app.url("reserved", &[]).unwrap().as_str(),
        "/article/reserved/"
    );
}

#[test]
fn static_parameter_sets_are_complete_portable_unique_and_bounded() {
    let mut app = App::new();
    for mode in [
        PageMode::Static(vec![]),
        PageMode::Static(vec![PageParams::default()]),
        PageMode::Static(vec![params("one"), params("one")]),
        PageMode::Static(vec![params("Été")]),
        PageMode::Static(vec![params("CON")]),
        PageMode::Static(vec![params("a.b")]),
    ] {
        assert!(
            app.page(route("article", "/article/{slug}/"), None, mode, hello)
                .is_err()
        );
    }
    assert!(
        app.page(
            route("fixed", "/"),
            None,
            PageMode::Static(vec![PageParams::default(), PageParams::default()]),
            hello
        )
        .is_err()
    );
    assert!(
        app.page(route("file", "/file"), None, PageMode::fixed(), hello)
            .is_err()
    );
    let sets = (0..MAX_STATIC_ENTRIES)
        .map(|i| params(&format!("item-{i}")))
        .collect();
    app.page(
        route("article", "/article/{slug}/"),
        None,
        PageMode::Static(sets),
        hello,
    )
    .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap().len(),
        MAX_STATIC_ENTRIES
    );
    assert_eq!(
        app.asset("/extra.txt", b"", MediaType::PlainText),
        Err(AppError::TooManyStaticPages)
    );
}

#[test]
fn static_asset_collisions_and_output_ancestor_conflicts_fail_without_callbacks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let mut app = App::new();
    app.page(route("home", "/"), None, PageMode::fixed(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        hello_dummy()
    })
    .unwrap();
    assert!(
        app.asset("/index.html", b"", MediaType::OctetStream)
            .is_err()
    );
    assert!(app.asset("/assets/site.css", b"", MediaType::Png).is_err());
    app.asset("/x.bin", b"", MediaType::OctetStream).unwrap();
    assert!(
        app.asset("/x.bin/y.txt", b"", MediaType::PlainText)
            .is_err()
    );
    assert!(
        app.page(
            route("duplicate", "/x.bin"),
            None,
            PageMode::ServerOnly(QueryMode::Uri),
            hello
        )
        .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(app.static_site(&origin()).unwrap().len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
fn hello_dummy() -> Result<Page, PageError> {
    Ok(Page::new("Home", "body"))
}

#[test]
fn app_route_layout_count_and_layout_depth_limits_do_not_consume_failed_slots() {
    let mut app = App::new();
    for i in 0..MAX_APP_ROUTES {
        app.page(
            route(&format!("p{i}"), &format!("/p{i}/")),
            None,
            PageMode::ServerOnly(QueryMode::Uri),
            hello,
        )
        .unwrap();
    }
    assert_eq!(
        app.page(route("extra", "/extra/"), None, PageMode::fixed(), hello),
        Err(AppError::TooManyRoutes)
    );
    assert_eq!(
        app.asset("/extra.txt", b"", MediaType::PlainText),
        Err(AppError::TooManyRoutes)
    );
    let mut app = App::new();
    assert_eq!(
        app.layout("child", Some("missing"), |_, _, body| body),
        Err(AppError::UnknownLayout)
    );
    assert_eq!(
        app.layout("cycle", Some("cycle"), |_, _, body| body),
        Err(AppError::UnknownLayout)
    );
    let mut parent = None;
    for i in 0..MAX_LAYOUT_DEPTH {
        let name = format!("l{i}");
        app.layout(&name, parent.as_deref(), |_, _, body| body)
            .unwrap();
        parent = Some(name);
    }
    assert_eq!(
        app.layout("deep", parent.as_deref(), |_, _, body| body),
        Err(AppError::LayoutDepth)
    );
    for i in MAX_LAYOUT_DEPTH..MAX_APP_LAYOUTS {
        app.layout(&format!("l{i}"), None, |_, _, body| body)
            .unwrap();
    }
    assert_eq!(
        app.layout("extra", None, |_, _, body| body),
        Err(AppError::TooManyLayouts)
    );
}

fn nested_app() -> App {
    let mut app = App::new();
    app.layout("root", None, |_, title, body| {
        element(Tag::Main)
            .child(element(Tag::H1).child(title))
            .child(body)
            .render()
    })
    .unwrap();
    app.layout("blog", Some("root"), |_, _, body| {
        element(Tag::Section)
            .attr(Attribute::Class, "blog")
            .child(body)
            .render()
    })
    .unwrap();
    app.page(
        route("article", "/article/{slug}/"),
        Some("blog"),
        PageMode::Static(vec![params("one")]),
        |context| {
            assert!(context.request().is_none());
            Ok(Page::new(
                "Article <one>",
                element(Tag::P).child(context.param("slug").unwrap()),
            )
            .metadata(context.metadata("Shared metadata.").unwrap())
            .stylesheet(InternalUrl::new("/assets/site.css").unwrap()))
        },
    )
    .unwrap();
    app.asset("/assets/site.css", b"main { color: #123; }", MediaType::Css)
        .unwrap();
    app
}

#[test]
fn nested_layouts_and_document_are_identical_in_server_and_static_with_same_origin() {
    let app = nested_app();
    let server = Running::start(&app);
    let site = app.static_site(&origin()).unwrap();
    let preview = Running::router(site.preview_router().unwrap(), ServerConfig::default());
    let get = server.request("GET", "/article/one/?ignored=1");
    let exported = preview.request("GET", "/article/one/");
    assert_eq!(get.0, 200);
    assert_eq!(get.2, exported.2);
    let html = String::from_utf8(get.2).unwrap();
    assert_eq!(html.matches("<!doctype html>").count(), 1);
    assert_eq!(html.matches("<html ").count(), 1);
    assert!(html.contains(
        "<main><h1>Article &lt;one&gt;</h1><section class=\"blog\"><p>one</p></section></main>"
    ));
    assert!(html.contains("href=\"https://site.example/article/one/\""));
    assert!(!html.contains("evil.example"));
    let head = server.request("HEAD", "/article/one/?ignored=1");
    assert_eq!(get.1, head.1);
    assert!(head.2.is_empty());
    assert_eq!(
        server.request("GET", "/assets/site.css").2,
        preview.request("GET", "/assets/site.css").2
    );
}

#[test]
fn static_context_decodes_captures_once_ignores_queries_and_rejects_malformed_data() {
    let mut app = nested_app();
    app.page(
        route("search", "/search/"),
        None,
        PageMode::ServerOnly(QueryMode::Form),
        |context| {
            let request = context.request().unwrap();
            Ok(Page::new(
                "Search",
                request.query_params().get("q").unwrap_or(""),
            ))
        },
    )
    .unwrap();
    assert_eq!(app.server_only_routes().collect::<Vec<_>>(), vec!["search"]);
    let server = Running::start(&app);
    assert!(
        String::from_utf8(server.request("GET", "/article/%252F/").2)
            .unwrap()
            .contains("<p>%2F</p>")
    );
    assert!(
        String::from_utf8(server.request("GET", "/article/%3Cscript%3E/").2)
            .unwrap()
            .contains("&lt;script&gt;")
    );
    assert!(
        String::from_utf8(server.request("GET", "/search/?q=Rust+%2B+HTML").2)
            .unwrap()
            .contains("Rust + HTML")
    );
    for path in [
        "/article/%2F/",
        "/article/%FF/",
        "/article/%00/",
        "/article/%2e%2e/",
        "/article/one/?q=x&%71=y",
        "/search/?q=x&%71=y",
    ] {
        assert_eq!(server.request("GET", path).0, 400, "{path}");
    }
    let site = app.static_site(&origin()).unwrap();
    assert!(!site.manifest().contains("/search/"));
}

#[test]
fn error_pages_are_cached_private_and_use_real_statuses_even_for_head_and_panics() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut app = nested_app();
    let count = Arc::clone(&calls);
    app.not_found(Some("root"), move || {
        count.fetch_add(1, Ordering::SeqCst);
        Page::new("Missing", "A custom missing page").metadata(
            Metadata::new()
                .robots(Robots::IndexFollow)
                .canonical(origin().canonical(&InternalUrl::new("/").unwrap())),
        )
    })
    .unwrap();
    app.internal_error(Some("root"), || Page::new("Failed", "A generic failure"))
        .unwrap();
    for (name, error) in [
        ("missing", PageError::NotFound),
        ("failed", PageError::InternalServerError),
        ("bad", PageError::BadRequest),
    ] {
        app.page(
            route(name, &format!("/{name}/")),
            None,
            PageMode::ServerOnly(QueryMode::Uri),
            move |_| Err(error),
        )
        .unwrap();
    }
    app.page(
        route("panic", "/panic/"),
        None,
        PageMode::ServerOnly(QueryMode::Uri),
        |_| panic!("local diagnostic secret"),
    )
    .unwrap();
    let server = Running::start(&app);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for (path, code, text) in [
        ("/unknown/", 404, "A custom missing page"),
        ("/missing/", 404, "A custom missing page"),
        ("/failed/", 500, "A generic failure"),
        ("/panic/", 500, "A generic failure"),
    ] {
        let get = server.request("GET", path);
        assert_eq!(get.0, code);
        let html = String::from_utf8(get.2).unwrap();
        assert!(html.contains(text));
        assert!(html.contains("noindex, follow"));
        assert!(!html.contains("canonical"));
        assert!(!html.contains("local diagnostic secret"));
        let head = server.request("HEAD", path);
        assert_eq!(get.1, head.1);
        assert!(head.2.is_empty());
    }
    assert_eq!(server.request("GET", "/bad/").0, 400);
    let malformed = server.request("GET", "/unknown/?q=%FF");
    assert_eq!(malformed.0, 400);
    assert!(
        !String::from_utf8(malformed.2)
            .unwrap()
            .contains("custom missing")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(server.request("GET", "/article/one/").0, 200);
}

#[test]
fn validation_pages_preserve_escaped_fields_but_cannot_be_exported() {
    let mut app = App::new();
    app.page(
        route("form", "/form/"),
        None,
        PageMode::ServerOnly(QueryMode::Form),
        |context| {
            Ok(Page::new(
                "Invalid",
                context
                    .request()
                    .unwrap()
                    .query_params()
                    .get("value")
                    .unwrap_or(""),
            )
            .metadata(context.metadata("Validation error.").unwrap())
            .bad_request())
        },
    )
    .unwrap();
    let server = Running::start(&app);
    let response = server.request("GET", "/form/?value=%3Cscript%3E");
    assert_eq!(response.0, 400);
    let html = String::from_utf8(response.2).unwrap();
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("noindex, follow"));
    assert!(!html.contains("canonical"));
    let mut app = App::new();
    app.page(route("invalid", "/"), None, PageMode::fixed(), |_| {
        Ok(Page::new("Invalid", "body").bad_request())
    })
    .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::RenderFailed
    );
}

#[test]
fn render_errors_panics_and_output_overflows_abort_static_planning() {
    for error in [
        PageError::NotFound,
        PageError::BadRequest,
        PageError::InternalServerError,
    ] {
        let mut app = App::new();
        app.page(route("home", "/"), None, PageMode::fixed(), move |_| {
            Err(error)
        })
        .unwrap();
        assert_eq!(
            app.static_site(&origin()).unwrap_err(),
            AppError::RenderFailed
        );
    }
    let mut app = App::new();
    app.page(route("home", "/"), None, PageMode::fixed(), |_| {
        panic!("trusted application panic")
    })
    .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::RenderFailed
    );
    let mut app = App::new();
    app.page(route("home", "/"), None, PageMode::fixed(), |_| {
        Ok(Page::new("Huge", "x".repeat(MAX_STATIC_FILE_BYTES)))
    })
    .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::OutputTooLarge
    );
}

#[test]
fn each_layout_intermediate_is_bounded_and_layout_panics_become_generic_500() {
    let mut app = App::new();
    app.layout("outer", None, |_, _, _| Html::from("small"))
        .unwrap();
    app.layout("inner", Some("outer"), |_, _, _| {
        Html::from("x".repeat(MAX_STATIC_FILE_BYTES + 1))
    })
    .unwrap();
    app.page(route("home", "/"), Some("inner"), PageMode::fixed(), hello)
        .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::OutputTooLarge
    );
    app.internal_error(None, || Page::new("Failed", "generic layout failure"))
        .unwrap();
    let server = Running::start(&app);
    assert_eq!(server.request("GET", "/").0, 500);
    let mut app = App::new();
    app.layout("panic", None, |_, _, _| panic!("layout diagnostic"))
        .unwrap();
    app.page(route("home", "/"), Some("panic"), PageMode::fixed(), hello)
        .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::RenderFailed
    );
    app.internal_error(None, || Page::new("Failed", "generic layout failure"))
        .unwrap();
    let server = Running::start(&app);
    let response = server.request("GET", "/");
    assert_eq!(response.0, 500);
    assert!(
        String::from_utf8(response.2)
            .unwrap()
            .contains("generic layout failure")
    );
}

#[test]
fn invalid_titles_languages_stylesheet_counts_and_error_callbacks_fail_closed() {
    for page in [
        Page::new("x".repeat(1025), "body"),
        Page::new("bad\ntitle", "body"),
        Page::new("Title", "body").language("bad\nlang"),
        Page::new("Title", "body").language(""),
    ] {
        let mut app = App::new();
        app.page(route("home", "/"), None, PageMode::fixed(), move |_| {
            Ok(page.clone())
        })
        .unwrap();
        assert_eq!(
            app.static_site(&origin()).unwrap_err(),
            AppError::InvalidPage
        );
    }
    let mut page = Page::new("Title", "body");
    for i in 0..=MAX_PAGE_STYLESHEETS {
        page = page.stylesheet(InternalUrl::new(&format!("/s{i}.css")).unwrap());
    }
    let mut app = App::new();
    app.page(route("home", "/"), None, PageMode::fixed(), move |_| {
        Ok(page.clone())
    })
    .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::InvalidPage
    );
    let mut app = App::new();
    app.internal_error(None, || panic!("error renderer panic"))
        .unwrap();
    assert!(matches!(app.router(None), Err(AppError::RenderFailed)));
    app.internal_error(None, || {
        Page::new("Error", "x".repeat(MAX_STATIC_FILE_BYTES + 1))
    })
    .unwrap();
    assert!(matches!(app.router(None), Err(AppError::OutputTooLarge)));
}

#[test]
fn custom_error_pages_never_bypass_the_server_response_byte_budget() {
    let mut app = App::new();
    app.internal_error(None, || Page::new("Error", "x".repeat(2048)))
        .unwrap();
    app.not_found(None, || Page::new("Missing", "x".repeat(2048)))
        .unwrap();
    app.page(route("home", "/"), None, PageMode::fixed(), |_| {
        panic!("diagnostic")
    })
    .unwrap();
    let config = ServerConfig {
        max_response_body_bytes: 64,
        ..ServerConfig::default()
    };
    let server = Running::router(app.router(None).unwrap(), config);
    for path in ["/", "/missing/"] {
        let response = server.request("GET", path);
        assert_eq!(response.0, 500);
        assert!(response.2.len() <= 64);
    }
}

#[test]
fn existing_router_snapshot_and_failed_layout_registration_preserve_application() {
    let mut app = nested_app();
    let old = app.router(None).unwrap();
    assert_eq!(
        app.page(
            route("unknown", "/unknown/"),
            Some("missing"),
            PageMode::fixed(),
            hello
        ),
        Err(AppError::UnknownLayout)
    );
    app.page(route("added", "/added/"), None, PageMode::fixed(), hello)
        .unwrap();
    let old = Running::router(old, ServerConfig::default());
    assert_eq!(old.request("GET", "/added/").0, 404);
    assert_eq!(Running::start(&app).request("GET", "/added/").0, 200);
    assert_eq!(app.static_site(&origin()).unwrap().len(), 3);
}

#[test]
fn mutation_corpus_of_route_patterns_and_values_never_panics() {
    let seed = b"/article/{slug}/";
    for position in 0..seed.len() {
        for byte in 0..=127u8 {
            let mut bytes = seed.to_vec();
            bytes[position] = byte;
            let value = String::from_utf8(bytes).unwrap();
            let _ = PageRoute::new("article", &value);
        }
    }
    let page = route("article", "/article/{slug}/");
    for byte in 0..=127u8 {
        let value = char::from(byte).to_string();
        let _ = page.url(&[("slug", &value)]);
        let _ = PageParams::new(&[("slug", &value)]);
    }
}

#[test]
fn empty_and_server_only_applications_cannot_produce_empty_static_exports() {
    let mut app = App::new();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::Export(ExportError::EmptySite)
    );
    app.page(
        route("server", "/server/"),
        None,
        PageMode::ServerOnly(QueryMode::Uri),
        hello,
    )
    .unwrap();
    assert_eq!(
        app.static_site(&origin()).unwrap_err(),
        AppError::Export(ExportError::EmptySite)
    );
    assert_eq!(
        Running::start(&app).request("GET", "/server/").0,
        StatusCode::Ok.as_u16()
    );
}

#[test]
fn decoded_aliases_cannot_generate_links_or_canonicals_owned_by_another_page() {
    let mut app = App::new();
    app.page(
        route("dynamic", "/item/{slug}/"),
        None,
        PageMode::ServerOnly(QueryMode::Uri),
        |context| {
            Ok(Page::new("Item", context.param("slug").unwrap())
                .metadata(context.metadata("Item description.").unwrap()))
        },
    )
    .unwrap();
    app.page(
        route("reserved", "/item/reserved/"),
        None,
        PageMode::fixed(),
        hello,
    )
    .unwrap();
    let server = Running::start(&app);
    assert_eq!(server.request("GET", "/item/reserved/").0, 200);
    assert_eq!(server.request("GET", "/item/%72eserved/").0, 404);
    assert_eq!(
        app.url("dynamic", &[("slug", "reserved")]),
        Err(AppError::RouteConflict)
    );
    assert_eq!(server.request("GET", "/item/%2572eserved/").0, 200);
}
