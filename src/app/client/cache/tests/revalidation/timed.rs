use super::*;
use crate::ServerCacheBackgroundLimits;

fn policy() -> PrerenderRevalidationPolicy {
    PrerenderRevalidationPolicy::new()
        .revalidate_after(60)
        .unwrap()
        .stale_while_revalidate(120)
        .unwrap()
        .tag("articles")
        .unwrap()
}
fn fixture(entries: usize, loads: usize, queue: usize) -> Fixture {
    let mut f = Fixture::new();
    f.cache = ServerCache::with_background(
        ServerCacheLimits::new(entries, 2 * 1024 * 1024, loads, 1000).unwrap(),
        ServerCacheBackgroundLimits::new(1, queue).unwrap(),
    )
    .unwrap();
    f.app.server_cache(f.cache.clone());
    f.app
        .client_page_revalidation("article", Some(policy()))
        .unwrap();
    let render = f.app.pages[0].render.clone();
    f.app.pages[0].render = Arc::new(move |context| {
        let mut page = render(context)?;
        if page.body.as_str().len() < 1024 {
            page.body = crate::text(format!("Fallback for {}", page.title));
        }
        Ok(page)
    });
    f
}
fn settled(f: &Fixture) {
    until(|| {
        let stats = f.cache.stats().unwrap();
        stats.active_loads == 0 && stats.background_running == 0 && stats.background_queued == 0
    });
}
fn warm(f: &Fixture, router: &Router, path: &str) -> Response {
    let html = request(router, path, false, "");
    assert_eq!(html.status(), StatusCode::Ok);
    assert_eq!(snapshot(&html).page().title(), "Title 2");
    assert_eq!(f.cache.stats().unwrap().active_loads, 0);
    html
}

#[test]
fn stale_html_and_navigation_return_immediately_and_replace_the_whole_output() {
    let mut f = fixture(8, 8, 2);
    f.app.client_page_streaming("article", true).unwrap();
    let router = f.app.router(None).unwrap();
    let html = warm(&f, &router, "/articles/a");
    f.value.store(3, Ordering::SeqCst);
    assert_eq!(
        request(&router, "/articles/a", false, "").body(),
        html.body()
    );
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    f.cache.expire_for_test(false);
    let release = f.block();
    let stale = request(&router, "/articles/a", false, "X-Marker: ignored\r\n");
    assert_eq!(stale.body(), html.body());
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    for typed in [true, false, true] {
        let response = request(&router, "/articles/a", typed, "");
        assert_eq!(response.cache_control(), "no-store");
        let page = if typed {
            seed(&response)
        } else {
            snapshot(&response).page().clone()
        };
        assert_eq!(page.title(), "Title 2");
        assert_eq!(
            page.metadata().unwrap().description_text(),
            Some("Description 2")
        );
    }
    let progressive = request(
        &router,
        "/articles/a",
        false,
        "Accept: application/vnd.ruvane.page-stream\r\nRuvane-Application-Revision: 123\r\nRuvane-Stream-Token: 1\r\n",
    );
    assert!(!progressive.is_streaming());
    assert_eq!(seed(&progressive).title(), "Title 2");
    assert_eq!(f.cache.stats().unwrap().refresh_attempts, 1);
    release.send(()).unwrap();
    settled(&f);
    let fresh = request(&router, "/articles/a", false, "");
    let nav = request(&router, "/articles/a", true, "");
    assert_eq!(snapshot(&fresh).page().encode(), seed(&nav).encode());
    assert_eq!(seed(&nav).title(), "Title 3");
    assert!(fresh.body_text().unwrap().contains("Description 3"));
    assert!(fresh.body_text().unwrap().contains("Fallback for Title 3"));
    assert_eq!(f.loads.load(Ordering::SeqCst), 4);
    assert_eq!(
        *f.observations.lock().unwrap(),
        vec![(true, true), (true, true), (false, true), (false, true)]
    );
}

#[test]
fn hard_expiry_waits_for_the_running_generation_and_never_returns_old_output() {
    let f = fixture(8, 8, 2);
    let router = Arc::new(f.app.router(None).unwrap());
    let _ = warm(&f, &router, "/articles/a");
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    let release = f.block();
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    f.cache.expire_for_test(true);
    let waiter = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/articles/a", false, ""))
    };
    until(|| f.cache.stats().unwrap().waiting_requests == 1);
    release.send(()).unwrap();
    let response = waiter.join().unwrap();
    assert_eq!(snapshot(&response).page().title(), "Title 3");
    settled(&f);
    assert_eq!(f.loads.load(Ordering::SeqCst), 4);
}

