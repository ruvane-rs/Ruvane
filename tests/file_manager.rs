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
fn operation() -> FileUploadId {
    FileUploadId::parse(&format!("{:016x}{}", 2_000_000_000u64, "01".repeat(32))).unwrap()
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
fn selected() -> Box<dyn Component> {
    let mut component = manager::create();
    component
        .page_data(listing(1, 1, false).to_seed().unwrap())
        .unwrap();
    component
        .dispatch_value(3, EventValue::File(selection()))
        .unwrap();
    component
}
fn submit(component: &mut dyn Component) -> UploadRequest {
    component.dispatch(2).unwrap();
    let prepare = component.take_json().unwrap();
    assert_eq!(prepare.action(), 4);
    assert_eq!(prepare.url().as_str(), "/private-files/upload-operation");
    component
        .json_result(
            4,
            reply(
                200,
                FileUploadStatus::new(operation(), FileUploadState::Unconfirmed).to_json(),
                false,
            ),
        )
        .unwrap();
    component.take_upload().unwrap()
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
    assert!(view.commands(1).is_ok());
    assert!(component.take_json().is_none());
    assert_eq!(
        component.page_data(ActionData::default()),
        Err(ComponentError::InvalidProps)
    );
}
#[test]
fn validation_and_invalid_list_retain_the_file_and_operation() {
    let mut empty = manager::create();
    empty
        .page_data(listing(1, 1, false).to_seed().unwrap())
        .unwrap();
    empty.dispatch(2).unwrap();
    assert!(empty.take_upload().is_none());
    assert!(empty.take_json().is_none());
    let mut component = selected();
    let request = submit(component.as_mut());
    assert_eq!(request.file().name(), "selected.bin");
    assert_eq!(request.fields(), format!("operation={}", operation()));
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
    assert!(text(component.as_ref()).contains("Invalid file list."));
    assert_eq!(text(component.as_ref()).matches("download=\"\"").count(), 1);
    component.dispatch(2).unwrap();
    assert_eq!(component.take_upload().unwrap().fields(), request.fields());
    assert!(component.take_json().is_none());
}
#[test]
fn uncertain_uploads_require_explicit_checks_or_reuse_of_the_same_identity() {
    let mut component = selected();
    let original = submit(component.as_mut());
    component
        .upload_result(2, Err(JsonFailure::protocol(true)))
        .unwrap();
    assert!(component.take_upload().is_none());
    assert!(component.take_json().is_none());
    assert!(text(component.as_ref()).contains("Upload result uncertain."));
    component.dispatch(10).unwrap();
    assert!(component.take_json().is_none());
    component.dispatch(1).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(200, listing(1, 1, false).to_json(), false))
        .unwrap();
    assert!(text(component.as_ref()).contains("still unconfirmed"));
    component.dispatch(2).unwrap();
    assert_eq!(component.take_upload().unwrap().fields(), original.fields());
    assert!(component.take_json().is_none());
    component
        .upload_result(2, Err(JsonFailure::protocol(true)))
        .unwrap();
    component.dispatch(7).unwrap();
    let check = component.take_json().unwrap();
    assert!(check.url().as_str().ends_with(&operation().to_string()));
    component
        .json_result(
            5,
            reply(
                200,
                FileUploadStatus::new(operation(), FileUploadState::Stored(info(2))).to_json(),
                false,
            ),
        )
        .unwrap();
    assert!(!text(component.as_ref()).contains("Selected: selected.bin"));
    assert_eq!(
        component.take_json().unwrap().url().as_str(),
        "/private-files/list"
    );
    assert!(component.take_upload().is_none());
}
#[test]
fn invalid_continuation_cannot_commit_a_cursor_or_retire_visible_rows() {
    let mut component = manager::create();
    let first = listing(1, 3, true);
    component.page_data(first.to_seed().unwrap()).unwrap();
    component.dispatch(6).unwrap();
    assert!(
        component
            .take_json()
            .unwrap()
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
    assert_eq!(text(component.as_ref()).matches("download=\"\"").count(), 3);
}
#[test]
fn authorization_loss_clears_metadata_selection_and_the_attempt() {
    let mut component = selected();
    submit(component.as_mut());
    component
        .upload_result(2, reply(400, JsonValue::null(), true))
        .unwrap();
    component.dispatch(1).unwrap();
    component.take_json().unwrap();
    component
        .json_result(1, reply(403, JsonValue::null(), false))
        .unwrap();
    let html = text(component.as_ref());
    assert!(!html.contains("download=\"\""));
    assert!(!html.contains("selected.bin"));
    assert!(!html.contains("Resume upload"));
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
        let mut component = selected();
        let request = submit(component.as_mut());
        component.upload_result(2, reply(200, value, true)).unwrap();
        assert!(text(component.as_ref()).contains("Invalid upload result."));
        assert!(text(component.as_ref()).contains("Selected: selected.bin"));
        assert!(component.take_json().is_none());
        assert!(component.take_upload().is_none());
        component.dispatch(2).unwrap();
        assert_eq!(component.take_upload().unwrap().fields(), request.fields());
    }
    let mut component = selected();
    submit(component.as_mut());
    component
        .upload_result(2, reply(200, info(2).to_json(), true))
        .unwrap();
    assert!(!text(component.as_ref()).contains("Selected: selected.bin"));
    component.take_json().unwrap();
    component
        .json_result(1, reply(200, listing(1, 2, false).to_json(), false))
        .unwrap();
    component.dispatch(10).unwrap();
    component.take_json().unwrap();
    component
        .json_result(3, reply(200, false.into(), true))
        .unwrap();
    assert!(text(component.as_ref()).contains("Invalid deletion result."));
    component.dispatch(10).unwrap();
    assert!(component.take_json().is_none());
}
#[test]
fn removed_or_expired_attempts_require_an_explicit_discard() {
    for state in [FileUploadState::Removed, FileUploadState::Expired] {
        let mut component = selected();
        submit(component.as_mut());
        component
            .upload_result(2, Err(JsonFailure::protocol(true)))
            .unwrap();
        component.dispatch(7).unwrap();
        component.take_json().unwrap();
        component
            .json_result(
                5,
                reply(
                    200,
                    FileUploadStatus::new(operation(), state).to_json(),
                    false,
                ),
            )
            .unwrap();
        component.dispatch(2).unwrap();
        assert!(component.take_upload().is_none());
        assert!(component.take_json().is_none());
        assert!(text(component.as_ref()).contains("Discard attempt"));
        component.dispatch(1).unwrap();
        component.take_json().unwrap();
        component
            .json_result(1, reply(200, listing(1, 1, false).to_json(), false))
            .unwrap();
        assert!(text(component.as_ref()).contains("attempt is unavailable"));
        component.dispatch(2).unwrap();
        assert!(component.take_upload().is_none());
        component.dispatch(4).unwrap();
        assert!(!text(component.as_ref()).contains("Resume upload"));
        assert!(text(component.as_ref()).contains("No file selected."));
    }
}
#[test]
fn malformed_preparation_and_wrong_status_id_cannot_submit_or_confirm() {
    let mut component = selected();
    component.dispatch(2).unwrap();
    component.take_json().unwrap();
    component
        .json_result(4, reply(200, JsonValue::null(), false))
        .unwrap();
    assert!(component.take_upload().is_none());
    submit(component.as_mut());
    component
        .upload_result(2, Err(JsonFailure::protocol(true)))
        .unwrap();
    component.dispatch(7).unwrap();
    component.take_json().unwrap();
    let other =
        FileUploadId::parse(&format!("{:016x}{}", 2_000_000_000u64, "02".repeat(32))).unwrap();
    component
        .json_result(
            5,
            reply(
                200,
                FileUploadStatus::new(other, FileUploadState::Stored(info(2))).to_json(),
                false,
            ),
        )
        .unwrap();
    assert!(text(component.as_ref()).contains("Wrong upload identity."));
    assert!(text(component.as_ref()).contains("Selected: selected.bin"));
    assert!(component.take_json().is_none());
    assert!(component.take_upload().is_none());
}
