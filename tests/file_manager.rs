use ruvane::*;
#[path = "../examples/support/file_manager_component.rs"]
mod manager;
fn info(n: u8) -> FileInfo {
    FileInfo::new(
        FileId::parse(&format!("{n:064x}")).unwrap(),
        "Rust <&🦀.bin",
        3,
    )
    .unwrap()
}
fn listing(start: u8, end: u8, more: bool) -> FileListing {
    let files = (start..=end).map(info).collect::<Vec<_>>();
    let next = more.then(|| files.last().unwrap().id());
    FileListing::new(files, next).unwrap()
}
fn reply(status: u16, value: JsonValue, mutation: bool) -> JsonOutcome {
    let mut bytes = b"RVJ1".to_vec();
    bytes.extend([0, 0]);
    bytes.extend(status.to_le_bytes());
    bytes.extend([0; 6]);
    bytes.extend(value.encode().as_bytes());
    decode_json_reply(&bytes, mutation)
}
fn selection() -> FileSelection {
    let name = b"selected.bin";
    let mut bytes = b"RUF1\x01".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(3u32.to_le_bytes());
    bytes.extend((name.len() as u16).to_le_bytes());
    bytes.extend(name);
    bytes.extend(0u16.to_le_bytes());
    FileSelection::decode(1, 7, 0, &bytes).unwrap()
}
fn text(component: &dyn Component) -> String {
    component.view().unwrap().render().unwrap().as_str().into()
}
#[test]
fn maximum_private_seed_renders_bounded_safe_native_downloads() {
    let mut component = manager::create();
    component
        .page_data(listing(1, 3, true).to_seed().unwrap())
        .unwrap();
    let view = component.view().unwrap();
    let html = view
        .hydratable(ComponentProps::new(30, InitialState::Number(0)).unwrap())
        .unwrap();
    assert_eq!(html.as_str().matches("download=\"\"").count(), 3);
    assert!(!html.as_str().contains("data-ruvane-nav"));
    assert!(html.as_str().contains("Rust &lt;&amp;🦀.bin"));
    assert!(!html.as_str().contains("file_owner"));
    assert!(!html.as_str().contains("value=\"selected"));
    assert!(view.commands(1).is_ok());
    assert_eq!(
        component.page_data(ActionData::default()),
        Err(ComponentError::InvalidProps)
    );
}
#[test]
fn validation_and_invalid_list_retain_the_file_without_an_extra_upload() {
    let mut component = manager::create();
    component
        .page_data(listing(1, 1, false).to_seed().unwrap())
        .unwrap();
    component.dispatch(2).unwrap();
    assert!(component.take_upload().is_none());
    component
        .dispatch_value(3, EventValue::File(selection()))
        .unwrap();
    component.dispatch(2).unwrap();
    let request = component.take_upload().unwrap();
    assert_eq!(request.file().name(), "selected.bin");
    component.dispatch(2).unwrap();
    assert!(component.take_upload().is_none());
    component
        .upload_result(2, reply(400, JsonValue::null(), true))
        .unwrap();
    assert!(text(component.as_ref()).contains("Selected: selected.bin"));
    component.dispatch(1).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(200, JsonValue::null(), false))
        .unwrap();
    let html = text(component.as_ref());
    assert!(html.contains("Invalid file list."));
    assert!(html.contains("Selected: selected.bin"));
    assert_eq!(html.matches("download=\"\"").count(), 1);
    assert!(component.take_upload().is_none());
}
#[test]
fn uncertain_mutations_require_a_valid_explicit_read() {
    let mut component = manager::create();
    component
        .page_data(listing(1, 1, false).to_seed().unwrap())
        .unwrap();
    component
        .dispatch_value(3, EventValue::File(selection()))
        .unwrap();
    component.dispatch(2).unwrap();
    component.take_upload().unwrap();
    component
        .upload_result(2, Err(JsonFailure::protocol(true)))
        .unwrap();
    for event in [2, 10] {
        component.dispatch(event).unwrap();
    }
    assert!(component.take_json().is_none());
    assert!(component.take_upload().is_none());
    assert!(text(component.as_ref()).contains("Result uncertain."));
    component.dispatch(1).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(200, JsonValue::null(), false))
        .unwrap();
    component.dispatch(2).unwrap();
    assert!(component.take_upload().is_none());
    component.dispatch(1).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(200, listing(1, 1, false).to_json(), false))
        .unwrap();
    component.dispatch(2).unwrap();
    assert!(component.take_upload().is_some());
}
#[test]
fn invalid_continuation_cannot_commit_a_cursor_or_retire_visible_rows() {
    let mut component = manager::create();
    let first = listing(1, 3, true);
    component.page_data(first.to_seed().unwrap()).unwrap();
    let initial = text(component.as_ref());
    component.dispatch(6).unwrap();
    let request = component.take_json().unwrap();
    assert!(
        request
            .url()
            .as_str()
            .ends_with(&first.next().unwrap().to_string())
    );
    component
        .json_result(1, reply(200, listing(1, 1, false).to_json(), false))
        .unwrap();
    assert!(text(component.as_ref()).contains("Invalid continuation."));
    assert_eq!(text(component.as_ref()).matches("download=\"\"").count(), 3);
    component.dispatch(6).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(200, listing(4, 4, false).to_json(), false))
        .unwrap();
    component.dispatch(5).unwrap();
    assert_eq!(
        component.take_json().unwrap().url().as_str(),
        "/private-files/list"
    );
    component
        .json_result(1, reply(200, first.to_json(), false))
        .unwrap();
    assert_eq!(
        text(component.as_ref()).matches("download=\"\"").count(),
        initial.matches("download=\"\"").count()
    );
}
#[test]
fn authorization_loss_clears_metadata_and_selection() {
    let mut component = manager::create();
    component
        .page_data(listing(1, 3, true).to_seed().unwrap())
        .unwrap();
    component
        .dispatch_value(3, EventValue::File(selection()))
        .unwrap();
    component.dispatch(1).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(403, JsonValue::null(), false))
        .unwrap();
    let html = text(component.as_ref());
    assert!(!html.contains("download=\"\""));
    assert!(!html.contains("selected.bin"));
    assert!(html.contains("Session unavailable."));
    assert!(html.contains("No file selected."));
}

