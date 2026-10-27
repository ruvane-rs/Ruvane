#![cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[allow(dead_code)]
#[path = "../examples/support/file_store_demo.rs"]
mod demo;
use ruvane::*;
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        loop {
            let root = parent.join(format!(
                "ruvane-private-http-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{error}"),
            }
        }
    }
    fn path(&self) -> PathBuf {
        self.0.join("files")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Running {
    address: SocketAddr,
    stop: ShutdownHandle,
    thread: Option<thread::JoinHandle<std::io::Result<()>>>,
}
type Reply = (String, Vec<u8>);
impl Running {
    fn start(path: &Path) -> Self {
        let files = FileStore::open(path, "http-test", FileStoreLimits::default()).unwrap();
        let server = Server::bind(
            "127.0.0.1:0".parse().unwrap(),
            demo::app(files).unwrap().router(None).unwrap(),
        )
        .unwrap();
        let address = server.local_addr().unwrap();
        let stop = server.shutdown_handle();
        Self {
            address,
            stop,
            thread: Some(thread::spawn(move || server.run())),
        }
    }
    fn request(&self, method: &str, path: &str, extra: &str, body: &[u8]) -> Reply {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\n{extra}\r\n",
            self.address, self.address
        )
        .unwrap();
        let _ = stream.write_all(body);
        let _ = stream.shutdown(Shutdown::Write);
        let mut bytes = Vec::new();
        if let Err(error) = stream.read_to_end(&mut bytes) {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        let end = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        (
            String::from_utf8(bytes[..end].to_vec()).unwrap(),
            bytes[end + 4..].to_vec(),
        )
    }
    fn session(&self, account: &str) -> (String, String) {
        let body = format!("account={account}");
        let response = self.request(
            "POST",
            "/private-files/session",
            &format!(
                "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
                body.len()
            ),
            body.as_bytes(),
        );
        assert_eq!(status(&response), 303);
        let cookie = header(&response, "Set-Cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let token = self.request(
            "GET",
            "/private-files/csrf",
            &format!("Cookie: {cookie}\r\n"),
            b"",
        );
        (cookie, header(&token, CSRF_HEADER).unwrap().to_string())
    }
    fn upload(&self, cookie: &str, token: &str, data: &[u8]) -> Reply {
        let mut body = b"--x\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"Rust \xf0\x9f\xa6\x80.bin\"\r\nContent-Type: text/html\r\n\r\n".to_vec();
        body.extend(data);
        body.extend(b"\r\n--x--\r\n");
        self.request("POST","/private-files/files",&format!("Cookie: {cookie}\r\n{CSRF_HEADER}: {token}\r\nAccept: application/json\r\nContent-Type: multipart/form-data; boundary=x\r\nContent-Length: {}\r\n",body.len()),&body)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn status(reply: &Reply) -> u16 {
    reply.0.split_whitespace().nth(1).unwrap().parse().unwrap()
}
fn header<'a>(reply: &'a Reply, name: &str) -> Option<&'a str> {
    reply
        .0
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}
fn id(reply: &Reply) -> String {
    JsonValue::parse(&reply.1)
        .unwrap()
        .get("id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}
#[test]
fn private_get_head_and_delete_preserve_ownership_binary_and_header_contracts() {
    let temp = Temp::new();
    let server = Running::start(&temp.path());
    assert_eq!(
        status(&server.request("GET", "/private-files/files", "", b"")),
        403
    );
    let (a, ta) = server.session("alpha");
    let (b, tb) = server.session("beta");
    let binary = b"\0\xff\r\n<script>\x80";
    let uploaded = server.upload(&a, &ta, binary);
    assert_eq!(status(&uploaded), 200);
    let id = id(&uploaded);
    let path = format!("/private-files/files/{id}");
    for method in ["GET", "HEAD"] {
        let response = server.request(method, &path, &format!("Cookie: {a}\r\n"), b"");
        assert_eq!(status(&response), 200);
        assert_eq!(
            header(&response, "content-type"),
            Some("application/octet-stream")
        );
        assert_eq!(
            header(&response, "content-length"),
            Some(binary.len().to_string().as_str())
        );
        assert_eq!(header(&response, "cache-control"), Some("no-store"));
        assert_eq!(header(&response, "x-content-type-options"), Some("nosniff"));
        assert_eq!(
            header(&response, "content-disposition"),
            Some("attachment; filename=\"download\"; filename*=UTF-8''Rust%20%F0%9F%A6%80.bin")
        );
        assert!(
            header(&response, "content-security-policy")
                .unwrap()
                .contains("script-src 'none'")
        );
        assert_eq!(
            response.1,
            if method == "GET" {
                binary.as_slice()
            } else {
                b""
            }
        );
    }
    let forged = server.request(
        "GET",
        &(path.clone() + "?owner=alpha"),
        &format!("Cookie: {b}\r\n"),
        b"",
    );
    let absent = server.request(
        "GET",
        &format!("/private-files/files/{}", "a".repeat(64)),
        &format!("Cookie: {b}\r\n"),
        b"",
    );
    assert_eq!(status(&forged), 404);
    assert_eq!(forged.1, absent.1);
    assert!(header(&forged, "content-disposition").is_none());
    let foreign_delete = server.request(
        "DELETE",
        &path,
        &format!("Cookie: {b}\r\n{CSRF_HEADER}: {tb}\r\n"),
        b"",
    );
    assert_eq!(status(&foreign_delete), 404);
    let denied = server.request("DELETE", &path, &format!("Cookie: {a}\r\n"), b"");
    assert_eq!(status(&denied), 403);
    assert_eq!(
        status(&server.request("GET", &path, &format!("Cookie: {a}\r\n"), b"")),
        200
    );
    let deleted = server.request(
        "DELETE",
        &path,
        &format!("Cookie: {a}\r\n{CSRF_HEADER}: {ta}\r\n"),
        b"",
    );
    assert_eq!(status(&deleted), 200);
    assert_eq!(deleted.1, b"true");
    assert_eq!(
        status(&server.request("GET", &path, &format!("Cookie: {a}\r\n"), b"")),
        404
    );
}
#[test]
fn clean_server_restarts_keep_files_but_require_a_new_server_session_owner_mapping() {
    let temp = Temp::new();
    let server = Running::start(&temp.path());
    let (a, ta) = server.session("alpha");
    let exact = vec![0xff; 32768];
    let saved = server.upload(&a, &ta, &exact);
    assert_eq!(status(&saved), 200);
    let file = id(&saved);
    let too_big = server.upload(&a, &ta, &vec![0; 32769]);
    assert_eq!(status(&too_big), 413);
    drop(server);
    let server = Running::start(&temp.path());
    let path = format!("/private-files/files/{file}");
    assert_eq!(
        status(&server.request("GET", &path, &format!("Cookie: {a}\r\n"), b"")),
        403
    );
    let (b, _) = server.session("beta");
    assert_eq!(
        status(&server.request("GET", &path, &format!("Cookie: {b}\r\n"), b"")),
        404
    );
    let (a, ta) = server.session("alpha");
    let fetched = server.request("GET", &path, &format!("Cookie: {a}\r\n"), b"");
    assert_eq!(fetched.1, exact);
    let body = format!("_ruvane_csrf={ta}");
    let deleted = server.request("POST",&(path.clone()+"/remove"),&format!("Cookie: {a}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",body.len()),body.as_bytes());
    assert_eq!(status(&deleted), 303);
    drop(server);
    let server = Running::start(&temp.path());
    let (a, _) = server.session("alpha");
    assert_eq!(
        status(&server.request("GET", &path, &format!("Cookie: {a}\r\n"), b"")),
        404
    );
}
