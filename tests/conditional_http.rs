#![cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[path = "../examples/support/json_tasks.rs"]
mod tasks;
use ruvane::*;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
struct Running {
    address: SocketAddr,
    stop: ShutdownHandle,
    thread: Option<thread::JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn new(app: App) -> Self {
        let server =
            Server::bind("127.0.0.1:0".parse().unwrap(), app.router(None).unwrap()).unwrap();
        let address = server.local_addr().unwrap();
        let stop = server.shutdown_handle();
        Self {
            address,
            stop,
            thread: Some(thread::spawn(move || server.run())),
        }
    }
    fn request(&self, method: &str, path: &str, extra: &str, body: &str) -> (String, Vec<u8>) {
        send(self.address, method, path, extra, body)
    }
    fn create(&self, title: &str) -> u64 {
        let body = JsonValue::object(vec![("title".into(), JsonValue::string(title).unwrap())])
            .unwrap()
            .encode();
        let response = self.request("POST", "/api/tasks", "", &body);
        assert_eq!(status(&response.0), 200);
        JsonValue::parse(&response.1)
            .unwrap()
            .get("id")
            .unwrap()
            .as_number()
            .unwrap()
            .to_u64()
            .unwrap()
    }
    fn tag(&self, id: u64) -> String {
        let response = self.request("GET", &format!("/api/tasks/{id}"), "", "");
        assert_eq!(status(&response.0), 200);
        header(&response.0, "etag").unwrap().to_owned()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    extra: &str,
    body: &str,
) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let profile = if matches!(method, "POST" | "PUT" | "PATCH") {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    } else {
        String::new()
    };
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\n{profile}{extra}\r\n{body}").unwrap();
    let mut bytes = Vec::new();
    if let Err(error) = stream.read_to_end(&mut bytes) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    let end = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    (
        String::from_utf8(bytes[..end].to_vec()).unwrap(),
        bytes[end + 4..].to_vec(),
    )
}
fn status(headers: &str) -> u16 {
    headers.split_whitespace().nth(1).unwrap().parse().unwrap()
}
fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}
fn app(store: Arc<tasks::Tasks>, guards: Vec<RouteGuard>) -> App {
    let mut app = App::new();
    let mut routes = FileRouteRegistry::new(&mut app);
    routes
        .endpoint(
            "api/tasks/route.rs",
            "/api/tasks",
            tasks::endpoint(store.clone()),
        )
        .unwrap();
    let mut endpoint = tasks::resource_endpoint(store);
    for guard in guards {
        endpoint = endpoint.guard(guard);
    }
    routes
        .endpoint("api/tasks/[id]/route.rs", "/api/tasks/{id}", endpoint)
        .unwrap();
    routes.finish().unwrap();
    app
}
#[test]
fn read_versions_are_strong_and_mutations_require_a_matching_precise_condition() {
    let store = Arc::new(tasks::Tasks::new().unwrap());
    let server = Running::new(app(store.clone(), vec![]));
    let id = server.create("first");
    let tag = server.tag(id);
    assert!(!EntityTag::parse(&tag).unwrap().is_weak());
    let head = server.request("HEAD", "/api/tasks/1", "", "");
    assert_eq!(header(&head.0, "etag"), Some(tag.as_str()));
    assert!(head.1.is_empty());
    assert_eq!(header(&head.0, "cache-control"), Some("no-store"));
    for method in ["PUT", "PATCH", "DELETE"] {
        let body = if method == "DELETE" {
            ""
        } else {
            "{\"title\":\"changed\",\"completed\":true}"
        };
        for (condition, expected) in [
            (String::new(), 428),
            ("If-Match: *\r\n".into(), 428),
            ("If-Match: \"wrong\"\r\n".into(), 412),
            (format!("If-Match: W/{tag}\r\n"), 412),
        ] {
            let response = server.request(method, "/api/tasks/1", &condition, body);
            assert_eq!(status(&response.0), expected, "{method}");
            assert_eq!(header(&response.0, "etag"), None);
            assert_eq!(header(&response.0, "cache-control"), Some("no-store"));
        }
    }
    assert_eq!(store.counts().unwrap().1, 1);
    let accepted = server.request(
        "PATCH",
        "/api/tasks/1",
        &format!("If-Match: , \"wrong\", W/{tag}, {tag},\r\n"),
        "{\"title\":\"changed\"}",
    );
    assert_eq!(status(&accepted.0), 200);
    let fresh = server.tag(id);
    assert_ne!(fresh, tag);
    let stale = server.request("GET", "/api/tasks/1", &format!("If-Match: {tag}\r\n"), "");
    assert_eq!(status(&stale.0), 412);
    let replace = server.request(
        "PUT",
        "/api/tasks/1",
        &format!("If-Match: {fresh}\r\n"),
        "{\"title\":\"replacement\",\"completed\":true}",
    );
    assert_eq!(status(&replace.0), 200);
    assert!(header(&replace.0, "etag").is_none());
}
#[test]
fn malformed_and_excessive_conditions_never_reach_native_guards() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let store = Arc::new(tasks::Tasks::new().unwrap());
    let server = Running::new(app(
        store,
        vec![RouteGuard::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            GuardDecision::Allow
        })],
    ));
    server.create("first");
    for method in ["GET", "HEAD", "PUT", "PATCH", "DELETE"] {
        for (extra, expected) in [
            ("If-Match: *,\"x\"\r\n".into(), 400),
            ("If-Match: \"x\"\r\nif-match: \"x\"\r\n".into(), 400),
            (format!("If-Match: {}\r\n", "x".repeat(2049)), 431),
            (
                format!("If-Match: {}\r\n", vec!["\"x\""; 17].join(",")),
                431,
            ),
        ] {
            let response = server.request(
                method,
                "/api/tasks/1",
                &extra,
                if matches!(method, "PUT" | "PATCH") {
                    "{}"
                } else {
                    ""
                },
            );
            assert_eq!(status(&response.0), expected);
            if method == "HEAD" {
                assert!(response.1.is_empty());
            }
        }
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}
#[test]
fn checking_the_version_and_committing_are_one_atomic_operation() {
    let store = Arc::new(tasks::Tasks::new().unwrap());
    let server = Running::new(app(store.clone(), vec![]));
    let id = server.create("original");
    let tag = server.tag(id);
    let before = store.counts().unwrap().1;
    let gate = Arc::new(Barrier::new(16));
    let address = server.address;
    let threads = (0..16)
        .map(|n| {
            let gate = gate.clone();
            let tag = tag.clone();
            thread::spawn(move || {
                gate.wait();
                send(
                    address,
                    "PUT",
                    "/api/tasks/1",
                    &format!("If-Match: {tag}\r\n"),
                    &format!("{{\"title\":\"writer {n}\",\"completed\":{}}}", n % 2 == 0),
                )
            })
        })
        .collect::<Vec<_>>();
    let responses = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.iter().filter(|r| status(&r.0) == 200).count(), 1);
    assert_eq!(responses.iter().filter(|r| status(&r.0) == 412).count(), 15);
    assert_eq!(store.counts().unwrap().1, before + 1);
    let winner = responses.iter().find(|r| status(&r.0) == 200).unwrap();
    assert_eq!(server.request("GET", "/api/tasks/1", "", "").1, winner.1);
    let fresh = server.tag(id);
    assert_ne!(fresh, tag);
    let no_op = server.request(
        "PATCH",
        "/api/tasks/1",
        &format!("If-Match: {fresh}\r\n"),
        "{\"completed\":true}",
    );
    assert_eq!(status(&no_op.0), 200);
    assert_ne!(server.tag(id), fresh);
}
#[test]
fn deletion_and_new_store_lifecycles_never_accept_retired_versions() {
    let server = Running::new(app(Arc::new(tasks::Tasks::new().unwrap()), vec![]));
    let id = server.create("old");
    let old = server.tag(id);
    assert_eq!(
        status(
            &server
                .request(
                    "DELETE",
                    "/api/tasks/1",
                    &format!("If-Match: {old}\r\n"),
                    ""
                )
                .0
        ),
        200
    );
    assert_eq!(
        status(
            &server
                .request(
                    "PATCH",
                    "/api/tasks/1",
                    &format!("If-Match: {old}\r\n"),
                    "{\"title\":\"old\"}"
                )
                .0
        ),
        404
    );
    let id = server.create("next");
    assert_eq!(id, 2);
    assert_ne!(server.tag(id), old);
    drop(server);
    let server = Running::new(app(Arc::new(tasks::Tasks::new().unwrap()), vec![]));
    let id = server.create("new lifecycle");
    assert_eq!(id, 1);
    assert_ne!(server.tag(id), old);
    assert_eq!(
        status(
            &server
                .request(
                    "PATCH",
                    "/api/tasks/1",
                    &format!("If-Match: {old}\r\n"),
                    "{\"title\":\"stale\"}"
                )
                .0
        ),
        412
    );
}
#[test]
fn session_csrf_and_quota_guards_precede_resource_preconditions() {
    let sessions = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("conditional_test").unwrap(),
    )
    .unwrap();
    let cookie = sessions.create(SessionData::default()).unwrap();
    let quota = RateLimiter::new(
        RateLimitPolicy::new(2, 60).unwrap(),
        RateLimitLimits::default(),
    );
    let tasks = Arc::new(tasks::Tasks::new().unwrap());
    let mut application = app(
        tasks.clone(),
        vec![sessions.csrf_guard(), quota.session_guard(sessions.clone())],
    );
    let tokens = sessions.clone();
    let mut routes = FileRouteRegistry::new(&mut application);
    routes
        .endpoint(
            "csrf/route.rs",
            "/csrf",
            FileEndpoint::new().get(move |r| tokens.csrf_response(r)),
        )
        .unwrap();
    routes.finish().unwrap();
    let server = Running::new(application);
    server.create("guarded");
    let credentials = format!("Cookie: {}={}\r\n", cookie.name(), cookie.value());
    let token = server.request("GET", "/csrf", &credentials, "");
    let claim = header(&token.0, "ruvane-csrf-token").unwrap();
    for method in ["PUT", "PATCH", "DELETE"] {
        for extra in [
            "".into(),
            credentials.clone(),
            format!("{credentials}Ruvane-Csrf-Token: forged\r\nIf-Match: \"stale\"\r\n"),
        ] {
            let response = server.request(
                method,
                "/api/tasks/1",
                &extra,
                if method == "DELETE" {
                    ""
                } else {
                    "{\"title\":\"blocked\",\"completed\":true}"
                },
            );
            assert_eq!(status(&response.0), 403);
            assert_eq!(header(&response.0, "etag"), None);
        }
    }
    assert_eq!(quota.stats().unwrap().allowed, 0);
    assert_eq!(tasks.counts().unwrap().1, 1);
    let authenticated =
        format!("{credentials}Ruvane-Csrf-Token: {claim}\r\nIf-Match: \"stale\"\r\n");
    for expected in [412, 412, 429] {
        assert_eq!(
            status(
                &server
                    .request(
                        "PATCH",
                        "/api/tasks/1",
                        &authenticated,
                        "{\"title\":\"blocked\"}"
                    )
                    .0
            ),
            expected
        );
    }
    assert_eq!(tasks.counts().unwrap().1, 1);
}
#[test]
fn put_transport_omits_validators_that_cannot_attest_to_untransformed_input() {
    let mut application = App::new();
    let mut routes = FileRouteRegistry::new(&mut application);
    let handler =
        |_: &Request| Response::json(&true.into()).with_etag(EntityTag::strong("tag").unwrap());
    routes
        .endpoint(
            "tag/route.rs",
            "/tag",
            FileEndpoint::new()
                .get(handler)
                .put_json(handler)
                .patch_json(handler),
        )
        .unwrap();
    routes.finish().unwrap();
    let server = Running::new(application);
    for method in ["GET", "HEAD", "PUT", "PATCH"] {
        let response = server.request(
            method,
            "/tag",
            "",
            if matches!(method, "PUT" | "PATCH") {
                "{}"
            } else {
                ""
            },
        );
        assert_eq!(status(&response.0), 200);
        assert_eq!(
            header(&response.0, "etag"),
            if method == "PUT" {
                None
            } else {
                Some("\"tag\"")
            }
        );
    }
}
