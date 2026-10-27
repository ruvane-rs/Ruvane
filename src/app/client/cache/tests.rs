use super::*;
use crate::{
    ActionValue, BrowserTag, ClientLayoutData, NavigationCachePolicy, ServerCacheLimits,
    SiteOrigin, View,
};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
mod background;
mod guards;
mod prerender;
mod revalidation;

struct Model {
    component: u32,
    value: String,
}
impl Component for Model {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if self.component == 1 {
            self.value = data
                .text("value")
                .ok_or(ComponentError::InvalidProps)?
                .into();
        } else if !data.is_empty() {
            return Err(ComponentError::InvalidProps);
        }
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
    if !(1..=3).contains(&props.component()) {
        return Err(ComponentError::InvalidProps);
    }
    Ok(Box::new(Model {
        component: props.component(),
        value: "Layout".into(),
    }))
}
fn zero(_: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(0))
}
fn fixture(cache: ServerCache, loads: Arc<AtomicUsize>, value: Arc<AtomicUsize>) -> App {
    let mut app = App::new();
    app.components(factory, 3).unwrap();
    app.client_metadata_origin(Some(SiteOrigin::parse("https://public.example").unwrap()));
    app.client_runtime(ClientRuntime::new(b"\0asm\x01\0\0\0", 123).unwrap())
        .unwrap();
    app.server_cache(cache);
    app.client_layout(
        crate::PageRoute::new("workspace", "/articles").unwrap(),
        None,
        2,
        zero,
        |context| {
            let data = ClientLayoutData::new(ActionData::default());
            Ok(
                if context.request().unwrap().query_params().get("mode") == Some("layout-private") {
                    data.no_store()
                } else {
                    data
                },
            )
        },
    )
    .unwrap();
    app.client_page(
        crate::PageRoute::new("article", "/articles/{id}").unwrap(),
        1,
        zero,
        move |context| {
            loads.fetch_add(1, Ordering::SeqCst);
            let mode = context.request().unwrap().query_params().get("mode");
            match mode {
                Some("missing") => return Err(PageError::NotFound),
                Some("panic") => panic!("fixture loader failed"),
                _ => {}
            }
            let private = context.request().unwrap().header("cookie").is_some()
                || context.request().unwrap().header("authorization").is_some();
            let value = if private {
                999
            } else {
                value.load(Ordering::SeqCst)
            };
            let mut data = ActionData::default();
            if mode != Some("bad-seed") {
                data.insert("value", ActionValue::Text(format!("Article {value}")))
                    .unwrap();
            }
            let description = format!("Description {value}");
            let page = ClientPageData::new(format!("Title {value}"), data)
                .metadata(context.metadata(&description).unwrap())
                .fallback(crate::text("Native fallback"));
            Ok(if mode == Some("private") {
                page.no_store()
            } else {
                page
            })
        },
    )
    .unwrap();
    app.client_page_layout("article", Some("workspace"))
        .unwrap();
    app.client_page_cache("article", NavigationCachePolicy::public(30).unwrap())
        .unwrap();
    app.client_layout_cache("workspace", NavigationCachePolicy::public(30).unwrap())
        .unwrap();
    app.client_page_server_cache(
        "article",
        ServerCachePolicy::public(30)
            .unwrap()
            .tag("articles")
            .unwrap(),
    )
    .unwrap();
    app
}
fn request(router: &Router, path: &str, typed: bool, extra: &str) -> Response {
    let accept = if typed {
        "Accept: application/vnd.ruvane.page\r\nRuvane-Application-Revision: 123\r\n"
    } else {
        ""
    };
    let bytes = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{accept}{extra}\r\n");
    router.dispatch(crate::http::parse(bytes.as_bytes(), 8192, 32).unwrap())
}
fn seed(response: &Response) -> NavigationPage {
    NavigationPage::decode(response.body()).unwrap()
}
fn setup() -> (ServerCache, Arc<AtomicUsize>, Arc<AtomicUsize>, App) {
    let cache = ServerCache::default();
    let loads = Arc::new(AtomicUsize::new(0));
    let value = Arc::new(AtomicUsize::new(1));
    let app = fixture(cache.clone(), loads.clone(), value.clone());
    (cache, loads, value, app)
}

