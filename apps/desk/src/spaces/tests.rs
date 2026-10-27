use super::*;
fn seed_members() -> ActionData {
    let mut d = ActionData::default();
    for (key, n) in [("workspace", 1), ("revision", 1)] {
        d.insert(key, ActionValue::Number(n)).unwrap();
    }
    for (key, value) in [
        ("workspace_name", "Ruvane"),
        ("actor", "alex"),
        ("etag", "\"one\""),
        ("alex", "owner"),
        ("blair", "editor"),
        ("casey", "viewer"),
        ("drew", "none"),
    ] {
        d.insert(key, ActionValue::Text(value.into())).unwrap();
    }
    d
}
fn reply(status: u16, tag: &str, body: JsonValue, mutation: bool) -> JsonOutcome {
    let mut frame = b"RVJ1\0\0".to_vec();
    frame.extend(status.to_le_bytes());
    frame.extend(0u32.to_le_bytes());
    frame.extend((tag.len() as u16).to_le_bytes());
    frame.extend(tag.as_bytes());
    frame.extend(body.encode().as_bytes());
    decode_json_reply(&frame, mutation)
}
#[test]
fn all_workspaces_and_member_controls_fit_the_snapshot_budget() {
    let mut d = ActionData::default();
    d.insert("count", ActionValue::Number(4)).unwrap();
    d.insert("actor", ActionValue::Text("alex".into())).unwrap();
    for i in 0..4 {
        d.insert(&format!("id{i}"), ActionValue::Number(i + 1))
            .unwrap();
        d.insert(&format!("name{i}"), ActionValue::Text("x".repeat(48)))
            .unwrap();
        d.insert(&format!("role{i}"), ActionValue::Text("owner".into()))
            .unwrap();
    }
    let mut c = workspaces();
    c.page_data(d).unwrap();
    c.view().unwrap().snapshot().unwrap();
    let mut m = members(1);
    m.page_data(seed_members()).unwrap();
    m.view().unwrap().snapshot().unwrap();
}
#[test]
fn membership_conflicts_preserve_the_draft_and_require_explicit_reload() {
    let mut m = members(1);
    m.page_data(seed_members()).unwrap();
    m.dispatch(11).unwrap();
    m.dispatch(1).unwrap();
    m.take_json().unwrap();
    m.json_result(1, reply(412, "", JsonValue::null(), true))
        .unwrap();
    let html = m.view().unwrap().render().unwrap();
    assert!(html.as_str().contains("Owner · change role"));
    assert!(html.as_str().contains("Conflict."));
    assert!(m.take_json().is_none());
    m.dispatch(2).unwrap();
    assert!(m.take_json().is_some());
}
#[test]
fn membership_loss_removes_member_controls_and_private_values() {
    let mut m = members(1);
    m.page_data(seed_members()).unwrap();
    m.dispatch(2).unwrap();
    m.take_json();
    m.json_result(2, reply(403, "", JsonValue::null(), false))
        .unwrap();
    let html = m.view().unwrap().render().unwrap();
    assert!(!html.as_str().contains("change role"));
    assert!(!html.as_str().contains("Save members"));
    assert!(m.dispatch(1).is_err());
}
#[test]
fn last_owner_removal_stays_a_local_validation_error() {
    let mut m = members(1);
    m.page_data(seed_members()).unwrap();
    m.dispatch(10).unwrap();
    m.dispatch(1).unwrap();
    assert!(m.take_json().is_none());
    assert!(
        m.view()
            .unwrap()
            .render()
            .unwrap()
            .as_str()
            .contains("Keep at least one owner")
    );
}
