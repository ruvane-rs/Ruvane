use ruvane::{
    FormError, GetForm, InternalUrl, MAX_COMPONENT_BYTES, MAX_FORM_NAME_BYTES,
    MAX_INTERNAL_URL_BYTES, MAX_QUERY_PAIRS, ParamError, Query, QueryMode, UrlError,
};

fn form() -> GetForm {
    GetForm::new(InternalUrl::new("/articles").unwrap()).unwrap()
}

#[test]
fn explicit_form_decoding_preserves_query_compatibility_and_decodes_once() {
    let raw = "search=Rust+%2B+HTML&once=%252B&delimiters=a%26b%3Dc&flag&equals=a=b";
    let uri = Query::parse(raw).unwrap();
    let form = Query::parse_with_mode(raw, QueryMode::Form).unwrap();
    assert_eq!(uri.mode(), QueryMode::Uri);
    assert_eq!(uri.get("search"), Some("Rust+++HTML"));
    assert_eq!(form.mode(), QueryMode::Form);
    assert_eq!(form.get("search"), Some("Rust + HTML"));
    assert_eq!(form.get("once"), Some("%2B"));
    assert_eq!(form.get("delimiters"), Some("a&b=c"));
    assert_eq!(form.get("flag"), Some(""));
    assert_eq!(form.get("equals"), Some("a=b"));
    assert_eq!(Query::parse_with_mode(raw, QueryMode::Uri).unwrap(), uri);
    assert_eq!(Query::default().mode(), QueryMode::Uri);
    assert_eq!(
        Query::parse_with_mode("", QueryMode::Form).unwrap().mode(),
        QueryMode::Form
    );
}

#[test]
fn duplicate_detection_uses_only_the_selected_decoding_mode() {
    let same_form_key = "a+b=1&a%20b=2";
    assert_eq!(
        Query::parse_with_mode(same_form_key, QueryMode::Form),
        Err(UrlError::DuplicateKey)
    );
    assert_eq!(Query::parse(same_form_key).unwrap().len(), 2);
    let same_uri_key = "a+b=1&a%2Bb=2";
    assert_eq!(Query::parse(same_uri_key), Err(UrlError::DuplicateKey));
    let form = Query::parse_with_mode(same_uri_key, QueryMode::Form).unwrap();
    assert_eq!(
        form.iter().collect::<Vec<_>>(),
        [("a b", "1"), ("a+b", "2")]
    );
    for mode in [QueryMode::Uri, QueryMode::Form] {
        for raw in ["a=1&%61=2", "%CE%BB=1&%ce%bb=2"] {
            assert_eq!(
                Query::parse_with_mode(raw, mode),
                Err(UrlError::DuplicateKey)
            );
        }
        assert_eq!(Query::parse_with_mode("A=1&a=2", mode).unwrap().len(), 2);
    }
}

#[test]
fn form_query_is_strict_about_encoding_controls_and_empty_keys() {
    for raw in [
        "q=%",
        "q=%0",
        "q=%GG",
        "q=%FF",
        "q=%C0%AF",
        "q=%ED%A0%80",
        "q=%F4%90%80%80",
        "q=%00",
        "q=%0D%0A",
        "q=%7F",
        "q=%C2%85",
        "q=λ",
        "q=a b",
        "q=\t",
        "q=1&",
        "&q=1",
        "q=1&&b=2",
        "=value",
    ] {
        for mode in [QueryMode::Uri, QueryMode::Form] {
            assert!(
                Query::parse_with_mode(raw, mode).is_err(),
                "{mode:?}: {raw:?}"
            );
        }
    }
    let query = Query::parse_with_mode("q=%F0%9F%A6%80+%F0%9F%A6%80", QueryMode::Form).unwrap();
    assert_eq!(query.get("q"), Some("🦀 🦀"));
}

#[test]
fn form_queries_keep_typed_missing_invalid_and_overflow_errors() {
    let query = Query::parse_with_mode(
        "page=2&small=256&empty=&bad=secret+value&enabled=true",
        QueryMode::Form,
    )
    .unwrap();
    assert_eq!(query.get_as::<usize>("page"), Ok(2));
    assert_eq!(query.get_as::<bool>("enabled"), Ok(true));
    for key in ["small", "empty", "bad"] {
        let error = query.get_as::<u8>(key).unwrap_err();
        assert_eq!(error, ParamError::Invalid);
        assert!(!error.to_string().contains("secret"));
    }
    assert_eq!(query.get_as::<usize>("missing"), Err(ParamError::Missing));
}

