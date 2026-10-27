use super::*;
use crate::{Request, Response, StatusCode};

#[test]
fn entity_tags_are_bounded_ascii_without_header_injection_or_escape_processing() {
    for byte in 0..=127 {
        let opaque = String::from_utf8(vec![byte]).unwrap();
        assert_eq!(
            EntityTag::strong(&opaque).is_ok(),
            byte == 0x21 || (0x23..=0x7e).contains(&byte)
        );
    }
    for invalid in [
        "plain",
        "w/\"x\"",
        "W/ \"x\"",
        " \"x\"",
        "\"x\" ",
        "\"x\r\nInjected: yes\"",
        "\"é\"",
        "\"a\"b\"",
    ] {
        assert_eq!(
            EntityTag::parse(invalid),
            Err(EntityTagError::InvalidSyntax)
        );
    }
    assert!(EntityTag::strong("").is_ok());
    assert_eq!(
        EntityTag::parse("\"a,b\\c\"").unwrap().as_str(),
        "\"a,b\\c\""
    );
    assert!(EntityTag::strong(&"x".repeat(MAX_ENTITY_TAG_BYTES - 2)).is_ok());
    assert_eq!(
        EntityTag::strong(&"x".repeat(MAX_ENTITY_TAG_BYTES - 1)),
        Err(EntityTagError::TooLarge)
    );
    assert_eq!(
        EntityTag::parse(&format!("W/\"{}\"", "x".repeat(MAX_ENTITY_TAG_BYTES - 3))),
        Err(EntityTagError::TooLarge)
    );
}
#[test]
fn if_match_uses_strong_comparison_and_wildcard_only_checks_existence() {
    let strong = EntityTag::strong("a,b\\c").unwrap();
    let weak = EntityTag::parse("W/\"a,b\\c\"").unwrap();
    assert!(!strong.strong_eq(&weak));
    assert!(!weak.strong_eq(&weak));
    let list = IfMatch::parse("\t, W/\"other\" , , \"a,b\\c\" ,\t").unwrap();
    assert!(list.matches(Some(&strong)));
    assert!(!list.matches(Some(&weak)));
    assert!(!list.matches(None));
    assert!(
        !IfMatch::parse("W/\"a,b\\c\"")
            .unwrap()
            .matches(Some(&strong))
    );
    let any = IfMatch::parse(" \t*\t ").unwrap();
    assert!(any.is_any() && any.matches(Some(&strong)) && any.matches(Some(&weak)));
    assert!(!any.matches(None));
    for empty in ["", " , ,\t", ","] {
        assert!(!IfMatch::parse(empty).unwrap().matches(Some(&strong)));
    }
}
#[test]
fn malformed_lists_and_resource_excess_fail_without_reflecting_contents() {
    for invalid in [
        "*,\"x\"",
        "\"x\",*",
        "*,*",
        "a",
        "W/",
        "\"unclosed",
        "\"a\" \"b\"",
        "\"a\\\"b\"",
        "\"é\"",
        "\"x\";param=1",
    ] {
        assert_eq!(
            IfMatch::parse(invalid),
            Err(EntityTagError::InvalidSyntax),
            "{invalid}"
        );
    }
    assert!(IfMatch::parse(&vec!["\"x\""; MAX_IF_MATCH_TAGS].join(",")).is_ok());
    assert_eq!(
        IfMatch::parse(&vec!["\"x\""; MAX_IF_MATCH_TAGS + 1].join(",")),
        Err(EntityTagError::TooManyTags)
    );
    assert!(IfMatch::parse(&",".repeat(MAX_IF_MATCH_TAGS - 1)).is_ok());
    assert_eq!(
        IfMatch::parse(&",".repeat(MAX_IF_MATCH_TAGS)),
        Err(EntityTagError::TooManyTags)
    );
    assert_eq!(
        IfMatch::parse(&" ".repeat(MAX_IF_MATCH_BYTES + 1)),
        Err(EntityTagError::TooLarge)
    );
    let tag = EntityTag::strong("PRIVATE_VALUE").unwrap();
    assert!(!format!("{tag:?}").contains("PRIVATE_VALUE"));
    assert!(!format!("{:?}", IfMatch::parse(tag.as_str()).unwrap()).contains("PRIVATE_VALUE"));
}
#[test]
fn request_validation_precedes_callbacks_and_sanitized_gets_drop_conditions() {
    let parse = |value: &str| {
        crate::http::parse(
            format!("GET /resource HTTP/1.1\r\nHost: localhost\r\nIf-Match: {value}\r\n\r\n")
                .as_bytes(),
            8192,
            64,
        )
    };
    let request = parse("\"PRIVATE_TAG\"").unwrap();
    assert!(
        request
            .if_match()
            .unwrap()
            .matches(Some(&EntityTag::strong("PRIVATE_TAG").unwrap()))
    );
    assert!(!format!("{request:?}").contains("PRIVATE_TAG"));
    assert!(request.background_get().if_match().is_none());
    assert!(
        Request::prerender_get(&crate::InternalUrl::new("/resource").unwrap(), vec![])
            .if_match()
            .is_none()
    );
    assert_eq!(parse("*,\"x\"").unwrap_err(), StatusCode::BadRequest);
    assert_eq!(
        parse(&"x".repeat(MAX_IF_MATCH_BYTES + 1)).unwrap_err(),
        StatusCode::RequestHeaderFieldsTooLarge
    );
    assert_eq!(
        parse(&vec!["\"x\""; 17].join(",")).unwrap_err(),
        StatusCode::RequestHeaderFieldsTooLarge
    );
}
#[test]
fn validators_do_not_enable_caching_or_survive_error_statuses_on_the_wire() {
    let tag = EntityTag::strong("version").unwrap();
    let response = Response::json(&true.into()).with_etag(tag.clone());
    assert_eq!(response.etag(), Some(&tag));
    assert_eq!(response.cache_control(), "no-store");
    assert!(response.wire_headers("").contains("ETag: \"version\"\r\n"));
    for status in [
        StatusCode::PreconditionFailed,
        StatusCode::PreconditionRequired,
        StatusCode::Forbidden,
    ] {
        let response = response.clone().with_status(status);
        assert!(response.etag().is_none());
        let headers = response.wire_headers("");
        assert!(!headers.contains("ETag:"));
        assert!(headers.contains("Cache-Control: no-store\r\n"));
    }
}
#[test]
fn deterministic_conditional_header_corpus_never_panics() {
    let seed = b"W/\"a,b\", \"c\\d\"";
    for end in 0..=seed.len() {
        let _ = IfMatch::parse(std::str::from_utf8(&seed[..end]).unwrap());
    }
    for position in 0..seed.len() {
        for byte in 0..=127 {
            let mut input = seed.to_vec();
            input[position] = byte;
            let _ = IfMatch::parse(std::str::from_utf8(&input).unwrap());
        }
    }
}
