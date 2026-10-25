use ruvane::{
    Document, InternalUrl, MAX_INTERNAL_URL_BYTES, MAX_METADATA_TEXT_BYTES, MAX_SITE_ORIGIN_BYTES,
    Metadata, MetadataError, OriginError, Robots, SiteOrigin,
};

fn origin() -> SiteOrigin {
    SiteOrigin::parse("https://journal.example").unwrap()
}

#[test]
fn origins_normalize_only_the_supported_http_authority_profile() {
    for (raw, expected) in [
        ("https://EXAMPLE.COM:443/", "https://example.com"),
        ("http://localhost:80", "http://localhost"),
        ("https://example.com:80", "https://example.com:80"),
        ("http://localhost:00081/", "http://localhost:81"),
        ("http://127.0.0.1:3000", "http://127.0.0.1:3000"),
        ("https://192.0.2.1/", "https://192.0.2.1"),
        ("http://[0:0:0:0:0:0:0:1]:3000/", "http://[::1]:3000"),
        ("https://[2001:db8::1]:65535", "https://[2001:db8::1]:65535"),
        ("https://a-b.example:1", "https://a-b.example:1"),
    ] {
        let origin = SiteOrigin::parse(raw).unwrap();
        assert_eq!(origin.as_str(), expected);
        assert_eq!(origin.to_string(), expected);
        assert_eq!(SiteOrigin::parse(expected).unwrap(), origin);
    }
}

#[test]
fn origins_reject_credentials_paths_controls_and_ambiguous_hosts() {
    for raw in [
        "",
        "example.com",
        "//example.com",
        "HTTPS://example.com",
        "javascript:evil",
        "file:///tmp",
        "https://",
        "https:///",
        "https://example.com//",
        "https://example.com/path",
        "https://example.com?x=1",
        "https://example.com/#x",
        "https://user:pass@example.com",
        "https://example.com@evil.test",
        "https://example.com\\evil",
        "https://%65xample.com",
        "https://λ.example",
        "https://xn--bcher-kva.example",
        "https://XN--a.example",
        "https://example.com.",
        "https://a..example",
        "https://-a.example",
        "https://a-.example",
        "https://a_b.example",
        "https://a.1example",
        "https://127.1",
        "https://2130706433",
        "https://0x7f000001",
        "https://0177.0.0.1",
        "https://127.000.0.1",
        "https://example.0x1",
        "https://[::1",
        "https://[]",
        "https://[::1]evil",
        "https://[::1]::80",
        "https://::1",
        "https://[fe80::1%25en0]",
        "https://example.com:",
        "https://example.com:0",
        "https://example.com:65536",
        "https://example.com:+80",
        "https://example.com:1:2",
        "https://example.com:000080",
        "https://example.com\r\nX-Evil: yes",
        "https://example.com\0",
        " https://example.com",
        "https://example.com ",
        "https://example.com\t",
        "https://[::1]#x",
    ] {
        let error = SiteOrigin::parse(raw).unwrap_err();
        assert_eq!(error, OriginError, "{raw:?}");
        assert_eq!(error.to_string(), "invalid site origin");
    }
}

#[test]
fn origin_dns_label_and_total_length_budgets_are_exact() {
    let label = "a".repeat(63);
    assert!(SiteOrigin::parse(&format!("https://{label}.example")).is_ok());
    assert!(SiteOrigin::parse(&format!("https://{label}a.example")).is_err());
    let host = format!("{label}.{label}.{label}.{}", "a".repeat(61));
    assert_eq!(host.len(), 253);
    assert!(SiteOrigin::parse(&format!("https://{host}:65535/")).is_ok());
    assert!(SiteOrigin::parse(&format!("https://{host}a")).is_err());
    assert!(SiteOrigin::parse(&"x".repeat(MAX_SITE_ORIGIN_BYTES + 1)).is_err());
}

