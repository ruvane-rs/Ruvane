use super::*;
fn listing(n: usize) -> FileListing {
    FileListing::new(
        (1..=n)
            .map(|i| {
                FileInfo::new(
                    FileId::parse(&format!("{i:064x}")).unwrap(),
                    "Rust <&🦀.bin",
                    3,
                )
                .unwrap()
            })
            .collect(),
        None,
    )
    .unwrap()
}
fn seed(role: Role) -> ActionData {
    let mut data = model::scope_seed(
        listing(3).to_seed().unwrap(),
        &model::Space {
            id: 1,
            name: "Ruvane".into(),
            revision: 1,
            members: [Some(role); 4],
        },
        role,
    );
    data.insert("ticket", ActionValue::Number(1)).unwrap();
    data
}
fn selected() -> FileSelection {
    let name = b"selected.bin";
    let mut bytes = b"RUF1\x01".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(3u32.to_le_bytes());
    bytes.extend((name.len() as u16).to_le_bytes());
    bytes.extend(name);
    bytes.extend(0u16.to_le_bytes());
    FileSelection::decode(1, 7, 0, &bytes).unwrap()
}
fn reply(status: u16, value: JsonValue, mutation: bool) -> JsonOutcome {
    let mut frame = b"RVJ1\0\0".to_vec();
    frame.extend(status.to_le_bytes());
    frame.extend([0; 6]);
    frame.extend(value.encode().as_bytes());
    decode_json_reply(&frame, mutation)
}
#[test]
fn all_roles_render_full_attachment_pages_within_the_view_budget() {
    for role in [Role::Owner, Role::Editor, Role::Viewer] {
        let mut component = create(1, 1);
        component.page_data(seed(role)).unwrap();
        let view = component.view().unwrap();
        view.snapshot().unwrap();
        let html = view.render().unwrap();
        assert_eq!(html.as_str().matches("download=\"\"").count(), 3);
        assert_eq!(html.as_str().contains("Store file"), role.can_edit());
        assert_eq!(html.as_str().contains("Delete file"), role.can_edit());
        assert!(view.event_count() <= MAX_VIEW_EVENTS);
    }
}
#[test]
fn viewers_cannot_select_upload_or_delete_files() {
    let mut c = create(1, 1);
    c.page_data(seed(Role::Viewer)).unwrap();
    assert!(c.dispatch_value(3, EventValue::File(selected())).is_err());
    assert!(c.dispatch(2).is_err());
    assert!(c.dispatch(10).is_err());
    assert!(c.take_upload().is_none() && c.take_json().is_none());
}
#[test]
fn live_permission_loss_retires_the_picker_and_private_metadata() {
    let mut c = create(1, 1);
    c.page_data(seed(Role::Editor)).unwrap();
    c.dispatch_value(3, EventValue::File(selected())).unwrap();
    c.dispatch(2).unwrap();
    c.take_json().unwrap();
    c.json_result(4, reply(403, JsonValue::null(), false))
        .unwrap();
    let text = c.view().unwrap().render().unwrap();
    assert!(!text.as_str().contains("selected.bin"));
    assert!(!text.as_str().contains("Rust &lt;"));
    assert!(!text.as_str().contains("Store file"));
    assert!(text.as_str().contains("Access changed"));
}
#[test]
fn attachment_seeds_and_lists_refuse_foreign_scopes_and_unknown_roles() {
    let mut c = create(2, 1);
    assert!(c.page_data(seed(Role::Editor)).is_err());
    let good = listing_json(&listing(3), Role::Viewer);
    assert!(parse_listing(&good).is_some());
    let mut fields = good.as_object().unwrap().to_vec();
    fields.push(("owner".into(), JsonValue::string("forged").unwrap()));
    assert!(parse_listing(&JsonValue::object(fields).unwrap()).is_none());
    let mut invalid = good.as_object().unwrap().to_vec();
    invalid.retain(|(k, _)| k != "role");
    invalid.push(("role".into(), JsonValue::string("administrator").unwrap()));
    assert!(parse_listing(&JsonValue::object(invalid).unwrap()).is_none());
}
