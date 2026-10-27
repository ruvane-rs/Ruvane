use ruvane::*;
#[test]
fn native_multipart_forms_escape_controls_and_never_prepopulate_files() {
    let token = CsrfToken::parse(&"a".repeat(64)).unwrap();
    let form = MultipartForm::new(InternalUrl::new("/upload").unwrap())
        .unwrap()
        .csrf(token.clone())
        .unwrap()
        .text("caption", "Caption <&>", "\"><script>")
        .unwrap()
        .textarea("notes", "Notes", "\n</textarea>")
        .unwrap()
        .checkbox("done", "Done", true)
        .unwrap()
        .hidden("id", "42")
        .unwrap()
        .file("attachment", "File <&>")
        .unwrap()
        .invalid("attachment")
        .unwrap()
        .submit_label("Upload <&>")
        .unwrap();
    assert!(!format!("{form:?}").contains(token.as_str()));
    let html = form.render().into_string();
    assert!(html.contains("enctype=\"multipart/form-data\" accept-charset=\"UTF-8\" novalidate"));
    assert!(html.contains("<input type=\"file\" name=\"attachment\" aria-invalid=\"true\">"));
    assert!(!html.contains("multiple"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("\n\n&lt;/textarea&gt;"));
}
#[test]
fn multipart_builders_reject_duplicate_reserved_and_oversized_controls() {
    let new = || MultipartForm::new(InternalUrl::new("/upload").unwrap()).unwrap();
    assert_eq!(
        new()
            .file("file", "File")
            .unwrap()
            .file("other", "Other")
            .unwrap_err(),
        FormError::TooManyFiles
    );
    assert_eq!(
        new()
            .file("file", "File")
            .unwrap()
            .text("file", "Text", "")
            .unwrap_err(),
        FormError::DuplicateName
    );
    assert_eq!(
        new().hidden(CSRF_FORM_FIELD, "secret").unwrap_err(),
        FormError::InvalidName
    );
    assert_eq!(
        new().file(CSRF_FORM_FIELD, "File").unwrap_err(),
        FormError::InvalidName
    );
    assert!(new().file("file", " ").is_err());
    assert!(new().text("text", "Text", "\n").is_err());
    assert!(new().file("bad[]", "File").is_err());
    let mut form = new();
    for i in 0..MAX_MULTIPART_FIELDS {
        form = form.hidden(format!("f{i}"), "v").unwrap();
    }
    let form = form.file("file", "File").unwrap();
    assert_eq!(
        form.hidden("overflow", "x").unwrap_err(),
        FormError::TooManyFields
    );
    assert!(
        MultipartForm::new(
            InternalUrl::new("/upload")
                .unwrap()
                .query("x", "1")
                .unwrap()
        )
        .is_err()
    );
}