#[test]
fn ssr_and_navigation_share_validated_data_and_metadata() {
    let (cache, loads, value, app) = setup();
    let router = app.router(None).unwrap();
    let html = request(&router, "/articles/a", false, "");
    assert_eq!(html.status(), StatusCode::Ok);
    assert!(html.body_text().unwrap().contains("Description 1"));
    assert!(html.body_text().unwrap().contains("Native fallback"));
    value.store(2, Ordering::SeqCst);
    let page = seed(&request(&router, "/articles/a", true, ""));
    assert_eq!(page.data().text("value"), Some("Article 1"));
    assert_eq!(
        page.metadata().unwrap().description_text(),
        Some("Description 1")
    );
    assert_eq!(
        page.metadata().unwrap().canonical_url().unwrap().as_str(),
        "https://public.example/articles/a"
    );
    assert_eq!(page.layouts().len(), 1);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(cache.stats().unwrap().hits, 1);
}

#[test]
fn credentials_neither_read_nor_replace_public_entries() {
    let (cache, loads, _, app) = setup();
    let router = app.router(None).unwrap();
    for extra in [
        "Cookie: fixture=present\r\n",
        "Authorization: Bearer fixture\r\n",
        "",
    ] {
        let response = request(&router, "/articles/a", true, extra);
        let private = !extra.is_empty();
        assert_eq!(
            seed(&response).data().text("value"),
            Some(if private { "Article 999" } else { "Article 1" })
        );
        if private {
            assert_eq!(response.cache_control(), "no-store");
            assert_eq!(cache.stats().unwrap().entries, 0);
        }
    }
    for extra in [
        "Cookie: fixture=present\r\n",
        "Authorization: Bearer fixture\r\n",
    ] {
        assert_eq!(
            seed(&request(&router, "/articles/a", true, extra))
                .data()
                .text("value"),
            Some("Article 999")
        );
    }
    assert_eq!(
        seed(&request(&router, "/articles/a", true, ""))
            .data()
            .text("value"),
        Some("Article 1")
    );
    assert_eq!(loads.load(Ordering::SeqCst), 5);
    assert_eq!(cache.stats().unwrap().misses, 1);
}

#[test]
fn query_keys_targeted_invalidation_and_new_router_namespaces() {
    let (cache, loads, value, app) = setup();
    let router = app.router(None).unwrap();
    for path in ["/articles/a?q=1", "/articles/a?q=2", "/articles/b?q=1"] {
        assert_eq!(request(&router, path, true, "").status(), StatusCode::Ok);
    }
    value.store(2, Ordering::SeqCst);
    assert_eq!(cache.invalidate_path("/articles/a").unwrap(), 2);
    assert_eq!(
        seed(&request(&router, "/articles/a?q=1", true, "")).title(),
        "Title 2"
    );
    assert_eq!(
        seed(&request(&router, "/articles/b?q=1", true, "")).title(),
        "Title 1"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 4);
    let second = app.router(None).unwrap();
    assert_eq!(
        seed(&request(&second, "/articles/b?q=1", true, "")).title(),
        "Title 2"
    );
    assert_eq!(cache.invalidate_tag("other").unwrap(), 0);
    assert_eq!(cache.invalidate_tag("articles").unwrap(), 3);
    assert_eq!(cache.stats().unwrap().entries, 0);
}

#[test]
fn no_store_errors_and_invalid_seeds_are_never_stored() {
    let (cache, loads, _, app) = setup();
    let router = app.router(None).unwrap();
    for (mode, status) in [
        ("private", StatusCode::Ok),
        ("layout-private", StatusCode::Ok),
        ("missing", StatusCode::NotFound),
        ("panic", StatusCode::InternalServerError),
        ("bad-seed", StatusCode::InternalServerError),
    ] {
        for _ in 0..2 {
            let response = request(&router, &format!("/articles/a?mode={mode}"), true, "");
            assert_eq!(response.status(), status);
            assert_eq!(response.cache_control(), "no-store");
        }
        assert_eq!(cache.stats().unwrap().entries, 0);
        assert_eq!(cache.stats().unwrap().active_loads, 0);
    }
    assert_eq!(loads.load(Ordering::SeqCst), 10);
}

#[test]
fn expiry_and_browser_freshness_do_not_extend_old_data() {
    let (cache, loads, value, mut app) = setup();
    app.client_page_server_cache("article", ServerCachePolicy::public(1).unwrap())
        .unwrap();
    let router = app.router(None).unwrap();
    let first = request(&router, "/articles/a", true, "");
    assert_eq!(first.cache_control(), "public, max-age=1, must-revalidate");
    let cached = request(&router, "/articles/a", true, "");
    assert_eq!(cached.cache_control(), "no-store");
    value.store(2, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    assert_eq!(cache.stats().unwrap().entries, 1);
}

#[test]
fn disabled_cache_and_small_storage_budgets_keep_loaders_live() {
    let cache = ServerCache::new(ServerCacheLimits::new(1, 1, 1, 10).unwrap());
    let loads = Arc::new(AtomicUsize::new(0));
    let mut app = fixture(cache.clone(), loads.clone(), Arc::new(AtomicUsize::new(1)));
    let router = app.router(None).unwrap();
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::Ok
    );
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::Ok
    );
    assert_eq!(cache.stats().unwrap().entries, 0);
    app.client_page_server_cache("article", ServerCachePolicy::no_store())
        .unwrap();
    let router = app.router(None).unwrap();
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::Ok
    );
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::Ok
    );
    assert_eq!(loads.load(Ordering::SeqCst), 4);
    assert!(
        app.client_page_server_cache("unknown", ServerCachePolicy::public(1).unwrap())
            .is_err()
    );
}

