use ruvane::{
    BrowserTag::*, ComponentProps, InitialState, MAX_BROWSER_TEXT_BYTES, MAX_VIEW_DEPTH,
    MAX_VIEW_EVENTS, MAX_VIEW_NODES, View, ViewError,
};
#[path = "../examples/support/widgets.rs"]
mod widgets;

#[test]
fn shared_components_render_their_initial_state_and_update_independently() {
    let props = ComponentProps::new(widgets::COUNTER, InitialState::Number(5)).unwrap();
    let mut counter = widgets::create(props).unwrap();
    let view = counter.view().unwrap();
    let html = view.hydratable(props).unwrap();
    assert!(html.as_str().starts_with(
        "<div data-ruvane-version=\"15\" data-ruvane-component=\"1\" data-ruvane-state=\"1:5\">"
    ));
    assert!(html.as_str().contains(">5</output>"));
    assert!(html.as_str().contains(">10</output>"));
    assert_eq!(html.as_str().matches(" disabled>").count(), 3);
    assert_eq!(
        view.render()
            .unwrap()
            .as_str()
            .matches(" disabled>")
            .count(),
        0
    );
    let mut toggle =
        widgets::create(ComponentProps::new(widgets::TOGGLE, InitialState::Boolean(true)).unwrap())
            .unwrap();
    toggle.dispatch(7).unwrap();
    assert!(
        toggle
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains(">Disabled</output>")
    );
    counter.dispatch(1).unwrap();
    let next = counter.view().unwrap();
    assert!(next.render().unwrap().as_str().contains(">6</output>"));
    assert!(next.render().unwrap().as_str().contains(">12</output>"));
    assert_eq!(next.updates(&view, 1).unwrap().count(), 2);
    assert_eq!(next.updates(&next, 1).unwrap().count(), 0);
}

#[test]
fn text_is_escaped_and_cannot_add_hydration_attributes() {
    let mut view = View::new();
    let label = "<script>\" data-ruvane-node=\"99\" & 🌱\n\t\u{feff}";
    view.leaf(0, Span, label).unwrap();
    let html = view.render().unwrap();
    assert!(
        html.as_str()
            .contains("&lt;script&gt;&quot; data-ruvane-node=&quot;99&quot; &amp; 🌱")
    );
    assert_eq!(html.as_str().matches("<span").count(), 1);
    assert!(!html.as_str().contains("<script>"));
    assert!(
        view.commands(1)
            .unwrap()
            .as_bytes()
            .windows(label.len())
            .any(|w| w == label.as_bytes())
    );
    for invalid in ["a\0b", "a\rb", "a\u{7f}b", "a\u{85}b"] {
        assert_eq!(view.leaf(0, Span, invalid), Err(ViewError::InvalidText));
    }
    assert_eq!(view.leaf(0, Span, "ok").unwrap(), 2);
}

#[test]
fn builder_rejects_invalid_html_without_consuming_ids() {
    let mut view = View::new();
    assert_eq!(view.render(), Err(ViewError::Empty));
    assert_eq!(view.commands(1).unwrap_err(), ViewError::Empty);
    assert_eq!(view.element(0, Button), Err(ViewError::InvalidStructure));
    assert_eq!(view.leaf(0, Section, "x"), Err(ViewError::InvalidStructure));
    assert_eq!(
        view.button(0, "x", 0, false),
        Err(ViewError::InvalidIdentifier)
    );
    assert_eq!(view.element(99, Section), Err(ViewError::InvalidIdentifier));
    let paragraph = view.element(0, Paragraph).unwrap();
    for tag in [Section, Paragraph, Heading] {
        assert_eq!(
            view.element(paragraph, tag),
            Err(ViewError::InvalidStructure)
        );
    }
    let span = view.leaf(paragraph, Span, "x").unwrap();
    assert_eq!(span, 2);
    assert_eq!(
        view.leaf(span, Span, "nested"),
        Err(ViewError::InvalidStructure)
    );
}

#[test]
fn node_depth_event_and_utf8_byte_limits_are_exact() {
    let mut nodes = View::new();
    for _ in 0..MAX_VIEW_NODES {
        nodes.leaf(0, Span, "x").unwrap();
    }
    assert_eq!(nodes.leaf(0, Span, "x"), Err(ViewError::Limit));
    let mut depth = View::new();
    let mut parent = 0;
    for _ in 0..MAX_VIEW_DEPTH {
        parent = depth.element(parent, Section).unwrap();
    }
    assert_eq!(depth.leaf(parent, Span, "x"), Err(ViewError::Limit));
    let mut events = View::new();
    for event in 1..=MAX_VIEW_EVENTS {
        events.button(0, "x", event as u32, false).unwrap();
    }
    assert_eq!(events.button(0, "x", 99, false), Err(ViewError::Limit));
    let mut text = View::new();
    text.leaf(0, Span, &"λ".repeat(MAX_BROWSER_TEXT_BYTES / 2))
        .unwrap();
    assert_eq!(
        text.leaf(0, Span, &"x".repeat(MAX_BROWSER_TEXT_BYTES + 1)),
        Err(ViewError::Limit)
    );
}

