use super::*;

#[test]
fn all_value_types_roundtrip_without_coercion_or_number_rounding() {
    let source=br#" {"items":[null,true,false,-0,18446744073709551615,1.00e+003],"text":"Rust \uD83E\uDD80"} "#;
    let value = JsonValue::parse(source).unwrap();
    let items = value.get("items").unwrap().as_array().unwrap();
    assert!(items[0].is_null());
    assert_eq!(items[1].as_bool(), Some(true));
    assert_eq!(items[2].as_bool(), Some(false));
    assert_eq!(items[3].as_number().unwrap().as_str(), "-0");
    assert_eq!(items[4].as_number().unwrap().to_u64(), Ok(u64::MAX));
    assert_eq!(items[5].as_number().unwrap().as_str(), "1.00e+003");
    assert_eq!(value.get("text").unwrap().as_str(), Some("Rust 🦀"));
    assert_eq!(
        JsonValue::parse(value.encode().as_bytes()),
        Ok(value.clone())
    );
    assert!(items[1].as_number().is_none());
    assert!(value.as_array().is_none());
    assert_eq!(
        value.encode(),
        "{\"items\":[null,true,false,-0,18446744073709551615,1.00e+003],\"text\":\"Rust 🦀\"}"
    );
}
#[test]
fn numbers_require_json_grammar_and_explicit_checked_conversions() {
    for raw in [
        "0",
        "-0",
        "1",
        "-12",
        "0.01",
        "1E+003",
        "1e-9999",
        "18446744073709551616",
        "-9223372036854775809",
    ] {
        assert_eq!(JsonNumber::parse(raw).unwrap().as_str(), raw);
    }
    for raw in [
        "", "+1", "01", "-01", ".5", "1.", "1e", "1e+", "--1", "-", "NaN", "Infinity", "0x10",
        "1_000", " 1", "1 ", "１",
    ] {
        assert!(JsonNumber::parse(raw).is_err(), "number {raw:?}");
    }
    assert_eq!(
        JsonNumber::parse("-9223372036854775808").unwrap().to_i64(),
        Ok(i64::MIN)
    );
    for raw in ["18446744073709551616", "-0", "1.0", "1e0"] {
        assert_eq!(
            JsonNumber::parse(raw).unwrap().to_u64(),
            Err(JsonError::NumberConversion)
        );
    }
    for raw in ["9223372036854775808", "-9223372036854775809", "1.0", "1e0"] {
        assert_eq!(
            JsonNumber::parse(raw).unwrap().to_i64(),
            Err(JsonError::NumberConversion)
        );
    }
    assert_eq!(
        JsonNumber::parse("1e9999").unwrap().to_f64(),
        Err(JsonError::NumberConversion)
    );
    assert_eq!(JsonNumber::parse("0.5").unwrap().to_f64(), Ok(0.5));
    assert_eq!(JsonValue::from(u64::MAX).encode(), u64::MAX.to_string());
    assert_eq!(JsonValue::from(i64::MIN).encode(), i64::MIN.to_string());
}
#[test]
fn strings_decode_unicode_pairs_and_escape_every_control_for_output() {
    let controls = (0..32)
        .map(|n| char::from_u32(n).unwrap())
        .collect::<String>();
    let source = format!("{controls}\"\\/λ🦀\u{2028}\u{2029}");
    let value = JsonValue::string(source.clone()).unwrap();
    let encoded = value.encode();
    assert!(!encoded.bytes().any(|byte| byte < 32));
    assert_eq!(
        JsonValue::parse(encoded.as_bytes()).unwrap().as_str(),
        Some(source.as_str())
    );
    assert_eq!(
        JsonValue::parse(br#""\u0000\u0041\uD83e\udd80\/""#)
            .unwrap()
            .as_str(),
        Some("\0A🦀/")
    );
    for raw in [
        br#""\uD800""#.as_slice(),
        br#""\uDC00""#,
        br#""\uD800\uD800""#,
        br#""\uD800\u0041""#,
        br#""\u12""#,
        br#""\uGGGG""#,
        br#""\x20""#,
        b"\"line\nbreak\"",
        b"\"\x00\"",
    ] {
        assert!(JsonValue::parse(raw).is_err());
    }
}
#[test]
fn malformed_utf8_extensions_trailing_data_and_bom_are_refused() {
    for raw in [
        b"".as_slice(),
        b" ",
        b"undefined",
        b"TRUE",
        b"[1,]",
        b"{\"a\":1,}",
        b"{a:1}",
        b"[1 2]",
        b"null false",
        b"/*x*/null",
        b"{\"x\":NaN}",
        b"\xef\xbb\xbfnull",
        b"\"\xc0\xaf\"",
        b"\"\xed\xa0\x80\"",
        b"\"\xf4\x90\x80\x80\"",
        b"\"\xff\"",
    ] {
        assert!(JsonValue::parse(raw).is_err(), "bytes {raw:?}");
    }
    let raw = br#"{"a":["Rust \uD83E\uDD80",false,null]}"#;
    for end in 0..raw.len() {
        assert!(JsonValue::parse(&raw[..end]).is_err(), "prefix {end}");
    }
    assert!(JsonValue::parse(&[raw.as_slice(), b"x"].concat()).is_err());
}
#[test]
fn duplicate_keys_are_compared_after_decoding_without_normalization() {
    for raw in [
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":1,"\u0078":2}"#,
        br#"{"\uD83E\uDD80":1,"\ud83e\udd80":2}"#,
    ] {
        assert_eq!(JsonValue::parse(raw), Err(JsonError::DuplicateKey));
    }
    assert_eq!(
        JsonValue::object(vec![
            ("x".into(), JsonValue::null()),
            ("x".into(), true.into())
        ]),
        Err(JsonError::DuplicateKey)
    );
    assert!(JsonValue::parse("{\"A\":1,\"a\":2,\"é\":3,\"e\\u0301\":4}".as_bytes()).is_ok());
}
#[test]
fn parsing_and_construction_enforce_depth_and_container_limits() {
    let mut value = JsonValue::null();
    for _ in 0..MAX_JSON_DEPTH {
        value = JsonValue::array(vec![value]).unwrap();
    }
    assert_eq!(
        JsonValue::parse(value.encode().as_bytes()),
        Ok(value.clone())
    );
    assert_eq!(JsonValue::array(vec![value]), Err(JsonError::TooDeep));
    assert_eq!(
        JsonValue::parse(
            format!(
                "{}null{}",
                "[".repeat(MAX_JSON_DEPTH + 1),
                "]".repeat(MAX_JSON_DEPTH + 1)
            )
            .as_bytes()
        ),
        Err(JsonError::TooDeep)
    );
    let array = JsonValue::array(vec![JsonValue::null(); MAX_JSON_ITEMS]).unwrap();
    assert!(JsonValue::parse(array.encode().as_bytes()).is_ok());
    assert_eq!(
        JsonValue::array(vec![JsonValue::null(); MAX_JSON_ITEMS + 1]),
        Err(JsonError::TooManyItems)
    );
    let raw = format!("[{}]", vec!["null"; MAX_JSON_ITEMS + 1].join(","));
    assert_eq!(
        JsonValue::parse(raw.as_bytes()),
        Err(JsonError::TooManyItems)
    );
    let members = (0..MAX_JSON_ITEMS)
        .map(|n| (format!("k{n}"), JsonValue::null()))
        .collect::<Vec<_>>();
    assert!(JsonValue::object(members).is_ok());
    let raw = format!(
        "{{{}}}",
        (0..=MAX_JSON_ITEMS)
            .map(|n| format!("\"k{n}\":null"))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(
        JsonValue::parse(raw.as_bytes()),
        Err(JsonError::TooManyItems)
    );
}
#[test]
fn aggregate_value_and_encoded_byte_limits_cannot_be_bypassed_by_builders() {
    let child = JsonValue::array(vec![JsonValue::null(); 127]).unwrap();
    let mut children = vec![child; 7];
    children.push(JsonValue::array(vec![JsonValue::null(); 126]).unwrap());
    let value = JsonValue::array(children.clone()).unwrap();
    assert_eq!(value.values, MAX_JSON_VALUES);
    assert_eq!(JsonValue::parse(value.encode().as_bytes()), Ok(value));
    children.push(JsonValue::null());
    assert_eq!(JsonValue::array(children), Err(JsonError::TooManyValues));
    let raw = format!(
        "[{}]",
        vec![format!("[{}]", vec!["null"; 127].join(",")); 8].join(",")
    );
    assert_eq!(
        JsonValue::parse(raw.as_bytes()),
        Err(JsonError::TooManyValues)
    );
    let large = JsonValue::string("x".repeat(MAX_JSON_STRING_BYTES)).unwrap();
    assert_eq!(JsonValue::array(vec![large; 16]), Err(JsonError::TooLarge));
    assert_eq!(
        JsonValue::parse(&vec![b' '; MAX_JSON_BYTES + 1]),
        Err(JsonError::TooLarge)
    );
}
#[test]
fn string_key_number_and_whole_document_byte_boundaries_are_exact() {
    let mut items = vec![JsonValue::string("x".repeat(MAX_JSON_STRING_BYTES)).unwrap(); 15];
    items.push(JsonValue::string("x".repeat(4047)).unwrap());
    let exact = JsonValue::array(items.clone()).unwrap();
    assert_eq!(exact.encode().len(), MAX_JSON_BYTES);
    assert_eq!(JsonValue::parse(exact.encode().as_bytes()), Ok(exact));
    items[15] = JsonValue::string("x".repeat(4048)).unwrap();
    assert_eq!(JsonValue::array(items), Err(JsonError::TooLarge));
    assert!(JsonValue::string("🦀".repeat(MAX_JSON_STRING_BYTES / 4)).is_ok());
    assert_eq!(
        JsonValue::string("🦀".repeat(MAX_JSON_STRING_BYTES / 4 + 1)),
        Err(JsonError::TooLarge)
    );
    assert!(JsonValue::string("x".repeat(MAX_JSON_STRING_BYTES)).is_ok());
    assert_eq!(
        JsonValue::string("x".repeat(MAX_JSON_STRING_BYTES + 1)),
        Err(JsonError::TooLarge)
    );
    assert_eq!(
        JsonValue::object(vec![(
            "k".repeat(MAX_JSON_STRING_BYTES + 1),
            JsonValue::null()
        )]),
        Err(JsonError::TooLarge)
    );
    assert_eq!(
        JsonValue::parse(format!("\"{}\"", "x".repeat(MAX_JSON_STRING_BYTES + 1)).as_bytes()),
        Err(JsonError::TooLarge)
    );
    assert!(JsonNumber::parse(&"1".repeat(MAX_JSON_NUMBER_BYTES)).is_ok());
    assert_eq!(
        JsonNumber::parse(&"1".repeat(MAX_JSON_NUMBER_BYTES + 1)),
        Err(JsonError::TooLarge)
    );
    let raw = format!("null{}", " ".repeat(MAX_JSON_BYTES - 4));
    assert!(JsonValue::parse(raw.as_bytes()).is_ok());
    let escaped = JsonValue::string("\0".repeat(MAX_JSON_STRING_BYTES)).unwrap();
    assert_eq!(escaped.encode().len(), 2 + 6 * MAX_JSON_STRING_BYTES);
    assert_eq!(JsonValue::parse(escaped.encode().as_bytes()), Ok(escaped));
}
#[test]
fn debug_and_errors_do_not_reflect_payloads_or_keys() {
    let value = JsonValue::object(vec![
        (
            "PRIVATE_KEY_FIXTURE".into(),
            JsonValue::string("PRIVATE_VALUE_FIXTURE").unwrap(),
        ),
        (
            "number".into(),
            JsonNumber::parse("98765432101234567890").unwrap().into(),
        ),
    ])
    .unwrap();
    let response = crate::Response::json(&value);
    let debug = format!(
        "{value:?} {response:?} {:?}",
        value.get("number").unwrap().as_number().unwrap()
    );
    for secret in [
        "PRIVATE_KEY_FIXTURE",
        "PRIVATE_VALUE_FIXTURE",
        "98765432101234567890",
    ] {
        assert!(!debug.contains(secret));
    }
    assert_eq!(response.content_type(), JSON_MEDIA_TYPE);
    assert_eq!(response.cache_control(), "no-store");
    assert_eq!(
        response.content_security_policy(),
        crate::CONTENT_SECURITY_POLICY
    );
    assert_eq!(response.body_text().unwrap(), value.encode());
    for error in [
        JsonError::InvalidSyntax,
        JsonError::DuplicateKey,
        JsonError::TooLarge,
    ] {
        assert!(!format!("{error:?}: {error}").contains("PRIVATE"));
    }
}
#[test]
fn requests_keep_body_profiles_separate_and_sanitized_gets_drop_json() {
    let mut router = crate::Router::new();
    router
        .post_json("/json", |request| {
            crate::Response::json(request.json().unwrap())
        })
        .unwrap();
    let body = br#"{"PRIVATE_KEY":"PRIVATE_BODY"}"#;
    let mut request=crate::http::parse(format!("POST /json HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",body.len()).as_bytes(),8192,32).unwrap();
    assert_eq!(
        router.dispatch(request.clone()).status(),
        crate::StatusCode::Forbidden
    );
    router.prepare_post(&request, MAX_JSON_BYTES).unwrap();
    router.parse_post(&mut request, body).unwrap();
    assert!(request.json().is_some());
    assert!(request.form_data().is_none());
    assert!(!format!("{request:?}").contains("PRIVATE_BODY"));
    assert!(!format!("{request:?}").contains("PRIVATE_KEY"));
    assert!(request.background_get().json().is_none());
    assert!(
        crate::Request::prerender_get(&crate::InternalUrl::new("/json").unwrap(), Vec::new())
            .json()
            .is_none()
    );
    assert_eq!(
        router.dispatch(request.clone()).status(),
        crate::StatusCode::Ok
    );
    request.form = Some(crate::FormData::default());
    assert_eq!(
        router.dispatch(request).status(),
        crate::StatusCode::Forbidden
    );
}
#[test]
fn deterministic_mutations_reparse_every_accepted_output_without_panics() {
    let seed = br#"{"a":[null,true,1.00e+3,"\uD83E\uDD80"],"b":{"x":"y"}}"#;
    for index in 0..seed.len() {
        for byte in [
            0, 9, 10, 13, 32, b'"', b'\\', b',', b':', b'0', b'9', b'[', b']', b'{', b'}', b'u',
            0x80, 0xff,
        ] {
            let mut raw = seed.to_vec();
            raw[index] = byte;
            if let Ok(value) = JsonValue::parse(&raw) {
                assert_eq!(JsonValue::parse(value.encode().as_bytes()), Ok(value));
            }
        }
    }
}
