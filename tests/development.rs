use ruvane::{Development, MAX_DEVELOPMENT_STATUS_BYTES, Response, Router, Server};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "ruvane-development-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn status(&self) -> PathBuf {
        let path = self.0.join("status");
        fs::write(&path, "RVD1\n9\n1\nbuilding\n0\n\n").unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Running {
    address: SocketAddr,
    stop: ruvane::ShutdownHandle,
    thread: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn new(router: Router) -> Self {
        let server = Server::bind("127.0.0.1:0".parse().unwrap(), router).unwrap();
        Self {
            address: server.local_addr().unwrap(),
            stop: server.shutdown_handle(),
            thread: Some(std::thread::spawn(move || server.run())),
        }
    }
    fn get(&self, path: &str, headers: &str) -> String {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            self.address
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.shutdown();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
const HEADERS: &str = "Accept: text/plain\r\nRuvane-Development: 9\r\n";

#[test]
fn status_requires_a_matching_session_and_same_origin() {
    let temp = Temp::new();
    let path = temp.status();
    let server = Running::new(
        Development::new(&path, 9)
            .unwrap()
            .waiting_router()
            .unwrap(),
    );
    let response = server.get("/_ruvane/dev/status", HEADERS);
    assert!(
        response.starts_with("HTTP/1.1 200") && response.ends_with("RVD1\n9\n1\nbuilding\n0\n\n")
    );
    assert!(
        response.contains("Cache-Control: no-store")
            && response.contains("X-Content-Type-Options: nosniff")
    );
    assert!(!response.contains("Access-Control-Allow-Origin"));
    for headers in [
        "Accept: text/plain\r\n",
        "Accept: text/plain\r\nRuvane-Development: 8\r\n",
    ] {
        assert!(
            server
                .get("/_ruvane/dev/status", headers)
                .starts_with("HTTP/1.1 409")
        );
    }
    for extra in [
        "Origin: http://example.com\r\n",
        "Sec-Fetch-Site: cross-site\r\n",
        "Sec-Fetch-Site: same-site\r\n",
    ] {
        assert!(
            server
                .get("/_ruvane/dev/status", &format!("{HEADERS}{extra}"))
                .starts_with("HTTP/1.1 403")
        );
    }
    assert!(
        server
            .get(
                "/_ruvane/dev/status",
                "Ruvane-Development: 9\r\nAccept: text/html\r\n"
            )
            .starts_with("HTTP/1.1 403")
    );
    let page = server.get("/tasks/123", "");
    assert!(
        page.contains("script-src 'self' 'wasm-unsafe-eval'")
            && page.contains("data-session=\"9\" data-revision=\"0\"")
    );
    assert!(!page.contains(path.to_str().unwrap()));
    let native = server.get("/tasks/123?native=yes", "");
    assert!(native.contains(ruvane::CONTENT_SECURITY_POLICY) && !native.contains("<script"));
    for asset in ["client.mjs", "style.css"] {
        assert!(
            server
                .get(&format!("/_ruvane/dev/{asset}"), "")
                .starts_with("HTTP/1.1 200")
        );
    }
    fs::write(&path, "not a status").unwrap();
    let failed = server.get("/_ruvane/dev/status", HEADERS);
    assert!(failed.starts_with("HTTP/1.1 500") && !failed.contains(path.to_str().unwrap()));
}

#[test]
fn status_files_are_bounded_and_revalidated_on_each_request() {
    let temp = Temp::new();
    let path = temp.status();
    assert!(Development::new(&path, 0).is_err());
    assert!(Development::new(&path, 8).is_err());
    let server = Running::new(
        Development::new(&path, 9)
            .unwrap()
            .waiting_router()
            .unwrap(),
    );
    for bytes in [
        vec![b'x'; MAX_DEVELOPMENT_STATUS_BYTES + 1],
        vec![255],
        b"RVD1\n9\n2\nready\n1\n\n\x1b".to_vec(),
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(
            server
                .get("/_ruvane/dev/status", HEADERS)
                .starts_with("HTTP/1.1 500")
        );
        assert!(Development::new(&path, 9).is_err());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(Development::new(&path, 9).is_err());
}

#[test]
fn development_mount_rejects_collisions_before_registering_any_route() {
    let temp = Temp::new();
    let development = Development::new(temp.status(), 9).unwrap();
    let mut router = Router::new();
    router
        .get("/_ruvane/dev/{name}", |_| Response::text("owned"))
        .unwrap();
    assert!(development.mount(&mut router).is_err());
    let server = Running::new(router);
    assert!(server.get("/_ruvane/dev/client.mjs", "").ends_with("owned"));
}

#[cfg(unix)]
#[test]
fn development_rejects_symlinks_in_files_and_ancestors() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let path = temp.status();
    symlink(&path, temp.0.join("linked-status")).unwrap();
    assert!(Development::new(temp.0.join("linked-status"), 9).is_err());
    symlink(&temp.0, temp.0.join("linked-dir")).unwrap();
    assert!(Development::new(temp.0.join("linked-dir/status"), 9).is_err());
    let server = Running::new(
        Development::new(&path, 9)
            .unwrap()
            .waiting_router()
            .unwrap(),
    );
    fs::rename(&path, temp.0.join("old")).unwrap();
    symlink(temp.0.join("old"), &path).unwrap();
    assert!(
        server
            .get("/_ruvane/dev/status", HEADERS)
            .starts_with("HTTP/1.1 500")
    );
}
