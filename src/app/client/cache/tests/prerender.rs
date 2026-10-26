use super::*;
use crate::{ClientPrerender, PageParams, PrerenderError};

fn declared() -> (Arc<AtomicUsize>, Arc<AtomicUsize>, App) {
    let (_, loads, value, mut app) = setup();
    app.client_page_server_cache("article", ServerCachePolicy::no_store())
        .unwrap();
    app.client_page_prerender(
        "article",
        Some(vec![
            PageParams::new(&[("id", "a")]).unwrap(),
            PageParams::new(&[("id", "b")]).unwrap(),
        ]),
    )
    .unwrap();
    (loads, value, app)
}
fn attach(app: &mut App, plan: &ClientPrerender) -> Result<(), PrerenderError> {
    app.client_prerendered(Box::leak(plan.encode().into_boxed_slice()))
}

#[test]
fn frozen_ssr_navigation_and_head_do_not_execute_runtime_loaders() {
    let (loads, value, mut app) = declared();
    let plan = app.client_prerender_plan().unwrap();
    assert_eq!(plan.len(), 2);
    assert_eq!(plan.revision(), 123);
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    value.store(2, Ordering::SeqCst);
    attach(&mut app, &plan).unwrap();
    let router = app.router(None).unwrap();
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    for path in ["/articles/a", "/articles/b"] {
        let html = request(&router, path, false, "");
        assert_eq!(html.status(), StatusCode::Ok);
        assert!(html.body_text().unwrap().contains("Description 1"));
        assert!(html.body_text().unwrap().contains("Native fallback"));
        let hex = html
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
        let snapshot = crate::NavigationSnapshot::decode(&bytes).unwrap();
        let nav = request(&router, path, true, "");
        assert_eq!(snapshot.page().encode(), seed(&nav).encode());
        assert_eq!(seed(&nav).title(), "Title 1");
        assert_eq!(seed(&nav).layouts().len(), 1);
        let raw = format!("HEAD {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert_eq!(
            router
                .dispatch(crate::http::parse(raw.as_bytes(), 8192, 32).unwrap())
                .status(),
            StatusCode::Ok
        );
    }
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    app.client_page_streaming("article", true).unwrap();
    let router = app.router(None).unwrap();
    let stream=crate::http::parse(b"GET /articles/a HTTP/1.1\r\nHost: localhost\r\nAccept: application/vnd.ruvane.page-stream\r\nRuvane-Application-Revision: 123\r\nRuvane-Stream-Token: 1\r\n\r\n",8192,32).unwrap();
    let response = router.dispatch(stream);
    assert!(!response.is_streaming());
    assert_eq!(response.content_type(), crate::NAVIGATION_MEDIA_TYPE);
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}

#[test]
fn frozen_navigation_preserves_the_intersection_of_page_and_layout_cache_policies() {
    let (loads, _, mut app) = declared();
    let plan = app.client_prerender_plan().unwrap();
    attach(&mut app, &plan).unwrap();
    app.client_layout_cache("workspace", NavigationCachePolicy::no_store())
        .unwrap();
    assert_eq!(
        request(&app.router(None).unwrap(), "/articles/a", true, "").cache_control(),
        "no-store"
    );
    app.client_layout_cache("workspace", NavigationCachePolicy::public(1).unwrap())
        .unwrap();
    assert_eq!(
        request(&app.router(None).unwrap(), "/articles/a", true, "").cache_control(),
        "public, max-age=1, must-revalidate"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 2);
}

#[test]
fn private_query_and_unlisted_urls_keep_dynamic_ssr() {
    let (loads, value, mut app) = declared();
    let plan = app.client_prerender_plan().unwrap();
    attach(&mut app, &plan).unwrap();
    value.store(2, Ordering::SeqCst);
    let router = app.router(None).unwrap();
    for header in [
        "Cookie: fixture=present\r\n",
        "Cookie:\r\n",
        "Authorization: Bearer fixture\r\n",
        "Authorization:\r\n",
    ] {
        let response = request(&router, "/articles/a", true, header);
        assert_eq!(seed(&response).title(), "Title 999");
        assert_eq!(response.cache_control(), "no-store");
    }
    for path in ["/articles/a?q=x", "/articles/a?", "/articles/c"] {
        assert_eq!(seed(&request(&router, path, true, "")).title(), "Title 2");
    }
    let native = request(&router, "/articles/a?native=yes", false, "");
    assert!(native.body_text().unwrap().contains("Title 2"));
    assert!(!native.body_text().unwrap().contains("<script"));
    assert_eq!(loads.load(Ordering::SeqCst), 10);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 1"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 10);
}