#[test]
fn blocking_expiry_needs_no_workers_and_manual_policy_remains_unchanged() {
    let mut f = Fixture::new();
    f.app
        .client_page_revalidation(
            "article",
            Some(
                PrerenderRevalidationPolicy::new()
                    .revalidate_after(1)
                    .unwrap(),
            ),
        )
        .unwrap();
    let router = f.app.router(None).unwrap();
    let _ = warm(&f, &router, "/articles/a");
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(true);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 3"
    );
    assert_eq!(f.cache.stats().unwrap().refresh_attempts, 0);
    assert_eq!(f.loads.load(Ordering::SeqCst), 4);
}

#[test]
fn transient_failures_keep_only_the_original_stale_window_and_unsafe_results_retire_it() {
    for mode in 1..=8 {
        let f = fixture(8, 8, 2);
        let router = f.app.router(None).unwrap();
        let html = warm(&f, &router, "/articles/a");
        f.value.store(3, Ordering::SeqCst);
        f.mode.store(mode, Ordering::SeqCst);
        f.cache.expire_for_test(false);
        assert_eq!(
            request(&router, "/articles/a", false, "").body(),
            html.body()
        );
        settled(&f);
        let retired = matches!(mode, 1 | 5 | 8);
        assert_eq!(
            f.cache.stats().unwrap().entries,
            usize::from(!retired),
            "mode {mode}"
        );
        f.mode.store(0, Ordering::SeqCst);
        f.value.store(4, Ordering::SeqCst);
        if !retired {
            assert_eq!(
                request(&router, "/articles/a", false, "").body(),
                html.body()
            );
            assert_eq!(f.loads.load(Ordering::SeqCst), 4);
            assert_eq!(f.cache.stats().unwrap().refresh_attempts, 1);
            f.cache.expire_for_test(true);
        }
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 4"
        );
        assert_eq!(f.loads.load(Ordering::SeqCst), 5);
    }
}

#[test]
fn invalidation_fences_background_publication_and_queued_output_work() {
    for tag in [false, true] {
        let f = fixture(8, 8, 2);
        let router = f.app.router(None).unwrap();
        let _ = warm(&f, &router, "/articles/a");
        let _ = warm(&f, &router, "/articles/b");
        f.cache.expire_for_test(false);
        f.value.store(3, Ordering::SeqCst);
        let release = f.block();
        let _ = request(&router, "/articles/a", true, "");
        until(|| f.loads.load(Ordering::SeqCst) == 5);
        let _ = request(&router, "/articles/b", true, "");
        assert_eq!(f.cache.stats().unwrap().background_queued, 1);
        if tag {
            f.cache.invalidate_tag("articles").unwrap();
        } else {
            f.cache.invalidate_path("/articles/a").unwrap();
            f.cache.invalidate_path("/articles/b").unwrap();
        }
        assert_eq!(
            request(&router, "/articles/a", true, "").status(),
            StatusCode::ServiceUnavailable
        );
        f.value.store(4, Ordering::SeqCst);
        release.send(()).unwrap();
        settled(&f);
        assert_eq!(f.cache.stats().unwrap().entries, 0);
        assert_eq!(f.loads.load(Ordering::SeqCst), 5);
        assert_eq!(
            seed(&request(&router, "/articles/b", true, "")).title(),
            "Title 4"
        );
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 4"
        );
    }
}

#[test]
fn eviction_cannot_resurrect_a_running_output_refresh() {
    let f = fixture(1, 8, 2);
    let router = f.app.router(None).unwrap();
    let _ = warm(&f, &router, "/articles/a");
    f.cache.expire_for_test(false);
    f.value.store(3, Ordering::SeqCst);
    let release = f.block();
    let _ = request(&router, "/articles/a", true, "");
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    assert_eq!(
        seed(&request(&router, "/articles/b", true, "")).title(),
        "Title 3"
    );
    release.send(()).unwrap();
    settled(&f);
    f.value.store(4, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/b", true, "")).title(),
        "Title 3"
    );
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 4"
    );
    assert_eq!(f.cache.stats().unwrap().entries, 1);
}