#[test]
fn both_modes_enforce_exact_pair_decoded_and_encoded_byte_budgets() {
    let pairs = (0..MAX_QUERY_PAIRS)
        .map(|i| format!("k{i}=v"))
        .collect::<Vec<_>>()
        .join("&");
    let maximum = "x".repeat(MAX_COMPONENT_BYTES);
    let prefix = format!("a={maximum}&b={maximum}&c={maximum}&d=");
    let exact = format!(
        "{prefix}{}",
        "x".repeat(MAX_INTERNAL_URL_BYTES - prefix.len())
    );
    for mode in [QueryMode::Uri, QueryMode::Form] {
        assert_eq!(
            Query::parse_with_mode(&pairs, mode).unwrap().len(),
            MAX_QUERY_PAIRS
        );
        assert_eq!(
            Query::parse_with_mode(&(pairs.clone() + "&extra=v"), mode),
            Err(UrlError::TooManyPairs)
        );
        assert!(Query::parse_with_mode(&exact, mode).is_ok());
        assert_eq!(
            Query::parse_with_mode(&(exact.clone() + "x"), mode),
            Err(UrlError::TooLarge)
        );
        for component in [
            "+".repeat(MAX_COMPONENT_BYTES),
            "%CE%BB".repeat(MAX_COMPONENT_BYTES / 2),
        ] {
            assert_eq!(
                Query::parse_with_mode(&format!("q={component}"), mode)
                    .unwrap()
                    .get("q")
                    .unwrap()
                    .len(),
                MAX_COMPONENT_BYTES
            );
            assert_eq!(
                Query::parse_with_mode(&format!("q={component}x"), mode),
                Err(UrlError::TooLarge)
            );
            assert!(Query::parse_with_mode(&format!("{component}=v"), mode).is_ok());
            assert_eq!(
                Query::parse_with_mode(&format!("{component}x=v"), mode),
                Err(UrlError::TooLarge)
            );
        }
    }
}

#[test]
fn internal_url_query_encoding_roundtrips_in_both_modes() {
    let ascii: String = (32..=126).map(char::from).collect();
    for value in [ascii.as_str(), "🦀 🦀 日本語", "a+b c&d=e", "%252B"] {
        let url = InternalUrl::new("/articles")
            .unwrap()
            .query("search", value)
            .unwrap();
        for mode in [QueryMode::Uri, QueryMode::Form] {
            let query =
                Query::parse_with_mode(url.as_str().split_once('?').unwrap().1, mode).unwrap();
            assert_eq!(query.get("search"), Some(value));
        }
    }
}

#[test]
fn get_form_renders_native_labeled_void_controls_in_order() {
    let html = form()
        .search("search", "Search articles", "Rust + HTML")
        .unwrap()
        .hidden("page", "1")
        .unwrap()
        .submit_label("Search")
        .unwrap()
        .render();
    assert_eq!(
        html.as_str(),
        "<form method=\"get\" action=\"/articles\" accept-charset=\"UTF-8\"><label><span>Search articles</span><input type=\"search\" name=\"search\" value=\"Rust + HTML\" maxlength=\"1024\"></label><input type=\"hidden\" name=\"page\" value=\"1\"><button type=\"submit\">Search</button></form>"
    );
    assert!(!html.as_str().contains("</input>"));
    assert!(!html.as_str().contains(" id="));
    assert_eq!(
        form().render().as_str(),
        "<form method=\"get\" action=\"/articles\" accept-charset=\"UTF-8\"><button type=\"submit\">Submit</button></form>"
    );
}

#[test]
fn form_escapes_all_text_and_internal_action_attributes() {
    let payload = "\"><script>&' </form><input autofocus onfocus=evil>";
    let escaped =
        "&quot;&gt;&lt;script&gt;&amp;&#39; &lt;/form&gt;&lt;input autofocus onfocus=evil&gt;";
    let html = GetForm::new(InternalUrl::new("/a'b").unwrap())
        .unwrap()
        .search("q", payload, payload)
        .unwrap()
        .hidden("token", payload)
        .unwrap()
        .submit_label(payload)
        .unwrap()
        .render();
    assert!(html.as_str().contains("action=\"/a&#39;b\""));
    assert_eq!(html.as_str().matches(escaped).count(), 4);
    assert_eq!(html.as_str().matches("</form>").count(), 1);
    assert_eq!(html.as_str().matches("<input ").count(), 2);
    assert!(!html.as_str().contains("<script>"));
    assert!(!html.as_str().contains("<input autofocus"));
}

