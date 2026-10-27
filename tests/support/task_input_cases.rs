use crate::task_input::{CreateTask, PatchTask, ReplaceTask};
use ruvane::{JsonObject, JsonValue, ValidationErrors};
pub const CASES: &[&str] = &[
    r#"{"title":"Rust"}"#,
    r#"{}"#,
    r#"{"title":null,"completed":null}"#,
    r#"{"title":3,"completed":"true"}"#,
    r#"{"title":"   ","completed":false}"#,
    r#"{"title":"line\nbreak","completed":true}"#,
    r#"{"title":"Rust 🦀","completed":true}"#,
    r#"{"completed":false}"#,
    r#"{"title":"Keep spaces  ","PRIVATE_UNKNOWN_KEY":"PRIVATE_VALUE"}"#,
    r#"null"#,
    r#"["PRIVATE_VALUE"]"#,
    r#"{"title":""}"#,
    r#"{"title":"éééééééé","completed":0}"#,
    r#"{"n":18446744073709551615,"i":-9223372036854775808}"#,
    r#"{"n":18446744073709551616,"i":-9223372036854775809}"#,
    r#"{"n":1e0,"i":"1"}"#,
];
fn result<T>(value: Result<T, ValidationErrors>) -> JsonValue {
    match value {
        Ok(_) => JsonValue::from(true),
        Err(errors) => errors.to_json(),
    }
}
pub fn report(index: usize) -> Option<JsonValue> {
    let value = JsonValue::parse(CASES.get(index)?.as_bytes()).ok()?;
    Some(
        JsonValue::array(vec![
            result(CreateTask::try_from(&value)),
            result(ReplaceTask::try_from(&value)),
            result(PatchTask::try_from(&value)),
            numbers(&value),
        ])
        .unwrap(),
    )
}

fn numbers(value: &JsonValue) -> JsonValue {
    let mut input = JsonObject::new(value, &["n", "i"]);
    let unsigned = input.required_u64("n", 0, u64::MAX);
    let signed = input.required_i64("i", i64::MIN, i64::MAX);
    match input.finish() {
        Ok(()) => JsonValue::object(vec![
            ("unsigned".into(), unsigned.unwrap().into()),
            ("signed".into(), signed.unwrap().into()),
        ])
        .unwrap(),
        Err(errors) => errors.to_json(),
    }
}