#[test]
fn planning_uses_headerless_get_and_export_context_and_keeps_encoded_captures() {
    let (_, _, mut app) = declared();
    let observations = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = observations.clone();
    app.client_page(
        crate::PageRoute::new("public", "/public/{slug}").unwrap(),
        1,
        zero,
        move |context| {
            let request = context.request().unwrap();
            seen.lock().unwrap().push((
                context.is_exporting(),
                request.header("host").is_none(),
                request.query().is_none(),
                context.param("slug").unwrap().to_owned(),
            ));
            let mut data = ActionData::default();
            data.insert("value", ActionValue::Text("Public".into()))
                .unwrap();
            Ok(ClientPageData::new("Public", data)
                .metadata(context.metadata("Public description").unwrap()))
        },
    )
    .unwrap();
    app.client_page_prerender("article", None).unwrap();
    app.client_page_prerender(
        "public",
        Some(vec![PageParams::new(&[("slug", "a b")]).unwrap()]),
    )
    .unwrap();
    let plan = app.client_prerender_plan().unwrap();
    assert_eq!(plan.urls().next().unwrap().as_str(), "/public/a%20b");
    assert_eq!(
        *observations.lock().unwrap(),
        vec![(true, true, true, "a b".into())]
    );
    attach(&mut app, &plan).unwrap();
    let router = app.router(None).unwrap();
    assert_eq!(
        request(&router, "/public/a%20b", false, "").status(),
        StatusCode::Ok
    );
    assert_eq!(observations.lock().unwrap().len(), 1);
}

#[test]
fn invalid_declarations_and_conflicting_cache_policies_are_atomic() {
    let (loads, _, mut app) = declared();
    for params in [
        vec![],
        vec![PageParams::default()],
        vec![PageParams::new(&[("id", "a")]).unwrap(); 2],
        vec![PageParams::new(&[("id", "a")]).unwrap(); 129],
    ] {
        assert!(app.client_page_prerender("article", Some(params)).is_err());
    }
    assert!(
        app.client_page_server_cache("article", ServerCachePolicy::public(1).unwrap())
            .is_err()
    );
    assert!(app.client_page_prerender("unknown", None).is_err());
    assert!(
        app.page(
            crate::PageRoute::new("shadow", "/articles/a").unwrap(),
            None,
            PageMode::ServerOnly(QueryMode::Uri),
            |_| Ok(Page::new("Shadow", ""))
        )
        .is_err()
    );
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    assert_eq!(app.client_prerender_plan().unwrap().len(), 2);
    let mut registry = crate::FileRouteRegistry::new(&mut app);
    assert!(
        registry
            .page(
                "bad/page.rs",
                "/bad",
                None,
                crate::FilePage::server("bad", PageMode::fixed(), |_| Ok(Page::new("Bad", "")))
                    .prerender(vec![PageParams::default()])
            )
            .is_err()
    );
    assert!(registry.finish().is_err());
    assert!(app.url("bad", &[]).is_err());
}

#[test]
fn loader_errors_no_store_and_invalid_metadata_abort_the_entire_plan() {
    for mode in 0..6 {
        let (_, _, mut app) = declared();
        app.client_page(
            crate::PageRoute::new("bad", "/bad").unwrap(),
            1,
            zero,
            move |context| {
                match mode {
                    0 => return Err(PageError::NotFound),
                    1 => panic!("build fixture panic"),
                    2 => return Ok(ClientPageData::new("Bad", ActionData::default())),
                    _ => {}
                }
                let mut data = ActionData::default();
                data.insert("value", ActionValue::Text("Bad".into()))
                    .unwrap();
                let page = ClientPageData::new("Bad", data);
                Ok(match mode {
                    3 => page.no_store(),
                    4 => page.fallback(crate::text("x".repeat(crate::MAX_STATIC_FILE_BYTES))),
                    _ => page.metadata(
                        crate::Metadata::new().canonical(
                            SiteOrigin::parse("https://foreign.example")
                                .unwrap()
                                .canonical(context.path()),
                        ),
                    ),
                })
            },
        )
        .unwrap();
        app.client_page_prerender("bad", Some(vec![PageParams::default()]))
            .unwrap();
        assert!(app.client_prerender_plan().is_err());
    }
}

#[test]
fn attachment_rejects_wrong_revision_coverage_and_invalid_seed_without_loading() {
    let (loads, _, mut app) = declared();
    let plan = app.client_prerender_plan().unwrap();
    let mut wrong = plan.clone();
    wrong.revision += 1;
    for row in &mut wrong.rows {
        let old = crate::NavigationSnapshot::new(plan.revision(), row.packet.clone())
            .unwrap()
            .html();
        let new = crate::NavigationSnapshot::new(wrong.revision, row.packet.clone())
            .unwrap()
            .html();
        row.html = row.html.replace(old.as_str(), new.as_str());
    }
    assert!(attach(&mut app, &wrong).is_err());
    let mut wrong = plan.clone();
    wrong.rows.pop();
    assert!(attach(&mut app, &wrong).is_err());
    let mut wrong = plan.clone();
    let row = &mut wrong.rows[0];
    let old = crate::NavigationSnapshot::new(123, row.packet.clone())
        .unwrap()
        .html();
    row.packet = NavigationPage::new(
        row.url.clone(),
        "Invalid seed",
        row.packet.props(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap()
    .with_layouts(row.packet.layouts().to_vec())
    .unwrap();
    let new = crate::NavigationSnapshot::new(123, row.packet.clone())
        .unwrap()
        .html();
    row.html = row.html.replace(old.as_str(), new.as_str());
    assert!(attach(&mut app, &wrong).is_err());
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    attach(&mut app, &plan).unwrap();
    app.client_page_prerender("article", None).unwrap();
    assert_eq!(
        seed(&request(
            &app.router(None).unwrap(),
            "/articles/a",
            true,
            ""
        ))
        .title(),
        "Title 1"
    );
    assert_eq!(loads.load(Ordering::SeqCst), 3);
}
