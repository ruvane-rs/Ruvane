use super::*;
use ruvane::{ActionData, ActionReply, Cookie};
fn post(server: &Running, target: &str, body: &str, origin: &str, cookie: &str) -> Vec<u8> {
    let cookie = if cookie.is_empty() {
        String::new()
    } else {
        format!("Cookie: {cookie}\r\n")
    };
    server.raw(format!("POST {target} HTTP/1.1\r\nHost: {}\r\nOrigin: {origin}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n{cookie}\r\n{body}",server.address,body.len()).as_bytes())
}
#[test]
fn cookies_native_create_read_delete_and_redirect_keep_existing_post_checks() {
    let mut router = Router::new();
    router
        .get("/preference", |request| {
            Response::text(request.cookies().get("preference").unwrap_or("none"))
        })
        .unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let calls = writes.clone();
    router
        .post_form("/preference", move |request| {
            calls.fetch_add(1, Ordering::SeqCst);
            let cookie = Cookie::local_http("preference", "dark").unwrap();
            let cookie = if request.form_data().unwrap().get("mode") == Some("clear") {
                cookie.removal()
            } else {
                cookie
            };
            Response::see_other(ruvane::InternalUrl::new("/preference").unwrap())
                .with_cookie(cookie)
                .unwrap()
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    let origin = format!("http://{}", server.address);
    let rejected = post(
        &server,
        "/preference",
        "mode=set",
        "http://foreign.example",
        "",
    );
    assert_eq!(status(&rejected), 403);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(!parts(&rejected).0.contains("Set-Cookie:"));
    let created = post(&server, "/preference", "mode=set", &origin, "");
    assert_eq!(status(&created), 303);
    let (headers, body) = parts(&created);
    assert!(body.is_empty());
    assert!(headers.contains("Location: /preference"));
    assert!(headers.contains("Set-Cookie: preference=dark; Path=/; SameSite=Lax; HttpOnly"));
    assert!(headers.contains("Ruvane-Cache-Reset: cookies"));
    assert!(headers.contains("Cache-Control: no-store"));
    let read = server.raw(
        format!(
            "GET /preference HTTP/1.1\r\nHost: {}\r\nCookie: preference=dark\r\n\r\n",
            server.address
        )
        .as_bytes(),
    );
    assert_eq!(parts(&read).1, b"dark");
    let removed = post(
        &server,
        "/preference",
        "mode=clear",
        &origin,
        "preference=dark",
    );
    assert_eq!(status(&removed), 303);
    assert!(
        parts(&removed)
            .0
            .contains("Set-Cookie: preference=; Path=/; SameSite=Lax; HttpOnly; Max-Age=0")
    );
    assert_eq!(parts(&server.request("GET", "/preference")).1, b"none");
    assert_eq!(writes.load(Ordering::SeqCst), 2);
}
#[test]
fn separate_cookie_fields_are_preserved_on_get_head_and_typed_action_responses() {
    fn reply() -> Response {
        Response::text("four")
            .with_cookie(Cookie::new("secure-preference", "dark").unwrap())
            .unwrap()
            .with_cookie(Cookie::local_http("local-preference", "light").unwrap())
            .unwrap()
    }
    let mut router = Router::new();
    router.get("/cookies", |_| reply()).unwrap();
    router
        .post_action_response("/cookie-action", |_| {
            Response::action(&ActionReply::success(ActionData::default()))
                .with_cookie(Cookie::local_http("action-preference", "dark").unwrap())
                .unwrap()
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for method in ["GET", "HEAD"] {
        let bytes = server.request(method, "/cookies");
        let (headers, body) = parts(&bytes);
        assert_eq!(status(&bytes), 200);
        assert_eq!(headers.matches("Set-Cookie: ").count(), 2);
        assert_eq!(content_length(headers), 4);
        assert_eq!(
            body,
            if method == "HEAD" {
                b"".as_slice()
            } else {
                b"four".as_slice()
            }
        );
    }
    let bytes = post(
        &server,
        "/cookie-action",
        "",
        &format!("http://{}", server.address),
        "",
    );
    let (headers, body) = parts(&bytes);
    assert_eq!(status(&bytes), 200);
    assert!(headers.contains("application/vnd.ruvane.action"));
    assert!(headers.contains("Set-Cookie: action-preference=dark;"));
    assert!(ActionReply::decode(body).unwrap().data().is_empty());
    assert!(!body.windows(4).any(|bytes| bytes == b"dark"));
}
#[test]
fn malformed_duplicate_and_oversized_request_cookies_never_reach_handlers() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let mut router = Router::new();
    router
        .get("/", move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Response::text("ok")
        })
        .unwrap();
    let server = Running::start(router, ServerConfig::default());
    for (value, expected) in [
        ("preference=dark; preference=light".to_owned(), 400),
        ("preference=one,two".to_owned(), 400),
        (
            format!("preference={}", "a".repeat(ruvane::MAX_COOKIE_HEADER_BYTES)),
            431,
        ),
    ] {
        let response = server.raw(
            format!(
                "GET / HTTP/1.1\r\nHost: {}\r\nCookie: {value}\r\n\r\n",
                server.address
            )
            .as_bytes(),
        );
        assert_eq!(status(&response), expected);
        assert!(
            !parts(&response)
                .1
                .windows(10)
                .any(|bytes| bytes == b"preference")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(status(&server.request("GET", "/")), 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