#[test]
fn props_and_structure_updates_are_checked() {
    for state in [
        InitialState::Number(0),
        InitialState::Number(u32::MAX),
        InitialState::Boolean(false),
        InitialState::Boolean(true),
    ] {
        let (kind, value) = state.to_wire();
        assert_eq!(InitialState::from_wire(kind, value), Ok(state));
    }
    for (kind, value) in [(0, 0), (3, 0), (2, 2), (2, u32::MAX)] {
        assert_eq!(
            InitialState::from_wire(kind, value),
            Err(ViewError::InvalidState)
        );
    }
    assert_eq!(
        ComponentProps::new(0, InitialState::Number(0)),
        Err(ViewError::InvalidIdentifier)
    );
    let mut old = View::new();
    old.button(0, "One", 1, false).unwrap();
    let mut next = View::new();
    next.button(0, "Two", 1, true).unwrap();
    assert_eq!(next.updates(&old, 1).unwrap().count(), 2);
    assert_eq!(
        next.updates(&old, 0).unwrap_err(),
        ViewError::InvalidIdentifier
    );
    let mut changed = View::new();
    changed.button(0, "One", 2, false).unwrap();
    assert!(changed.updates(&old, 1).unwrap().count() > 0);
    changed.leaf(0, Span, "new").unwrap();
    assert!(changed.updates(&old, 1).unwrap().count() > 0);
}

fn rows(keys: &[u32]) -> View {
    let mut view = View::new();
    let list = view.keyed_element(0, 1, List).unwrap();
    for key in keys {
        let row = view.keyed_element(list, *key, ListItem).unwrap();
        view.keyed_leaf(row, 1, Span, &format!("Row {key}"))
            .unwrap();
        view.keyed_button(row, 2, "Run", *key, false).unwrap();
    }
    view
}

#[test]
fn keyed_reorder_retains_handles_and_removed_keys_get_fresh_handles() {
    let original = rows(&[1, 2, 3]).snapshot().unwrap();
    let list = original.node_for_key(0, 1).unwrap();
    let a = original.node_for_key(list, 1).unwrap();
    let b = original.node_for_key(list, 2).unwrap();
    let c = original.node_for_key(list, 3).unwrap();
    let event_b = original.node_for_event(2).unwrap();
    let (changed, batch) = original.update(&rows(&[3, 1]), 1).unwrap();
    assert!(batch.count() > 0);
    assert_eq!(changed.node_for_key(0, 1), Some(list));
    assert_eq!(changed.node_for_key(list, 1), Some(a));
    assert_eq!(changed.node_for_key(list, 3), Some(c));
    assert_eq!(changed.node_for_key(list, 2), None);
    assert!(!changed.has_event(event_b, 2));
    let (restored, _) = changed.update(&rows(&[2, 3, 1]), 1).unwrap();
    assert!(restored.node_for_key(list, 2).unwrap() > b);
    assert!(restored.node_for_event(2).unwrap() > event_b);
    assert!(!restored.has_event(event_b, 2));
}

#[test]
fn duplicate_keys_are_rejected_per_parent_without_mutating_the_view() {
    let mut view = View::new();
    let one = view.keyed_element(0, 1, Section).unwrap();
    let two = view.keyed_element(0, 2, Section).unwrap();
    view.keyed_leaf(one, 7, Span, "First").unwrap();
    let before = view.clone();
    assert_eq!(
        view.keyed_button(one, 7, "Duplicate", 1, false),
        Err(ViewError::DuplicateKey)
    );
    assert_eq!(view, before);
    assert!(view.keyed_leaf(two, 7, Span, "Other parent").is_ok());
    assert_eq!(
        view.keyed_leaf(two, 0, Span, "Zero"),
        Err(ViewError::InvalidIdentifier)
    );
    assert!(view.keyed_leaf(two, u32::MAX, Span, "Maximum key").is_ok());
    assert!(
        view.render()
            .unwrap()
            .as_str()
            .contains("data-ruvane-key=\"4294967295\"")
    );
    assert_eq!(view.element(0, ListItem), Err(ViewError::InvalidStructure));
    let list = view.element(one, List).unwrap();
    assert_eq!(
        view.leaf(list, Span, "Wrong child"),
        Err(ViewError::InvalidStructure)
    );
    let paragraph = view.element(one, Paragraph).unwrap();
    assert_eq!(
        view.leaf(paragraph, Heading, "Wrong heading"),
        Err(ViewError::InvalidStructure)
    );
}

