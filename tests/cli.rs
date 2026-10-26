use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const CLI: &str = env!("CARGO_BIN_EXE_ruvane");

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        loop {
            let path = base.join(format!(
                "ruvane cli tests {} {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("temporary directory: {error}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(CLI)
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn scaffold(root: &Path) -> PathBuf {
    let output = Command::new(CLI)
        .current_dir(root)
        .args(["new", "starter-site", "--framework-path"])
        .arg(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    root.join("starter-site")
}

struct Running {
    child: Child,
    events: mpsc::Receiver<String>,
    logs: Arc<Mutex<String>>,
}
impl Running {
    fn start(root: &Path, args: &[&str]) -> Self {
        let mut child = Command::new(CLI)
            .current_dir(root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, events) = mpsc::channel();
        let logs = Arc::new(Mutex::new(String::new()));
        let streams: Vec<Box<dyn Read + Send>> = vec![
            Box::new(child.stdout.take().unwrap()),
            Box::new(child.stderr.take().unwrap()),
        ];
        for stream in streams {
            let tx = tx.clone();
            let logs = Arc::clone(&logs);
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let mut text = logs.lock().unwrap();
                    text.push_str(&line);
                    text.push('\n');
                    if text.len() > 32000 {
                        let start = text.len() - 16000;
                        let start = (start..text.len())
                            .find(|i| text.is_char_boundary(*i))
                            .unwrap();
                        text.drain(..start);
                    }
                    drop(text);
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            child,
            events,
            logs,
        }
    }

    fn until(&mut self, fragment: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                Instant::now() < deadline,
                "timeout for {fragment}: {}",
                self.logs.lock().unwrap()
            );
            if let Ok(line) = self.events.recv_timeout(Duration::from_millis(200)) {
                if line.contains(fragment) {
                    return line;
                }
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "CLI exited: {}",
                self.logs.lock().unwrap()
            );
        }
    }

    fn address(&mut self, label: &str) -> SocketAddr {
        let line = self.until(label);
        line.split("http://")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    fn quit(&mut self) {
        self.child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"q\n")
            .unwrap();
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{}", self.logs.lock().unwrap());
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "dev did not stop"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn request(address: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let response = String::from_utf8(bytes).unwrap();
    assert!(response.contains("Content-Security-Policy: default-src 'none'; script-src 'none'"));
    response
}

fn get(address: SocketAddr, path: &str) -> String {
    let response = request(address, path);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response
}

#[test]
fn generator_refuses_path_and_shell_payloads_without_creating_destinations() {
    let temp = Temp::new();
    for name in [
        "../escape",
        "a/b",
        "A",
        "λ",
        "a;touch owned",
        "x$(touch owned)",
        "con",
        "com1",
        "fn",
        "gen",
        "-flag",
    ] {
        let output = cli(&temp.0, &["new", name]);
        assert!(!output.status.success(), "{name}");
    }
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    fs::create_dir(temp.0.join("existing")).unwrap();
    fs::write(temp.0.join("existing/sentinel"), b"keep").unwrap();
    assert!(!cli(&temp.0, &["new", "existing"]).status.success());
    assert_eq!(fs::read(temp.0.join("existing/sentinel")).unwrap(), b"keep");
}

#[test]
fn cli_options_require_an_explicit_project_and_valid_arguments() {
    let temp = Temp::new();
    for args in [
        vec!["dev", "--port", "-1"],
        vec!["dev", "--offline", "--offline"],
        vec!["build", "--origin", "javascript:evil"],
        vec![
            "build",
            "--origin",
            "https://site.example",
            "--out",
            "../escape",
        ],
        vec!["preview", "missing", "--port", "65536"],
        vec!["unknown"],
    ] {
        assert!(!cli(&temp.0, &args).status.success());
    }
    assert!(!cli(&temp.0, &["dev", "--offline"]).status.success());
    assert!(!temp.0.join(".ruvane").exists());
    assert!(cli(&temp.0, &["--help"]).status.success());
    assert!(
        String::from_utf8(cli(&temp.0, &["--version"]).stdout)
            .unwrap()
            .contains(env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
#[test]
fn generator_rejects_symlink_destinations_and_framework_aliases() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let target = temp.0.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), b"keep").unwrap();
    symlink(&target, temp.0.join("alias")).unwrap();
    assert!(!cli(&temp.0, &["new", "alias"]).status.success());
    symlink(env!("CARGO_MANIFEST_DIR"), temp.0.join("framework")).unwrap();
    assert!(
        !cli(&temp.0, &["new", "site", "--framework-path", "framework"])
            .status
            .success()
    );
    assert!(!temp.0.join("site").exists());
    assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn generated_project_build_dev_recompile_failure_recovery_and_preview() {
    let temp = Temp::new();
    let project = scaffold(&temp.0);
    let build = ["build", "--origin", "https://site.example", "--offline"];
    let first = cli(&project, &build);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000001\n"
    );
    let first_html = fs::read(project.join("dist/build-000001/index.html")).unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let lint = Command::new(&cargo)
        .current_dir(&project)
        .args(["clippy", "--offline", "--target-dir"])
        .arg(project.join(".ruvane/target"))
        .args(["--", "-D", "warnings"])
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "generated project Clippy: {}",
        String::from_utf8_lossy(&lint.stderr)
    );
    let format = Command::new(&cargo)
        .current_dir(&project)
        .args(["fmt", "--all", "--check"])
        .output()
        .unwrap();
    assert!(
        format.status.success(),
        "generated project formatting: {}{}",
        String::from_utf8_lossy(&format.stdout),
        String::from_utf8_lossy(&format.stderr)
    );
    let pages = project.join("src/pages.rs");
    let original = fs::read_to_string(&pages).unwrap();
    let second = original.replace(
        "Welcome. Make something worth sharing.",
        "Version two. A working site.",
    );
    fs::write(&pages, &second).unwrap();
    let output = cli(&project, &build);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000002\n"
    );
    assert_eq!(
        fs::read(project.join("dist/build-000001/index.html")).unwrap(),
        first_html
    );
    assert!(
        fs::read_to_string(project.join("dist/build-000002/index.html"))
            .unwrap()
            .contains("Version two")
    );
    fs::write(&pages, format!("{second}\nnot valid rust!!!\n")).unwrap();
    assert!(!cli(&project, &build).status.success());
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000002\n"
    );
    fs::write(&pages, &second).unwrap();
    let failed_export = second.replace("app.static_site(origin)", "Err(AppError::RenderFailed)");
    assert_ne!(failed_export, second, "the export failure must be injected");
    fs::write(&pages, failed_export).unwrap();
    assert!(!cli(&project, &build).status.success());
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000002\n"
    );
    assert!(!project.join("dist/build-000003/_ruvane/complete").exists());
    fs::write(&pages, &second).unwrap();
    let mut dev = Running::start(&project, &["dev", "--offline", "--port", "0"]);
    let address = dev.address("Ruvane dev at");
    assert!(get(address, "/").contains("Version two"));
    let missing = request(address, "/missing/");
    assert!(missing.starts_with("HTTP/1.1 404 "));
    assert!(missing.contains("That page is not available."));
    assert!(missing.contains("noindex, follow"));
    assert!(!cli(&project, &build).status.success());
    let dynamic = get(address, "/hello/%3Cscript%3E/");
    assert!(dynamic.contains("Hello, &lt;script&gt;!"));
    let third = second.replace("Version two. A working site.", "Version three. Recompiled.");
    fs::write(&pages, &third).unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    assert!(get(address, "/").contains("Version three"));
    fs::write(&pages, format!("{third}\nnot valid rust!!!\n")).unwrap();
    dev.until("keeping the previous server");
    assert!(get(address, "/").contains("Version three"));
    let recovered = third.replace("Version three. Recompiled.", "Recovered. Still serving.");
    fs::write(&pages, &recovered).unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    assert!(get(address, "/").contains("Recovered"));
    fs::rename(project.join("assets"), project.join("assets-away")).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(get(address, "/").contains("Recovered"));
    fs::rename(project.join("assets-away"), project.join("assets")).unwrap();
    let css = project.join("assets/site.css");
    fs::write(
        &css,
        fs::read_to_string(&css)
            .unwrap()
            .replace("#193e33", "#193e34"),
    )
    .unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    let main = project.join("src/main.rs");
    let original_main = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        original_main.replace("let port: u16 =", "std::process::exit(23); let port: u16 ="),
    )
    .unwrap();
    dev.until("Restored the previous server");
    assert!(get(address, "/").contains("Recovered"));
    fs::write(&main, original_main).unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    dev.quit();
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_err());
    assert!(!project.join(".ruvane/lock").exists());
    assert_eq!(
        fs::read_dir(project.join(".ruvane/runs")).unwrap().count(),
        0
    );
    let mut preview = Running::start(&project, &["preview", "dist", "--port", "0"]);
    let address = preview.address("Ruvane preview at");
    let html = get(address, "/");
    assert!(html.contains("Version two"));
    assert!(!html.contains("Recovered"));
    drop(preview);
    fs::create_dir(project.join("legacy")).unwrap();
    fs::write(project.join("legacy/sentinel"), b"keep").unwrap();
    assert!(
        !cli(
            &project,
            &[
                "build",
                "--origin",
                "https://site.example",
                "--out",
                "legacy",
                "--offline"
            ]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(project.join("legacy/sentinel")).unwrap(), b"keep");
}

