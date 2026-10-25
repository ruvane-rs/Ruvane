use ruvane::{
    ActionData, ActionValue, BrowserTag, ComponentProps, InitialState, InternalUrl,
    NavigationError, NavigationPage, NavigationRoutes, PageRoute, StatusCode, View,
    navigation_link,
};

fn page() -> NavigationPage {
    let mut data = ActionData::default();
    data.insert("message", ActionValue::Text("<script> 🦀\n".into()))
        .unwrap();
    NavigationPage::new(
        InternalUrl::from_target("/app/tasks/42?q=Rust+%26%20HTML").unwrap(),
        "Task <42> / 🦀",
        ComponentProps::new(10, InitialState::Number(42)).unwrap(),
        data,
        StatusCode::Ok,
    )
    .unwrap()
}
#[test]
fn routes_share_capture_decoding_fixed_priority_and_named_url_ownership() {
    let mut routes = NavigationRoutes::new();
    routes
        .register(PageRoute::new("task", "/tasks/{id}").unwrap(), 10)
        .unwrap();
    routes
        .register(PageRoute::new("new", "/tasks/new").unwrap(), 9)
        .unwrap();
    let target = InternalUrl::from_target("/tasks/%F0%9F%A6%80%20%252F?q=a+b&once=%252F").unwrap();
    let matched = routes.resolve(&target).unwrap().unwrap();
    assert_eq!(
        (matched.name(), matched.component(), matched.param("id")),
        ("task", 10, Some("🦀 %2F"))
    );
    assert_eq!(matched.query().get("q"), Some("a+b"));
    assert_eq!(matched.query().get("once"), Some("%2F"));
    assert_eq!(
        routes
            .resolve(&InternalUrl::new("/tasks/new").unwrap())
            .unwrap()
            .unwrap()
            .name(),
        "new"
    );
    assert_eq!(
        routes.url("task", &[("id", "new")]),
        Err(NavigationError::Conflict)
    );
    assert_eq!(
        routes.url("task", &[("id", "🦀 %2F")]).unwrap().as_str(),
        target.as_str().split('?').next().unwrap()
    );
    for (name, path, component) in [
        ("task", "/other", 1),
        ("other", "/tasks/{name}", 1),
        ("other", "/tasks/new", 1),
        ("other", "/zero", 0),
    ] {
        assert!(
            routes
                .register(PageRoute::new(name, path).unwrap(), component)
                .is_err()
        );
    }
    assert!(
        routes
            .resolve(&InternalUrl::new("/missing").unwrap())
            .unwrap()
            .is_none()
    );
}
#[test]
fn route_budget_and_failed_registrations_are_atomic() {
    let mut routes = NavigationRoutes::new();
    for n in 0..128 {
        routes
            .register(
                PageRoute::new(&format!("r{n}"), &format!("/p{n}")).unwrap(),
                1,
            )
            .unwrap();
    }
    assert_eq!(
        routes.register(PageRoute::new("extra", "/extra").unwrap(), 1),
        Err(NavigationError::Limit)
    );
    assert_eq!(routes.url("r127", &[]).unwrap().as_str(), "/p127");
}
#[test]
fn complete_targets_keep_encoding_and_refuse_normalized_or_ambiguous_urls() {
    for target in [
        "/",
        "/tasks/",
        "/tasks?",
        "/tasks?q=a+b&escaped=%252F",
        "/tasks?a=1?2",
    ] {
        assert_eq!(InternalUrl::from_target(target).unwrap().as_str(), target);
    }
    for target in [
        "https://example.com/",
        "//evil/",
        "/a/../b",
        "/a/%2e%2E",
        "/a/%2f",
        "/a/%5C",
        "/a?x=%FF",
        "/a?x=1&%78=2",
        "/a?x=1&",
        "/a#fragment",
        "/a?x=%00",
        "/a\\b",
    ] {
        assert!(InternalUrl::from_target(target).is_err(), "{target}");
    }
    assert_eq!(
        InternalUrl::from_target("/tasks?q=a?")
            .unwrap()
            .query("next", "x")
            .unwrap()
            .as_str(),
        "/tasks?q=a?&next=x"
    );
    let url = InternalUrl::from_target("/tasks?").unwrap();
    assert!(url.clone().segment("1").is_err());
    assert_eq!(url.query("q", "x").unwrap().as_str(), "/tasks?q=x");
}
#[test]
fn page_packets_roundtrip_and_reject_every_truncation_or_trailing_byte() {
    let page = page();
    let bytes = page.encode();
    let decoded = NavigationPage::decode(&bytes).unwrap();
    assert_eq!(decoded.url(), page.url());
    assert_eq!(decoded.title(), page.title());
    assert_eq!(decoded.props(), page.props());
    assert_eq!(decoded.data(), page.data());
    assert_eq!(decoded.status(), page.status());
    for n in 0..bytes.len() {
        assert!(NavigationPage::decode(&bytes[..n]).is_err());
    }
    assert!(NavigationPage::decode(&[bytes.as_slice(), &[0]].concat()).is_err());
    assert!(NavigationPage::decode(&vec![0; 65537]).is_err());
    for title in ["", "   ", "line\nbreak", "\u{85}", &"x".repeat(1025)] {
        assert!(
            NavigationPage::new(
                page.url().clone(),
                title,
                page.props(),
                page.data().clone(),
                StatusCode::Ok
            )
            .is_err()
        );
    }
    assert!(
        NavigationPage::new(
            page.url().clone(),
            "Title",
            page.props(),
            page.data().clone(),
            StatusCode::SeeOther
        )
        .is_err()
    );
    for status in [
        StatusCode::BadRequest,
        StatusCode::NotFound,
        StatusCode::InternalServerError,
    ] {
        let page = NavigationPage::new(
            page.url().clone(),
            "Error",
            page.props(),
            ActionData::default(),
            status,
        )
        .unwrap();
        assert_eq!(
            NavigationPage::decode(&page.encode()).unwrap().status(),
            status
        );
    }
}
#[test]
fn mutated_pages_never_panic_and_decoded_pages_stay_within_the_profile() {
    let bytes = page().encode();
    for index in 0..bytes.len() {
        for value in 0..=255 {
            let mut bytes = bytes.clone();
            bytes[index] = value;
            if let Ok(page) = NavigationPage::decode(&bytes) {
                assert!(page.encode().len() <= 65536);
                assert!(page.title().len() <= 1024);
                assert!(matches!(
                    page.status(),
                    StatusCode::Ok | StatusCode::BadRequest | StatusCode::NotFound
                ));
            }
        }
    }
}
#[test]
fn total_page_budget_counts_metadata_and_seed_with_an_exact_boundary() {
    let mut data = ActionData::default();
    for n in 0..63 {
        data.insert(&format!("k{n:02}"), ActionValue::Text("x".repeat(1024)))
            .unwrap();
    }
    let props = ComponentProps::new(8, InitialState::Number(0)).unwrap();
    let url = InternalUrl::new("/app").unwrap();
    let small = NavigationPage::new(url.clone(), "X", props, data.clone(), StatusCode::Ok).unwrap();
    let length = 65536 - small.encode().len() + 1;
    assert!(length <= 1024);
    let exact = NavigationPage::new(
        url.clone(),
        &"x".repeat(length),
        props,
        data.clone(),
        StatusCode::Ok,
    )
    .unwrap();
    assert_eq!(exact.encode().len(), 65536);
    assert!(NavigationPage::decode(&exact.encode()).is_ok());
    assert_eq!(
        NavigationPage::new(url, &"x".repeat(length + 1), props, data, StatusCode::Ok).unwrap_err(),
        NavigationError::Limit
    );
}
#[test]
fn links_escape_text_and_targets_and_patch_only_the_href_when_it_changes() {
    let url = InternalUrl::from_target("/tasks?q=%22&other=%26").unwrap();
    let html = navigation_link(&url, "<script> & 🦀");
    assert!(html.as_str().contains("q=%22&amp;other=%26"));
    assert!(html.as_str().contains("&lt;script&gt; &amp; 🦀"));
    let make = |url: &InternalUrl| {
        let mut view = View::new();
        let section = view.keyed_element(0, 1, BrowserTag::Section).unwrap();
        view.link(section, 1, url, "Open").unwrap();
        view
    };
    let first = make(&url);
    let snapshot = first.snapshot().unwrap();
    let next = make(&InternalUrl::new("/tasks/42").unwrap());
    let (_, commands) = snapshot.update(&next, 1).unwrap();
    assert_eq!(commands.count(), 1);
    assert_eq!(commands.as_bytes()[8], 12);
    let rendered = first
        .hydratable(ComponentProps::new(1, InitialState::Number(0)).unwrap())
        .unwrap();
    assert!(
        rendered
            .as_str()
            .contains("data-ruvane-nav=\"\" href=\"/tasks?q=%22&amp;other=%26\"")
    );
}
