use ruvane::{Attribute, Document, Html, Tag, element, fragment, text};

#[test]
fn user_markup_is_rendered_as_text() {
    let payload = "<script>alert(\"x\")</script> & 'quoted'";
    assert_eq!(
        text(payload).as_str(),
        "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;quoted&#39;"
    );
}

#[test]
fn existing_entities_are_treated_as_literal_input() {
    assert_eq!(
        text("&lt;img&gt;&#34;").as_str(),
        "&amp;lt;img&amp;gt;&amp;#34;"
    );
}

#[test]
fn unicode_text_is_preserved() {
    assert_eq!(text("Rust 🦀 日本語").as_str(), "Rust 🦀 日本語");
}

#[test]
fn null_characters_are_replaced_in_text_and_attributes() {
    assert_eq!(text("a\0b").as_str(), "a\u{fffd}b");
    assert_eq!(
        element(Tag::P)
            .attr(Attribute::Title, "a\0b")
            .render()
            .as_str(),
        "<p title=\"a\u{fffd}b\"></p>"
    );
}

#[test]
fn quoted_attribute_payload_cannot_add_markup() {
    let html = element(Tag::Div)
        .attr(
            Attribute::Title,
            "\" onmouseover=\"alert(1)\"><script>x</script>",
        )
        .render();
    assert_eq!(
        html.as_str(),
        "<div title=\"&quot; onmouseover=&quot;alert(1)&quot;&gt;&lt;script&gt;x&lt;/script&gt;\"></div>"
    );
}

#[test]
fn attribute_replacement_does_not_create_duplicate_attributes() {
    let html = element(Tag::Div)
        .attr(Attribute::Id, "first")
        .attr(Attribute::Class, "container")
        .attr(Attribute::Id, "replacement")
        .render();
    assert_eq!(
        html.as_str(),
        "<div id=\"replacement\" class=\"container\"></div>"
    );
}

#[test]
fn nested_elements_keep_markup_and_escape_only_their_text() {
    let html = element(Tag::Main)
        .child(element(Tag::H1).child("Rust & HTML"))
        .child(element(Tag::P).child(String::from("<untrusted>")))
        .render();
    assert_eq!(
        html.as_str(),
        "<main><h1>Rust &amp; HTML</h1><p>&lt;untrusted&gt;</p></main>"
    );
}

#[test]
fn fragments_keep_order_without_double_escaping() {
    let children = fragment([
        text("a & b"),
        element(Tag::Strong).child("<c>").render(),
        text("d"),
    ]);
    assert_eq!(
        element(Tag::P).child(children).render().as_str(),
        "<p>a &amp; b<strong>&lt;c&gt;</strong>d</p>"
    );
}

#[test]
fn empty_fragments_and_default_html_are_empty() {
    assert_eq!(fragment([]).as_str(), "");
    assert_eq!(Html::default().as_str(), "");
}

#[test]
fn document_contains_a_doctype_and_utf8_declaration() {
    let page = Document::new("Ruvane", element(Tag::H1).child("Hello"))
        .language("en-GB")
        .render();
    assert_eq!(
        page.as_str(),
        "<!doctype html><html lang=\"en-GB\"><head><meta charset=\"utf-8\"><title>Ruvane</title><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"></head><body><h1>Hello</h1></body></html>"
    );
}

#[test]
fn document_title_cannot_break_out_of_its_element() {
    let page = Document::new("</title><script>alert(1)</script>", "").render();
    assert!(
        page.as_str()
            .contains("<title>&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;</title>")
    );
    assert!(!page.as_str().contains("<script>"));
}

#[test]
fn document_language_cannot_add_an_attribute() {
    let page = Document::new("", "")
        .language("en-GB\" onload=\"alert(1)")
        .render();
    assert!(
        page.as_str()
            .starts_with("<!doctype html><html lang=\"en-GB&quot; onload=&quot;alert(1)\">")
    );
}

#[test]
fn document_body_strings_are_text() {
    let page = Document::new("", "<b>literal</b>").render();
    assert!(
        page.as_str()
            .contains("<body>&lt;b&gt;literal&lt;/b&gt;</body>")
    );
}

#[test]
fn owned_and_borrowed_strings_have_the_same_text_semantics() {
    let borrowed: Html = "<p>&</p>".into();
    let owned: Html = String::from("<p>&</p>").into();
    assert_eq!(borrowed, owned);
    assert_eq!(borrowed.as_str(), "&lt;p&gt;&amp;&lt;/p&gt;");
}

#[test]
fn display_and_owned_output_agree() {
    let html = element(Tag::Code).child("a < b").render();
    assert_eq!(format!("{html}"), html.clone().into_string());
    assert_eq!(html.as_ref(), html.as_str());
}

#[test]
fn lists_compose_from_an_iterator_of_fragments() {
    let items = ["<one>", "two & three"];
    let children = fragment(
        items
            .into_iter()
            .map(|item| element(Tag::Li).child(item).render()),
    );
    assert_eq!(
        element(Tag::Ul).child(children).render().as_str(),
        "<ul><li>&lt;one&gt;</li><li>two &amp; three</li></ul>"
    );
}

#[test]
fn text_output_has_no_raw_markup_delimiters_or_nulls() {
    let mut input = String::new();
    for scalar in 0..=0x7f {
        input.push(char::from_u32(scalar).expect("ASCII scalar"));
    }
    input.push_str("λ🦀<script>&#0;\"'");
    let output = text(input).into_string();
    assert!(!output.contains(['<', '>', '"', '\'', '\0']));
}
