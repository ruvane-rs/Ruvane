use super::*;
fn ticket(id: u32, title: &str, revision: u64) -> Ticket {
    Ticket {
        id,
        workspace: 1,
        revision,
        input: TicketInput::new(title, "Description", "open").unwrap(),
    }
}
fn seeded(id: u32) -> Editor {
    let mut e = Editor::new(1, id);
    e.page_data(if id == 0 {
        scoped(model::empty_seed())
    } else {
        scoped(ticket(id, "Original", 1).seed("\"one\""))
    })
    .unwrap();
    e
}
fn reply(status: u16, tag: &str, value: JsonValue, mutation: bool) -> JsonOutcome {
    let mut frame = b"RVJ1\0\0".to_vec();
    frame.extend(status.to_le_bytes());
    frame.extend(0u32.to_le_bytes());
    frame.extend((tag.len() as u16).to_le_bytes());
    frame.extend(tag.as_bytes());
    frame.extend(value.encode().as_bytes());
    decode_json_reply(&frame, mutation)
}
fn failed() -> JsonOutcome {
    decode_json_reply(b"RVJ1\x01\x01\0\0\0\0\0\0\0\0", true)
}
#[test]
fn two_editors_keep_the_losing_draft_until_explicit_reload() {
    let mut a = seeded(1);
    let mut b = seeded(1);
    a.dispatch_value(10, EventValue::Text("Winner".into()))
        .unwrap();
    b.dispatch_value(10, EventValue::Text("My draft".into()))
        .unwrap();
    a.dispatch(1).unwrap();
    b.dispatch(1).unwrap();
    a.json_result(
        1,
        reply(200, "\"two\"", ticket(1, "Winner", 2).json(), true),
    )
    .unwrap();
    b.json_result(1, reply(412, "", JsonValue::null(), true))
        .unwrap();
    assert_eq!(a.title, "Winner");
    assert_eq!(b.title, "My draft");
    assert!(b.blocked);
    assert!(b.tag.as_ref().unwrap().as_str() == "\"one\"");
    assert!(b.dispatch(1).is_err());
    b.dispatch(3).unwrap();
    assert!(b.take_json().is_some());
    b.json_result(
        3,
        reply(200, "\"two\"", ticket(1, "Winner", 2).json(), false),
    )
    .unwrap();
    assert_eq!(b.title, "Winner");
    assert_eq!(b.revision, 2);
    assert!(!b.blocked);
}
#[test]
fn lost_create_acknowledgement_has_no_automatic_or_implicit_replay() {
    let mut e = seeded(0);
    e.dispatch_value(10, EventValue::Text("Uncertain ticket".into()))
        .unwrap();
    e.dispatch(1).unwrap();
    e.take_json().unwrap();
    e.json_result(1, failed()).unwrap();
    assert!(e.blocked && e.uncertain);
    assert_eq!(e.title, "Uncertain ticket");
    assert!(e.take_json().is_none());
    assert!(e.dispatch(1).is_err());
    let epoch = e.epoch;
    e.dispatch(4).unwrap();
    assert_eq!(e.title, "");
    assert!(!e.blocked);
    assert_ne!(e.epoch, epoch);
    e.dispatch_value(10, EventValue::Text("Second".into()))
        .unwrap();
    e.dispatch(1).unwrap();
    e.take_json();
    e.json_result(1, failed()).unwrap();
    let epoch = e.epoch;
    e.dispatch(4).unwrap();
    assert_ne!(e.epoch, epoch);
}
#[test]
fn pending_requests_validation_and_auth_loss_are_explicit() {
    let mut e = seeded(0);
    e.dispatch(1).unwrap();
    assert!(!e.pending);
    assert!(e.take_json().is_none());
    e.dispatch_value(10, EventValue::Text("Draft".into()))
        .unwrap();
    e.dispatch(1).unwrap();
    assert!(e.pending);
    assert!(e.dispatch(1).is_err());
    assert!(
        e.dispatch_value(10, EventValue::Text("Other".into()))
            .is_err()
    );
    e.take_json();
    e.json_result(1, reply(400, "", JsonValue::null(), true))
        .unwrap();
    assert_eq!(e.title, "Draft");
    assert!(!e.blocked);
    e.dispatch(1).unwrap();
    e.take_json();
    e.json_result(1, reply(403, "", JsonValue::null(), true))
        .unwrap();
    assert_eq!(e.title, "");
    assert_eq!(e.description, "");
    assert!(e.tag.is_none() && e.blocked);
    assert_eq!(e.revision, 0);
}
#[test]
fn malformed_success_is_uncertain_and_cannot_switch_an_existing_id() {
    let mut e = seeded(1);
    e.dispatch(1).unwrap();
    e.take_json();
    e.json_result(
        1,
        reply(200, "\"wrong\"", ticket(2, "Foreign", 2).json(), true),
    )
    .unwrap();
    assert_eq!(e.id, 1);
    assert_eq!(e.title, "Original");
    assert!(e.blocked && e.uncertain);
}
#[test]
fn seeds_reject_wrong_ids_weak_versions_unknown_fields_and_bad_pagination() {
    let mut e = Editor::new(1, 1);
    assert!(
        e.page_data(scoped(ticket(2, "Wrong", 1).seed("\"one\"")))
            .is_err()
    );
    assert!(
        e.page_data(scoped(ticket(1, "Weak", 1).seed("W/\"one\"")))
            .is_err()
    );
    let mut data = scoped(ticket(1, "Extra", 1).seed("\"one\""));
    data.insert("extra", ActionValue::Boolean(true)).unwrap();
    assert!(e.page_data(data).is_err());
    let mut listing = Listing {
        workspace: 1,
        workspace_name: "Ruvane".into(),
        role: Role::Editor,
        page: 0,
        total: 0,
        items: vec![],
    };
    let mut data = ActionData::default();
    for (k, v) in [("page", 1), ("total", 0), ("count", 0)] {
        data.insert(k, ActionValue::Number(v)).unwrap();
    }
    assert!(listing.page_data(data).is_err());
}
#[test]
fn full_fields_and_pages_fit_view_and_snapshot_limits() {
    let mut e = seeded(1);
    e.title = "é".repeat(60);
    e.description = "é".repeat(512);
    e.view().unwrap().snapshot().unwrap();
    let listing = Listing {
        workspace: 1,
        workspace_name: "Ruvane".into(),
        role: Role::Editor,
        page: 0,
        total: model::MAX_TICKETS,
        items: (1..=PAGE_SIZE)
            .map(|id| (id as u32, "é".repeat(60), State::Closed))
            .collect(),
    };
    let view = listing.view().unwrap();
    view.snapshot().unwrap();
    assert!(view.event_count() <= MAX_VIEW_EVENTS);
}

