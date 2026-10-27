use super::*;
use crate::{PageParams, PrerenderRevalidationPolicy};
use std::sync::{Mutex, mpsc};
#[cfg(unix)]
mod disk;
mod timed;

struct Fixture {
    app: App,
    cache: ServerCache,
    loads: Arc<AtomicUsize>,
    value: Arc<AtomicUsize>,
    mode: Arc<AtomicUsize>,
    gate: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    observations: Arc<Mutex<Vec<(bool, bool)>>>,
}
impl Fixture {
    fn new() -> Self {
        Self::limited(ServerCacheLimits::default())
    }
    fn limited(limits: ServerCacheLimits) -> Self {
        let (loads, value, mut app) = prerender::declared();
        let cache = ServerCache::new(limits);
        app.server_cache(cache.clone());
        let mode = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Mutex::new(None::<mpsc::Receiver<()>>));
        let observations = Arc::new(Mutex::new(Vec::new()));
        let (behavior, block, seen) = (mode.clone(), gate.clone(), observations.clone());
        let render = app.pages[0].render.clone();
        app.pages[0].render = Arc::new(move |context| {
            let request = context.request().unwrap();
            seen.lock().unwrap().push((
                context.is_exporting(),
                request.header("host").is_none()
                    && request.method() == crate::Method::Get
                    && request.form.is_none()
                    && request.header("accept").is_none()
                    && request.header("x-marker").is_none()
                    && request.content_length.unwrap_or(0) == 0
                    && request.query().is_none(),
            ));
            let mut page = render(context)?;
            let receiver = block.lock().unwrap().take();
            if let Some(receiver) = receiver {
                receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            match behavior.load(Ordering::SeqCst) {
                1 => return Err(PageError::NotFound),
                2 => return Err(PageError::InternalServerError),
                3 => panic!("regeneration fixture panic"),
                4 => page.client_data = Some(ActionData::default()),
                5 => page.client_no_store = true,
                6 => page.body = crate::text("x".repeat(crate::MAX_STATIC_FILE_BYTES)),
                7 => {
                    page.client_metadata = Some(
                        crate::Metadata::new().canonical(
                            SiteOrigin::parse("https://foreign.example")
                                .unwrap()
                                .canonical(context.path()),
                        ),
                    );
                }
                _ => {}
            }
            Ok(page)
        });
        let behavior = mode.clone();
        let layout = app.client_layouts[0].load.clone();
        app.client_layouts[0].load = Arc::new(move |context| {
            let mut data = layout(context)?;
            data.no_store |= behavior.load(Ordering::SeqCst) == 8;
            Ok(data)
        });
        app.client_page_revalidation(
            "article",
            Some(PrerenderRevalidationPolicy::new().tag("articles").unwrap()),
        )
        .unwrap();
        let plan = app.client_prerender_plan().unwrap();
        prerender::attach(&mut app, &plan).unwrap();
        value.store(2, Ordering::SeqCst);
        Self {
            app,
            cache,
            loads,
            value,
            mode,
            gate,
            observations,
        }
    }
    fn block(&self) -> mpsc::Sender<()> {
        let (send, receive) = mpsc::channel();
        *self.gate.lock().unwrap() = Some(receive);
        send
    }
}
fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "regeneration did not settle");
        std::thread::yield_now();
    }
}
fn snapshot(response: &Response) -> crate::NavigationSnapshot {
    let hex = response
        .body_text()
        .unwrap()
        .split("<div hidden id=\"ruvane-initial\">")
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap();
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    crate::NavigationSnapshot::decode(&bytes).unwrap()
}

#[test]
fn first_read_replaces_embedded_data_with_one_complete_output_until_invalidated() {
    let f = Fixture::new();
    let router = f.app.router(None).unwrap();
    assert_eq!(f.loads.load(Ordering::SeqCst), 2);
    let html = request(&router, "/articles/a", false, "X-Marker: ignored\r\n");
    assert_eq!(snapshot(&html).page().title(), "Title 2");
    assert!(html.body_text().unwrap().contains("Description 2"));
    assert!(html.body_text().unwrap().contains("Native fallback"));
    assert_eq!(html.cache_control(), "no-store");
    let nav = request(&router, "/articles/a", true, "");
    assert_eq!(nav.cache_control(), "no-store");
    assert_eq!(snapshot(&html).page().encode(), seed(&nav).encode());
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(true);
    assert_eq!(
        request(&router, "/articles/a", false, "").body(),
        html.body()
    );
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    assert_eq!(f.cache.stats().unwrap().stale_entries, 0);
    assert_eq!(f.cache.invalidate_path("/articles/a").unwrap(), 1);
    let nav = request(&router, "/articles/a", true, "");
    assert_eq!(seed(&nav).title(), "Title 3");
    let html = request(&router, "/articles/a", false, "");
    assert_eq!(snapshot(&html).page().encode(), seed(&nav).encode());
    assert!(html.body_text().unwrap().contains("Description 3"));
    assert_eq!(f.loads.load(Ordering::SeqCst), 4);
    assert_eq!(
        *f.observations.lock().unwrap(),
        vec![(true, true), (true, true), (false, true), (false, true)]
    );
    assert_eq!(f.cache.invalidate_tag("articles").unwrap(), 1);
    assert_eq!(f.cache.stats().unwrap().entries, 0);
}

