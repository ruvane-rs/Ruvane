use super::*;
fn codes(errors: &ValidationErrors) -> Vec<(Option<&'static str>, ValidationCode)> {
    errors.iter().map(|e| (e.field(), e.code())).collect()
}
fn text() -> TextRules {
    TextRules::new(1, 8).unwrap().non_blank().no_controls()
}
#[test]
fn typed_fields_preserve_false_spaces_and_exact_integer_values() {
    let value=JsonValue::parse(br#"{"title":" Keep ","completed":false,"u":18446744073709551615,"i":-9223372036854775808}"#).unwrap();
    let mut input = JsonObject::new(&value, &["title", "completed", "u", "i"]);
    assert_eq!(input.required_string("title", text()), Some(" Keep "));
    assert_eq!(input.required_bool("completed"), Some(false));
    assert_eq!(input.required_u64("u", 0, u64::MAX), Some(u64::MAX));
    assert_eq!(input.required_i64("i", i64::MIN, i64::MAX), Some(i64::MIN));
    assert!(input.finish().is_ok());
}
#[test]
fn absence_and_null_remain_distinct_and_optional_fields_do_not_coerce() {
    let value = JsonValue::parse(br#"{"title":null,"completed":"false"}"#).unwrap();
    let mut input = JsonObject::new(&value, &["title", "completed", "missing"]);
    assert!(matches!(input.field("title"), JsonField::Null));
    assert!(matches!(input.field("missing"), JsonField::Missing));
    assert!(matches!(input.field("completed"), JsonField::Value(_)));
    assert_eq!(input.optional_string("missing", text()), None);
    assert_eq!(input.optional_string("title", text()), None);
    assert_eq!(input.optional_bool("completed"), None);
    assert_eq!(
        codes(&input.finish().unwrap_err()),
        vec![
            (Some("title"), ValidationCode::NullNotAllowed),
            (Some("completed"), ValidationCode::ExpectedBoolean)
        ]
    );
}
#[test]
fn all_declared_field_errors_are_reported_without_input_order_dependence() {
    for raw in [
        br#"{"completed":1,"title":false}"#.as_slice(),
        br#"{"title":false,"completed":1}"#.as_slice(),
    ] {
        let value = JsonValue::parse(raw).unwrap();
        let mut input = JsonObject::new(&value, &["title", "completed", "count"]);
        input.required_string("title", text());
        input.required_bool("completed");
        input.required_u64("count", 1, 10);
        assert_eq!(
            codes(&input.finish().unwrap_err()),
            vec![
                (Some("title"), ValidationCode::ExpectedString),
                (Some("completed"), ValidationCode::ExpectedBoolean),
                (Some("count"), ValidationCode::Required)
            ]
        );
    }
}
#[test]
fn nonobjects_and_unknown_keys_produce_generic_object_issues() {
    for raw in ["null", "false", "1", "[]", "\"PRIVATE_VALUE\""] {
        let value = JsonValue::parse(raw.as_bytes()).unwrap();
        let mut input = JsonObject::new(&value, &["title"]);
        input.required_string("title", text());
        input.require_nonempty();
        assert_eq!(
            codes(&input.finish().unwrap_err()),
            vec![(None, ValidationCode::ExpectedObject)]
        );
    }
    let value =
        JsonValue::parse(br#"{"title":"ok","PRIVATE_UNKNOWN_KEY":"PRIVATE_VALUE","another":1}"#)
            .unwrap();
    let mut input = JsonObject::new(&value, &["title"]);
    input.required_string("title", text());
    let errors = input.finish().unwrap_err();
    assert_eq!(codes(&errors), vec![(None, ValidationCode::UnknownField)]);
    for output in [
        errors.to_json().encode(),
        format!("{errors:?} {errors}"),
        format!("{:?}", errors.iter().next().unwrap()),
    ] {
        assert!(!output.contains("PRIVATE"));
    }
}
#[test]
fn text_rules_use_utf8_bytes_and_reject_blank_and_control_input() {
    for (title, expected) in [
        ("", Some(ValidationCode::TooShort)),
        ("   ", Some(ValidationCode::Blank)),
        ("éééé", None),
        ("ééééa", Some(ValidationCode::TooLong)),
        ("abc\0", Some(ValidationCode::InvalidCharacters)),
        ("a\nb", Some(ValidationCode::InvalidCharacters)),
        ("🦀🦀", None),
    ] {
        let value =
            JsonValue::object(vec![("title".into(), JsonValue::string(title).unwrap())]).unwrap();
        let mut input = JsonObject::new(&value, &["title"]);
        let result = input.required_string("title", text());
        match expected {
            None => {
                assert_eq!(result, Some(title));
                assert!(input.finish().is_ok());
            }
            Some(code) => {
                assert!(result.is_none());
                assert_eq!(
                    codes(&input.finish().unwrap_err()),
                    vec![(Some("title"), code)]
                );
            }
        }
    }
    assert!(TextRules::new(9, 8).is_none());
    assert!(TextRules::new(0, crate::MAX_JSON_STRING_BYTES + 1).is_none());
}
#[test]
fn integer_reads_check_representation_overflow_and_application_bounds() {
    for (literal, expected) in [
        ("0", None),
        ("18446744073709551615", None),
        ("-0", Some(ValidationCode::OutOfRange)),
        ("-1", Some(ValidationCode::OutOfRange)),
        ("18446744073709551616", Some(ValidationCode::OutOfRange)),
        ("1.0", Some(ValidationCode::ExpectedInteger)),
        ("1e0", Some(ValidationCode::ExpectedInteger)),
        ("\"1\"", Some(ValidationCode::ExpectedInteger)),
        ("true", Some(ValidationCode::ExpectedInteger)),
    ] {
        let value = JsonValue::parse(format!("{{\"n\":{literal}}}").as_bytes()).unwrap();
        let mut input = JsonObject::new(&value, &["n"]);
        let result = input.required_u64("n", 0, u64::MAX);
        match expected {
            None => {
                assert!(result.is_some());
                assert!(input.finish().is_ok());
            }
            Some(code) => assert_eq!(codes(&input.finish().unwrap_err()), vec![(Some("n"), code)]),
        }
    }
    for literal in ["-9223372036854775809", "9223372036854775808", "9"] {
        let value = JsonValue::parse(format!("{{\"n\":{literal}}}").as_bytes()).unwrap();
        let mut input = JsonObject::new(&value, &["n"]);
        assert!(input.required_i64("n", -2, 2).is_none());
        assert_eq!(
            codes(&input.finish().unwrap_err()),
            vec![(Some("n"), ValidationCode::OutOfRange)]
        );
    }
    let value = JsonValue::parse(b"{}").unwrap();
    let mut input = JsonObject::new(&value, &["n"]);
    assert_eq!(input.optional_i64("n", -2, 2), None);
    assert_eq!(input.optional_u64("n", 0, 2), None);
    assert!(input.finish().is_ok());
}
#[test]
fn patch_presence_checks_and_custom_domain_rules_fail_before_success() {
    let empty = JsonValue::parse(b"{}").unwrap();
    let mut input = JsonObject::new(&empty, &["title"]);
    input.require_nonempty();
    assert_eq!(
        codes(&input.finish().unwrap_err()),
        vec![(None, ValidationCode::EmptyObject)]
    );
    let value = JsonValue::parse(br#"{"title":"abc<"}"#).unwrap();
    let mut input = JsonObject::new(&value, &["title"]);
    if input
        .required_string("title", text())
        .is_some_and(|s| s.contains('<'))
    {
        input.error(Some("title"), ValidationCode::InvalidCharacters);
    }
    assert_eq!(
        codes(&input.finish().unwrap_err()),
        vec![(Some("title"), ValidationCode::InvalidCharacters)]
    );
}
#[test]
fn invalid_definitions_and_empty_error_responses_fail_closed() {
    let value = JsonValue::parse(b"{}").unwrap();
    for fields in [
        &["title", "title"][..],
        &["PRIVATE INVALID NAME"][..],
        &["é"][..],
        &["x"; 33][..],
    ] {
        let errors = JsonObject::new(&value, fields).finish().unwrap_err();
        assert!(errors.is_definition_error());
        assert_eq!(
            crate::Response::validation_errors(&errors).status(),
            crate::StatusCode::InternalServerError
        );
        assert_eq!(
            errors.to_json().encode(),
            "{\"error\":\"Internal Server Error\"}"
        );
    }
    let mut input = JsonObject::new(&value, &["title"]);
    input.required_bool("undeclared");
    assert!(input.finish().unwrap_err().is_definition_error());
    let mut input = JsonObject::new(&value, &["title"]);
    input.error(Some("undeclared"), ValidationCode::InvalidValue);
    assert!(input.finish().unwrap_err().is_definition_error());
    let mut input = JsonObject::new(&value, &["n"]);
    input.optional_u64("n", 2, 1);
    assert!(input.finish().unwrap_err().is_definition_error());
    let mut errors = ValidationErrors::new();
    errors.add(Some("PRIVATE INVALID NAME"), ValidationCode::Required);
    assert!(errors.is_definition_error());
    assert!(!errors.to_json().encode().contains("PRIVATE"));
    assert_eq!(
        crate::Response::validation_errors(&ValidationErrors::new()).status(),
        crate::StatusCode::InternalServerError
    );
}
#[test]
fn issue_overflow_never_becomes_success_and_reports_fit_the_fixed_budget() {
    let mut errors = ValidationErrors::new();
    for n in 0..MAX_VALIDATION_ERRORS {
        let name: &'static str = Box::leak(format!("f{n:0>63}").into_boxed_str());
        errors.add(Some(name), ValidationCode::InvalidCharacters);
    }
    assert_eq!(errors.len(), MAX_VALIDATION_ERRORS);
    assert!(!errors.truncated());
    let first = errors.iter().next().unwrap().field();
    errors.add(first, ValidationCode::InvalidCharacters);
    assert!(!errors.truncated());
    errors.add(None, ValidationCode::InvalidValue);
    assert!(errors.truncated());
    assert!(errors.clone().finish().is_err());
    let body = errors.to_json().encode();
    assert!(body.len() <= MAX_VALIDATION_RESPONSE_BYTES);
    assert!(JsonValue::parse(body.as_bytes()).is_ok());
    let response = crate::Response::validation_errors(&errors);
    assert_eq!(response.status(), crate::StatusCode::BadRequest);
    assert_eq!(response.cache_control(), "no-store");
    assert_eq!(response.content_type(), crate::JSON_MEDIA_TYPE);
    assert_eq!(
        response.content_security_policy(),
        crate::CONTENT_SECURITY_POLICY
    );
}
#[test]
fn request_decode_uses_the_explicit_conversion_and_borrowed_fields_stay_valid() {
    struct Title<'a>(&'a str);
    impl<'a> TryFrom<&'a JsonValue> for Title<'a> {
        type Error = ValidationErrors;
        fn try_from(value: &'a JsonValue) -> Result<Self, Self::Error> {
            let mut input = JsonObject::new(value, &["title"]);
            let title = input.required_string("title", text());
            input.finish()?;
            Ok(Self(title.unwrap()))
        }
    }
    let mut request =
        crate::http::parse(b"POST / HTTP/1.1\r\nHost: localhost\r\n\r\n", 4096, 64).unwrap();
    assert!(request.decode_json::<Title<'_>>().is_err());
    request.json = Some(JsonValue::parse(br#"{"title":"Rust"}"#).unwrap());
    assert_eq!(request.decode_json::<Title<'_>>().unwrap().0, "Rust");
}
#[test]
fn every_error_code_has_a_unique_bounded_safe_wire_name() {
    let codes = [
        ValidationCode::Required,
        ValidationCode::NullNotAllowed,
        ValidationCode::ExpectedObject,
        ValidationCode::ExpectedString,
        ValidationCode::ExpectedBoolean,
        ValidationCode::ExpectedInteger,
        ValidationCode::OutOfRange,
        ValidationCode::TooShort,
        ValidationCode::TooLong,
        ValidationCode::Blank,
        ValidationCode::InvalidCharacters,
        ValidationCode::UnknownField,
        ValidationCode::EmptyObject,
        ValidationCode::InvalidValue,
    ];
    let names = codes
        .into_iter()
        .map(|code| {
            assert!(code.as_str().len() <= 32);
            assert!(
                code.as_str()
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_')
            );
            assert!(!code.message().is_empty());
            code.as_str()
        })
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(names.len(), codes.len());
}
