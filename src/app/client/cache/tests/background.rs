use super::*;
use crate::ServerCacheBackgroundLimits;
use std::sync::{Mutex, mpsc};

struct Fixture {
    cache: ServerCache,
    app: App,
    loads: Arc<AtomicUsize>,
    version: Arc<AtomicUsize>,
    mode: Arc<AtomicUsize>,
    gate: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    markers: Arc<Mutex<Vec<bool>>>,
}
impl Fixture {
    fn new() -> Self {
        Self::limited(8, 2)
    }
    fn limited(loads_limit: usize, queue: usize) -> Self {
        let cache = ServerCache::with_background(
            ServerCacheLimits::new(32, 2 * 1024 * 1024, loads_limit, 1000).unwrap(),
            ServerCacheBackgroundLimits::new(1, queue).unwrap(),
        )
        .unwrap();
        let mut app = fixture(
            cache.clone(),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(1)),
        );
        let loads = Arc::new(AtomicUsize::new(0));
        let version = Arc::new(AtomicUsize::new(1));
        let mode = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Mutex::new(None::<mpsc::Receiver<()>>));
        let markers = Arc::new(Mutex::new(Vec::new()));
        let layout_mode = mode.clone();
        app.client_layout(
            crate::PageRoute::new("refresh-layout", "/refresh").unwrap(),
            None,
            2,
            zero,
            move |_| {
                let data = ClientLayoutData::new(ActionData::default());
                Ok(if layout_mode.load(Ordering::SeqCst) == 7 {
                    data.no_store()
                } else {
                    data
                })
            },
        )
        .unwrap();
        let (count, value, behavior, block, seen) = (
            loads.clone(),
            version.clone(),
            mode.clone(),
            gate.clone(),
            markers.clone(),
        );
        app.client_page(
            crate::PageRoute::new("refresh", "/refresh").unwrap(),
            1,
            zero,
            move |context| {
                let value = value.load(Ordering::SeqCst);
                seen.lock().unwrap().push(
                    context
                        .request()
                        .unwrap()
                        .header("x-private-marker")
                        .is_some(),
                );
                let receiver = block.lock().unwrap().take();
                count.fetch_add(1, Ordering::SeqCst);
                if let Some(receiver) = receiver {
                    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                let mode = behavior.load(Ordering::SeqCst);
                match mode {
                    2 | 7 => return Err(PageError::InternalServerError),
                    3 => return Err(PageError::NotFound),
                    6 => panic!("refresh fixture panic"),
                    _ => {}
                }
                let mut data = ActionData::default();
                if mode != 5 {
                    data.insert("value", ActionValue::Text(format!("Value {value}")))
                        .unwrap();
                }
                let page = ClientPageData::new(format!("Version {value}"), data)
                    .metadata(context.metadata(&format!("Description {value}")).unwrap())
                    .fallback(crate::text("Native fallback"));
                Ok(if mode == 4 { page.no_store() } else { page })
            },
        )
        .unwrap();
        app.client_page_layout("refresh", Some("refresh-layout"))
            .unwrap();
        app.client_layout_cache("refresh-layout", NavigationCachePolicy::public(30).unwrap())
            .unwrap();
        app.client_page_cache("refresh", NavigationCachePolicy::public(30).unwrap())
            .unwrap();
        app.client_page_server_cache(
            "refresh",
            ServerCachePolicy::public(30)
                .unwrap()
                .stale_while_revalidate(60)
                .unwrap()
                .tag("refresh")
                .unwrap(),
        )
        .unwrap();
        Self {
            cache,
            app,
            loads,
            version,
            mode,
            gate,
            markers,
        }
    }
    fn block_next(&self) -> mpsc::Sender<()> {
        let (send, receive) = mpsc::channel();
        *self.gate.lock().unwrap() = Some(receive);
        send
    }
    fn settled(&self) {
        until(|| {
            self.cache.stats().unwrap().active_loads == 0
                && self.cache.stats().unwrap().background_running == 0
        });
    }
}
fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "background work did not settle");
        std::thread::yield_now();
    }
}

