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
        "<div data-ruvane-version=\"3\" data-ruvane-component=\"1\" data-ruvane-state=\"1:5\">"
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
