use ruvane::{
    ActionData, ActionError, ActionReply, ActionRequest, ActionValue, FormData, InternalUrl,
    MAX_ACTION_FIELDS, MAX_ACTION_REPLY_BYTES,
};

fn data() -> ActionData {
    let mut data = ActionData::default();
    data.insert(
        "message",
        ActionValue::Text("\u{feff}<script> 🦀\n\t".into()),
    )
    .unwrap();
    data.insert("id", ActionValue::Number(u64::MAX)).unwrap();
    data.insert("done", ActionValue::Boolean(true)).unwrap();
    data
}
#[test]
fn typed_reply_roundtrips_without_integer_or_text_coercion() {
    for reply in [
        ActionReply::success(data()),
        ActionReply::validation(data()),
    ] {
        assert_eq!(ActionReply::decode(&reply.encode()), Ok(reply.clone()));
        assert_eq!(reply.data().number("id"), Some(u64::MAX));
        assert_eq!(reply.data().boolean("done"), Some(true));
        assert_eq!(reply.data().text("id"), None);
        assert!(
            reply
                .data()
                .text("message")
                .unwrap()
                .contains("<script> 🦀\n\t")
        );
        let bytes = reply.encode();
        for n in 0..bytes.len() {
            assert!(ActionReply::decode(&bytes[..n]).is_err());
        }
        assert!(ActionReply::decode(&[bytes.as_slice(), &[0]].concat()).is_err());
    }
}
#[test]
fn reply_fields_are_unique_bounded_and_failed_insertions_are_atomic() {
    let mut value = data();
    let before = value.clone();
    for (name, field) in [
        ("done", ActionValue::Boolean(false)),
        ("bad.name", ActionValue::Number(1)),
        ("text", ActionValue::Text("\0".into())),
        ("text", ActionValue::Text("x".repeat(1025))),
    ] {
        assert!(value.insert(name, field).is_err());
        assert_eq!(value, before);
    }
    let mut fields = ActionData::default();
    for n in 0..MAX_ACTION_FIELDS {
        fields
            .insert(&format!("k{n}"), ActionValue::Boolean(false))
            .unwrap();
    }
    assert_eq!(
        fields.insert("extra", ActionValue::Boolean(false)),
        Err(ActionError::Limit)
    );
    let mut large = ActionData::default();
    let mut n = 0;
    while large
        .insert(&format!("k{n}"), ActionValue::Text("🦀".repeat(256)))
        .is_ok()
    {
        n += 1;
    }
    let reply = ActionReply::success(large);
    assert!(reply.encode().len() <= MAX_ACTION_REPLY_BYTES);
    assert!(n > 60);
    assert!(ActionReply::decode(&vec![0; MAX_ACTION_REPLY_BYTES + 1]).is_err());
    for bytes in [
        b"RVA1\x02\0\0".as_slice(),
        b"RVA1\0\xA1\0",
        b"RVA1\0\x01\0\x01a\x03\x02",
        b"RVA1\0\x01\0\x01a\x01\x01\0\0\0\xff",
        b"RVA1\0\x02\0\x01a\x03\0\x01a\x03\x01",
    ] {
        assert!(ActionReply::decode(bytes).is_err());
    }
}
#[test]
fn outgoing_actions_encode_fields_once_and_refuse_external_or_oversized_requests() {
    let fields = FormData::default()
        .field("title", "Rust + & %2F")
        .unwrap()
        .field("notes", "one\ntwo\t🦀")
        .unwrap();
    let request = ActionRequest::new(
        2,
        InternalUrl::new("/actions/tasks/create").unwrap(),
        fields.clone(),
    )
    .unwrap();
    assert_eq!(request.action(), 2);
    assert_eq!(FormData::parse(request.body().as_bytes()).unwrap(), fields);
    assert!(request.body().contains("%252F"));
    assert!(
        ActionRequest::new(
            0,
            InternalUrl::new("/actions").unwrap(),
            FormData::default()
        )
        .is_err()
    );
    assert!(
        ActionRequest::new(
            1,
            InternalUrl::new("/actions")
                .unwrap()
                .query("q", "1")
                .unwrap(),
            FormData::default()
        )
        .is_err()
    );
    let mut fields = FormData::default();
    for n in 0..8 {
        fields = fields.field(&format!("k{n}"), &"🦀".repeat(256)).unwrap();
    }
    assert!(ActionRequest::new(1, InternalUrl::new("/actions").unwrap(), fields).is_err());
    assert!(
        FormData::default()
            .field("name", "x")
            .unwrap()
            .field("name", "y")
            .is_err()
    );
    assert!(FormData::default().field("name", "\r").is_err());
}
#[test]
fn deterministic_reply_byte_mutations_never_panic() {
    let seed = ActionReply::success(data()).encode();
    for n in 0..seed.len() {
        for byte in 0..=255 {
            let mut bytes = seed.clone();
            bytes[n] = byte;
            let _ = ActionReply::decode(&bytes);
        }
    }
}
