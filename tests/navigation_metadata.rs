use ruvane::{
    ActionData, ComponentProps, InitialState, InternalUrl, Metadata, NavigationPage, Robots,
    SiteOrigin, StatusCode,
};
fn page(status: StatusCode) -> NavigationPage {
    NavigationPage::new(
        InternalUrl::new("/meta").unwrap(),
        "Metadata page",
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        status,
    )
    .unwrap()
}
fn metadata() -> Metadata {
    Metadata::new()
        .description("Rust <metadata> & 🦀")
        .unwrap()
        .robots(Robots::IndexNoFollow)
        .canonical(
            SiteOrigin::parse("https://public.example")
                .unwrap()
                .canonical(&InternalUrl::new("/meta").unwrap()),
        )
        .favicon_png(InternalUrl::new("/icon.png").unwrap())
}
#[test]
fn complete_metadata_round_trips_and_legacy_packets_remain_unchanged() {
    let legacy = page(StatusCode::Ok);
    assert!(legacy.encode().starts_with(b"RVP1"));
    assert!(
        NavigationPage::decode(&legacy.encode())
            .unwrap()
            .metadata()
            .is_none()
    );
    let expected = metadata();
    let page = legacy.with_metadata(expected.clone()).unwrap();
    let bytes = page.encode();
    assert!(bytes.starts_with(b"RVP5"));
    let decoded = NavigationPage::decode(&bytes).unwrap();
    assert_eq!(decoded.metadata(), Some(&expected));
    assert_eq!(decoded.encode(), bytes);
    assert_eq!(
        decoded
            .metadata()
            .unwrap()
            .canonical_url()
            .unwrap()
            .as_str(),
        "https://public.example/meta"
    );
}
#[test]
fn metadata_envelopes_reject_truncation_nested_frames_flags_utf8_and_trailing_bytes() {
    let bytes = page(StatusCode::Ok)
        .with_metadata(metadata())
        .unwrap()
        .encode();
    for end in 0..bytes.len() {
        assert!(NavigationPage::decode(&bytes[..end]).is_err(), "{end}");
    }
    let mut bad = bytes.clone();
    bad[11] = b'5';
    assert!(NavigationPage::decode(&bad).is_err());
    let offset = 8 + u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let mut bad = bytes.clone();
    bad[offset] = 16;
    assert!(NavigationPage::decode(&bad).is_err());
    let mut bad = bytes.clone();
    bad[offset + 5] = 255;
    assert!(NavigationPage::decode(&bad).is_err());
    let mut bad = bytes.clone();
    bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(NavigationPage::decode(&bad).is_err());
    let mut bad = bytes.clone();
    bad.push(0);
    assert!(NavigationPage::decode(&bad).is_err());
    for byte in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[byte] ^= 127;
        let _ = NavigationPage::decode(&bad);
    }
}
#[test]
fn errors_sanitize_canonical_and_indexing_and_pending_prefixes_cannot_carry_metadata() {
    for status in [
        StatusCode::BadRequest,
        StatusCode::NotFound,
        StatusCode::InternalServerError,
    ] {
        let page = page(status).with_metadata(metadata()).unwrap();
        let metadata = page.metadata().unwrap();
        assert!(metadata.canonical_url().is_none());
        assert_eq!(metadata.robots_policy(), Some(Robots::NoIndexNoFollow));
        let mut bytes = page.encode();
        let offset = 8 + u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let text = u32::from_le_bytes(bytes[offset + 1..offset + 5].try_into().unwrap()) as usize;
        bytes[offset + 5 + text] = 1;
        assert!(NavigationPage::decode(&bytes).is_err());
    }
    assert!(
        page(StatusCode::Ok)
            .with_pending_layouts(0, Vec::new())
            .unwrap()
            .with_metadata(Metadata::new())
            .is_err()
    );
    assert!(
        page(StatusCode::Ok)
            .with_metadata(Metadata::new())
            .unwrap()
            .with_pending_layouts(0, Vec::new())
            .is_err()
    );
}
#[test]
fn metadata_and_full_packets_keep_their_independent_byte_budgets() {
    assert_eq!(ruvane::MAX_NAVIGATION_METADATA_BYTES, 8192);
    let path = InternalUrl::new(&format!("/{}", vec!["x".repeat(1000); 4].join("/"))).unwrap();
    let oversized = Metadata::new()
        .description("d".repeat(1024))
        .unwrap()
        .canonical(
            SiteOrigin::parse("https://public.example")
                .unwrap()
                .canonical(&path),
        )
        .favicon_png(path);
    assert!(page(StatusCode::Ok).with_metadata(oversized).is_err());
}