#[test]
fn concurrent_ssr_and_navigation_run_one_public_loader() {
    let cache = ServerCache::default();
    let loads = Arc::new(AtomicUsize::new(0));
    let mut app = fixture(
        cache.clone(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(1)),
    );
    let gate = Arc::new(Barrier::new(2));
    let (entered, receive) = std::sync::mpsc::channel();
    let (count, block) = (loads.clone(), gate.clone());
    app.client_page(
        crate::PageRoute::new("slow", "/slow").unwrap(),
        1,
        zero,
        move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            entered.send(()).unwrap();
            block.wait();
            let mut data = ActionData::default();
            data.insert("value", ActionValue::Text("Shared".into()))
                .unwrap();
            Ok(ClientPageData::new("Shared", data))
        },
    )
    .unwrap();
    app.client_page_server_cache("slow", ServerCachePolicy::public(30).unwrap())
        .unwrap();
    let router = Arc::new(app.router(None).unwrap());
    let owner = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/slow", false, ""))
    };
    receive.recv_timeout(Duration::from_secs(2)).unwrap();
    let waiter = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/slow", true, ""))
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while cache.stats().unwrap().waiting_requests != 1 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    gate.wait();
    assert_eq!(owner.join().unwrap().status(), StatusCode::Ok);
    assert_eq!(seed(&waiter.join().unwrap()).title(), "Shared");
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(cache.stats().unwrap().waiting_requests, 0);
}

#[test]
fn file_cache_declarations_are_atomic_and_require_an_attached_handle() {
    let (cache, _, _, mut app) = setup();
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = loads.clone();
    let mut registry = crate::FileRouteRegistry::new(&mut app);
    registry
        .page(
            "file/page.rs",
            "/file",
            None,
            crate::FilePage::client("file", 1, zero, move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut data = ActionData::default();
                data.insert("value", ActionValue::Text("File".into()))
                    .unwrap();
                Ok(ClientPageData::new("File", data))
            })
            .server_cache(ServerCachePolicy::public(30).unwrap().tag("file").unwrap()),
        )
        .unwrap();
    registry.finish().unwrap();
    let router = app.router(None).unwrap();
    for _ in 0..2 {
        assert_eq!(request(&router, "/file", true, "").status(), StatusCode::Ok);
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(cache.invalidate_tag("file").unwrap(), 1);
    let mut registry = crate::FileRouteRegistry::new(&mut app);
    assert!(
        registry
            .page(
                "bad/page.rs",
                "/bad",
                None,
                crate::FilePage::server("bad", PageMode::ServerOnly(QueryMode::Uri), |_| Ok(
                    Page::new("Bad", "")
                ))
                .server_cache(ServerCachePolicy::public(1).unwrap())
            )
            .is_err()
    );
    assert!(registry.finish().is_err());
    assert!(app.url("bad", &[]).is_err());
    app.server_cache = None;
    assert!(app.router(None).is_err());
}

#[test]
fn actual_progressive_responses_bypass_even_a_populated_cache() {
    let (cache, loads, _, mut app) = setup();
    app.client_page_streaming("article", true).unwrap();
    app.client_page_document_streaming(
        "article",
        Some(|context| {
            Ok(crate::ClientDocument::new("Title 1")
                .metadata(context.metadata("Description 1").unwrap()))
        }),
    )
    .unwrap();
    let router = app.router(None).unwrap();
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::Ok
    );
    let before = cache.stats().unwrap();
    let document = request(&router, "/articles/a", false, "");
    assert!(document.is_streaming());
    let request=crate::http::parse(b"GET /articles/a HTTP/1.1\r\nHost: localhost\r\nAccept: application/vnd.ruvane.page-stream\r\nRuvane-Application-Revision: 123\r\nRuvane-Stream-Token: 1\r\n\r\n",8192,32).unwrap();
    let navigation = router.dispatch(request);
    assert!(navigation.is_streaming());
    assert_eq!(navigation.cache_control(), "no-store");
    assert_eq!(cache.stats().unwrap().hits, before.hits);
    assert_eq!(cache.stats().unwrap().misses, before.misses);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
}
