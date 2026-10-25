use ruvane::*;
fn prefix(count: usize) -> NavigationPage {
    let props = ComponentProps::new(2, InitialState::Number(0)).unwrap();
    NavigationPage::new(
        InternalUrl::new("/tasks/project").unwrap(),
        "Project <&🦀",
        props,
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap()
    .with_pending_layouts(
        2,
        (0..count)
            .map(|n| {
                NavigationLayout::new(
                    &format!("layout{n}"),
                    InternalUrl::new("/tasks").unwrap(),
                    props,
                    ActionData::default(),
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
#[test]
fn layout_snapshots_round_trip_ordered_prefixes_and_exact_revisions() {
    for revision in [0, 1, 9_007_199_254_740_993, u64::MAX] {
        for count in 1..=2 {
            let snapshot = LayoutSnapshot::new(revision, prefix(count)).unwrap();
            let bytes = snapshot.encode();
            assert!(bytes.starts_with(b"RVH1"));
            let decoded = LayoutSnapshot::decode(&bytes).unwrap();
            assert_eq!(decoded.revision(), revision);
            assert_eq!(decoded.page().layouts().len(), count);
            assert_eq!(decoded.page().pending_layouts(), Some(2));
            assert_eq!(decoded.encode(), bytes);
        }
    }
}
#[test]
fn layout_snapshots_reject_truncation_trailing_bytes_and_bad_envelopes() {
    let bytes = LayoutSnapshot::new(42, prefix(1)).unwrap().encode();
    for end in 0..bytes.len() {
        assert!(LayoutSnapshot::decode(&bytes[..end]).is_err());
    }
    let mut bad = bytes.clone();
    bad.push(0);
    assert!(LayoutSnapshot::decode(&bad).is_err());
    for offset in [0, 3, 12, 13, 14, 15, 16] {
        let mut bad = bytes.clone();
        bad[offset] ^= 255;
        assert!(LayoutSnapshot::decode(&bad).is_err());
    }
    assert!(LayoutSnapshot::decode(&vec![0; MAX_LAYOUT_SNAPSHOT_BYTES + 1]).is_err());
}
#[test]
fn complete_pages_empty_manifests_and_selected_layouts_cannot_be_records() {
    assert!(LayoutSnapshot::new(1, prefix(0)).is_err());
    assert!(LayoutSnapshot::new(1, prefix(1).select_segment(1).unwrap()).is_err());
    let page = NavigationPage::new(
        InternalUrl::new("/tasks").unwrap(),
        "Tasks",
        ComponentProps::new(2, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap();
    assert!(LayoutSnapshot::new(1, page).is_err());
}
#[test]
fn initial_and_layout_envelopes_are_not_interchangeable() {
    let mut bytes = LayoutSnapshot::new(42, prefix(1)).unwrap().encode();
    assert!(NavigationSnapshot::decode(&bytes).is_err());
    bytes[..4].copy_from_slice(b"RVI1");
    assert!(NavigationSnapshot::decode(&bytes).is_err());
}
