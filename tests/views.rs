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
        "<div data-ruvane-version=\"2\" data-ruvane-component=\"1\" data-ruvane-state=\"1:5\">"
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
fn prop_encoding_and_fixed_structure_updates_are_checked() {
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
    assert_eq!(
        changed.updates(&old, 1).unwrap_err(),
        ViewError::ShapeChanged
    );
    changed.leaf(0, Span, "new").unwrap();
    assert_eq!(
        changed.updates(&old, 1).unwrap_err(),
        ViewError::ShapeChanged
    );
}
