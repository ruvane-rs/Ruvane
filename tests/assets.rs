use ruvane::{
    AssetError, Document, ImageError, InternalUrl, MAX_ASSET_BYTES, MAX_ASSETS,
    MAX_IMAGE_DIMENSION, MAX_TOTAL_ASSET_BYTES, MediaType, Response, RouteError, Router,
    StatusCode, Tag, element, image,
};

static ASSET_BYTES: [u8; MAX_ASSET_BYTES + 1] = [0xa5; MAX_ASSET_BYTES + 1];

#[test]
fn binary_responses_preserve_every_byte_and_offer_fallible_utf8() {
    let bytes: Vec<_> = (0..=255).collect();
    let response =
        Response::bytes(bytes.clone(), MediaType::OctetStream).with_status(StatusCode::NotFound);
    assert_eq!(response.body(), bytes);
    assert_eq!(response.clone().body(), bytes);
    assert!(response.body_text().is_err());
    assert_eq!(response.content_type(), "application/octet-stream");
    assert_eq!(response.status(), StatusCode::NotFound);
    let text = Response::text("Été 🦀");
    assert_eq!(text.body(), "Été 🦀".as_bytes());
    assert_eq!(text.body_text().unwrap(), "Été 🦀");
    let html = Response::html(element(Tag::P).child("<text>").render());
    assert_eq!(html.body_text().unwrap(), "<p>&lt;text&gt;</p>");
    assert_eq!(
        Response::bytes(Vec::new(), MediaType::PlainText)
            .body_text()
            .unwrap(),
        ""
    );
}

#[test]
fn mime_types_are_fixed_values_without_filename_inference() {
    for (media, expected) in [
        (MediaType::PlainText, "text/plain; charset=utf-8"),
        (MediaType::Css, "text/css; charset=utf-8"),
        (MediaType::Png, "image/png"),
        (MediaType::Jpeg, "image/jpeg"),
        (MediaType::WebP, "image/webp"),
        (MediaType::OctetStream, "application/octet-stream"),
    ] {
        assert_eq!(media.as_str(), expected);
        assert_eq!(Response::bytes(vec![0], media).content_type(), expected);
        assert!(!expected.contains(['\r', '\n']));
    }
}

#[test]
fn assets_reject_invalid_internal_paths_and_share_the_get_namespace() {
    let mut router = Router::new();
    for path in [
        "",
        "relative",
        "https://evil.test/a",
        "//evil.test/a",
        "/a?x=1",
        "/a#b",
        "/a/{name}",
        "/../secret",
        "/a/%2e%2e/b",
        "/a%2fb",
        "/%5Cevil",
        "/%FF",
        "/a\r\nHeader:x",
    ] {
        assert_eq!(
            router.asset(path, b"data", MediaType::PlainText),
            Err(AssetError::InvalidPath),
            "{path:?}"
        );
    }
    router
        .get("/existing", |_| Response::text("route"))
        .unwrap();
    assert_eq!(
        router.asset("/existing", b"data", MediaType::PlainText),
        Err(AssetError::DuplicatePath)
    );
    router
        .asset("/assets/data", b"data", MediaType::PlainText)
        .unwrap();
    assert_eq!(
        router.asset("/assets/data", b"other", MediaType::Css),
        Err(AssetError::DuplicatePath)
    );
    assert_eq!(
        router.get("/assets/data", |_| Response::text("other")),
        Err(RouteError::DuplicatePath)
    );
}

#[test]
fn individual_and_total_asset_byte_limits_are_exact_and_atomic() {
    let mut router = Router::new();
    assert_eq!(
        router.asset("/too-large", &ASSET_BYTES, MediaType::OctetStream),
        Err(AssetError::TooLarge)
    );
    let maximum = &ASSET_BYTES[..MAX_ASSET_BYTES];
    for index in 0..MAX_TOTAL_ASSET_BYTES / MAX_ASSET_BYTES {
        router
            .asset(format!("/large-{index}"), maximum, MediaType::OctetStream)
            .unwrap();
    }
    assert_eq!(
        router.asset("/overflow", b"x", MediaType::PlainText),
        Err(AssetError::TotalTooLarge)
    );
    // The failed registrations did not reserve a route or consume byte budget.
    router
        .asset("/overflow", b"", MediaType::PlainText)
        .unwrap();
    router
        .asset("/too-large", b"", MediaType::PlainText)
        .unwrap();
}

#[test]
fn asset_count_limit_counts_empty_assets_and_failures_do_not_reserve_paths() {
    let mut router = Router::new();
    for index in 0..MAX_ASSETS {
        router
            .asset(format!("/asset-{index}"), b"", MediaType::Css)
            .unwrap();
    }
    assert_eq!(
        router.asset("/extra", b"", MediaType::Css),
        Err(AssetError::TooManyAssets)
    );
    router
        .get("/extra", |_| Response::text("still available"))
        .unwrap();
}

#[test]
fn stylesheets_are_ordered_deduplicated_and_attribute_escaped() {
    let first = InternalUrl::new("/assets/a'b.css")
        .unwrap()
        .query("x", "<")
        .unwrap()
        .query("y", "\"")
        .unwrap();
    let second = InternalUrl::new("/assets/second.css").unwrap();
    let page = Document::new("<title>", "body")
        .stylesheet(first.clone())
        .stylesheet(second)
        .stylesheet(first)
        .render();
    let html = page.as_str();
    let expected = "<link rel=\"stylesheet\" href=\"/assets/a&#39;b.css?x=%3C&amp;y=%22\"><link rel=\"stylesheet\" href=\"/assets/second.css\">";
    assert!(html.contains(expected));
    assert!(html.find(expected).unwrap() < html.find("</head>").unwrap());
    assert_eq!(html.matches("<link ").count(), 2);
    assert!(!html.contains("</link>"));
    assert!(html.contains("<title>&lt;title&gt;</title>"));
}

#[test]
fn images_escape_sources_and_alternatives_without_creating_attributes() {
    let source = InternalUrl::new("/assets/a'b.png")
        .unwrap()
        .query("a", "1")
        .unwrap()
        .query("b", "2")
        .unwrap();
    let html = image(&source, "\" onerror=\"x\"><script>&\0", 960, 480).unwrap();
    assert_eq!(
        html.as_str(),
        "<img src=\"/assets/a&#39;b.png?a=1&amp;b=2\" alt=\"&quot; onerror=&quot;x&quot;&gt;&lt;script&gt;&amp;�\" width=\"960\" height=\"480\">"
    );
    assert!(!html.as_str().contains("<script>"));
    assert!(!html.as_str().contains("</img>"));
    assert_eq!(
        image(&InternalUrl::new("/").unwrap(), "", 1, 1)
            .unwrap()
            .as_str(),
        "<img src=\"/\" alt=\"\" width=\"1\" height=\"1\">"
    );
}

#[test]
fn image_dimensions_reject_zero_and_excessive_values() {
    let source = InternalUrl::new("/image.png").unwrap();
    for (width, height) in [(0, 1), (1, 0), (MAX_IMAGE_DIMENSION + 1, 1), (1, u32::MAX)] {
        assert_eq!(image(&source, "image", width, height), Err(ImageError));
    }
    assert!(image(&source, "image", MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION).is_ok());
}
