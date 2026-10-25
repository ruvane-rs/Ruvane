use ruvane::{
    BrowserCommandError as Error, BrowserCommands, BrowserTag, CONTENT_SECURITY_POLICY, Document,
    INTERACTIVE_CONTENT_SECURITY_POLICY, InternalUrl, MAX_BROWSER_COMMAND_BYTES,
    MAX_BROWSER_COMMANDS, MAX_BROWSER_TEXT_BYTES, MediaType, Response, Router, Server, StaticPath,
    StaticSite, module_script,
};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

#[test]
fn command_encoding_is_bounded_atomic_and_utf8_preserving() {
    let mut commands = BrowserCommands::new();
    assert_eq!(commands.as_bytes(), b"RVN6\0\0\0\0");
    commands.element(1, 1, 0, BrowserTag::Section).unwrap();
    commands.text(1, 2, "Rust <script> 🌱").unwrap();
    commands.click(1, 3, 9).unwrap();
    commands.disabled(1, 3, true).unwrap();
    assert_eq!(commands.count(), 4);
    assert!(
        commands
            .as_bytes()
            .windows("Rust <script> 🌱".len())
            .any(|w| w == "Rust <script> 🌱".as_bytes())
    );
    let before = commands.as_bytes().to_vec();
    assert_eq!(
        commands.text(1, 2, &"x".repeat(MAX_BROWSER_TEXT_BYTES + 1)),
        Err(Error::Limit)
    );
    assert_eq!(commands.text(0, 2, "bad"), Err(Error::InvalidIdentifier));
    assert_eq!(commands.click(1, 2, 0), Err(Error::InvalidIdentifier));
    assert_eq!(
        commands.keyed_element(1, 3, 0, BrowserTag::Section, 0),
        Err(Error::InvalidIdentifier)
    );
    assert_eq!(commands.place(1, 3, 3), Err(Error::InvalidIdentifier));
    assert_eq!(commands.remove_node(0, 3), Err(Error::InvalidIdentifier));
    assert_eq!(commands.as_bytes(), before);
    commands.clear();
    for _ in 0..MAX_BROWSER_COMMANDS {
        commands.remove(1).unwrap();
    }
    let before = commands.as_bytes().to_vec();
    assert_eq!(commands.remove(1), Err(Error::Limit));
    assert_eq!(commands.as_bytes(), before);
    commands.clear();
    let text = "x".repeat(MAX_BROWSER_TEXT_BYTES);
    while commands.text(1, 2, &text).is_ok() {}
    assert!(commands.as_bytes().len() <= MAX_BROWSER_COMMAND_BYTES);
    let before = commands.as_bytes().to_vec();
    assert_eq!(commands.text(1, 2, &text), Err(Error::Limit));
    assert_eq!(commands.as_bytes(), before);
}

#[test]
fn script_helpers_and_response_policies_are_explicit() {
    let url = InternalUrl::new("/assets/bridge.js")
        .unwrap()
        .query("a", "<&\"")
        .unwrap()
        .query("b", "2")
        .unwrap();
    let script = module_script(&url);
    assert_eq!(
        script.as_str(),
        "<script type=\"module\" src=\"/assets/bridge.js?a=%3C%26%22&amp;b=2\"></script>"
    );
    assert_eq!(
        Response::html(script.clone()).content_security_policy(),
        CONTENT_SECURITY_POLICY
    );
    assert_eq!(
        Response::interactive_html(script).content_security_policy(),
        INTERACTIVE_CONTENT_SECURITY_POLICY
    );
    assert!(!INTERACTIVE_CONTENT_SECURITY_POLICY.contains("'unsafe-inline'"));
    assert!(!INTERACTIVE_CONTENT_SECURITY_POLICY.contains("'unsafe-eval'"));
    assert_eq!(
        Response::bytes([], MediaType::JavaScript).content_type(),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        Response::bytes([], MediaType::WebAssembly).content_type(),
        "application/wasm"
    );
}

#[test]
fn interactive_policy_is_per_response_and_does_not_follow_request_headers() {
    let mut router = Router::new();
    router
        .get("/", |_| {
            Response::interactive_html(Document::new("Counter", "client root").render())
        })
        .unwrap();
    router
        .get("/static", |_| {
            Response::html(Document::new("Static", "static").render())
        })
        .unwrap();
    router
        .asset("/assets/a.js", b"export {};", MediaType::JavaScript)
        .unwrap();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), router).unwrap();
    let address = server.local_addr().unwrap();
    let stop = server.shutdown_handle();
    let thread = std::thread::spawn(move || server.run().unwrap());
    for (method, path, policy) in [
        ("GET", "/", INTERACTIVE_CONTENT_SECURITY_POLICY),
        ("HEAD", "/", INTERACTIVE_CONTENT_SECURITY_POLICY),
        ("GET", "/static", CONTENT_SECURITY_POLICY),
        ("GET", "/missing", CONTENT_SECURITY_POLICY),
        ("GET", "/assets/a.js", CONTENT_SECURITY_POLICY),
    ] {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Security-Policy: default-src *\r\n\r\n").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut output = String::new();
        stream.read_to_string(&mut output).unwrap();
        let (headers, body) = output.split_once("\r\n\r\n").unwrap();
        assert_eq!(
            headers
                .lines()
                .filter(|line| line.starts_with("Content-Security-Policy:"))
                .count(),
            1
        );
        assert!(headers.contains(&format!("Content-Security-Policy: {policy}")));
        if method == "HEAD" {
            assert!(body.is_empty());
        }
    }
    stop.shutdown();
    thread.join().unwrap();
}

#[test]
fn static_js_and_wasm_assets_keep_the_default_static_hosting_contract() {
    let mut site = StaticSite::new();
    site.page(
        StaticPath::new("/").unwrap(),
        Document::new("Static", "static").render(),
    )
    .unwrap();
    for (path, mime) in [
        ("/a.js", MediaType::JavaScript),
        ("/a.mjs", MediaType::JavaScript),
        ("/a.wasm", MediaType::WebAssembly),
    ] {
        site.asset(StaticPath::new(path).unwrap(), b"data", mime)
            .unwrap();
    }
    assert!(
        site.asset(
            StaticPath::new("/wrong.txt").unwrap(),
            [],
            MediaType::WebAssembly
        )
        .is_err()
    );
    assert!(site.required_headers().contains(CONTENT_SECURITY_POLICY));
}