#[test]
fn stale_ssr_and_navigation_do_not_wait_and_replace_data_with_metadata() {
    let f = Fixture::new();
    let router = Arc::new(f.app.router(None).unwrap());
    assert_eq!(
        request(
            &router,
            "/refresh?q=1",
            true,
            "X-Private-Marker: fixture\r\n"
        )
        .status(),
        StatusCode::Ok
    );
    f.version.store(2, Ordering::SeqCst);
    let release = f.block_next();
    f.cache.expire_for_test(false);
    let html = request(&router, "/refresh?q=1", false, "");
    assert!(html.body_text().unwrap().contains("Description 1"));
    until(|| f.loads.load(Ordering::SeqCst) == 2);
    let requests = (0..8)
        .map(|_| {
            let router = router.clone();
            std::thread::spawn(move || request(&router, "/refresh?q=1", true, ""))
        })
        .collect::<Vec<_>>();
    for handle in requests {
        let response = handle.join().unwrap();
        assert_eq!(seed(&response).title(), "Version 1");
        assert_eq!(response.cache_control(), "no-store");
    }
    assert_eq!(f.loads.load(Ordering::SeqCst), 2);
    assert_eq!(f.cache.stats().unwrap().waiting_requests, 0);
    release.send(()).unwrap();
    f.settled();
    let page = seed(&request(&router, "/refresh?q=1", true, ""));
    assert_eq!(page.title(), "Version 2");
    assert_eq!(page.data().text("value"), Some("Value 2"));
    assert_eq!(
        page.metadata().unwrap().description_text(),
        Some("Description 2")
    );
    assert_eq!(
        page.metadata().unwrap().canonical_url().unwrap().as_str(),
        "https://public.example/refresh"
    );
    assert_eq!(*f.markers.lock().unwrap(), vec![true, false]);
}

