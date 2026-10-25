use ruvane::{
    FormData, FormError, InternalUrl, MAX_COMPONENT_BYTES, MAX_POST_BODY_BYTES, ParamError,
    PostForm,
};

fn form() -> PostForm {
    PostForm::new(InternalUrl::new("/tasks").unwrap()).unwrap()
}

#[test]
fn post_fields_decode_once_and_keep_typed_access_separate_from_queries() {
    let fields = FormData::parse(
        b"title=Rust+%2B+HTML&notes=%0D%0Aone%0D%0Atwo%09%F0%9F%A6%80&done=on&id=42&once=%2526%3D",
    )
    .unwrap();
    assert_eq!(fields.get("title"), Some("Rust + HTML"));
    assert_eq!(fields.get("notes"), Some("\none\ntwo\t🦀"));
    assert_eq!(fields.get("once"), Some("%26="));
    assert_eq!(fields.get_as::<u8>("id"), Ok(42));
    assert_eq!(fields.get_as::<u8>("title"), Err(ParamError::Invalid));
    assert_eq!(fields.get_as::<u8>("missing"), Err(ParamError::Missing));
    assert_eq!(fields.checkbox("done"), Ok(true));
    assert_eq!(fields.checkbox("missing"), Ok(false));
    assert_eq!(fields.checkbox("id"), Err(ParamError::Invalid));
    assert_eq!(fields.iter().next(), Some(("title", "Rust + HTML")));
    assert!(fields.has_only(&["title", "notes", "done", "id", "once"]));
    assert!(!fields.has_only(&["title"]));
    assert_eq!(fields.len(), 5);
    assert!(FormData::parse(b"").unwrap().is_empty());
}

#[test]
fn malformed_post_fields_reject_controls_duplicates_and_invalid_utf8() {
    for raw in [
        "q=%",
        "q=%0",
        "q=%GG",
        "q=%FF",
        "q=%C0%AF",
        "q=%ED%A0%80",
        "q=%F4%90%80%80",
        "q=%00",
        "q=%0D",
        "q=%0Dtext",
        "q=%7f",
        "q=%C2%85",
        "q=raw space",
        "q=λ",
        "q=1&",
        "&q=1",
        "q=1&&b=2",
        "=empty",
        "bare",
        "q[]=1",
        "%CE%BB=value",
        "q=1&%71=2",
        "q=1&q=2",
        "q=\t",
        "q=\r\n",
        "1name=x",
        "q.x=1",
    ] {
        assert!(FormData::parse(raw.as_bytes()).is_err(), "{raw}");
    }
    assert_eq!(FormData::parse(b"q=1&%71=2"), Err(FormError::DuplicateName));
    assert!(FormData::parse(b"Q=1&q=2&empty=").is_ok());
    assert_eq!(
        FormData::parse(b"q=%EF%BB%BFvalue").unwrap().get("q"),
        Some("\u{feff}value")
    );
}

#[test]
fn post_budgets_cover_normalized_newlines_values_fields_and_encoded_body() {
    for exact in [
        "x".repeat(MAX_COMPONENT_BYTES),
        "%F0%9F%A6%80".repeat(256),
        "%0D%0A".repeat(MAX_COMPONENT_BYTES),
    ] {
        let raw = format!("q={exact}");
        assert_eq!(
            FormData::parse(raw.as_bytes())
                .unwrap()
                .get("q")
                .unwrap()
                .len(),
            MAX_COMPONENT_BYTES
        );
        assert_eq!(
            FormData::parse(format!("{raw}x").as_bytes()),
            Err(FormError::TooLarge)
        );
    }
    let pairs = (0..32)
        .map(|n| format!("q{n}=v"))
        .collect::<Vec<_>>()
        .join("&");
    assert_eq!(FormData::parse(pairs.as_bytes()).unwrap().len(), 32);
    assert_eq!(
        FormData::parse(format!("{pairs}&extra=v").as_bytes()),
        Err(FormError::TooManyFields)
    );
    let mut exact = (0..21)
        .map(|n| format!("q{n}={}", "%61".repeat(1024)))
        .collect::<Vec<_>>()
        .join("&");
    exact.push_str("&tail=");
    exact.push_str(&"x".repeat(MAX_POST_BODY_BYTES - exact.len()));
    assert_eq!(exact.len(), MAX_POST_BODY_BYTES);
    assert!(FormData::parse(exact.as_bytes()).is_ok());
    exact.push('x');
    assert_eq!(FormData::parse(exact.as_bytes()), Err(FormError::TooLarge));
}