fn scoped(data: ActionData) -> ActionData {
    model::scope_seed(
        data,
        &model::Space {
            id: 1,
            name: "Ruvane".into(),
            revision: 1,
            members: [Some(Role::Owner); 4],
        },
        Role::Editor,
    )
}

#[test]
fn viewers_can_inspect_a_ticket_but_cannot_queue_mutations() {
    let mut e = Editor::new(1, 1);
    let space = model::Space {
        id: 1,
        name: "Ruvane".into(),
        revision: 1,
        members: [Some(Role::Viewer); 4],
    };
    e.page_data(model::scope_seed(
        ticket(1, "Read only", 1).seed("\"one\""),
        &space,
        Role::Viewer,
    ))
    .unwrap();
    let html = e.view().unwrap().render().unwrap();
    assert!(!html.as_str().contains("Save ticket"));
    assert!(html.as_str().contains("Attachments"));
    assert!(e.dispatch(1).is_err());
    assert!(e.dispatch(2).is_err());
    assert!(
        e.dispatch_value(10, EventValue::Text("Denied".into()))
            .is_err()
    );
    assert!(e.take_json().is_none());
}
#[test]
fn a_valid_response_from_another_workspace_cannot_replace_the_draft() {
    let mut e = seeded(1);
    e.dispatch(1).unwrap();
    e.take_json();
    let mut foreign = ticket(1, "Foreign", 2);
    foreign.workspace = 2;
    e.json_result(1, reply(200, "\"foreign\"", foreign.json(), true))
        .unwrap();
    assert_eq!(e.title, "Original");
    assert!(e.blocked && e.uncertain);
}