#[test]
fn canonical_urls_preserve_encoded_data_and_do_not_reinterpret_paths() {
    let path = InternalUrl::new("/articles")
        .unwrap()
        .segment("🦀 & %2F")
        .unwrap()
        .query("page", "2")
        .unwrap()
        .query("q", "a+b c&d=e")
        .unwrap();
    let canonical = origin().canonical(&path);
    assert_eq!(
        canonical.as_str(),
        "https://journal.example/articles/%F0%9F%A6%80%20%26%20%252F?page=2&q=a%2Bb%20c%26d%3De"
    );
    assert_eq!(canonical.to_string(), canonical.as_str());
    let html = Document::new("Canonical", "")
        .metadata(Metadata::new().canonical(canonical))
        .render();
    assert!(html.as_str().contains("?page=2&amp;q=a%2Bb%20c%26d%3De\""));
    for path in [
        "//evil.test",
        "/\\evil.test",
        "/%5Cevil.test",
        "/%2e%2e",
        "/?x=1",
        "/#x",
    ] {
        assert!(InternalUrl::new(path).is_err(), "{path}");
    }
}

#[test]
fn canonical_url_combination_remains_bounded_at_the_internal_url_limit() {
    let path = format!(
        "/{}/{}/{}/{}",
        "a".repeat(1023),
        "b".repeat(1023),
        "c".repeat(1023),
        "d".repeat(1023)
    );
    assert_eq!(path.len(), MAX_INTERNAL_URL_BYTES);
    let url = origin().canonical(&InternalUrl::new(&path).unwrap());
    assert_eq!(
        url.as_str().len(),
        origin().as_str().len() + MAX_INTERNAL_URL_BYTES
    );
    assert!(url.as_str().len() <= MAX_SITE_ORIGIN_BYTES + MAX_INTERNAL_URL_BYTES);
}

#[test]
fn metadata_fields_render_once_in_a_fixed_order_and_allow_zoom() {
    let metadata = Metadata::new()
        .description("Rust & the web")
        .unwrap()
        .robots(Robots::IndexFollow)
        .canonical(origin().canonical(&InternalUrl::new("/articles").unwrap()))
        .favicon_png(InternalUrl::new("/assets/mark.png").unwrap());
    let html = Document::new("Journal", "Body").metadata(metadata).render();
    assert_eq!(
        html.as_str(),
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Journal</title><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"description\" content=\"Rust &amp; the web\"><meta name=\"robots\" content=\"index, follow\"><link rel=\"canonical\" href=\"https://journal.example/articles\"><link rel=\"icon\" type=\"image/png\" href=\"/assets/mark.png\"></head><body>Body</body></html>"
    );
    assert!(!html.as_str().contains("user-scalable"));
    assert!(!html.as_str().contains("maximum-scale"));
    assert!(!html.as_str().contains("</meta>"));
    assert!(!html.as_str().contains("</link>"));
}

#[test]
fn last_setter_wins_without_duplicate_metadata_or_order_changes() {
    let a = InternalUrl::new("/a").unwrap();
    let b = InternalUrl::new("/b").unwrap();
    let first = Metadata::new()
        .description("first")
        .unwrap()
        .robots(Robots::IndexFollow)
        .canonical(origin().canonical(&a))
        .favicon_png(a);
    let second = first
        .description("second")
        .unwrap()
        .robots(Robots::NoIndexFollow)
        .canonical(origin().canonical(&b))
        .favicon_png(b.clone());
    let expected = Metadata::new()
        .favicon_png(b.clone())
        .canonical(origin().canonical(&b))
        .robots(Robots::NoIndexFollow)
        .description("second")
        .unwrap();
    let html = Document::new("x", "").metadata(second).render();
    assert_eq!(html, Document::new("x", "").metadata(expected).render());
    for field in [
        "name=\"viewport\"",
        "name=\"description\"",
        "name=\"robots\"",
        "rel=\"canonical\"",
        "rel=\"icon\"",
    ] {
        assert_eq!(html.as_str().matches(field).count(), 1);
    }
    let replaced = Document::new("x", "")
        .metadata(Metadata::new().description("old").unwrap())
        .metadata(Metadata::new())
        .render();
    assert!(!replaced.as_str().contains("name=\"description\""));
    assert_eq!(replaced.as_str().matches("name=\"viewport\"").count(), 1);
}