#[test]
fn a_full_refresh_queue_returns_permitted_old_output_without_extra_loaders() {
    let mut f = fixture(8, 8, 1);
    f.app
        .client_page_prerender(
            "article",
            Some(
                ["a", "b", "c"]
                    .map(|id| PageParams::new(&[("id", id)]).unwrap())
                    .to_vec(),
            ),
        )
        .unwrap();
    let plan = f.app.client_prerender_plan().unwrap();
    prerender::attach(&mut f.app, &plan).unwrap();
    let router = f.app.router(None).unwrap();
    for path in ["/articles/a", "/articles/b", "/articles/c"] {
        let _ = warm(&f, &router, path);
    }
    let before = f.loads.load(Ordering::SeqCst);
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    let release = f.block();
    let _ = request(&router, "/articles/a", true, "");
    until(|| f.loads.load(Ordering::SeqCst) == before + 1);
    let _ = request(&router, "/articles/b", true, "");
    assert_eq!(
        seed(&request(&router, "/articles/c", true, "")).title(),
        "Title 2"
    );
    let stats = f.cache.stats().unwrap();
    assert_eq!(stats.active_loads, 2);
    assert_eq!(stats.background_queued, 1);
    assert_eq!(stats.refresh_failures, 1);
    assert_eq!(f.loads.load(Ordering::SeqCst), before + 1);
    release.send(()).unwrap();
    settled(&f);
    assert_eq!(f.loads.load(Ordering::SeqCst), before + 2);
}

#[test]
fn private_query_and_unlisted_reads_never_refresh_a_stale_output_entry() {
    let f = fixture(8, 8, 2);
    let router = f.app.router(None).unwrap();
    let _ = warm(&f, &router, "/articles/a");
    f.cache.expire_for_test(false);
    f.value.store(3, Ordering::SeqCst);
    for header in [
        "Cookie: fixture=present\r\n",
        "Cookie:\r\n",
        "Authorization: Bearer fixture\r\n",
        "Authorization:\r\n",
    ] {
        assert_eq!(
            seed(&request(&router, "/articles/a", true, header)).title(),
            "Title 999"
        );
    }
    for path in ["/articles/a?q=1", "/articles/a?", "/articles/c"] {
        assert_eq!(seed(&request(&router, path, true, "")).title(), "Title 3");
    }
    let stats = f.cache.stats().unwrap();
    assert_eq!(stats.refresh_attempts, 0);
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.stale_entries, 1);
    assert_eq!(stats.active_loads, 0);
}

#[test]
fn stopping_workers_retires_current_output_work_and_hard_expiry_still_loads_fresh() {
    let f = fixture(8, 8, 2);
    let router = f.app.router(None).unwrap();
    let _ = warm(&f, &router, "/articles/a");
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    let release = f.block();
    let _ = request(&router, "/articles/a", true, "");
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    f.cache.stop_background().unwrap();
    release.send(()).unwrap();
    settled(&f);
    f.cache.expire_for_test(true);
    f.value.store(4, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 4"
    );
}

#[test]
fn timed_policies_validate_durations_and_require_workers_only_for_stale_delivery() {
    let default = PrerenderRevalidationPolicy::new();
    assert_eq!(default.max_age_seconds(), None);
    assert_eq!(default.stale_seconds(), 0);
    assert!(default.clone().stale_while_revalidate(1).is_err());
    for seconds in [0, 301, u32::MAX] {
        assert!(default.clone().revalidate_after(seconds).is_err());
        assert!(policy().stale_while_revalidate(seconds).is_err());
    }
    let p = policy()
        .revalidate_after(300)
        .unwrap()
        .stale_while_revalidate(300)
        .unwrap();
    assert_eq!(p.max_age_seconds(), Some(300));
    assert_eq!(p.stale_seconds(), 300);
    assert_eq!(p.tags().collect::<Vec<_>>(), vec!["articles"]);
    let mut f = Fixture::new();
    f.app
        .client_page_revalidation("article", Some(policy()))
        .unwrap();
    assert!(f.app.router(None).is_err());
    f.app
        .client_page_revalidation(
            "article",
            Some(
                PrerenderRevalidationPolicy::new()
                    .revalidate_after(60)
                    .unwrap(),
            ),
        )
        .unwrap();
    assert!(f.app.router(None).is_ok());
}

#[test]
fn output_refreshes_share_the_handle_load_budget_with_cold_reads() {
    let f = fixture(8, 1, 2);
    let router = f.app.router(None).unwrap();
    let _ = warm(&f, &router, "/articles/a");
    let _ = warm(&f, &router, "/articles/b");
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    let release = f.block();
    let _ = request(&router, "/articles/a", true, "");
    until(|| f.loads.load(Ordering::SeqCst) == 5);
    assert_eq!(
        seed(&request(&router, "/articles/b", true, "")).title(),
        "Title 2"
    );
    assert_eq!(f.cache.stats().unwrap().refresh_attempts, 1);
    f.cache.expire_for_test(true);
    assert_eq!(
        request(&router, "/articles/b", false, "").status(),
        StatusCode::ServiceUnavailable
    );
    assert_eq!(f.loads.load(Ordering::SeqCst), 5);
    release.send(()).unwrap();
    settled(&f);
    assert_eq!(
        seed(&request(&router, "/articles/b", true, "")).title(),
        "Title 3"
    );
}