#[test]
fn generated_content_is_embedded_recompiled_and_retained_on_invalid_edits_or_startup_failure() {
    let temp = Temp::new();
    let project = scaffold(&temp.0);
    let post = project.join("content/welcome.md");
    let initial = fs::read_to_string(&post).unwrap()
        + "\nFirst content AAA.\n\n\"); panic!(\"injected\"); //\n<script>not code</script>\n";
    fs::write(&post, &initial).unwrap();
    let build = ["build", "--origin", "https://site.example", "--offline"];
    let output = cli(&project, &build);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let first_html =
        fs::read(project.join("dist/build-000001/articles/welcome/index.html")).unwrap();
    let exported = String::from_utf8(first_html.clone()).unwrap();
    assert!(exported.contains("First content AAA."));
    assert!(exported.contains("panic!(&quot;injected&quot;)"));
    assert!(exported.contains("&lt;script&gt;not code&lt;/script&gt;"));
    let generated = fs::read_dir(project.join(".ruvane/target/debug/build"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("out/ruvane-content.rs"))
        .find(|path| path.is_file())
        .expect("generated content module");
    let original_module = fs::read(&generated).unwrap();
    fs::write(&generated, b"foreign output sentinel").unwrap();
    assert!(!cli(&project, &build).status.success());
    assert_eq!(fs::read(&generated).unwrap(), b"foreign output sentinel");
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000001\n"
    );
    fs::write(&generated, original_module).unwrap();
    let mut dev = Running::start(&project, &["dev", "--offline", "--port", "0"]);
    let address = dev.address("Ruvane dev at");
    assert!(get(address, "/articles/welcome/").contains("First content AAA."));
    let modified = fs::metadata(&post).unwrap().modified().unwrap();
    let next = initial.replace("First content AAA.", "Next! content BBB.");
    assert_eq!(initial.len(), next.len());
    fs::write(&post, &next).unwrap();
    fs::File::options()
        .write(true)
        .open(&post)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    assert!(get(address, "/articles/welcome/").contains("Next! content BBB."));
    fs::write(&post, next.replace("slug: welcome", "slug: ../escape")).unwrap();
    dev.until("keeping the previous server");
    assert!(get(address, "/articles/welcome/").contains("Next! content BBB."));
    let second = project.join("content/second.md");
    fs::write(&post, &next).unwrap();
    fs::write(&second, "---ruvane\ntitle: Second article\nslug: second\ndescription: Another page.\n---\nSecond article body.\n").unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    assert!(get(address, "/articles/second/").contains("Second article body."));
    assert!(get(address, "/").contains("Second article"));
    fs::remove_file(&second).unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    assert!(request(address, "/articles/second/").starts_with("HTTP/1.1 404 "));
    let main = project.join("src/main.rs");
    let original_main = fs::read_to_string(&main).unwrap();
    let final_content = next.replace("Next! content BBB.", "Final content CCC.");
    fs::write(&post, &final_content).unwrap();
    fs::write(
        &main,
        original_main.replace("let port: u16 =", "std::process::exit(23); let port: u16 ="),
    )
    .unwrap();
    dev.until("Restored the previous server");
    let restored = get(address, "/articles/welcome/");
    assert!(restored.contains("Next! content BBB."));
    assert!(!restored.contains("Final content CCC."));
    fs::write(&main, original_main).unwrap();
    assert_eq!(dev.address("Ruvane dev at"), address);
    assert!(get(address, "/articles/welcome/").contains("Final content CCC."));
    dev.quit();
    fs::write(
        &post,
        final_content.replace("slug: welcome", "slug: ../escape"),
    )
    .unwrap();
    let failed = cli(&project, &build);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("welcome.md:3:"));
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000001\n"
    );
    assert_eq!(
        fs::read(project.join("dist/build-000001/articles/welcome/index.html")).unwrap(),
        first_html
    );
    fs::write(&post, &final_content).unwrap();
    let output = cli(&project, &build);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("dist/current")).unwrap(),
        "build-000002\n"
    );
    fs::remove_file(&post).unwrap();
    let mut preview = Running::start(&project, &["preview", "dist", "--port", "0"]);
    let address = preview.address("Ruvane preview at");
    assert!(get(address, "/articles/welcome/").contains("Final content CCC."));
    drop(preview);
    assert_eq!(
        fs::read_dir(project.join(".ruvane/runs")).unwrap().count(),
        0
    );
}

