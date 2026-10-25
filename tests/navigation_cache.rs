use ruvane::{
    ActionData, BrowserTag, ComponentProps, InitialState, InternalUrl, NavigationCachePolicy,
    NavigationPage, Response, StatusCode, View,
};

fn page(status: StatusCode) -> NavigationPage {
    NavigationPage::new(
        InternalUrl::new("/public").unwrap(),
        "Public page",
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        status,
    )
    .unwrap()
}
#[test]
fn navigation_cache_policy_is_explicit_bounded_and_restricted_to_successful_pages() {
    assert_eq!(NavigationCachePolicy::default().max_age_seconds(), None);
    for seconds in [0, 301, u32::MAX] {
        assert!(NavigationCachePolicy::public(seconds).is_err());
    }
    for seconds in [1, 30, 300] {
        let policy = NavigationCachePolicy::public(seconds).unwrap();
        assert_eq!(policy.max_age_seconds(), Some(seconds));
        assert_eq!(
            Response::navigation_with_cache(&page(StatusCode::Ok), policy).cache_control(),
            format!("public, max-age={seconds}, must-revalidate")
        );
    }
    assert_eq!(
        Response::navigation(&page(StatusCode::Ok)).cache_control(),
        "no-store"
    );
    for status in [StatusCode::BadRequest, StatusCode::NotFound] {
        assert_eq!(
            Response::navigation_with_cache(
                &page(status),
                NavigationCachePolicy::public(30).unwrap()
            )
            .cache_control(),
            "no-store"
        );
    }
    let public = Response::navigation_with_cache(
        &page(StatusCode::Ok),
        NavigationCachePolicy::public(30).unwrap(),
    );
    assert_eq!(
        public
            .with_status(StatusCode::InternalServerError)
            .cache_control(),
        "no-store"
    );
    assert_eq!(Response::text("value").cache_control(), "no-store");
}
#[test]
fn marked_links_escape_urls_and_text_and_keep_prefetch_flags_in_the_typed_view() {
    let url = InternalUrl::from_target("/public?q=%22&other=%26").unwrap();
    let html = ruvane::prefetch_link(&url, "<script> & Rust");
    assert!(html.as_str().contains("data-ruvane-prefetch=\"\""));
    assert!(html.as_str().contains("q=%22&amp;other=%26"));
    assert!(html.as_str().contains("&lt;script&gt; &amp; Rust"));
    let make = |prefetch: bool| {
        let mut view = View::new();
        let root = view.keyed_element(0, 1, BrowserTag::Section).unwrap();
        if prefetch {
            view.prefetch_link(root, 1, &url, "Open").unwrap();
        } else {
            view.link(root, 1, &url, "Open").unwrap();
        }
        view
    };
    let ordinary = make(false);
    let marked = make(true);
    let snapshot = ordinary.snapshot().unwrap();
    let (next, commands) = snapshot.update(&marked, 1).unwrap();
    assert_eq!(commands.count(), 1);
    assert_eq!(commands.as_bytes()[8], 13);
    let (_, commands) = next.update(&ordinary, 1).unwrap();
    assert_eq!(commands.count(), 1);
    assert_eq!(commands.as_bytes().last(), Some(&0));
    assert!(
        marked
            .render()
            .unwrap()
            .as_str()
            .contains("data-ruvane-prefetch=\"\"")
    );
    assert!(
        !ordinary
            .render()
            .unwrap()
            .as_str()
            .contains("data-ruvane-prefetch")
    );
}