#[test]
fn thousand_key_lifecycles_do_not_alias_previous_bindings() {
    let mut snapshot = rows(&[1]).snapshot().unwrap();
    let list = snapshot.node_for_key(0, 1).unwrap();
    let a = snapshot.node_for_key(list, 1).unwrap();
    let mut last = 0;
    for _ in 0..1000 {
        let (next, _) = snapshot.update(&rows(&[1, 2]), 1).unwrap();
        snapshot = next;
        let binding = snapshot.node_for_event(2).unwrap();
        assert!(binding > last);
        assert!(!snapshot.has_event(last, 2));
        last = binding;
        let (next, _) = snapshot.update(&rows(&[1]), 1).unwrap();
        snapshot = next;
        assert!(!snapshot.has_event(binding, 2));
        assert_eq!(snapshot.node_for_key(list, 1), Some(a));
    }
}

#[test]
fn tag_and_binding_changes_replace_the_node_lifetime() {
    let mut first = View::new();
    first.keyed_button(0, 1, "Run", 1, false).unwrap();
    let snapshot = first.snapshot().unwrap();
    let old = snapshot.node_for_event(1).unwrap();
    let mut second = View::new();
    second.keyed_button(0, 1, "Run", 2, false).unwrap();
    let (changed, _) = snapshot.update(&second, 1).unwrap();
    assert_ne!(changed.node_for_event(2), Some(old));
    let (restored, _) = changed.update(&first, 1).unwrap();
    assert_ne!(restored.node_for_event(1), Some(old));
    let mut different = View::new();
    different.keyed_leaf(0, 1, Output, "Result").unwrap();
    let (changed, _) = restored.update(&different, 1).unwrap();
    assert_eq!(changed.node_for_event(1), None);
    assert!(changed.node_for_key(0, 1).unwrap() > restored.node_for_key(0, 1).unwrap());
}

#[test]
fn list_state_survives_order_changes_and_scopes_drop_with_items() {
    let props = ComponentProps::new(widgets::ITEM_LIST, InitialState::Number(3)).unwrap();
    let mut component = widgets::create(props).unwrap();
    assert_eq!(component.stats().signals, 6);
    component.dispatch(20).unwrap();
    component.dispatch(26).unwrap();
    assert_eq!(component.stats().signals, 4);
    component.dispatch(2).unwrap();
    let html = component.view().unwrap().render().unwrap().into_string();
    assert!(html.find("Item C:").unwrap() < html.find("Item A:").unwrap());
    assert!(html.contains(">1</output>"));
    component.dispatch(21).unwrap();
    component.dispatch(3).unwrap();
    assert!(
        !component
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains("Item A:")
    );
    assert_eq!(component.stats().signals, 4);
    component.dispatch(3).unwrap();
    assert!(
        component
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains(">1</output>")
    );
    component.dispatch(22).unwrap();
    component.dispatch(30).unwrap();
    assert_eq!(component.stats().signals, 0);
    assert!(
        component
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains("No visible items.")
    );
}

#[test]
fn controls_require_labels_and_valid_html_and_escape_initial_values() {
    let mut view = View::new();
    assert_eq!(
        view.input(0, 1, "text", 1, false),
        Err(ViewError::InvalidStructure)
    );
    assert_eq!(
        view.submit(0, 1, "Go", false),
        Err(ViewError::InvalidStructure)
    );
    let form = view.form(0, 1, 1).unwrap();
    assert_eq!(view.form(form, 1, 2), Err(ViewError::InvalidStructure));
    let label = view.keyed_element(form, 1, Label).unwrap();
    view.keyed_leaf(label, 1, Span, "Name").unwrap();
    let title = view.input(label, 2, "\"<script>& 🦀", 2, false).unwrap();
    let snapshot = view.snapshot().unwrap();
    let handle = snapshot.node_for_event(2).unwrap();
    assert!(snapshot.accepts_value(handle, 2, &ruvane::EventValue::Text("Rust 🦀".into())));
    assert!(!snapshot.accepts_value(handle, 2, &ruvane::EventValue::Text("a\nb".into())));
    assert!(!snapshot.accepts_value(handle, 2, &ruvane::EventValue::Boolean(true)));
    let html = view.render().unwrap();
    assert!(
        html.as_str()
            .contains("value=\"&quot;&lt;script&gt;&amp; 🦀\"")
    );
    assert!(!html.as_str().contains("</input>"));
    assert!(
        view.hydratable(ComponentProps::new(5, InitialState::Number(0)).unwrap())
            .unwrap()
            .as_str()
            .contains("disabled>")
    );
    view.field_disabled(title, true).unwrap();
    assert!(!view.snapshot().unwrap().accepts_value(
        handle,
        2,
        &ruvane::EventValue::Text("x".into())
    ));
    assert!(view.input(label, 3, "second", 3, false).is_ok());
    assert_eq!(view.render(), Err(ViewError::InvalidStructure));
}

