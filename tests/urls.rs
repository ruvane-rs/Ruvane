use ruvane::{
    InternalUrl, MAX_COMPONENT_BYTES, MAX_INTERNAL_URL_BYTES, MAX_QUERY_PAIRS, ParamError, Query,
    Tag, UrlError, element, link,
};

#[test]
fn query_splits_before_decoding_and_preserves_plus_and_equals() {
    let query = Query::parse("x=a%26b%3Dc&plus=a+b&flag&empty=&equals=a=b&once=%2526").unwrap();
    assert_eq!(query.get("x"), Some("a&b=c"));
    assert_eq!(query.get("plus"), Some("a+b"));
    assert_eq!(query.get("flag"), Some(""));
    assert_eq!(query.get("empty"), Some(""));
    assert_eq!(query.get("equals"), Some("a=b"));
    assert_eq!(query.get("once"), Some("%26"));
    assert_eq!(query.iter().next(), Some(("x", "a&b=c")));
    assert_eq!(query.len(), 6);
    assert!(Query::parse("").unwrap().is_empty());
}

#[test]
fn queries_reject_duplicate_decoded_keys_and_empty_fields() {
    for raw in ["a=1&a=2", "a=1&%61=2", "%C3%A9=1&%c3%a9=2"] {
        assert_eq!(Query::parse(raw), Err(UrlError::DuplicateKey), "{raw}");
    }
    for raw in ["&", "a=1&", "a=1&&b=2", "=value"] {
        assert!(Query::parse(raw).is_err(), "{raw}");
    }
    assert_eq!(Query::parse("A=1&a=2").unwrap().len(), 2);
}

#[test]
fn typed_values_distinguish_missing_invalid_overflow_and_empty() {
    let query = Query::parse("page=2&large=256&bad=secret&empty=&enabled=true").unwrap();
    assert_eq!(query.get_as::<usize>("page"), Ok(2));
    assert_eq!(query.get_as::<bool>("enabled"), Ok(true));
    assert_eq!(query.get_as::<u8>("large"), Err(ParamError::Invalid));
    assert_eq!(query.get_as::<usize>("bad"), Err(ParamError::Invalid));
    assert_eq!(query.get_as::<usize>("empty"), Err(ParamError::Invalid));
    assert_eq!(query.get_as::<usize>("absent"), Err(ParamError::Missing));
    assert!(
        !query
            .get_as::<u8>("bad")
            .unwrap_err()
            .to_string()
            .contains("secret")
    );
}

#[test]
fn malformed_escapes_utf8_and_controls_are_rejected() {
    for value in [
        "%",
        "%0",
        "%GG",
        "%FF",
        "%C0%AF",
        "%ED%A0%80",
        "%F4%90%80%80",
        "%00",
        "%0D%0A",
        "%7f",
        "%C2%85",
        "é",
        "a b",
    ] {
        assert!(Query::parse(&format!("q={value}")).is_err(), "{value}");
    }
    assert_eq!(
        Query::parse("q=%C3%A9%F0%9F%A6%80").unwrap().get("q"),
        Some("é🦀")
    );
}

#[test]
fn query_count_and_decoded_byte_budgets_have_exact_boundaries() {
    let pairs = (0..MAX_QUERY_PAIRS)
        .map(|i| format!("k{i}=v"))
        .collect::<Vec<_>>()
        .join("&");
    assert_eq!(Query::parse(&pairs).unwrap().len(), MAX_QUERY_PAIRS);
    assert_eq!(
        Query::parse(&(pairs + "&extra=v")),
        Err(UrlError::TooManyPairs)
    );
    let maximum = "x".repeat(MAX_COMPONENT_BYTES);
    assert!(Query::parse(&format!("q={maximum}")).is_ok());
    assert_eq!(
        Query::parse(&format!("q={maximum}x")),
        Err(UrlError::TooLarge)
    );
    assert!(Query::parse(&format!("{maximum}=v")).is_ok());
    assert!(Query::parse(&format!("{maximum}x=v")).is_err());
    let prefix = format!("a={maximum}&b={maximum}&c={maximum}&d=");
    let exact = format!(
        "{prefix}{}",
        "d".repeat(MAX_INTERNAL_URL_BYTES - prefix.len())
    );
    assert!(Query::parse(&exact).is_ok());
    assert_eq!(Query::parse(&(exact + "d")), Err(UrlError::TooLarge));
}

#[test]
fn internal_urls_reject_schemes_authorities_and_browser_path_aliases() {
    for path in [
        "",
        "relative",
        "javascript:alert(1)",
        "https://evil.test/",
        "//evil.test/",
        "/\\evil.test",
        "/%5cevil.test",
        "/a%2fb",
        "/a/%2e%2E/b",
        "/./",
        "/../",
        "/a//b",
        "/?q=1",
        "/#fragment",
        "/%00",
        "/%FF",
        "/%C0%AF",
        "/\r\nHost:evil",
    ] {
        assert!(InternalUrl::new(path).is_err(), "{path:?}");
    }
    for path in ["/", "/articles/", "/articles/%C3%A9t%C3%A9", "/%61", "/x:y"] {
        assert_eq!(InternalUrl::new(path).unwrap().as_str(), path);
    }
}