#[test]
fn descriptions_and_metadata_urls_cannot_add_head_markup_or_attributes() {
    let payload = "\"><script>alert('x')</script></head>&lt;meta&gt;";
    let url = InternalUrl::new("/a'b")
        .unwrap()
        .query("q", payload)
        .unwrap();
    let metadata = Metadata::new()
        .description(payload)
        .unwrap()
        .favicon_png(url.clone())
        .canonical(origin().canonical(&url));
    let html = Document::new("</title><script>bad</script>", "safe")
        .metadata(metadata)
        .render();
    assert!(!html.as_str().contains("<script>"));
    assert_eq!(html.as_str().matches("</head>").count(), 1);
    assert_eq!(html.as_str().matches("<meta ").count(), 3);
    assert_eq!(html.as_str().matches("<link ").count(), 2);
    assert!(html.as_str().contains("content=\"&quot;&gt;&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&lt;/head&gt;&amp;lt;meta&amp;gt;\""));
    assert!(
        html.as_str()
            .contains("href=\"https://journal.example/a&#39;b?q=%22%3E")
    );
    assert!(!html.as_str().contains("http-equiv"));
}

#[test]
fn description_limits_are_utf8_bytes_and_reject_controls_or_empty_text() {
    for value in ["", " ", "\u{2003}"] {
        assert_eq!(
            Metadata::new().description(value).unwrap_err(),
            MetadataError::EmptyDescription
        );
    }
    for value in ["\0", "line\nline", "\r\n", "\t", "\u{7f}", "\u{85}"] {
        assert_eq!(
            Metadata::new().description(value).unwrap_err(),
            MetadataError::InvalidText
        );
    }
    for exact in [
        "x".repeat(MAX_METADATA_TEXT_BYTES),
        "λ".repeat(MAX_METADATA_TEXT_BYTES / 2),
    ] {
        assert!(Metadata::new().description(&exact).is_ok());
        let error = Metadata::new().description(exact + "x").unwrap_err();
        assert_eq!(error, MetadataError::TooLarge);
        assert_eq!(error.to_string(), "page description byte limit exceeded");
    }
}

#[test]
fn robots_policies_emit_only_consistent_fixed_tokens() {
    for (robots, expected) in [
        (Robots::IndexFollow, "index, follow"),
        (Robots::IndexNoFollow, "index, nofollow"),
        (Robots::NoIndexFollow, "noindex, follow"),
        (Robots::NoIndexNoFollow, "noindex, nofollow"),
    ] {
        let html = Document::new("x", "")
            .metadata(Metadata::new().robots(robots))
            .render();
        assert!(
            html.as_str()
                .contains(&format!("name=\"robots\" content=\"{expected}\""))
        );
    }
    let default = Document::new("x", "").render();
    for optional in [
        "name=\"robots\"",
        "name=\"description\"",
        "rel=\"canonical\"",
        "rel=\"icon\"",
    ] {
        assert!(!default.as_str().contains(optional));
    }
}

#[test]
fn deterministic_origin_mutations_do_not_panic_and_accepted_origins_are_stable() {
    for seed in [b"https://example.com:443/".as_slice(), b"http://[::1]:3000"] {
        for position in 0..seed.len() {
            for byte in 0..=127 {
                let mut input = seed.to_vec();
                input[position] = byte;
                let raw = std::str::from_utf8(&input).unwrap();
                if let Ok(origin) = SiteOrigin::parse(raw) {
                    assert_eq!(SiteOrigin::parse(origin.as_str()).unwrap(), origin);
                    let canonical = origin.canonical(&InternalUrl::new("/a").unwrap());
                    assert!(canonical.as_str().ends_with("/a"));
                }
            }
        }
    }
}