#[test]
fn mutation_success_replies_must_match_their_metadata_contract() {
    for value in [
        JsonValue::null(),
        FileInfo::new(info(2).id(), "wrong-size.bin", 4)
            .unwrap()
            .to_json(),
    ] {
        let mut component = manager::create();
        component
            .page_data(listing(1, 1, false).to_seed().unwrap())
            .unwrap();
        component
            .dispatch_value(3, EventValue::File(selection()))
            .unwrap();
        component.dispatch(2).unwrap();
        component.take_upload().unwrap();
        component.upload_result(2, reply(200, value, true)).unwrap();
        assert!(text(component.as_ref()).contains("Invalid upload result."));
        assert!(text(component.as_ref()).contains("Selected: selected.bin"));
        assert!(component.take_json().is_none());
        component.dispatch(2).unwrap();
        assert!(component.take_upload().is_none());
    }
    let mut component = manager::create();
    component
        .page_data(listing(1, 1, false).to_seed().unwrap())
        .unwrap();
    component
        .dispatch_value(3, EventValue::File(selection()))
        .unwrap();
    component.dispatch(2).unwrap();
    component.take_upload().unwrap();
    component
        .upload_result(2, reply(200, info(2).to_json(), true))
        .unwrap();
    assert!(!text(component.as_ref()).contains("Selected: selected.bin"));
    assert_eq!(
        component.take_json().unwrap().url().as_str(),
        "/private-files/list"
    );
    component
        .json_result(1, reply(200, listing(1, 2, false).to_json(), false))
        .unwrap();
    component.dispatch(10).unwrap();
    component.take_json().unwrap();
    component
        .json_result(3, reply(200, false.into(), true))
        .unwrap();
    assert!(text(component.as_ref()).contains("Invalid deletion result."));
    assert!(component.take_json().is_none());
    component.dispatch(10).unwrap();
    assert!(component.take_json().is_none());
}