#[test]
fn private_query_unlisted_and_native_requests_never_read_or_replace_output() {
    let f = Fixture::new();
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    f.value.store(3, Ordering::SeqCst);
    for extra in [
        "Cookie: fixture=present\r\n",
        "Cookie:\r\n",
        "Authorization: Bearer fixture\r\n",
        "Authorization:\r\n",
    ] {
        let response = request(&router, "/articles/a", true, extra);
        assert_eq!(seed(&response).title(), "Title 999");
        assert_eq!(response.cache_control(), "no-store");
    }
    for path in ["/articles/a?q=1", "/articles/a?", "/articles/c"] {
        assert_eq!(seed(&request(&router, path, true, "")).title(), "Title 3");
    }
    let native = request(&router, "/articles/a?native=yes", false, "");
    assert!(!native.body_text().unwrap().contains("<script"));
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    assert_eq!(f.cache.stats().unwrap().entries, 1);
    assert_eq!(f.loads.load(Ordering::SeqCst), 11);
}

#[test]
fn eviction_byte_budgets_clear_and_new_namespaces_always_reload() {
    for bytes in [1, 2 * 1024 * 1024] {
        let f = Fixture::limited(ServerCacheLimits::new(1, bytes, 8, 1000).unwrap());
        let router = f.app.router(None).unwrap();
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 2"
        );
        f.value.store(3, Ordering::SeqCst);
        let _ = request(&router, "/articles/b", true, "");
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 3"
        );
        f.value.store(4, Ordering::SeqCst);
        let restarted = f.app.router(None).unwrap();
        assert_eq!(
            seed(&request(&restarted, "/articles/a", true, "")).title(),
            "Title 4"
        );
        f.cache.clear().unwrap();
        f.value.store(5, Ordering::SeqCst);
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 5"
        );
        assert!(f.cache.stats().unwrap().bytes <= bytes);
    }
}

#[test]
fn failures_and_no_store_never_revive_embedded_or_previous_output() {
    for mode in 1..=8 {
        let f = Fixture::new();
        let router = f.app.router(None).unwrap();
        let _ = request(&router, "/articles/a", true, "");
        f.cache.invalidate_tag("articles").unwrap();
        f.value.store(3, Ordering::SeqCst);
        f.mode.store(mode, Ordering::SeqCst);
        for typed in [true, false] {
            let response = request(&router, "/articles/a", typed, "");
            let expected = match mode {
                1 => StatusCode::NotFound,
                5 | 8 => StatusCode::Ok,
                _ => StatusCode::InternalServerError,
            };
            assert_eq!(response.status(), expected, "mode {mode}");
            assert_eq!(response.cache_control(), "no-store");
            if expected == StatusCode::Ok {
                let page = if typed {
                    seed(&response)
                } else {
                    snapshot(&response).page().clone()
                };
                assert_eq!(page.title(), "Title 3");
            }
            assert_eq!(f.cache.stats().unwrap().entries, 0);
            assert_eq!(f.cache.stats().unwrap().active_loads, 0);
        }
        f.mode.store(0, Ordering::SeqCst);
        f.value.store(4, Ordering::SeqCst);
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 4"
        );
    }
}