#[test]
fn link_builder_encodes_data_then_escapes_html_attributes_and_text() {
    let url = InternalUrl::new("/articles")
        .unwrap()
        .segment("été & Rust")
        .unwrap()
        .query("search", "\"><script>&#x22;")
        .unwrap()
        .query("page", "2")
        .unwrap();
    assert_eq!(
        url.as_str(),
        "/articles/%C3%A9t%C3%A9%20%26%20Rust?search=%22%3E%3Cscript%3E%26%23x22%3B&page=2"
    );
    let html = link(&url, "<img onerror='x'>");
    assert!(html.as_str().contains("&amp;page=2\""));
    assert!(html.as_str().contains("&lt;img onerror=&#39;x&#39;&gt;"));
    assert!(!html.as_str().contains("<script>"));
    assert!(!html.as_str().contains("<img"));
    assert_eq!(
        link(
            &InternalUrl::new("/").unwrap(),
            element(Tag::Strong).child("Home & Rust")
        )
        .as_str(),
        "<a href=\"/\"><strong>Home &amp; Rust</strong></a>"
    );
    let quoted = InternalUrl::new("/a'b").unwrap();
    assert_eq!(link(&quoted, "x").as_str(), "<a href=\"/a&#39;b\">x</a>");
}

#[test]
fn percent_data_is_encoded_and_decoded_exactly_once() {
    let url = InternalUrl::new("/")
        .unwrap()
        .segment("%2F")
        .unwrap()
        .query("value", "%26")
        .unwrap();
    assert_eq!(url.as_str(), "/%252F?value=%2526");
    assert_eq!(
        Query::parse(url.as_str().split_once('?').unwrap().1)
            .unwrap()
            .get("value"),
        Some("%26")
    );
}

#[test]
fn building_urls_enforces_order_uniqueness_and_component_limits() {
    let url = InternalUrl::new("/articles")
        .unwrap()
        .query("page", "2")
        .unwrap();
    assert_eq!(
        url.clone().segment("late"),
        Err(UrlError::QueryAlreadyStarted)
    );
    assert_eq!(url.query("page", "3"), Err(UrlError::DuplicateKey));
    for segment in ["", ".", "..", "/", "a/b", "a\\b", "\0", "\u{85}"] {
        assert!(
            InternalUrl::new("/").unwrap().segment(segment).is_err(),
            "{segment:?}"
        );
    }
    assert!(InternalUrl::new("/").unwrap().query("", "value").is_err());
    assert!(InternalUrl::new("/").unwrap().query("key", "\r\n").is_err());
    assert!(
        InternalUrl::new("/")
            .unwrap()
            .segment(&"x".repeat(MAX_COMPONENT_BYTES))
            .is_ok()
    );
    assert_eq!(
        InternalUrl::new("/")
            .unwrap()
            .segment(&"x".repeat(MAX_COMPONENT_BYTES + 1)),
        Err(UrlError::TooLarge)
    );
    let exact = format!(
        "/{}/{}/{}/{}",
        "x".repeat(1023),
        "x".repeat(1023),
        "x".repeat(1023),
        "x".repeat(1023)
    );
    assert_eq!(exact.len(), MAX_INTERNAL_URL_BYTES);
    let url = InternalUrl::new(&exact).unwrap();
    assert_eq!(url.clone().segment("x"), Err(UrlError::TooLarge));
    assert_eq!(url.query("q", "v"), Err(UrlError::TooLarge));
    let mut url = InternalUrl::new("/").unwrap();
    for i in 0..MAX_QUERY_PAIRS {
        url = url.query(&format!("k{i}"), "v").unwrap();
    }
    assert_eq!(url.query("extra", "v"), Err(UrlError::TooManyPairs));
}

#[test]
fn url_query_roundtrips_printable_ascii_unicode_and_percent_data() {
    let ascii: String = (32..=126).map(char::from).collect();
    for value in [
        ascii.as_str(),
        "é🦀日本語",
        "%252F",
        "a+b c&d=e",
        "\"'><script>",
    ] {
        let url = InternalUrl::new("/").unwrap().query("key", value).unwrap();
        let parsed = Query::parse(url.as_str().split_once('?').unwrap().1).unwrap();
        assert_eq!(parsed.get("key"), Some(value));
    }
}

#[test]
fn deterministic_query_mutations_do_not_panic() {
    let seed = b"key=hello%20world&other=42";
    for position in 0..seed.len() {
        for byte in 0..=127 {
            let mut input = seed.to_vec();
            input[position] = byte;
            let raw = std::str::from_utf8(&input).unwrap();
            let _ = Query::parse(raw);
        }
    }
    for byte in 0..=255 {
        let _ = Query::parse(&format!("value=%{byte:02X}"));
    }
}
