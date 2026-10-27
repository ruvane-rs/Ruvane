use super::*;
use crate::{
    ActionData, ComponentProps, InitialState, NavigationCachePolicy, NavigationPage, Request,
    Response, StatusCode,
};
fn request(cookie: &str) -> Result<Request, StatusCode> {
    let raw = format!("GET / HTTP/1.1\r\nHost: localhost\r\nCookie: {cookie}\r\n\r\n");
    crate::http::parse(raw.as_bytes(), 8192, 32)
}
#[test]
fn parses_case_sensitive_names_quoted_empty_values_and_preserves_encoding() {
    let cookies =
        Cookies::parse("a=x%2F+==; A=second; quoted=\"one=two\"; empty=; quoted_empty=\"\"")
            .unwrap();
    assert_eq!(cookies.len(), 5);
    assert_eq!(cookies.get("a"), Some("x%2F+=="));
    assert_eq!(cookies.get("A"), Some("second"));
    assert_eq!(cookies.get("quoted"), Some("one=two"));
    assert_eq!(cookies.get("empty"), Some(""));
    assert_eq!(cookies.get("quoted_empty"), Some(""));
    assert_eq!(cookies.iter().next(), Some(("a", "x%2F+==")));
}
#[test]
fn rejects_ambiguous_pairs_injections_and_invalid_octets() {
    for raw in [
        "a=1; a=2",
        "a=1;a=\"1\"",
        "=x",
        "a",
        "a=b;",
        ";a=b",
        "a=b;;c=d",
        "a b=c",
        "a=one,two",
        "a=two words",
        "a=\"unclosed",
        "a=unclosed\"",
        "a=\"quote\\\"\"",
        "a=☃",
        "a=x\r\nInjected: y",
        "a=\0",
        "a=one\ttwo",
    ] {
        assert!(Cookies::parse(raw).is_err(), "accepted invalid fixture");
    }
    for raw in ["a=1; a=2", "a=x,y", "a=unclosed\""] {
        assert_eq!(request(raw).unwrap_err(), StatusCode::BadRequest);
    }
}
#[test]
fn enforces_request_count_name_value_and_header_budgets() {
    assert!(
        Cookie::new(
            &"a".repeat(MAX_COOKIE_NAME_BYTES),
            &"v".repeat(MAX_COOKIE_VALUE_BYTES)
        )
        .is_ok()
    );
    assert!(Cookie::new(&"a".repeat(MAX_COOKIE_NAME_BYTES + 1), "v").is_err());
    assert!(Cookie::new("a", &"v".repeat(MAX_COOKIE_VALUE_BYTES + 1)).is_err());
    let full = (0..MAX_REQUEST_COOKIES)
        .map(|n| format!("k{n}=v"))
        .collect::<Vec<_>>()
        .join("; ");
    assert_eq!(Cookies::parse(&full).unwrap().len(), MAX_REQUEST_COOKIES);
    assert_eq!(
        Cookies::parse(&(full + "; extra=v")),
        Err(CookieError::TooManyCookies)
    );
    assert_eq!(
        request(&"x".repeat(MAX_COOKIE_HEADER_BYTES + 1)).unwrap_err(),
        StatusCode::RequestHeaderFieldsTooLarge
    );
    assert_eq!(
        request(&" ".repeat(MAX_COOKIE_HEADER_BYTES + 1)).unwrap_err(),
        StatusCode::RequestHeaderFieldsTooLarge
    );
    let count = (0..=MAX_REQUEST_COOKIES)
        .map(|n| format!("k{n}=v"))
        .collect::<Vec<_>>()
        .join("; ");
    assert_eq!(
        request(&count).unwrap_err(),
        StatusCode::RequestHeaderFieldsTooLarge
    );
}
#[test]
fn absent_empty_and_background_cookies_keep_cache_semantics_explicit() {
    let empty = request("").unwrap();
    assert!(empty.cookies().is_empty());
    assert!(empty.cache_bypass());
    let original = request("fixture=private").unwrap();
    assert_eq!(original.cookies().get("fixture"), Some("private"));
    let background = original.background_get();
    assert!(background.cookies().is_empty());
    assert!(!background.cache_bypass());
    let built = Request::prerender_get(&InternalUrl::new("/").unwrap(), Vec::new());
    assert!(built.cookies().is_empty());
    assert!(!built.cache_bypass());
    let repeated = b"GET / HTTP/1.1\r\nHost: localhost\r\nCookie: a=b\r\ncOoKiE: c=d\r\n\r\n";
    assert_eq!(
        crate::http::parse(repeated, 8192, 32).unwrap_err(),
        StatusCode::BadRequest
    );
}
#[test]
fn secure_defaults_and_explicit_local_http_do_not_downgrade_silently() {
    let cookie = Cookie::new("preference", "dark").unwrap();
    assert!(cookie.is_secure() && cookie.is_http_only());
    assert_eq!(cookie.same_site_policy(), SameSite::Lax);
    assert_eq!(cookie.cookie_path(), "/");
    assert_eq!(cookie.age_seconds(), None);
    assert_eq!(
        cookie.wire_line(),
        "Set-Cookie: preference=dark; Path=/; SameSite=Lax; Secure; HttpOnly\r\n"
    );
    let local = Cookie::local_http("preference", "dark")
        .unwrap()
        .http_only(false)
        .unwrap()
        .same_site(SameSite::Strict)
        .unwrap()
        .max_age(3600)
        .unwrap();
    assert_eq!(
        local.wire_line(),
        "Set-Cookie: preference=dark; Path=/; SameSite=Strict; Max-Age=3600\r\n"
    );
    assert!(
        Cookie::local_http("a", "v")
            .unwrap()
            .same_site(SameSite::None)
            .is_err()
    );
    assert!(
        Cookie::new("a", "v")
            .unwrap()
            .same_site(SameSite::None)
            .is_ok()
    );
    assert!(
        Cookie::new("a", "v")
            .unwrap()
            .max_age(MAX_COOKIE_AGE_SECONDS + 1)
            .is_err()
    );
}
#[test]
fn prefixes_enforce_secure_host_path_and_http_only_requirements() {
    for key in [
        "__Secure-pref",
        "__Host-pref",
        "__Http-pref",
        "__Host-Http-pref",
    ] {
        assert!(Cookie::local_http(key, "v").is_err());
        assert!(Cookie::new(key, "v").is_ok());
    }
    assert!(
        Cookie::new("__Host-pref", "v")
            .unwrap()
            .path("/account")
            .is_err()
    );
    assert!(
        Cookie::new("__Http-pref", "v")
            .unwrap()
            .http_only(false)
            .is_err()
    );
    assert!(
        Cookie::new("__Host-Http-pref", "v")
            .unwrap()
            .http_only(false)
            .is_err()
    );
    assert!(
        Cookie::new("__Secure-pref", "v")
            .unwrap()
            .path("/account")
            .is_ok()
    );
}
#[test]
fn paths_reject_header_attributes_controls_and_external_targets() {
    for path in [
        "",
        "account",
        "//other.example",
        "/a?x=1",
        "/a#b",
        "/;Secure",
        "/;HttpOnly",
        "/x,y",
        "/\"x",
        "/\\x",
        "/../x",
        "/%0d%0a",
        "/a b",
        "/x\r\nSet-Cookie:a=b",
    ] {
        assert!(
            Cookie::new("a", "v").unwrap().path(path).is_err(),
            "accepted invalid path fixture"
        );
    }
    let path = format!("/{}", "a".repeat(MAX_COOKIE_PATH_BYTES - 1));
    assert!(Cookie::new("a", "v").unwrap().path(&path).is_ok());
    assert!(Cookie::new("a", "v").unwrap().path(&(path + "a")).is_err());
    assert!(
        Cookie::new("a", "v")
            .unwrap()
            .path("/account/%3Bsettings")
            .is_ok()
    );
}
#[test]
fn removal_preserves_the_exact_scope_and_security_flags() {
    let cookie = Cookie::new("preference", "dark")
        .unwrap()
        .path("/account")
        .unwrap()
        .same_site(SameSite::Strict)
        .unwrap();
    let removed = cookie.removal();
    assert_eq!(removed.value(), "");
    assert_eq!(removed.cookie_path(), cookie.cookie_path());
    assert_eq!(removed.age_seconds(), Some(0));
    assert!(removed.is_secure() && removed.is_http_only());
    assert_eq!(removed.same_site_policy(), SameSite::Strict);
    assert!(removed.wire_line().contains("Max-Age=0"));
    assert!(!removed.wire_line().contains("Domain="));
    assert_eq!(cookie.value(), "dark");
}
#[test]
fn response_headers_are_separate_and_survive_clone_status_redirect_and_head_framing() {
    let response = Response::see_other(InternalUrl::new("/account").unwrap())
        .with_cookie(Cookie::local_http("one", "1").unwrap())
        .unwrap()
        .with_cookie(Cookie::local_http("two", "2").unwrap())
        .unwrap();
    let cloned = response.clone();
    assert_eq!(cloned.cookies(), response.cookies());
    let wire = response.wire_headers("GET, HEAD");
    assert_eq!(wire.matches("Set-Cookie: ").count(), 2);
    assert!(wire.contains("Location: /account\r\n"));
    assert!(wire.contains("Content-Length: 0\r\n"));
    assert!(wire.contains("Ruvane-Cache-Reset: cookies\r\n"));
    assert!(wire.ends_with("\r\n\r\n"));
    let error = cloned.with_status(StatusCode::BadRequest);
    assert_eq!(error.cookies().len(), 2);
    assert!(error.location().is_none());
    assert!(error.wire_headers("").contains("Set-Cookie: one=1;"));
}
#[test]
fn duplicate_and_count_limits_leave_existing_response_unchanged() {
    let mut response = Response::text("ok");
    let cookie = Cookie::new("one", "a").unwrap();
    response.add_cookie(cookie.clone()).unwrap();
    assert_eq!(
        response.add_cookie(cookie),
        Err(CookieError::DuplicateCookie)
    );
    assert_eq!(response.cookies().len(), 1);
    response
        .add_cookie(Cookie::new("one", "b").unwrap().path("/account").unwrap())
        .unwrap();
    for n in 2..MAX_RESPONSE_COOKIES {
        response
            .add_cookie(Cookie::new(&format!("n{n}"), "v").unwrap())
            .unwrap();
    }
    assert_eq!(
        response.add_cookie(Cookie::new("extra", "v").unwrap()),
        Err(CookieError::TooManyCookies)
    );
    assert_eq!(response.cookies().len(), MAX_RESPONSE_COOKIES);
}
#[test]
fn total_response_cookie_bytes_are_bounded_without_partial_append() {
    let mut response = Response::text("ok");
    for n in 0..2 {
        response
            .add_cookie(
                Cookie::new(&format!("n{n}"), &"v".repeat(MAX_COOKIE_VALUE_BYTES))
                    .unwrap()
                    .path(&format!("/{}", "p".repeat(MAX_COOKIE_PATH_BYTES - 1)))
                    .unwrap(),
            )
            .unwrap();
    }
    let before = response.wire_headers("");
    let next = Cookie::new("extra", &"v".repeat(MAX_COOKIE_VALUE_BYTES))
        .unwrap()
        .path(&format!("/{}", "p".repeat(MAX_COOKIE_PATH_BYTES - 1)))
        .unwrap();
    assert_eq!(response.add_cookie(next), Err(CookieError::TooLarge));
    assert_eq!(response.wire_headers(""), before);
}
#[test]
fn cookie_changes_override_public_navigation_policy_and_are_not_packet_data() {
    let page = NavigationPage::new(
        InternalUrl::new("/").unwrap(),
        "Public",
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap();
    let mut response =
        Response::navigation_with_cache(&page, NavigationCachePolicy::public(30).unwrap());
    assert!(response.cache_control().starts_with("public,"));
    let bytes = response.body().to_vec();
    response
        .add_cookie(Cookie::new("fixture", "secret").unwrap())
        .unwrap();
    response = response.limit_navigation_freshness(10);
    assert_eq!(response.cache_control(), "no-store");
    assert_eq!(response.body(), bytes);
    assert!(!response.body().windows(6).any(|bytes| bytes == b"secret"));
}
#[test]
fn cookie_debug_and_errors_omit_values_including_original_request_headers() {
    let secret = "PRIVATE_COOKIE_FIXTURE";
    let cookie = Cookie::new("fixture", secret).unwrap();
    assert!(!format!("{cookie:?}").contains(secret));
    let cookies = Cookies::parse(&format!("fixture={secret}")).unwrap();
    assert!(!format!("{cookies:?}").contains(secret));
    let request = request(&format!("fixture={secret}")).unwrap();
    assert!(!format!("{request:?}").contains(secret));
    let response = Response::text("ok").with_cookie(cookie).unwrap();
    assert!(!format!("{response:?}").contains(secret));
    let error = Cookie::new("fixture", &format!("{secret}\r\n")).unwrap_err();
    assert!(!format!("{error:?}: {error}").contains(secret));
}