#[test]
fn concurrent_html_and_navigation_share_one_atomic_generation() {
    let f = Fixture::new();
    let router = Arc::new(f.app.router(None).unwrap());
    let release = f.block();
    let owner = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/articles/a", true, ""))
    };
    until(|| f.loads.load(Ordering::SeqCst) == 3);
    let waiting = (0..6)
        .map(|n| {
            let router = router.clone();
            std::thread::spawn(move || {
                (n % 2 == 0, request(&router, "/articles/a", n % 2 == 0, ""))
            })
        })
        .collect::<Vec<_>>();
    until(|| f.cache.stats().unwrap().waiting_requests == 6);
    f.value.store(3, Ordering::SeqCst);
    release.send(()).unwrap();
    let expected = seed(&owner.join().unwrap()).encode();
    for waiting in waiting {
        let (typed, response) = waiting.join().unwrap();
        assert_eq!(response.status(), StatusCode::Ok);
        let page = if typed {
            seed(&response)
        } else {
            snapshot(&response).page().clone()
        };
        assert_eq!(page.encode(), expected);
        assert_eq!(page.title(), "Title 2");
    }
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    assert_eq!(f.cache.stats().unwrap().entries, 1);
}

#[test]
fn invalidation_during_generation_retires_owner_and_waiters_without_late_publication() {
    for tag in [false, true] {
        let f = Fixture::new();
        let router = Arc::new(f.app.router(None).unwrap());
        let release = f.block();
        let owner = {
            let router = router.clone();
            std::thread::spawn(move || request(&router, "/articles/a", false, ""))
        };
        until(|| f.loads.load(Ordering::SeqCst) == 3);
        let waiter = {
            let router = router.clone();
            std::thread::spawn(move || request(&router, "/articles/a", true, ""))
        };
        until(|| f.cache.stats().unwrap().waiting_requests == 1);
        if tag {
            f.cache.invalidate_tag("articles").unwrap();
        } else {
            f.cache.invalidate_path("/articles/a").unwrap();
        }
        assert_eq!(
            waiter.join().unwrap().status(),
            StatusCode::ServiceUnavailable
        );
        f.value.store(3, Ordering::SeqCst);
        release.send(()).unwrap();
        assert_eq!(
            owner.join().unwrap().status(),
            StatusCode::ServiceUnavailable
        );
        assert_eq!(f.cache.stats().unwrap().entries, 0);
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 3"
        );
    }
}

#[test]
fn output_waits_and_loads_use_the_existing_handle_budgets() {
    let f = Fixture::limited(ServerCacheLimits::new(4, 1024 * 1024, 1, 5).unwrap());
    let router = Arc::new(f.app.router(None).unwrap());
    let release = f.block();
    let owner = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/articles/a", true, ""))
    };
    until(|| f.loads.load(Ordering::SeqCst) == 3);
    for path in ["/articles/a", "/articles/b"] {
        assert_eq!(
            request(&router, path, true, "").status(),
            StatusCode::ServiceUnavailable
        );
    }
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    release.send(()).unwrap();
    assert_eq!(owner.join().unwrap().status(), StatusCode::Ok);
    assert_eq!(f.cache.stats().unwrap().waiting_requests, 0);
}

#[test]
fn unretained_generations_release_waiters_without_unbudgeted_duplicate_loads() {
    for mode in [0, 1, 5, 8] {
        let f = Fixture::limited(ServerCacheLimits::new(2, 1, 1, 1000).unwrap());
        f.mode.store(mode, Ordering::SeqCst);
        let router = Arc::new(f.app.router(None).unwrap());
        let release = f.block();
        let owner = {
            let router = router.clone();
            std::thread::spawn(move || request(&router, "/articles/a", false, ""))
        };
        until(|| f.loads.load(Ordering::SeqCst) == 3);
        let waiting = (0..3)
            .map(|_| {
                let router = router.clone();
                std::thread::spawn(move || request(&router, "/articles/a", true, ""))
            })
            .collect::<Vec<_>>();
        until(|| f.cache.stats().unwrap().waiting_requests == 3);
        release.send(()).unwrap();
        assert_eq!(
            owner.join().unwrap().status(),
            if mode == 1 {
                StatusCode::NotFound
            } else {
                StatusCode::Ok
            }
        );
        for waiting in waiting {
            assert_eq!(
                waiting.join().unwrap().status(),
                StatusCode::ServiceUnavailable
            );
        }
        assert_eq!(f.loads.load(Ordering::SeqCst), 3);
        assert_eq!(f.cache.stats().unwrap().active_loads, 0);
        assert_eq!(f.cache.stats().unwrap().entries, 0);
    }
}

