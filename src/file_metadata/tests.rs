use super::*;
fn info(id: u32) -> FileInfo {
    FileInfo::new(
        FileId::parse(&format!("{id:064x}")).unwrap(),
        "<img src=x> Rust 🦀.bin",
        5,
    )
    .unwrap()
}
#[test]
fn records_and_initial_seeds_round_trip_without_storage_fields() {
    let page = FileListing::new(vec![info(1), info(2), info(3)], Some(info(3).id())).unwrap();
    assert_eq!(FileListing::from_json(&page.to_json()).unwrap(), page);
    assert_eq!(
        FileListing::from_seed(&page.to_seed().unwrap()).unwrap(),
        page
    );
    let encoded = page.to_json().encode();
    assert!(
        !encoded.contains("owner") && !encoded.contains("path") && !encoded.contains("payload")
    );
    assert_eq!(page.to_seed().unwrap().len(), 11);
    assert!(!format!("{page:?}").contains("<img"));
}
#[test]
fn metadata_rejects_extra_members_coercion_and_out_of_range_data() {
    let id = format!("{:064x}", 1);
    for body in [
        format!(r#"{{"id":"{id}","name":"x","bytes":"5"}}"#),
        format!(r#"{{"id":"{id}","name":"x","bytes":32769}}"#),
        format!(r#"{{"id":"{id}","name":"../x","bytes":5}}"#),
        format!(r#"{{"id":"{id}","name":"x","bytes":5,"owner":"PRIVATE"}}"#),
        format!(r#"{{"id":"{id}","name":"x","bytes":5.0}}"#),
    ] {
        assert!(FileInfo::from_json(&JsonValue::parse(body.as_bytes()).unwrap()).is_err());
    }
    assert!(FileId::parse(&"A".repeat(64)).is_err());
    assert!(FileId::parse(&"0".repeat(64)).is_err());
    assert!(FileInfo::new(info(1).id(), "x", 32769).is_err());
}
#[test]
fn cursors_require_ordered_unique_bounded_full_pages() {
    for (files, next) in [
        (vec![info(2), info(1)], None),
        (vec![info(1), info(1)], None),
        (vec![info(1)], Some(info(1).id())),
        (vec![info(1), info(2), info(3)], Some(info(4).id())),
        ((1..=4).map(info).collect(), None),
    ] {
        assert!(FileListing::new(files, next).is_err());
    }
    for value in [
        r#"{"files":[],"next":false}"#,
        r#"{"files":[],"next":null,"owner":"PRIVATE"}"#,
        r#"{"files":[]}"#,
    ] {
        assert!(FileListing::from_json(&JsonValue::parse(value.as_bytes()).unwrap()).is_err());
    }
}
#[test]
fn seeds_require_exact_types_and_no_unknown_fields() {
    let mut data = FileListing::new(vec![], None).unwrap().to_seed().unwrap();
    data.insert("owner", ActionValue::Text("PRIVATE".into()))
        .unwrap();
    assert!(FileListing::from_seed(&data).is_err());
    let mut data = ActionData::default();
    data.insert("count", ActionValue::Text("0".into())).unwrap();
    data.insert("next", ActionValue::Text("".into())).unwrap();
    assert!(FileListing::from_seed(&data).is_err());
    let page = FileListing::new(
        vec![FileInfo::new(info(1).id(), &"\"".repeat(255), 32768).unwrap()],
        None,
    )
    .unwrap();
    assert!(page.to_json().encode().len() < 4096);
    assert_eq!(
        FileListing::from_seed(&page.to_seed().unwrap()).unwrap(),
        page
    );
}
