use ruvane::*;

fn page(status: StatusCode) -> NavigationPage {
    let mut data = ActionData::default();
    data.insert("value", ActionValue::Text("</div><script> & 🦀".into()))
        .unwrap();
    NavigationPage::new(
        InternalUrl::from_target("/tasks/1?q=%3Cscript%3E").unwrap(),
        "<Report> 🦀",
        ComponentProps::new(2, InitialState::Number(1)).unwrap(),
        data,
        status,
    )
    .unwrap()
}
#[test]
fn snapshots_round_trip_complete_pages_and_exact_u64_revisions() {
    for revision in [0, 1, 9_007_199_254_740_993, u64::MAX] {
        for status in [
            StatusCode::Ok,
            StatusCode::BadRequest,
            StatusCode::NotFound,
            StatusCode::InternalServerError,
        ] {
            let snapshot = NavigationSnapshot::new(revision, page(status)).unwrap();
            let bytes = snapshot.encode();
            assert!(bytes.starts_with(b"RVI1"));
            let decoded = NavigationSnapshot::decode(&bytes).unwrap();
            assert_eq!(decoded.revision(), revision);
            assert_eq!(decoded.page().status(), status);
            assert_eq!(
                decoded.page().data().text("value"),
                Some("</div><script> & 🦀")
            );
            assert_eq!(decoded.encode(), bytes);
        }
    }
}
#[test]
fn snapshots_reject_truncation_trailing_bytes_bad_lengths_and_magic() {
    let bytes = NavigationSnapshot::new(7, page(StatusCode::Ok))
        .unwrap()
        .encode();
    for len in 0..bytes.len() {
        assert!(NavigationSnapshot::decode(&bytes[..len]).is_err());
    }
    for index in [0, 12, 13, 14, 15, 16] {
        let mut bad = bytes.clone();
        bad[index] ^= 0xff;
        assert!(NavigationSnapshot::decode(&bad).is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(NavigationSnapshot::decode(&trailing).is_err());
    assert_eq!(
        NavigationSnapshot::decode(&vec![0; MAX_NAVIGATION_SNAPSHOT_BYTES + 1]).err(),
        Some(NavigationError::Limit)
    );
}
#[test]
fn only_complete_unselected_branches_can_initialize_a_document() {
    let layout = NavigationLayout::new(
        "root",
        InternalUrl::new("/tasks").unwrap(),
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        ActionData::default(),
    )
    .unwrap();
    let full = page(StatusCode::Ok)
        .with_layouts(vec![layout.clone()])
        .unwrap();
    assert!(NavigationSnapshot::new(1, full.clone()).is_ok());
    assert!(NavigationSnapshot::new(1, full.select_segment(1).unwrap()).is_err());
    let pending = NavigationPage::new(
        InternalUrl::new("/tasks/1").unwrap(),
        "Loading",
        ComponentProps::new(2, InitialState::Number(1)).unwrap(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap()
    .with_pending_layouts(1, vec![layout.clone()])
    .unwrap();
    assert!(NavigationSnapshot::new(1, pending.clone()).is_err());
    let packet = pending.encode();
    let mut wire = b"RVI1".to_vec();
    wire.extend(1u64.to_le_bytes());
    wire.extend((packet.len() as u32).to_le_bytes());
    wire.extend(packet);
    assert!(NavigationSnapshot::decode(&wire).is_err());
    let error = NavigationPage::new(
        InternalUrl::new("/tasks/1").unwrap(),
        "Unavailable",
        ComponentProps::new(3, InitialState::Number(500)).unwrap(),
        ActionData::default(),
        StatusCode::InternalServerError,
    )
    .unwrap()
    .with_failure(1, vec![layout])
    .unwrap();
    let bytes = NavigationSnapshot::new(1, error).unwrap().encode();
    assert_eq!(
        NavigationSnapshot::decode(&bytes)
            .unwrap()
            .page()
            .failed_segment(),
        Some(1)
    );
}
