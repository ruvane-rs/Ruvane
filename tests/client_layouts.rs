use ruvane::{
    ActionData, ActionValue, ComponentProps, InitialState, InternalUrl, NavigationLayout,
    NavigationPage, StatusCode,
};
fn props(value: u32) -> ComponentProps {
    ComponentProps::new(1, InitialState::Number(value)).unwrap()
}
fn layout(name: &str) -> NavigationLayout {
    NavigationLayout::new(
        name,
        InternalUrl::new("/tasks/1").unwrap(),
        props(1),
        ActionData::default(),
    )
    .unwrap()
}
fn page() -> NavigationPage {
    NavigationPage::new(
        InternalUrl::new("/tasks/1").unwrap(),
        "Task",
        props(2),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap()
}
#[test]
fn branch_packets_round_trip_all_segments_and_reject_truncation_and_trailing_bytes() {
    let page = page()
        .with_layouts(vec![layout("workspace"), layout("task")])
        .unwrap();
    for selected in 0..=2 {
        let bytes = page.clone().select_segment(selected).unwrap().encode();
        assert!(bytes.starts_with(b"RVP2"));
        let decoded = NavigationPage::decode(&bytes).unwrap();
        assert_eq!(decoded.selected_segment(), selected);
        assert_eq!(decoded.layouts().len(), 2);
        assert_eq!(
            decoded.segment().0,
            props(if selected == 0 { 2 } else { 1 })
        );
        for length in 0..bytes.len() {
            assert!(NavigationPage::decode(&bytes[..length]).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(NavigationPage::decode(&trailing).is_err());
    }
    assert!(page.select_segment(3).is_err());
}
#[test]
fn layout_counts_names_statuses_and_total_wire_bytes_are_bounded() {
    assert!(
        NavigationLayout::new(
            "bad\"name",
            InternalUrl::new("/").unwrap(),
            props(1),
            ActionData::default()
        )
        .is_err()
    );
    assert!(
        page()
            .with_layouts(vec![layout("same"), layout("same")])
            .is_err()
    );
    let layouts = (0..7).map(|n| layout(&format!("l{n}"))).collect();
    assert!(page().with_layouts(layouts).is_err());
    let error = NavigationPage::new(
        InternalUrl::new("/").unwrap(),
        "Missing",
        props(404),
        ActionData::default(),
        StatusCode::NotFound,
    )
    .unwrap();
    assert!(error.with_layouts(vec![layout("root")]).is_err());
    let mut data = ActionData::default();
    for n in 0..50 {
        data.insert(&format!("v{n}"), ActionValue::Text("x".repeat(1024)))
            .unwrap();
    }
    let large =
        NavigationLayout::new("large", InternalUrl::new("/").unwrap(), props(1), data).unwrap();
    let mut other = large.clone();
    other = NavigationLayout::new(
        "other",
        other.key().clone(),
        other.props(),
        other.data().clone(),
    )
    .unwrap();
    assert!(page().with_layouts(vec![large, other]).is_err());
    assert!(page().encode().starts_with(b"RVP1"));
}

#[test]
fn failure_packets_round_trip_and_bound_status_scope_and_parent_count() {
    let page = NavigationPage::new(
        InternalUrl::new("/tasks/1").unwrap(),
        "Unavailable",
        props(500),
        ActionData::default(),
        StatusCode::InternalServerError,
    )
    .unwrap();
    let packet = page.clone().with_failure(2, vec![layout("root")]).unwrap();
    let bytes = packet.encode();
    assert!(bytes.starts_with(b"RVP3"));
    let decoded = NavigationPage::decode(&bytes).unwrap();
    assert_eq!(decoded.failed_segment(), Some(2));
    assert_eq!(decoded.layouts().len(), 1);
    for length in 0..bytes.len() {
        assert!(NavigationPage::decode(&bytes[..length]).is_err());
    }
    assert!(page.clone().with_failure(0, vec![layout("root")]).is_err());
    assert!(page.with_failure(7, vec![]).is_err());
    assert!(self::page().with_failure(0, vec![]).is_err());
    let mut bad = bytes.clone();
    bad[5] = 0;
    assert!(NavigationPage::decode(&bad).is_err());
    let mut bad = bytes.clone();
    bad[4] = 2;
    assert!(NavigationPage::decode(&bad).is_err());
    let mut bad = bytes;
    bad.push(0);
    assert!(NavigationPage::decode(&bad).is_err());
}