#[test]
fn native_post_controls_escape_values_and_preserve_textarea_defaults() {
    let html = PostForm::new(InternalUrl::new("/a'b").unwrap())
        .unwrap()
        .text("title", "Name <&>", "\"><script>&")
        .unwrap()
        .textarea("notes", "Notes", "\n</textarea> & 🦀\n\t")
        .unwrap()
        .checkbox("done", "Completed", true)
        .unwrap()
        .hidden("id", "42")
        .unwrap()
        .invalid("title")
        .unwrap()
        .submit_label("Save <&>")
        .unwrap()
        .render()
        .into_string();
    assert!(html.starts_with("<form method=\"post\" action=\"/a&#39;b\""));
    assert!(html.contains(
        "enctype=\"application/x-www-form-urlencoded\" accept-charset=\"UTF-8\" novalidate>"
    ));
    assert!(html.contains("name=\"title\" aria-invalid=\"true\" maxlength=\"1024\" value=\"&quot;&gt;&lt;script&gt;&amp;\""));
    assert!(html.contains(">\n\n&lt;/textarea&gt; &amp; 🦀\n\t</textarea>"));
    assert!(html.contains("name=\"done\" aria-invalid=\"false\" value=\"on\" checked>"));
    assert!(html.contains("<input type=\"hidden\" name=\"id\" value=\"42\">"));
    assert_eq!(html.matches("<label>").count(), 3);
    assert!(!html.contains("<script>"));
    assert!(!html.contains("</input>"));
    assert_eq!(html.matches("</textarea>").count(), 1);
    assert!(
        !form()
            .checkbox("done", "Completed", false)
            .unwrap()
            .render()
            .as_str()
            .contains(" checked")
    );
}

#[test]
fn post_builders_reject_invalid_names_and_controls_across_field_types() {
    for name in [
        "", "1q", "q[]", "q.x", "q x", "q\"", "λ", "q\n", "q=1", "q&x",
    ] {
        assert_eq!(
            form().text(name, "Name", "").unwrap_err(),
            FormError::InvalidName
        );
        assert_eq!(
            form().checkbox(name, "Name", false).unwrap_err(),
            FormError::InvalidName
        );
    }
    assert!(form().hidden("x".repeat(64), "").is_ok());
    assert_eq!(
        form().hidden("x".repeat(65), "").unwrap_err(),
        FormError::InvalidName
    );
    assert_eq!(
        form()
            .text("q", "Name", "")
            .unwrap()
            .checkbox("q", "Other", false)
            .unwrap_err(),
        FormError::DuplicateName
    );
    for value in ["\0", "\r", "\u{85}"] {
        assert_eq!(
            form().textarea("q", "Notes", value).unwrap_err(),
            FormError::InvalidText
        );
    }
    assert_eq!(
        form().text("q", "Name", "line\nbreak").unwrap_err(),
        FormError::InvalidText
    );
    assert_eq!(
        form().textarea("q", "Notes", "x".repeat(1025)).unwrap_err(),
        FormError::TooLarge
    );
    assert_eq!(
        form().checkbox("done", " ", false).unwrap_err(),
        FormError::EmptyLabel
    );
    assert_eq!(
        form().hidden("id", "1").unwrap().invalid("id").unwrap_err(),
        FormError::InvalidName
    );
    assert_eq!(
        form().invalid("missing").unwrap_err(),
        FormError::InvalidName
    );
    assert_eq!(
        PostForm::new(
            InternalUrl::new("/tasks")
                .unwrap()
                .query("id", "1")
                .unwrap()
        )
        .unwrap_err(),
        FormError::ActionHasQuery
    );
    let mut builder = form();
    for n in 0..32 {
        builder = builder.hidden(format!("q{n}"), "").unwrap();
    }
    assert_eq!(
        builder.hidden("extra", "").unwrap_err(),
        FormError::TooManyFields
    );
}

#[test]
fn deterministic_post_body_mutations_never_panic() {
    let seed = b"title=Rust+%2B+HTML&notes=one%0D%0Atwo&done=on";
    for index in 0..seed.len() {
        for byte in 0..=255 {
            let mut bytes = seed.to_vec();
            bytes[index] = byte;
            let _ = FormData::parse(&bytes);
        }
    }
}
