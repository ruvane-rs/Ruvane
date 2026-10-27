use super::*;
fn selected() -> BrowserFile {
    let mut bytes = b"RUF1\x01".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(5u16.to_le_bytes());
    bytes.extend(b"x.bin");
    bytes.extend(0u16.to_le_bytes());
    let FileSelection::Selected(file) = FileSelection::decode(1, 2, 0, &bytes).unwrap() else {
        panic!()
    };
    file
}
#[test]
fn requests_keep_metadata_opaque_and_do_not_mix_security_or_business_fields() {
    let file = selected();
    assert_eq!(file.name(), "x.bin");
    assert_eq!(file.size(), 3);
    assert!(!format!("{file:?}").contains("x.bin"));
    let request = UploadRequest::new(
        1,
        InternalUrl::new("/upload").unwrap(),
        "attachment",
        file.clone(),
        FormData::default().field("caption", "Rust + %").unwrap(),
    )
    .unwrap();
    assert_eq!(request.fields(), "caption=Rust%20%2B%20%25");
    assert!(
        UploadRequest::new(
            1,
            InternalUrl::new("/upload").unwrap(),
            "caption",
            file.clone(),
            FormData::default().field("caption", "x").unwrap()
        )
        .is_err()
    );
    assert!(
        UploadRequest::new(
            1,
            InternalUrl::new("/upload").unwrap(),
            crate::CSRF_FORM_FIELD,
            file,
            FormData::default()
        )
        .is_err()
    );
}
fn frame(token: u32, size: u32, name: &[u8], mime: &[u8]) -> Vec<u8> {
    let mut bytes = b"RUF1\x01".to_vec();
    bytes.extend(token.to_le_bytes());
    bytes.extend(size.to_le_bytes());
    bytes.extend((name.len() as u16).to_le_bytes());
    bytes.extend(name);
    bytes.extend((mime.len() as u16).to_le_bytes());
    bytes.extend(mime);
    bytes
}
#[test]
fn metadata_frames_enforce_every_budget_and_never_expose_paths() {
    let valid = frame(
        1,
        32768,
        "PRIVATE 🦀.bin".as_bytes(),
        b"application/octet-stream",
    );
    for length in 0..valid.len() {
        assert!(FileSelection::decode(1, 1, 0, &valid[..length]).is_err());
    }
    for bytes in [
        frame(0, 0, b"x", b""),
        frame(1, 32769, b"x", b""),
        frame(1, 0, b"", b""),
        frame(1, 0, b"../secret", b""),
        frame(1, 0, b"C:secret", b""),
        frame(1, 0, b"a\\b", b""),
        frame(1, 0, &[255], b""),
        frame(1, 0, b"a\0b", b""),
        frame(1, 0, b"x", b"bad\r\nheader"),
        frame(1, 0, &[b'x'; 256], b""),
        frame(1, 0, b"x", &[b'x'; 128]),
    ] {
        assert!(FileSelection::decode(1, 1, 0, &bytes).is_err());
    }
    let FileSelection::Selected(file) = FileSelection::decode(1, 2, 3, &valid).unwrap() else {
        panic!()
    };
    assert_eq!(file.size(), 32768);
    assert!(file.is_bound_to(1, 2, 3));
    assert!(!file.is_bound_to(2, 2, 3));
    assert!(!format!("{file:?}").contains("PRIVATE"));
    for kind in [0, 2, 3, 4] {
        let bytes = [b'R', b'U', b'F', b'1', kind];
        assert!(FileSelection::decode(1, 1, 0, &bytes).is_ok());
    }
    assert!(FileSelection::decode(0, 1, 0, &valid).is_err());
    assert!(FileSelection::decode(1, 0, 0, &valid).is_err());
    let empty = frame(1, 0, b"empty.bin", b"");
    let FileSelection::Selected(empty) = FileSelection::decode(1, 1, 0, &empty).unwrap() else {
        panic!()
    };
    assert_eq!(empty.size(), 0);
}
#[test]
fn upload_requests_and_file_views_keep_existing_budgets() {
    let new = || crate::View::new();
    let mut view = new();
    let label = view.keyed_element(0, 1, crate::BrowserTag::Label).unwrap();
    view.keyed_leaf(label, 1, crate::BrowserTag::Span, "File")
        .unwrap();
    let file = view.file(label, 2, 1, false).unwrap();
    view.field_epoch(file, 4).unwrap();
    let snapshot = view.snapshot().unwrap();
    assert_eq!(snapshot.file_epoch(file), Some(4));
    assert!(view.render().unwrap().as_str().contains("type=\"file\""));
    assert!(!view.render().unwrap().as_str().contains("value="));
    assert!(view.file(label, 3, 2, false).is_err());
    let mut fields = FormData::default();
    for index in 0..17 {
        fields = fields.field(&format!("f{index}"), "x").unwrap();
    }
    assert_eq!(
        UploadRequest::new(
            1,
            InternalUrl::new("/u").unwrap(),
            "file",
            selected(),
            fields
        )
        .unwrap_err(),
        ActionError::Limit
    );
    let fields = FormData::parse(format!("_ruvane_csrf={}", "a".repeat(64)).as_bytes()).unwrap();
    assert!(
        UploadRequest::new(
            1,
            InternalUrl::new("/u").unwrap(),
            "file",
            selected(),
            fields
        )
        .is_err()
    );
    let mut fields = FormData::default();
    for index in 0..8 {
        fields = fields
            .field(&format!("f{index}"), &"🦀".repeat(256))
            .unwrap();
    }
    assert_eq!(
        UploadRequest::new(
            1,
            InternalUrl::new("/u").unwrap(),
            "file",
            selected(),
            fields
        )
        .unwrap_err(),
        ActionError::Limit
    );
}