fn interactive_wire(address: SocketAddr, target: &str, typed: bool) -> Vec<u8> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: {address}\r\n{}\r\n",
        if typed {
            "Accept: application/vnd.ruvane.page\r\n"
        } else {
            ""
        }
    )
    .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    bytes
}
fn development_status(address: SocketAddr, session: &str, extra: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(stream, "GET /_ruvane/dev/status HTTP/1.1\r\nHost: {address}\r\nAccept: text/plain\r\nRuvane-Development: {session}\r\n{extra}\r\n").unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    text
}
fn development_session(html: &str) -> &str {
    html.split("data-session=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}
fn response_body(bytes: &[u8]) -> &[u8] {
    let index = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    &bytes[index + 4..]
}
fn application_html(address: SocketAddr) -> String {
    let bytes = interactive_wire(address, "/", false);
    assert!(bytes.starts_with(b"HTTP/1.1 200"));
    String::from_utf8(response_body(&bytes).to_vec()).unwrap()
}
fn module_path(html: &str) -> &str {
    html.split("data-ruvane-module=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}
fn revision(html: &str) -> &str {
    html.split("data-ruvane-revision=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}
fn application_post(address: SocketAddr, body: &str, revision: Option<&str>) -> Vec<u8> {
    application_post_accept(address, body, revision, "application/vnd.ruvane.action")
}
fn application_post_accept(
    address: SocketAddr,
    body: &str,
    revision: Option<&str>,
    accept: &str,
) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(stream, "POST /actions/tasks/create HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nAccept: {accept}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n{}\r\n{body}", body.len(), revision.map(|value| format!("Ruvane-Application-Revision: {value}\r\n")).unwrap_or_default()).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn interactive_project_builds_both_targets_and_retains_its_matching_pair_on_errors() {
    let temp = Temp::new();
    let output = Command::new(CLI)
        .current_dir(&temp.0)
        .args(["new", "application", "--interactive", "--framework-path"])
        .arg(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = temp.0.join("application");
    let wasm_path = root.join("browser/src/lib.rs");
    let wasm = fs::read_to_string(&wasm_path).unwrap();
    fs::write(
        &wasm_path,
        format!("{wasm}\ncompile_error!(\"first build failure <script>\");\n"),
    )
    .unwrap();
    let mut running = Running::start(&root, &["dev", "--offline", "--port", "0"]);
    let waiting = running.address("Ruvane dev waiting at");
    running.until("keeping the previous server");
    let waiting_html = application_html(waiting);
    let session = development_session(&waiting_html).to_owned();
    let status = development_status(waiting, &session, "");
    assert!(
        status.starts_with("HTTP/1.1 200")
            && status.contains("\nerror\n0\n\n")
            && status.contains("first build failure <script>")
    );
    assert!(
        status.contains("Cache-Control: no-store")
            && status.contains("X-Content-Type-Options: nosniff")
    );
    assert!(development_status(waiting, "1", "").starts_with("HTTP/1.1 409"));
    for header in [
        "Sec-Fetch-Site: cross-site\r\n",
        "Origin: http://example.com\r\n",
    ] {
        assert!(development_status(waiting, &session, header).starts_with("HTTP/1.1 403"));
    }
    assert!(
        String::from_utf8(interactive_wire(waiting, "/tasks/12", false))
            .unwrap()
            .contains("Waiting for the application build")
    );
    fs::write(&wasm_path, &wasm).unwrap();
    let address = running.address("Ruvane dev at");
    assert_eq!(address, waiting);
    let html = application_html(address);
    assert_eq!(development_session(&html), session);
    let version = revision(&html).to_owned();
    let path = module_path(&html).to_owned();
    assert!(html.contains("<a href=\"/stream\">Streaming SSR</a>"));
    let streamed = interactive_wire(address, "/stream?q=%3Cscript%3E%26", false);
    let wire = String::from_utf8(streamed).unwrap();
    assert!(wire.starts_with("HTTP/1.1 200") && wire.contains("Transfer-Encoding: chunked\r\n"));
    assert!(!wire.contains("Content-Length:") && !wire.contains("<script>"));
    assert!(wire.contains("Deferred report") && wire.contains("&lt;script&gt;&amp;"));
    assert!(wire.ends_with("0\r\n\r\n") && wire.contains("script-src 'none'"));
    let early = interactive_wire(address, "/stream?fail=early", false);
    assert!(early.starts_with(b"HTTP/1.1 500"));
    assert!(
        !String::from_utf8(early)
            .unwrap()
            .contains("Transfer-Encoding:")
    );
    let late = interactive_wire(address, "/stream?fail=late", false);
    assert!(late.starts_with(b"HTTP/1.1 200") && !late.ends_with(b"0\r\n\r\n"));
    for wrong in [None, Some("1")] {
        assert!(application_post(address, "title=Rejected", wrong).starts_with(b"HTTP/1.1 409"));
    }
    assert!(
        application_post_accept(address, "title=Bypass", None, "text/html")
            .starts_with(b"HTTP/1.1 409")
    );
    let unchanged = interactive_wire(address, "/tasks", true);
    assert_eq!(
        ruvane::NavigationPage::decode(response_body(&unchanged))
            .unwrap()
            .data()
            .number("count"),
        Some(0)
    );
    assert!(
        application_post(address, "title=Accepted", Some(&version)).starts_with(b"HTTP/1.1 200")
    );

    let css_path = root.join("assets/site.css");
    let css = fs::read_to_string(&css_path).unwrap();
    let first_status = development_status(address, &session, "");
    let first_generation = first_status
        .split("\r\n\r\n")
        .nth(1)
        .unwrap()
        .lines()
        .nth(5)
        .unwrap();
    let first_url = format!("/assets/site.css?ruvane-css={session}-{version}-{first_generation}");
    assert_eq!(
        response_body(&interactive_wire(address, &first_url, false)),
        css.as_bytes()
    );
    for index in 1..=10 {
        let changed = format!("{css}\nbody {{ --development-check: {index}; }}\n");
        fs::write(&css_path, &changed).unwrap();
        running.until("Updated styles without compilation or restart");
        assert_eq!(revision(&application_html(address)), version);
        assert_eq!(
            response_body(&interactive_wire(address, "/assets/site.css", false)),
            changed.as_bytes()
        );
    }
    assert!(interactive_wire(address, &first_url, false).starts_with(b"HTTP/1.1 404"));
    let retained = interactive_wire(address, "/tasks", true);
    assert_eq!(
        ruvane::NavigationPage::decode(response_body(&retained))
            .unwrap()
            .data()
            .number("count"),
        Some(1)
    );
    fs::write(&css_path, &css).unwrap();
    running.until("Updated styles without compilation or restart");

    let module = interactive_wire(address, &path, false);
    assert!(response_body(&module).starts_with(b"\0asm\x01\0\0\0"));
    assert!(response_body(&module).len() <= ruvane::MAX_ASSET_BYTES);
    let typed = interactive_wire(address, "/tasks", true);
    let page = ruvane::NavigationPage::decode(response_body(&typed)).unwrap();
    assert_eq!(page.props().component(), 9);
    let missing = interactive_wire(address, "/unknown", true);
    assert_eq!(
        ruvane::NavigationPage::decode(response_body(&missing))
            .unwrap()
            .status(),
        ruvane::StatusCode::NotFound
    );
    fs::write(
        &wasm_path,
        format!("{wasm}\ncompile_error!(\"wasm failure\");\n"),
    )
    .unwrap();
    running.until("keeping the previous server");
    assert!(development_status(address, &session, "").contains("wasm failure"));
    fs::write(&css_path, format!("{css}\nbody {{ --mixed-edit: 1; }}")).unwrap();
    running.until("keeping the previous server");
    assert!(development_status(address, &session, "").contains("wasm failure"));
    assert_eq!(
        response_body(&interactive_wire(address, "/assets/site.css", false)),
        css.as_bytes()
    );
    assert_eq!(revision(&application_html(address)), version);
    assert_eq!(
        response_body(&interactive_wire(address, &path, false)),
        response_body(&module)
    );
    fs::write(&css_path, &css).unwrap();
    fs::write(&wasm_path, &wasm).unwrap();
    let restored = running.address("Ruvane dev at");
    assert_eq!(restored, address);
    let updated = application_html(address);
    assert!(
        development_status(address, &session, "")
            .contains(&format!("\nready\n{}\n", revision(&updated)))
    );
    assert_ne!(revision(&updated), version);
    let native_path = root.join("src/main.rs");
    let native = fs::read_to_string(&native_path).unwrap();
    fs::write(
        &native_path,
        format!("{native}\ncompile_error!(\"native failure\");\n"),
    )
    .unwrap();
    running.until("keeping the previous server");
    assert!(development_status(address, &session, "").contains("native failure"));
    assert_eq!(revision(&application_html(address)), revision(&updated));
    fs::write(&native_path, &native).unwrap();
    running.address("Ruvane dev at");
    // A compiled server that cannot start must also restore its original module.
    let healthy = application_html(address);
    fs::write(
        &native_path,
        native.replace(
            "let server = Server::bind",
            "return Err(\"startup fixture\".into());\nlet server = Server::bind",
        ),
    )
    .unwrap();
    running.until("Restored the previous server");
    assert_eq!(revision(&application_html(address)), revision(&healthy));
    assert!(development_status(address, &session, "").contains("Application startup failed"));
    running.quit();
    assert!(!fs::read_dir(root.join(".ruvane")).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("dev-")
    }));
    fs::write(&native_path, &native).unwrap();
    let built = Command::new(CLI)
        .current_dir(&root)
        .args(["build", "--offline"])
        .env("RUVANE_DEV_STATUS", "invalid-inherited-path")
        .env("RUVANE_DEV_SESSION", "123")
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("dist/current")).unwrap(),
        "build-000001\n"
    );
    let bundle = root.join("dist/build-000001");
    let manifest = fs::read_to_string(bundle.join("Ruvane.bundle")).unwrap();
    assert!(!manifest.contains("/_ruvane/dev/"));
    assert!(manifest.starts_with("ruvane-application\t1\nrevision\t"));
    assert_eq!(
        fs::read(bundle.join("complete")).unwrap(),
        b"ruvane-application-complete\t1\n"
    );
    for line in manifest
        .lines()
        .filter_map(|line| line.strip_prefix("file\t"))
    {
        let (path, size) = line.split_once('\t').unwrap();
        assert_eq!(
            fs::metadata(bundle.join(path)).unwrap().len(),
            size.parse::<u64>().unwrap()
        );
    }
    fs::write(
        &wasm_path,
        format!("{wasm}\ncompile_error!(\"bundle failure\");\n"),
    )
    .unwrap();
    assert!(!cli(&root, &["build", "--offline"]).status.success());
    assert_eq!(
        fs::read_to_string(root.join("dist/current")).unwrap(),
        "build-000001\n"
    );
    assert!(!bundle.join(".ruvane-builds").exists());
}
#[test]
fn interactive_options_fail_before_creating_an_incomplete_project() {
    let temp = Temp::new();
    assert!(
        !cli(&temp.0, &["new", "site", "--interactive"])
            .status
            .success()
    );
    assert!(!temp.0.join("site").exists());
    for name in ["ruvane", "ruvane-browser", "ruvane-application-browser"] {
        let output = Command::new(CLI)
            .current_dir(&temp.0)
            .args(["new", name, "--interactive", "--framework-path"])
            .arg(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!temp.0.join(name).exists());
    }
}

#[test]
fn file_routed_projects_rebuild_on_add_rename_remove_and_keep_the_previous_pair_on_errors() {
    let temp = Temp::new();
    let parent = temp.0.join("quoted \" project");
    fs::create_dir(&parent).unwrap();
    assert!(
        !cli(&parent, &["new", "invalid", "--file-routes"])
            .status
            .success()
    );
    assert!(!parent.join("invalid").exists());
    let created = Command::new(CLI)
        .current_dir(&parent)
        .args([
            "new",
            "file-application",
            "--interactive",
            "--file-routes",
            "--framework-path",
        ])
        .arg(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let root = parent.join("file-application");
    let mut running = Running::start(&root, &["dev", "--offline", "--port", "0"]);
    let address = running.address("Ruvane dev at");
    let html = application_html(address);
    let session = development_session(&html).to_owned();
    let version = revision(&html).to_owned();
    let settings = interactive_wire(address, "/tasks/settings", true);
    let settings = ruvane::NavigationPage::decode(response_body(&settings)).unwrap();
    assert_eq!(settings.status(), ruvane::StatusCode::Ok);
    assert_eq!(settings.props().component(), 14);
    assert_eq!(settings.layouts().len(), 1);
    assert!(
        application_post(address, "title=File+route+task", Some(&version))
            .starts_with(b"HTTP/1.1 200")
    );
    let detail = interactive_wire(address, "/tasks/1", true);
    let detail = ruvane::NavigationPage::decode(response_body(&detail)).unwrap();
    assert_eq!(detail.props().component(), 10);
    assert_eq!(detail.layouts().len(), 2);
    let native =
        String::from_utf8(interactive_wire(address, "/tasks/1?native=yes", false)).unwrap();
    assert!(
        native.contains("File route task")
            && native.contains("script-src 'none'")
            && !native.contains("<script")
    );
    let added = root.join("src/routes/added");
    fs::create_dir(&added).unwrap();
    let page = "pub fn page() -> ruvane::FilePage { ruvane::FilePage::client(\"added\", crate::components::HOME, crate::pages::zero, |_| Ok(ruvane::ClientPageData::new(\"Added\", crate::store::snapshot()?))) }\n";
    fs::write(added.join("page.rs"), page).unwrap();
    running.address("Ruvane dev at");
    let packet = interactive_wire(address, "/added", true);
    assert_eq!(
        ruvane::NavigationPage::decode(response_body(&packet))
            .unwrap()
            .status(),
        ruvane::StatusCode::Ok
    );
    let renamed = root.join("src/routes/renamed");
    fs::rename(&added, &renamed).unwrap();
    running.address("Ruvane dev at");
    for (url, status) in [
        ("/added", ruvane::StatusCode::NotFound),
        ("/renamed", ruvane::StatusCode::Ok),
    ] {
        assert_eq!(
            ruvane::NavigationPage::decode(response_body(&interactive_wire(address, url, true)))
                .unwrap()
                .status(),
            status
        );
    }
    fs::remove_file(renamed.join("page.rs")).unwrap();
    running.address("Ruvane dev at");
    assert_eq!(
        ruvane::NavigationPage::decode(response_body(&interactive_wire(address, "/renamed", true)))
            .unwrap()
            .status(),
        ruvane::StatusCode::NotFound
    );
    let healthy = application_html(address);
    let retained = revision(&healthy).to_owned();
    let wasm_path = module_path(&healthy).to_owned();
    let wasm = interactive_wire(address, &wasm_path, false);
    let ambiguous = root.join("src/routes/tasks/[slug]");
    fs::create_dir(&ambiguous).unwrap();
    fs::write(ambiguous.join("page.rs"), "pub fn page() {}\n").unwrap();
    running.until("keeping the previous server");
    assert!(development_status(address, &session, "").contains("ambiguous"));
    assert_eq!(revision(&application_html(address)), retained);
    assert_eq!(
        response_body(&interactive_wire(address, &wasm_path, false)),
        response_body(&wasm)
    );
    fs::remove_file(ambiguous.join("page.rs")).unwrap();
    let home_path = root.join("src/routes/page.rs");
    let home = fs::read_to_string(&home_path).unwrap();
    fs::write(&home_path, "pub fn page() {}\n").unwrap();
    running.until("keeping the previous server");
    assert!(development_status(address, &session, "").contains("mismatched types"));
    assert_eq!(revision(&application_html(address)), retained);
    fs::write(&home_path, &home).unwrap();
    running.address("Ruvane dev at");
    let retained = revision(&application_html(address)).to_owned();
    let duplicate = root.join("src/routes/duplicate");
    fs::create_dir(&duplicate).unwrap();
    fs::write(duplicate.join("page.rs"), &home).unwrap();
    running.until("Restored the previous server");
    assert_eq!(revision(&application_html(address)), retained);
    assert!(
        running
            .logs
            .lock()
            .unwrap()
            .contains("duplicate application name")
    );
    fs::remove_file(duplicate.join("page.rs")).unwrap();
    running.address("Ruvane dev at");
    let settings_path = root.join("src/routes/tasks/settings/page.rs");
    let settings_source = fs::read_to_string(&settings_path).unwrap();
    fs::remove_file(&settings_path).unwrap();
    running.address("Ruvane dev at");
    let removed = interactive_wire(address, "/tasks/settings", true);
    assert_eq!(
        ruvane::NavigationPage::decode(response_body(&removed))
            .unwrap()
            .status(),
        ruvane::StatusCode::BadRequest
    );
    fs::write(&settings_path, settings_source).unwrap();
    running.address("Ruvane dev at");
    let native_dir = root.join("src/routes/native");
    fs::create_dir(&native_dir).unwrap();
    fs::write(native_dir.join("page.rs"), "pub fn page() -> ruvane::FilePage { ruvane::FilePage::server(\"native\", ruvane::PageMode::fixed(), |_| Ok(ruvane::Page::new(\"Native file page\", \"Rendered from a file\"))) }\n").unwrap();
    running.address("Ruvane dev at");
    let native = String::from_utf8(interactive_wire(address, "/native/", false)).unwrap();
    assert!(
        native.starts_with("HTTP/1.1 200")
            && native.contains("Rendered from a file")
            && native.contains("script-src 'none'")
            && !native.contains("<script")
    );
    fs::remove_file(native_dir.join("page.rs")).unwrap();
    running.address("Ruvane dev at");
    running.quit();
    let built = cli(&root, &["build", "--offline"]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(root.join("dist/build-000001/complete").is_file());
}