#[test]
fn revalidation_configuration_is_explicit_and_disabling_prerender_restores_dynamic_loading() {
    let (loads, _, mut app) = prerender::declared();
    let policy = PrerenderRevalidationPolicy::new().tag("articles").unwrap();
    assert!(
        app.client_page_revalidation("missing", Some(policy.clone()))
            .is_err()
    );
    app.client_page_prerender("article", None).unwrap();
    assert!(
        app.client_page_revalidation("article", Some(policy.clone()))
            .is_err()
    );
    app.client_page_prerender(
        "article",
        Some(vec![PageParams::new(&[("id", "a")]).unwrap()]),
    )
    .unwrap();
    app.client_page_revalidation("article", Some(policy))
        .unwrap();
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let plan = app.client_prerender_plan().unwrap();
    prerender::attach(&mut app, &plan).unwrap();
    app.server_cache = None;
    assert!(app.router(None).is_err());
    app.server_cache(ServerCache::default());
    assert!(app.router(None).is_ok());
    app.client_page_prerender("article", None).unwrap();
    assert!(app.pages[0].revalidation.is_none());
    let router = app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    let _ = request(&router, "/articles/a", true, "");
    assert_eq!(loads.load(Ordering::SeqCst), 3);
}

#[test]
fn file_revalidation_requires_a_client_prerender_declaration_and_is_atomic() {
    for native in [false, true] {
        let (_, _, mut app) = prerender::declared();
        let mut registry = crate::FileRouteRegistry::new(&mut app);
        let page = if native {
            crate::FilePage::server("bad", PageMode::fixed(), |_| Ok(Page::new("Bad", "")))
        } else {
            crate::FilePage::client("bad", 1, zero, |_| {
                Ok(ClientPageData::new("Bad", ActionData::default()))
            })
        }
        .revalidation(PrerenderRevalidationPolicy::new());
        assert!(registry.page("bad/page.rs", "/bad", None, page).is_err());
        assert!(registry.finish().is_err());
        assert!(app.url("bad", &[]).is_err());
    }
}

#[test]
fn policy_tags_are_bounded_validated_and_deduplicated() {
    for tag in ["", "a b", "a/b", "é", &"a".repeat(49)] {
        assert!(PrerenderRevalidationPolicy::new().tag(tag).is_err());
    }
    let mut policy = PrerenderRevalidationPolicy::new();
    for n in 0..crate::MAX_SERVER_CACHE_TAGS {
        policy = policy.tag(&format!("tag{n}")).unwrap();
    }
    assert_eq!(policy.tags().count(), 8);
    assert!(policy.clone().tag("extra").is_err());
    assert_eq!(policy.tag("tag0").unwrap().tags().count(), 8);
}

#[test]
fn custom_heads_are_validated_once_with_the_generation_and_errors_remain_uncached() {
    static VERSION: AtomicUsize = AtomicUsize::new(2);
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn head(context: &PageContext<'_>) -> Result<crate::ClientDocument, PageError> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        let n = if context.is_exporting() {
            1
        } else {
            VERSION.load(Ordering::SeqCst)
        };
        Ok(crate::ClientDocument::new(format!("Title {n}"))
            .metadata(context.metadata("Head metadata").unwrap()))
    }
    let (loads, value, mut app) = prerender::declared();
    let cache = ServerCache::default();
    app.server_cache(cache.clone());
    app.client_page_document_streaming("article", Some(head))
        .unwrap();
    app.client_page_revalidation("article", Some(PrerenderRevalidationPolicy::new()))
        .unwrap();
    let plan = app.client_prerender_plan().unwrap();
    prerender::attach(&mut app, &plan).unwrap();
    value.store(2, Ordering::SeqCst);
    let router = app.router(None).unwrap();
    let html = request(&router, "/articles/a", false, "");
    assert_eq!(snapshot(&html).page().title(), "Title 2");
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    cache.invalidate_path("/articles/a").unwrap();
    VERSION.store(3, Ordering::SeqCst);
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::InternalServerError
    );
    assert_eq!(cache.stats().unwrap().entries, 0);
    value.store(3, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 3"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 5);
}

#[test]
fn listed_progressive_navigation_gets_the_same_buffered_generation() {
    let mut f = Fixture::new();
    f.app.client_page_streaming("article", true).unwrap();
    let router = f.app.router(None).unwrap();
    let stream=crate::http::parse(b"GET /articles/a HTTP/1.1\r\nHost: localhost\r\nAccept: application/vnd.ruvane.page-stream\r\nRuvane-Application-Revision: 123\r\nRuvane-Stream-Token: 1\r\n\r\n",8192,32).unwrap();
    let response = router.dispatch(stream);
    assert!(!response.is_streaming());
    assert_eq!(response.content_type(), crate::NAVIGATION_MEDIA_TYPE);
    assert_eq!(seed(&response).title(), "Title 2");
    let html = request(&router, "/articles/a", false, "");
    assert_eq!(snapshot(&html).page().encode(), seed(&response).encode());
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
}
