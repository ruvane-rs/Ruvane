use super::*;
use crate::{FileEndpoint, FileRouteRegistry, GuardDecision, Method, RouteGuard};
use std::sync::Mutex;

fn guard(mode: Arc<AtomicUsize>, calls: Arc<AtomicUsize>) -> RouteGuard {
    RouteGuard::new(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        match mode.load(Ordering::SeqCst) {
            0 => GuardDecision::Allow,
            1 => GuardDecision::Private,
            2 => GuardDecision::Forbidden,
            3 => GuardDecision::Unavailable,
            _ => panic!("guard fixture secret"),
        }
    })
}
#[test]
fn every_request_runs_guards_before_memory_cache_and_guarded_navigation_is_no_store() {
    let (cache, loads, value, mut app) = setup();
    let mode = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    app.guard(guard(mode.clone(), calls.clone())).unwrap();
    let router = app.router(None).unwrap();
    for _ in 0..2 {
        let response = request(&router, "/articles/a", true, "");
        assert_eq!(seed(&response).title(), "Title 1");
        assert_eq!(response.cache_control(), "no-store");
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let hits = cache.stats().unwrap().hits;
    value.store(2, Ordering::SeqCst);
    mode.store(1, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    assert_eq!(cache.stats().unwrap().hits, hits);
    assert_eq!(cache.stats().unwrap().entries, 1);
    mode.store(0, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 1"
    );
    let before = loads.load(Ordering::SeqCst);
    for (behavior, status) in [
        (2, StatusCode::Forbidden),
        (3, StatusCode::ServiceUnavailable),
        (4, StatusCode::InternalServerError),
    ] {
        mode.store(behavior, Ordering::SeqCst);
        for typed in [false, true] {
            let response = request(&router, "/articles/a", typed, "");
            assert_eq!(response.status(), status);
            assert_eq!(response.cache_control(), "no-store");
            if typed {
                let page = seed(&response);
                assert_eq!(page.props().component(), crate::GUARD_COMPONENT_ID);
                assert!(page.layouts().is_empty());
                assert!(page.data().is_empty());
                assert!(app.validate_navigation(&page));
            } else {
                let body = response.body_text().unwrap();
                assert!(!body.contains("guard fixture secret"));
                assert!(!body.contains("Native fallback"));
                assert!(body.contains("noindex"));
            }
        }
    }
    assert_eq!(loads.load(Ordering::SeqCst), before);
}

#[test]
fn inheritance_is_root_first_private_is_sticky_and_context_keeps_original_request() {
    let (cache, loads, _, mut app) = setup();
    let order = Arc::new(Mutex::new(Vec::new()));
    let seen = order.clone();
    app.guard_branch(
        "/articles/{owner}",
        RouteGuard::new(move |ctx| {
            seen.lock().unwrap().push("leaf");
            assert!(ctx.cache_bypass());
            assert_eq!(ctx.request().param("id"), Some("a b"));
            GuardDecision::Allow
        }),
    )
    .unwrap();
    let seen = order.clone();
    app.guard_branch(
        "/articles",
        RouteGuard::new(move |ctx| {
            seen.lock().unwrap().push("branch");
            assert!(ctx.cache_bypass());
            GuardDecision::Allow
        }),
    )
    .unwrap();
    let seen = order.clone();
    app.guard(RouteGuard::new(move |ctx| {
        seen.lock().unwrap().push("root");
        assert_eq!(ctx.pattern(), Some("/articles/{id}"));
        assert_eq!(ctx.page_name(), Some("article"));
        assert_eq!(ctx.request().header("x-marker"), Some("original"));
        assert_eq!(ctx.request().header("cookie"), None);
        assert_eq!(ctx.request().method(), Method::Get);
        GuardDecision::Private
    }))
    .unwrap();
    let seen = order.clone();
    app.page_guard(
        "article",
        RouteGuard::new(move |ctx| {
            seen.lock().unwrap().push("page");
            assert!(ctx.cache_bypass());
            assert_eq!(ctx.request().query_params().get("q"), Some("a+b"));
            GuardDecision::Allow
        }),
    )
    .unwrap();
    let render = app.pages[0].render.clone();
    app.pages[0].render = Arc::new(move |ctx| {
        let request = ctx.request().unwrap();
        assert!(request.cache_bypass());
        assert_eq!(request.header("cookie"), None);
        render(ctx)
    });
    let router = app.router(None).unwrap();
    assert!(order.lock().unwrap().is_empty());
    assert_eq!(
        request(
            &router,
            "/articles/a%20b?q=a+b",
            true,
            "X-Marker: original\r\n"
        )
        .status(),
        StatusCode::Ok
    );
    assert_eq!(*order.lock().unwrap(), ["root", "branch", "leaf", "page"]);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(cache.stats().unwrap().entries, 0);
}

#[test]
fn decoded_scope_matching_closes_percent_aliases_and_preserves_segment_boundaries() {
    let (_, loads, _, mut app) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    app.guard_branch(
        "/articles/a",
        RouteGuard::new(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            GuardDecision::Forbidden
        }),
    )
    .unwrap();
    let router = app.router(None).unwrap();
    for path in ["/articles/a", "/articles/%61", "/articles/a/child"] {
        assert_eq!(
            request(&router, path, true, "").status(),
            StatusCode::Forbidden
        );
    }
    assert_eq!(
        request(&router, "/articles/ab", true, "").status(),
        StatusCode::Ok
    );
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn head_unknown_pages_assets_and_native_endpoints_have_explicit_guard_scope() {
    let (_, _, _, mut app) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    app.guard(RouteGuard::new(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        GuardDecision::Forbidden
    }))
    .unwrap();
    app.asset("/public.txt", b"public", crate::MediaType::PlainText)
        .unwrap();
    app.file_endpoint(
        "/api",
        FileEndpoint::new().get(|_| Response::text("must not run")),
    )
    .unwrap();
    let router = app.router(None).unwrap();
    for path in ["/unknown", "/api", "/articles/a"] {
        assert_eq!(
            request(&router, path, false, "").status(),
            StatusCode::Forbidden
        );
    }
    let head = crate::http::parse(
        b"HEAD /articles/a HTTP/1.1\r\nHost: localhost\r\n\r\n",
        8192,
        32,
    )
    .unwrap();
    assert_eq!(router.dispatch(head).status(), StatusCode::Forbidden);
    assert_eq!(request(&router, "/public.txt", false, "").body(), b"public");
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let runtime = app.client_runtime.as_ref().unwrap().asset("app.wasm");
    assert_eq!(
        request(&router, &runtime, false, "").status(),
        StatusCode::Ok
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[test]
fn post_protocol_checks_and_endpoint_guards_precede_mutation() {
    let (_, _, _, mut app) = setup();
    let guards = Arc::new(AtomicUsize::new(0));
    let mutations = Arc::new(AtomicUsize::new(0));
    let seen = guards.clone();
    let writes = mutations.clone();
    app.file_endpoint(
        "/action",
        FileEndpoint::new()
            .post_action(move |_| {
                writes.fetch_add(1, Ordering::SeqCst);
                Ok(crate::ActionReply::success(ActionData::default()))
            })
            .guard(RouteGuard::new(move |ctx| {
                seen.fetch_add(1, Ordering::SeqCst);
                assert_eq!(ctx.request().method(), Method::Post);
                assert!(ctx.request().form_data().is_some());
                GuardDecision::Forbidden
            })),
    )
    .unwrap();
    let router = app.router(None).unwrap();
    for (origin, revision) in [
        ("http://foreign.example", "123"),
        ("http://localhost", "122"),
        ("http://localhost", "123"),
    ] {
        let raw = format!(
            "POST /action HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\nRuvane-Application-Revision: {revision}\r\n\r\n"
        );
        let mut request = crate::http::parse(raw.as_bytes(), 8192, 32).unwrap();
        if router.prepare_mutation(&request, 1024).is_err() {
            continue;
        }
        request.form = Some(crate::FormData::default());
        let response = router.dispatch(request);
        assert_eq!(
            response.status(),
            if revision == "122" {
                StatusCode::Conflict
            } else {
                StatusCode::Forbidden
            }
        );
    }
    assert_eq!(guards.load(Ordering::SeqCst), 1);
    assert_eq!(mutations.load(Ordering::SeqCst), 0);
}

#[test]
fn native_static_export_cannot_bypass_guards_and_configuration_runs_no_callbacks() {
    let mut app = App::new();
    let loads = Arc::new(AtomicUsize::new(0));
    let seen = loads.clone();
    app.page(
        crate::PageRoute::new("private", "/private/").unwrap(),
        None,
        PageMode::Static(vec![crate::PageParams::default()]),
        move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(Page::new("Private", "data"))
        },
    )
    .unwrap();
    app.guard_branch(
        "/private",
        RouteGuard::new(|_| panic!("must not run during build")),
    )
    .unwrap();
    assert!(
        app.static_site(&SiteOrigin::parse("https://public.example").unwrap())
            .is_err()
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let router = app.router(None).unwrap();
    assert_eq!(
        request(&router, "/private/", false, "").status(),
        StatusCode::InternalServerError
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
}

#[test]
fn guard_limits_and_bad_file_registrations_fail_atomically() {
    let (_, loads, _, mut app) = setup();
    for pattern in ["bad", "/a/", "/a//b", "/a/../b"] {
        assert!(
            app.guard_branch(pattern, RouteGuard::new(|_| GuardDecision::Allow))
                .is_err()
        );
    }
    for _ in 0..crate::MAX_ROUTE_GUARDS {
        app.page_guard("article", RouteGuard::new(|_| GuardDecision::Allow))
            .unwrap();
    }
    assert!(
        app.page_guard("article", RouteGuard::new(|_| GuardDecision::Allow))
            .is_err()
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    let mut registry = FileRouteRegistry::new(&mut app);
    assert!(
        registry
            .guard("guard.rs", "/", RouteGuard::new(|_| GuardDecision::Allow))
            .is_err()
    );
    assert!(registry.finish().is_err());
    assert_eq!(
        request(&app.router(None).unwrap(), "/articles/a", true, "").status(),
        StatusCode::Ok
    );
    let mut app = App::new();
    for n in 0..crate::MAX_APP_GUARDS {
        app.guard_branch(
            &format!("/branch{n}"),
            RouteGuard::new(|_| GuardDecision::Allow),
        )
        .unwrap();
    }
    assert!(
        app.guard_branch("/extra", RouteGuard::new(|_| GuardDecision::Allow))
            .is_err()
    );
}

#[test]
fn guard_failure_envelopes_are_closed_and_use_only_the_reserved_read_only_component() {
    let (_, _, _, app) = setup();
    for (code, status, title) in [
        (403, StatusCode::Forbidden, "Access denied"),
        (429, StatusCode::TooManyRequests, "Too many requests"),
        (503, StatusCode::ServiceUnavailable, "Service unavailable"),
        (500, StatusCode::InternalServerError, "Page unavailable"),
    ] {
        let page = NavigationPage::new(
            InternalUrl::new("/articles/a").unwrap(),
            title,
            ComponentProps::new(crate::GUARD_COMPONENT_ID, InitialState::Number(code)).unwrap(),
            ActionData::default(),
            status,
        )
        .unwrap()
        .with_metadata(crate::Metadata::new())
        .unwrap();
        assert!(app.validate_navigation(&page));
        assert!(page.clone().with_failure(0, Vec::new()).is_err());
        let mut component = crate::guard_component(page.props()).unwrap().unwrap();
        assert!(component.dispatch(1).is_err());
        assert_eq!(component.view().unwrap().event_count(), 0);
        assert!(
            NavigationPage::new(
                page.url().clone(),
                "Wrong",
                page.props(),
                ActionData::default(),
                status
            )
            .is_err()
        );
    }
    assert!(
        NavigationPage::new(
            InternalUrl::new("/").unwrap(),
            "Access denied",
            ComponentProps::new(1, InitialState::Number(403)).unwrap(),
            ActionData::default(),
            StatusCode::Forbidden
        )
        .is_err()
    );
}

#[test]
fn file_page_and_endpoint_local_guards_inherit_branches_without_running_at_registration() {
    let (_, _, _, mut app) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let mut registry = FileRouteRegistry::new(&mut app);
    registry
        .guard(
            "guard.rs",
            "/local",
            RouteGuard::new(|_| GuardDecision::Private),
        )
        .unwrap();
    registry
        .page(
            "local/page.rs",
            "/local",
            None,
            crate::FilePage::client("local", 2, zero, |_| {
                Ok(ClientPageData::new("Local", ActionData::default()))
            })
            .guard(RouteGuard::new(move |ctx| {
                seen.fetch_add(1, Ordering::SeqCst);
                assert!(ctx.cache_bypass());
                assert_eq!(ctx.page_name(), Some("local"));
                GuardDecision::Forbidden
            })),
        )
        .unwrap();
    let seen = calls.clone();
    registry
        .endpoint(
            "local/route.rs",
            "/local",
            FileEndpoint::new()
                .post_form(|_| panic!("local handler must not run"))
                .guard(RouteGuard::new(move |ctx| {
                    seen.fetch_add(1, Ordering::SeqCst);
                    assert!(ctx.cache_bypass());
                    assert_eq!(ctx.page_name(), None);
                    GuardDecision::Unavailable
                })),
        )
        .unwrap();
    registry.finish().unwrap();
    let router = app.router(None).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        request(&router, "/local", true, "").status(),
        StatusCode::Forbidden
    );
    let mut post = crate::http::parse(b"POST /local HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nOrigin: http://localhost\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\n",8192,32).unwrap();
    router.prepare_mutation(&post, 8192).unwrap();
    post.form = Some(crate::FormData::default());
    assert_eq!(
        router.dispatch(post).status(),
        StatusCode::ServiceUnavailable
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn denied_pages_skip_application_factories_and_shell_and_streams_get_a_buffered_error() {
    let (_, loads, _, mut app) = setup();
    app.guard(RouteGuard::new(|_| GuardDecision::Forbidden))
        .unwrap();
    app.client_page_streaming("article", true).unwrap();
    app.client_runtime.as_mut().unwrap().shell = |_| panic!("private shell must not run");
    static BLOCK_FACTORY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    app.client_factory = Some(|props| {
        assert!(
            !BLOCK_FACTORY.load(Ordering::SeqCst),
            "private factory must not run"
        );
        factory(props)
    });
    let router = app.router(None).unwrap();
    BLOCK_FACTORY.store(true, Ordering::SeqCst);
    for accept in [
        "application/vnd.ruvane.page",
        "application/vnd.ruvane.page-stream",
        "text/html",
    ] {
        let bytes = format!(
            "GET /articles/a HTTP/1.1\r\nHost: localhost\r\nAccept: {accept}\r\nRuvane-Application-Revision: 123\r\nRuvane-Stream-Token: 1\r\n\r\n"
        );
        let response = router.dispatch(crate::http::parse(bytes.as_bytes(), 8192, 32).unwrap());
        assert_eq!(response.status(), StatusCode::Forbidden);
        assert!(!response.is_streaming());
        assert_eq!(response.cache_control(), "no-store");
        if accept != "text/html" {
            assert_eq!(
                seed(&response).props().component(),
                crate::GUARD_COMPONENT_ID
            );
        }
    }
    assert_eq!(loads.load(Ordering::SeqCst), 0);
}

#[test]
fn frozen_public_build_output_still_requires_a_guard_and_private_requests_load_live() {
    let (loads, value, mut app) = super::prerender::declared();
    let mode = Arc::new(AtomicUsize::new(2));
    let calls = Arc::new(AtomicUsize::new(0));
    app.guard(guard(mode.clone(), calls.clone())).unwrap();
    let plan = app.client_prerender_plan().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    super::prerender::attach(&mut app, &plan).unwrap();
    let router = app.router(None).unwrap();
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::Forbidden
    );
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    mode.store(0, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 1"
    );
    mode.store(1, Ordering::SeqCst);
    value.store(2, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 3);
    mode.store(0, Ordering::SeqCst);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 1"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
