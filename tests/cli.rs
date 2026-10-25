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

fn get(address: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let response = String::from_utf8(bytes).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("Content-Security-Policy: default-src 'none'; script-src 'none'"));
    response
}

#[test]
fn generator_refuses_path_and_shell_payloads_without_creating_destinations() {
    let temp = Temp::new();
    for name in [
        "../escape",
        "a/b",
        "A",
        "é",
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
            .contains("0.1.0-alpha.8")
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
    let failed_export = second.replace(
        "let mut site = StaticSite::new();",
        "return Err(\"forced export failure\".into());\n    let mut site = StaticSite::new();",
    );
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