#[test]
fn get_actions_cannot_silently_discard_queries_or_escape_the_site() {
    let action = InternalUrl::new("/articles")
        .unwrap()
        .query("page", "2")
        .unwrap();
    assert_eq!(GetForm::new(action).unwrap_err(), FormError::ActionHasQuery);
    for path in [
        "//evil.test",
        "https://evil.test",
        "javascript:evil",
        "/?page=1",
        "/#fragment",
    ] {
        assert!(InternalUrl::new(path).is_err());
    }
    assert!(GetForm::new(InternalUrl::new("/literal%3Fpath").unwrap()).is_ok());
}

#[test]
fn field_names_are_bounded_identifiers_unique_across_control_types() {
    for name in [
        "", "1q", "q[]", "q.x", "q x", "q\"", "λ", "q\n", "q=1", "q&x",
    ] {
        assert_eq!(
            form().search(name, "Label", "").unwrap_err(),
            FormError::InvalidName,
            "{name:?}"
        );
        assert_eq!(form().hidden(name, "").unwrap_err(), FormError::InvalidName);
    }
    assert!(form().hidden("_page-2", "").is_ok());
    assert!(form().hidden("x".repeat(MAX_FORM_NAME_BYTES), "").is_ok());
    assert_eq!(
        form()
            .hidden("x".repeat(MAX_FORM_NAME_BYTES + 1), "")
            .unwrap_err(),
        FormError::InvalidName
    );
    assert_eq!(
        form()
            .search("q", "Label", "")
            .unwrap()
            .hidden("q", "")
            .unwrap_err(),
        FormError::DuplicateName
    );
    assert_eq!(
        form()
            .hidden("q", "")
            .unwrap()
            .search("q", "Label", "")
            .unwrap_err(),
        FormError::DuplicateName
    );
    assert!(form().hidden("q", "").unwrap().hidden("Q", "").is_ok());
}

#[test]
fn form_field_count_matches_query_budget_including_hidden_fields() {
    let mut builder = form();
    for index in 0..MAX_QUERY_PAIRS {
        builder = if index % 2 == 0 {
            builder.search(format!("q{index}"), "Label", "").unwrap()
        } else {
            builder.hidden(format!("q{index}"), "").unwrap()
        };
    }
    assert_eq!(
        builder.hidden("extra", "").unwrap_err(),
        FormError::TooManyFields
    );
}

#[test]
fn form_text_rejects_controls_empty_labels_and_byte_overflow() {
    for value in ["\0", "\r\n", "\t", "\u{7f}", "\u{85}"] {
        assert_eq!(
            form().search("q", "Label", value).unwrap_err(),
            FormError::InvalidText
        );
        assert_eq!(
            form().hidden("q", value).unwrap_err(),
            FormError::InvalidText
        );
        assert_eq!(
            form().search("q", value, "").unwrap_err(),
            FormError::InvalidText
        );
        assert_eq!(
            form().submit_label(value).unwrap_err(),
            FormError::InvalidText
        );
    }
    for label in ["", " ", "\u{2003}"] {
        assert_eq!(
            form().search("q", label, "").unwrap_err(),
            FormError::EmptyLabel
        );
        assert_eq!(
            form().submit_label(label).unwrap_err(),
            FormError::EmptyLabel
        );
    }
    for exact in [
        "x".repeat(MAX_COMPONENT_BYTES),
        "λ".repeat(MAX_COMPONENT_BYTES / 2),
    ] {
        assert!(
            form()
                .search("q", &exact, &exact)
                .unwrap()
                .hidden("h", &exact)
                .unwrap()
                .submit_label(&exact)
                .is_ok()
        );
        let too_big = exact + "x";
        assert_eq!(
            form().search("q", "Label", &too_big).unwrap_err(),
            FormError::TooLarge
        );
        assert_eq!(
            form().hidden("q", &too_big).unwrap_err(),
            FormError::TooLarge
        );
        assert_eq!(
            form().search("q", &too_big, "").unwrap_err(),
            FormError::TooLarge
        );
        assert_eq!(
            form().submit_label(&too_big).unwrap_err(),
            FormError::TooLarge
        );
    }
    assert!(
        form()
            .search("q", "Label", "")
            .unwrap()
            .hidden("h", "")
            .is_ok()
    );
}

#[test]
fn deterministic_form_query_mutations_never_panic() {
    let seed = b"search=Rust+%2B+HTML&page=1";
    for position in 0..seed.len() {
        for byte in 0..=127 {
            let mut input = seed.to_vec();
            input[position] = byte;
            let raw = std::str::from_utf8(&input).unwrap();
            let _ = Query::parse_with_mode(raw, QueryMode::Form);
        }
    }
    for byte in 0..=255 {
        let _ = Query::parse_with_mode(&format!("value=%{byte:02X}"), QueryMode::Form);
    }
}