#[test]
fn textarea_newlines_and_field_reset_epochs_roundtrip() {
    let mut view = View::new();
    let label = view.keyed_element(0, 1, Label).unwrap();
    view.keyed_leaf(label, 1, Span, "Notes").unwrap();
    let field = view
        .textarea(label, 2, "\n</textarea> & 🦀\n\t", 1, false)
        .unwrap();
    assert!(
        view.render()
            .unwrap()
            .as_str()
            .contains(">\n\n&lt;/textarea&gt; &amp; 🦀\n\t</textarea>")
    );
    let initial = view.snapshot().unwrap();
    assert!(initial.accepts_value(
        initial.node_for_event(1).unwrap(),
        1,
        &ruvane::EventValue::Text("\n\t".into())
    ));
    view.field_epoch(field, 1).unwrap();
    let (snapshot, commands) = initial.update(&view, 1).unwrap();
    assert!(commands.count() > 0);
    view.field_epoch(field, 0).unwrap();
    assert!(matches!(
        snapshot.update(&view, 1),
        Err(ViewError::InvalidState)
    ));
}

#[test]
fn task_form_validates_and_keeps_user_strings_as_text() {
    use ruvane::EventValue;
    let mut component =
        widgets::create(ComponentProps::new(widgets::TASK_FORM, InitialState::Number(0)).unwrap())
            .unwrap();
    component.dispatch(1).unwrap();
    assert!(
        component
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains("Enter a task name.")
    );
    component
        .dispatch_value(5, EventValue::Text("🦀".repeat(21)))
        .unwrap();
    component.dispatch(1).unwrap();
    assert!(
        component
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains("80 UTF-8 bytes")
    );
    component
        .dispatch_value(5, EventValue::Text("<script>task</script>".into()))
        .unwrap();
    component
        .dispatch_value(6, EventValue::Text("notes & text".into()))
        .unwrap();
    component.dispatch(1).unwrap();
    let html = component.view().unwrap().render().unwrap().into_string();
    assert!(html.contains("&lt;script&gt;task&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
    assert_eq!(component.stats().signals, 1);
    component
        .dispatch_value(20, EventValue::Boolean(true))
        .unwrap();
    component.dispatch(3).unwrap();
    assert!(
        component
            .view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains("No visible tasks.")
    );
    component.dispatch(3).unwrap();
    component.dispatch(21).unwrap();
    assert_eq!(component.stats().signals, 0);
}

#[test]
fn download_views_emit_native_links_with_checked_retained_targets() {
    use ruvane::InternalUrl;
    let mut view = View::new();
    let paragraph = view.element(0, Paragraph).unwrap();
    view.download(
        paragraph,
        1,
        &InternalUrl::new("/private/files/a")
            .unwrap()
            .query("x", "1")
            .unwrap()
            .query("y", "2")
            .unwrap(),
        "Download <&",
    )
    .unwrap();
    let html = view.render().unwrap();
    assert!(
        html.as_str()
            .contains("download=\"\" href=\"/private/files/a?x=1&amp;y=2\">Download &lt;&amp;</a>")
    );
    assert!(!html.as_str().contains("data-ruvane-nav"));
    assert!(!html.as_str().contains("prefetch"));
    assert!(!html.as_str().contains("disabled"));
    let mut next = View::new();
    let paragraph = next.element(0, Paragraph).unwrap();
    next.download(
        paragraph,
        1,
        &InternalUrl::new("/private/files/b").unwrap(),
        "Download <&",
    )
    .unwrap();
    let updates = next.updates(&view, 1).unwrap();
    assert!(updates.as_bytes().starts_with(b"RVN9"));
    assert!(
        updates
            .as_bytes()
            .windows(16)
            .any(|bytes| bytes == b"/private/files/b")
    );
    assert_eq!(view.element(0, Download), Err(ViewError::InvalidStructure));
}