#[test]
fn hard_expiry_waits_for_the_running_refresh_instead_of_serving_stale() {
    let f = Fixture::new();
    let router = Arc::new(f.app.router(None).unwrap());
    assert_eq!(
        request(&router, "/refresh", true, "").status(),
        StatusCode::Ok
    );
    let release = f.block_next();
    f.version.store(2, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    assert_eq!(
        seed(&request(&router, "/refresh", true, "")).title(),
        "Version 1"
    );
    until(|| f.loads.load(Ordering::SeqCst) == 2);
    f.cache.expire_for_test(true);
    let waiter = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/refresh", true, ""))
    };
    until(|| f.cache.stats().unwrap().waiting_requests == 1);
    release.send(()).unwrap();
    assert_eq!(seed(&waiter.join().unwrap()).title(), "Version 2");
    f.settled();
    assert_eq!(f.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn invalidation_retires_running_and_queued_refreshes_without_resurrection() {
    let f = Fixture::new();
    let router = f.app.router(None).unwrap();
    for path in ["/refresh?q=a", "/refresh?q=b"] {
        assert_eq!(request(&router, path, true, "").status(), StatusCode::Ok);
    }
    let release = f.block_next();
    f.cache.expire_for_test(false);
    assert_eq!(
        request(&router, "/refresh?q=a", true, "").status(),
        StatusCode::Ok
    );
    until(|| f.loads.load(Ordering::SeqCst) == 3);
    assert_eq!(
        request(&router, "/refresh?q=b", true, "").status(),
        StatusCode::Ok
    );
    assert_eq!(f.cache.stats().unwrap().background_queued, 1);
    f.cache.invalidate_tag("refresh").unwrap();
    assert_eq!(
        request(&router, "/refresh?q=a", true, "").status(),
        StatusCode::ServiceUnavailable
    );
    release.send(()).unwrap();
    f.settled();
    assert_eq!(f.cache.stats().unwrap().entries, 0);
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    f.version.store(3, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/refresh?q=b", true, "")).title(),
        "Version 3"
    );
}

#[test]
fn failed_refreshes_do_not_extend_stale_lifetime_and_apply_backoff() {
    for mode in [2, 5, 6] {
        let f = Fixture::new();
        let router = f.app.router(None).unwrap();
        assert_eq!(
            request(&router, "/refresh", true, "").status(),
            StatusCode::Ok
        );
        f.mode.store(mode, Ordering::SeqCst);
        f.cache.expire_for_test(false);
        assert_eq!(
            seed(&request(&router, "/refresh", true, "")).title(),
            "Version 1"
        );
        f.settled();
        for _ in 0..4 {
            assert_eq!(
                seed(&request(&router, "/refresh", true, "")).title(),
                "Version 1"
            );
        }
        assert_eq!(f.loads.load(Ordering::SeqCst), 2);
        assert_eq!(f.cache.stats().unwrap().refresh_failures, 1);
        f.cache.expire_for_test(true);
        assert_eq!(
            request(&router, "/refresh", true, "").status(),
            StatusCode::InternalServerError
        );
        assert_eq!(f.cache.stats().unwrap().entries, 0);
    }
}

#[test]
fn no_store_disappearance_and_private_layout_failure_retire_old_data() {
    for mode in [3, 4, 7] {
        let f = Fixture::new();
        let router = f.app.router(None).unwrap();
        assert_eq!(
            request(&router, "/refresh", true, "").status(),
            StatusCode::Ok
        );
        f.mode.store(mode, Ordering::SeqCst);
        f.cache.expire_for_test(false);
        assert_eq!(
            request(&router, "/refresh", true, "").status(),
            StatusCode::Ok
        );
        f.settled();
        assert_eq!(f.cache.stats().unwrap().entries, 0);
        let next = request(&router, "/refresh", true, "");
        assert_eq!(
            next.status(),
            match mode {
                3 => StatusCode::NotFound,
                4 => StatusCode::Ok,
                _ => StatusCode::InternalServerError,
            }
        );
        assert_eq!(next.cache_control(), "no-store");
    }
}

#[test]
fn full_queue_declines_extra_refreshes_without_blocking_stale_responses() {
    let f = Fixture::limited(8, 1);
    let router = f.app.router(None).unwrap();
    for path in ["/refresh?a=1", "/refresh?a=2", "/refresh?a=3"] {
        assert_eq!(request(&router, path, true, "").status(), StatusCode::Ok);
    }
    let release = f.block_next();
    f.cache.expire_for_test(false);
    assert_eq!(
        request(&router, "/refresh?a=1", true, "").status(),
        StatusCode::Ok
    );
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    for path in ["/refresh?a=2", "/refresh?a=3"] {
        assert_eq!(request(&router, path, true, "").status(), StatusCode::Ok);
    }
    let stats = f.cache.stats().unwrap();
    assert_eq!(stats.background_queued, 1);
    assert_eq!(stats.background_running, 1);
    assert_eq!(stats.active_loads, 2);
    assert_eq!(stats.refresh_failures, 1);
    release.send(()).unwrap();
    f.settled();
    assert_eq!(f.loads.load(Ordering::SeqCst), 5);
}

#[test]
fn stopping_refreshes_retires_running_jobs_and_releases_the_queue() {
    let f = Fixture::new();
    let router = f.app.router(None).unwrap();
    for path in ["/refresh?a=1", "/refresh?a=2"] {
        assert_eq!(request(&router, path, true, "").status(), StatusCode::Ok);
    }
    let release = f.block_next();
    f.version.store(2, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    assert_eq!(
        request(&router, "/refresh?a=1", true, "").status(),
        StatusCode::Ok
    );
    until(|| f.loads.load(Ordering::SeqCst) == 3);
    assert_eq!(
        request(&router, "/refresh?a=2", true, "").status(),
        StatusCode::Ok
    );
    f.cache.stop_background().unwrap();
    assert_eq!(f.cache.stats().unwrap().background_queued, 0);
    release.send(()).unwrap();
    f.settled();
    until(|| f.cache.stats().unwrap().background_workers == 0);
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    assert_eq!(
        seed(&request(&router, "/refresh?a=1", true, "")).title(),
        "Version 1"
    );
    f.cache.expire_for_test(true);
    assert_eq!(
        seed(&request(&router, "/refresh?a=1", true, "")).title(),
        "Version 2"
    );
}

#[test]
fn private_and_progressive_requests_do_not_schedule_refreshes() {
    let mut f = Fixture::new();
    f.app.client_page_streaming("refresh", true).unwrap();
    let router = f.app.router(None).unwrap();
    assert_eq!(
        request(&router, "/refresh", true, "").status(),
        StatusCode::Ok
    );
    f.cache.expire_for_test(false);
    for extra in [
        "Cookie: fixture=present\r\n",
        "Authorization: Bearer fixture\r\n",
        "Cookie:\r\n",
        "Authorization:\r\n",
    ] {
        assert_eq!(
            request(&router, "/refresh", true, extra).cache_control(),
            "no-store"
        );
    }
    let raw=crate::http::parse(b"GET /refresh HTTP/1.1\r\nHost: localhost\r\nAccept: application/vnd.ruvane.page-stream\r\nRuvane-Stream-Token: 1\r\nRuvane-Application-Revision: 123\r\n\r\n",8192,32).unwrap();
    assert!(router.dispatch(raw).is_streaming());
    assert_eq!(f.cache.stats().unwrap().refresh_attempts, 0);
    assert_eq!(f.cache.stats().unwrap().stale_hits, 0);
}

#[test]
fn background_policy_requires_a_pool_and_queued_jobs_do_not_keep_a_router_alive() {
    let mut f = Fixture::new();
    let handle = f.cache.clone();
    f.app.server_cache(ServerCache::default());
    assert!(f.app.router(None).is_err());
    f.app.server_cache(handle);
    let router = f.app.router(None).unwrap();
    for path in ["/refresh?a=1", "/refresh?a=2"] {
        assert_eq!(request(&router, path, true, "").status(), StatusCode::Ok);
    }
    let release = f.block_next();
    f.cache.expire_for_test(false);
    assert_eq!(
        request(&router, "/refresh?a=1", true, "").status(),
        StatusCode::Ok
    );
    until(|| f.loads.load(Ordering::SeqCst) == 3);
    assert_eq!(
        request(&router, "/refresh?a=2", true, "").status(),
        StatusCode::Ok
    );
    drop(router);
    release.send(()).unwrap();
    f.settled();
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
}

#[test]
fn buffered_custom_heads_stay_with_the_cached_version_and_seed_metadata() {
    static VERSION: AtomicUsize = AtomicUsize::new(1);
    fn head(context: &PageContext<'_>) -> Result<crate::ClientDocument, PageError> {
        let value = VERSION.load(Ordering::SeqCst);
        Ok(
            crate::ClientDocument::new(format!("Head {value}")).metadata(
                context
                    .metadata(&format!("Head description {value}"))
                    .unwrap(),
            ),
        )
    }
    let mut f = Fixture::new();
    f.app.client_not_found(Some(3)).unwrap();
    f.app
        .client_page(
            crate::PageRoute::new("headed", "/headed").unwrap(),
            1,
            zero,
            |_| {
                let value = VERSION.load(Ordering::SeqCst);
                let mut data = ActionData::default();
                data.insert("value", ActionValue::Text(format!("Value {value}")))
                    .unwrap();
                Ok(ClientPageData::new(format!("Head {value}"), data))
            },
        )
        .unwrap();
    f.app
        .client_page_document_streaming("headed", Some(head))
        .unwrap();
    f.app
        .client_page_server_cache(
            "headed",
            ServerCachePolicy::public(30)
                .unwrap()
                .stale_while_revalidate(60)
                .unwrap(),
        )
        .unwrap();
    let router = f.app.router(None).unwrap();
    let cold = seed(&request(&router, "/headed", true, ""));
    assert_eq!(
        cold.metadata().unwrap().description_text(),
        Some("Head description 1")
    );
    VERSION.store(2, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    let stale = request(&router, "/headed", false, "");
    assert_eq!(stale.status(), StatusCode::Ok);
    assert!(stale.body_text().unwrap().contains("Head description 1"));
    f.settled();
    let fresh = request(&router, "/headed", false, "");
    assert_eq!(fresh.status(), StatusCode::Ok);
    assert!(fresh.body_text().unwrap().contains("Head description 2"));
}
