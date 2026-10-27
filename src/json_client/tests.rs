use super::*;
fn frame(kind: u8, flags: u8, status: u16, retry: u32, tag: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"RVJ1".to_vec();
    bytes.extend([kind, flags]);
    bytes.extend(status.to_le_bytes());
    bytes.extend(retry.to_le_bytes());
    bytes.extend((tag.len() as u16).to_le_bytes());
    bytes.extend(tag.as_bytes());
    bytes.extend(body);
    bytes
}
#[test]
fn requests_require_explicit_methods_bodies_and_retained_strong_versions() {
    for method in [
        JsonMethod::Get,
        JsonMethod::Post,
        JsonMethod::Put,
        JsonMethod::Patch,
        JsonMethod::Delete,
    ] {
        for body in [None, Some(JsonValue::null())] {
            let valid = body.is_some()
                == matches!(
                    method,
                    JsonMethod::Post | JsonMethod::Put | JsonMethod::Patch
                );
            assert_eq!(
                JsonRequest::new(
                    1,
                    method,
                    InternalUrl::new("/api")
                        .unwrap()
                        .query("view", "all")
                        .unwrap(),
                    body
                )
                .is_ok(),
                valid
            );
        }
    }
    assert!(JsonRequest::new(0, JsonMethod::Get, InternalUrl::new("/").unwrap(), None).is_err());
    let request = JsonRequest::new(
        1,
        JsonMethod::Get,
        InternalUrl::new("/private").unwrap(),
        None,
    )
    .unwrap();
    assert!(
        request
            .clone()
            .with_if_match(EntityTag::parse("W/\"weak\"").unwrap())
            .is_err()
    );
    let request = request
        .with_if_match(EntityTag::strong("retained").unwrap())
        .unwrap();
    assert_eq!(request.if_match().unwrap().as_str(), "\"retained\"");
    assert!(!format!("{request:?}").contains("private"));
    let large = JsonValue::array(
        (0..5)
            .map(|_| JsonValue::string("x".repeat(4096)).unwrap())
            .collect(),
    )
    .unwrap();
    assert_eq!(
        JsonRequest::new(
            1,
            JsonMethod::Post,
            InternalUrl::new("/").unwrap(),
            Some(large)
        )
        .unwrap_err(),
        ActionError::Limit
    );
}
#[test]
fn replies_preserve_exact_numbers_and_require_explicit_conversion() {
    let bytes = frame(
        0,
        0,
        200,
        0,
        "\"snapshot\"",
        br#"{"n":18446744073709551615,"exponent":1e3}"#,
    );
    let reply = decode_json_reply(&bytes, true).unwrap();
    assert_eq!(
        reply
            .value()
            .get("n")
            .unwrap()
            .as_number()
            .unwrap()
            .to_u64(),
        Ok(u64::MAX)
    );
    assert!(
        reply
            .value()
            .get("exponent")
            .unwrap()
            .as_number()
            .unwrap()
            .to_u64()
            .is_err()
    );
    assert_eq!(reply.etag().unwrap().as_str(), "\"snapshot\"");
    assert!(!reply.uncertain());
}
#[test]
fn http_rejections_conflicts_and_quotas_remain_distinct() {
    for (status, flags, retry, uncertain) in [
        (400, 0, 0, false),
        (412, 0, 0, false),
        (428, 0, 0, false),
        (429, 2, 3, false),
        (429, 0, 3, true),
        (503, 0, 0, true),
        (403, 4, 0, false),
        (429, 6, 3, false),
        (503, 4, 0, true),
    ] {
        let bytes = frame(
            0,
            flags,
            status,
            retry,
            "",
            if flags & 4 == 0 { b"{}" } else { b"" },
        );
        let reply = decode_json_reply(&bytes, true).unwrap();
        assert_eq!(reply.status(), status);
        assert_eq!(reply.has_json(), flags & 4 == 0);
        assert_eq!(reply.retry_after(), (retry != 0).then_some(retry));
        assert_eq!(reply.uncertain(), uncertain);
        assert!(!decode_json_reply(&bytes, false).unwrap().uncertain());
    }
}
#[test]
fn failures_report_submission_uncertainty_without_retrying() {
    for kind in 1..=5 {
        for sent in 0..=1 {
            for mutation in [false, true] {
                let failure = decode_json_reply(
                    &frame(kind, sent, 0, if kind == 5 { 8 } else { 0 }, "", b""),
                    mutation,
                )
                .unwrap_err();
                assert_eq!(failure.uncertain(), mutation && sent != 0);
                assert_eq!(failure.kind() as u32, kind as u32);
            }
        }
    }
}
#[test]
fn malformed_metadata_json_and_size_limits_fail_closed() {
    let good = frame(0, 0, 200, 0, "\"v\"", b"{}");
    for len in 0..good.len() {
        assert_eq!(
            decode_json_reply(&good[..len], true).unwrap_err().kind(),
            ActionFailure::Protocol
        );
    }
    for bytes in [
        frame(6, 0, 0, 0, "", b""),
        frame(0, 1, 200, 0, "", b"{}"),
        frame(0, 4, 200, 0, "", b""),
        frame(0, 4, 403, 0, "", b"{}"),
        frame(0, 4, 403, 0, "\"v\"", b""),
        frame(0, 2, 200, 1, "", b"{}"),
        frame(0, 2, 429, 0, "", b"{}"),
        frame(0, 0, 302, 0, "", b"{}"),
        frame(0, 0, 200, 1, "", b"{}"),
        frame(1, 0, 200, 0, "", b""),
        frame(1, 0, 0, 0, "", b"{}"),
        frame(0, 0, 200, 0, "bad", b"{}"),
        frame(0, 0, 200, 0, "", br#"{"x":1,"x":2}"#),
        frame(0, 0, 200, 0, "", &[255]),
        frame(0, 0, 200, 0, "", &vec![b' '; crate::MAX_JSON_BYTES + 1]),
    ] {
        let failure = decode_json_reply(&bytes, true).unwrap_err();
        assert_eq!(failure.kind(), ActionFailure::Protocol);
        assert!(failure.uncertain());
    }
}
#[test]
fn remote_reports_use_only_declared_static_fields_and_fixed_codes() {
    let mut report = crate::ValidationErrors::new();
    report.add(Some("title"), crate::ValidationCode::Blank);
    report.add(None, crate::ValidationCode::UnknownField);
    let reply = decode_json_reply(
        &frame(0, 0, 400, 0, "", report.to_json().encode().as_bytes()),
        true,
    )
    .unwrap();
    assert_eq!(reply.validation(&["title"]), Ok(report));
    assert!(reply.validation(&["completed"]).is_err());
    for body in [
        r#"{"error":"Bad Request","errors":[{"field":"PRIVATE","code":"required"}],"truncated":false}"#,
        r#"{"error":"Bad Request","errors":[{"code":"made_up"}],"truncated":false}"#,
        r#"{"error":"Bad Request","errors":[],"truncated":false}"#,
        r#"{"error":"Bad Request","errors":[{"code":"required","value":"PRIVATE"}],"truncated":false}"#,
    ] {
        assert!(
            crate::ValidationErrors::from_json(
                &JsonValue::parse(body.as_bytes()).unwrap(),
                &["title"]
            )
            .is_err()
        );
    }
}
