use super::{Result, development, project, watch};
use ruvane::SiteOrigin;
use std::fs;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);
static READING_COMPILER: AtomicBool = AtomicBool::new(false);
const EXECUTABLE: &str = if cfg!(windows) {
    "ruvane-app.exe"
} else {
    "ruvane-app"
};

pub(super) struct Binary {
    directory: PathBuf,
    executable: PathBuf,
    ready_files: Vec<PathBuf>,
    wasm: Option<PathBuf>,
    revision: u64,
    sources: Option<watch::Snapshot>,
}
impl Drop for Binary {
    fn drop(&mut self) {
        for path in &self.ready_files {
            let _ = fs::remove_file(path);
        }
        if let Some(wasm) = &self.wasm {
            let _ = fs::remove_file(wasm);
        }
        let _ = fs::remove_file(&self.executable);
        let _ = fs::remove_dir(&self.directory);
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn wait(child: &mut ChildGuard, quit: &AtomicBool, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        if quit.load(Ordering::SeqCst) {
            return Err("operation cancelled".into());
        }
        if let Some(status) = child.0.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err("application build/export failed".into())
            };
        }
        if start.elapsed() >= timeout {
            return Err("application build/export timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn compile(
    project: &project::Project,
    offline: bool,
    quit: &AtomicBool,
    development: Option<&development::Session>,
) -> Result<Binary> {
    compile_profile(project, offline, quit, false, development)
}
fn compile_profile(
    project: &project::Project,
    offline: bool,
    quit: &AtomicBool,
    release: bool,
    development: Option<&development::Session>,
) -> Result<Binary> {
    let content = project.root.join("content");
    if fs::symlink_metadata(&content).is_ok() {
        let _ = ruvane::ContentBundle::read_dir(&content)?;
    }
    let target = project.work.join("target");
    match fs::create_dir(&target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    project::checked_dir(&target)?;
    let runs = project.work.join("runs");
    match fs::create_dir(&runs) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    project::checked_dir(&runs)?;
    if fs::read_dir(&runs)?.take(65).count() >= 64 {
        return Err("too many retained run directories".into());
    }
    let directory = loop {
        let candidate = runs.join(format!(
            "run-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let mut binary = Binary {
        executable: directory.join(EXECUTABLE),
        directory,
        ready_files: Vec::new(),
        wasm: None,
        revision: 0,
        sources: None,
    };
    let mut before = if project.interactive {
        Some(watch::snapshot(&project.root)?)
    } else {
        None
    };
    if project.interactive {
        binary.revision = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
        )?
        .checked_add(NEXT.fetch_add(1, Ordering::Relaxed) as u64)
        .ok_or("application revision exhausted")?;
        let mut wasm = cargo(project, &target, offline);
        wasm.args([
            "--package",
            "ruvane-application-browser",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ])
        .env("RUVANE_APPLICATION_REVISION", binary.revision.to_string());
        run_compiler(wasm, quit)?;
        let bytes = project::read(
            &target.join("wasm32-unknown-unknown/release/ruvane_application_browser.wasm"),
            ruvane::MAX_ASSET_BYTES,
        )?;
        if !bytes.starts_with(b"\0asm\x01\0\0\0") {
            return Err("invalid application Wasm".into());
        }
        let path = binary.directory.join("app.wasm");
        project::write_new(&path, &bytes)?;
        binary.wasm = Some(path);
        let middle = watch::snapshot(&project.root)?;
        let mut previous = before.clone().unwrap();
        previous.remove(Path::new("Cargo.lock"));
        let mut comparable = middle.clone();
        comparable.remove(Path::new("Cargo.lock"));
        if previous != comparable {
            return Err(
                "sources changed during Wasm compilation; waiting for the next build".into(),
            );
        }
        before = Some(middle);
    }
    let mut command = cargo(project, &target, offline);
    command.args(["--bin", "ruvane-app"]);
    if release {
        command.arg("--release");
    }
    if let Some(path) = &binary.wasm {
        command.arg("--locked");
        command
            .env("RUVANE_WASM_FILE", path)
            .env("RUVANE_APPLICATION_REVISION", binary.revision.to_string());
    }
    if !release {
        if let Some(development) = development {
            command
                .env("RUVANE_DEV_STATUS", &development.path)
                .env("RUVANE_DEV_SESSION", development.id.to_string());
        }
    }
    command.env(
        "RUVANE_CONTENT_REVISION",
        format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ),
    );
    run_compiler(command, quit)?;
    if let Some(before) = &before {
        if *before != watch::snapshot(&project.root)? {
            return Err("sources changed during compilation; waiting for the next build".into());
        }
    }
    binary.sources = before;
    let compiled = target
        .join(if release { "release" } else { "debug" })
        .join(EXECUTABLE);
    let bytes = project::read(&compiled, 64 * 1024 * 1024)?;
    project::write_new(&binary.executable, &bytes)?;
    fs::set_permissions(&binary.executable, fs::metadata(compiled)?.permissions())?;
    Ok(binary)
}

fn cargo(project: &project::Project, target: &Path, offline: bool) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .current_dir(&project.root)
        .arg("build")
        .arg("--manifest-path")
        .arg(project.root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target)
        .arg("--color")
        .arg("never")
        .env_remove("RUVANE_DEV_STATUS")
        .env_remove("RUVANE_DEV_SESSION")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if offline {
        command.arg("--offline");
    }
    command
}

struct CompilerReader;
impl Drop for CompilerReader {
    fn drop(&mut self) {
        READING_COMPILER.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct CompilerFailure {
    reason: String,
    output: String,
}
impl std::fmt::Display for CompilerFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}
impl std::error::Error for CompilerFailure {}

pub(super) fn diagnostics(error: &(dyn std::error::Error + 'static)) -> String {
    error.downcast_ref::<CompilerFailure>().map_or_else(
        || error.to_string(),
        |failure| format!("{}\n{}", failure.reason, failure.output),
    )
}

fn run_compiler(mut command: Command, quit: &AtomicBool) -> Result<()> {
    if READING_COMPILER.swap(true, Ordering::SeqCst) {
        return Err(
            "previous compiler output is still open; stop its descendants before rebuilding".into(),
        );
    }
    let guard = CompilerReader;
    let mut child = ChildGuard(command.stderr(Stdio::piped()).spawn()?);
    let mut stream = child.0.stderr.take().ok_or("missing compiler output")?;
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let captured = Arc::clone(&output);
    let reader = std::thread::spawn(move || {
        let _guard = guard;
        let mut chunk = [0; 4096];
        loop {
            let size = match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => size,
            };
            let _ = std::io::stderr().lock().write_all(&chunk[..size]);
            let mut tail = captured.lock().unwrap_or_else(|error| error.into_inner());
            tail.extend_from_slice(&chunk[..size]);
            let excess = tail.len().saturating_sub(24 * 1024);
            tail.drain(..excess);
        }
    });
    let outcome = wait(&mut child, quit, Duration::from_secs(120));
    drop(child);
    let start = Instant::now();
    while !reader.is_finished() && start.elapsed() < Duration::from_millis(200) {
        std::thread::sleep(Duration::from_millis(5));
    }
    if reader.is_finished() {
        let _ = reader.join();
    }
    outcome.map_err(|error| {
        let bytes = output.lock().unwrap_or_else(|error| error.into_inner());
        Box::new(CompilerFailure {
            reason: error.to_string(),
            output: String::from_utf8_lossy(&bytes).into_owned(),
        }) as Box<dyn std::error::Error>
    })
}

pub(super) fn build_application(
    project: &project::Project,
    output: &str,
    offline: bool,
) -> Result<()> {
    let container = project.root.join(output);
    project::managed(&container, ".ruvane-app-builds", super::APP_BUILDS_MARKER)?;
    let version = project::next_build(&container)?;
    let destination = container.join(&version);
    let binary = compile_profile(project, offline, &AtomicBool::new(false), true, None)?;
    fs::create_dir(&destination)?;
    let resources = destination.join("resources");
    let identity = destination.join("Ruvane.runtime");
    let mut child = ChildGuard(
        Command::new(&binary.executable)
            .arg("--ruvane-client-assets")
            .arg(&resources)
            .arg(&identity)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    wait(&mut child, &AtomicBool::new(false), Duration::from_secs(30))?;
    let expected = format!(
        "ruvane-runtime\t1\t{}\t{}\n",
        ruvane::BROWSER_ABI_VERSION,
        binary.revision
    );
    if project::read(&identity, 128)? != expected.as_bytes() {
        return Err("application bundle ABI or revision mismatch".into());
    }
    ruvane::StaticSite::preview_export(&resources)?;
    let module_path = resources.join(format!("ruvane-runtime/{:016x}/app.wasm", binary.revision));
    if project::read(&module_path, ruvane::MAX_ASSET_BYTES)?
        != project::read(
            binary.wasm.as_ref().ok_or("missing application Wasm")?,
            ruvane::MAX_ASSET_BYTES,
        )?
    {
        return Err("application bundle module mismatch".into());
    }
    let bytes = project::read(&binary.executable, 64 * 1024 * 1024)?;
    project::write_new(&destination.join(EXECUTABLE), &bytes)?;
    fs::set_permissions(
        destination.join(EXECUTABLE),
        fs::metadata(&binary.executable)?.permissions(),
    )?;
    let manifest = format!(
        "ruvane-application\t1\nrevision\t{}\nabi\t{}\nfile\t{EXECUTABLE}\t{}\nfile\tRuvane.runtime\t{}\nresources\tresources\n",
        binary.revision,
        ruvane::BROWSER_ABI_VERSION,
        bytes.len(),
        expected.len()
    );
    if binary.sources.as_ref() != Some(&watch::snapshot(&project.root)?) {
        return Err("sources changed while preparing the application bundle".into());
    }
    project::write_new(&destination.join("Ruvane.bundle"), manifest.as_bytes())?;
    project::write_new(
        &destination.join("complete"),
        b"ruvane-application-complete\t1\n",
    )?;
    project::publish(&container, &version)?;
    println!(
        "Built {}\nRun: {} --port 3000",
        destination.display(),
        destination.join(EXECUTABLE).display()
    );
    Ok(())
}

pub(super) fn export(
    binary: &Binary,
    destination: &Path,
    origin: &SiteOrigin,
    quit: &AtomicBool,
) -> Result<()> {
    let mut child = ChildGuard(
        Command::new(&binary.executable)
            .arg("--ruvane-export")
            .arg(destination)
            .arg(origin.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    wait(&mut child, quit, Duration::from_secs(30))
}

pub(super) struct App {
    binary: Binary,
    child: Option<ChildGuard>,
    pub address: SocketAddr,
}

impl App {
    pub fn revision(&self) -> u64 {
        self.binary.revision
    }
    pub fn start(binary: Binary, port: u16, quit: &AtomicBool) -> Result<Self> {
        let mut app = Self {
            binary,
            child: None,
            address: ([127, 0, 0, 1], port).into(),
        };
        app.restart(port, quit)?;
        Ok(app)
    }

    pub fn restart(&mut self, port: u16, quit: &AtomicBool) -> Result<()> {
        let ready = self
            .binary
            .directory
            .join(format!("ready-{}", self.binary.ready_files.len()));
        self.binary.ready_files.push(ready.clone());
        self.child = Some(ChildGuard(
            Command::new(&self.binary.executable)
                .arg("--ruvane-serve")
                .arg(port.to_string())
                .arg(&ready)
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?,
        ));
        let start = Instant::now();
        loop {
            if quit.load(Ordering::SeqCst) {
                self.stop();
                return Err("operation cancelled".into());
            }
            if self.exited()? {
                self.stop();
                return Err("application exited before readiness".into());
            }
            if fs::symlink_metadata(&ready).is_ok() {
                let data = project::read(&ready, 128)?;
                let text = std::str::from_utf8(&data)?;
                if !text.ends_with('\n') {
                    if start.elapsed() >= Duration::from_secs(5) {
                        self.stop();
                        return Err("application readiness timed out".into());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
                let address: SocketAddr = text
                    .strip_prefix("ruvane-ready\t1\t")
                    .and_then(|s| s.strip_suffix('\n'))
                    .ok_or("invalid application readiness record")?
                    .parse()?;
                if address.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                    || address.port() == 0
                    || port != 0 && address.port() != port
                {
                    return Err("invalid application address".into());
                }
                self.address = address;
                return Ok(());
            }
            if start.elapsed() >= Duration::from_secs(5) {
                self.stop();
                return Err("application startup timed out".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn exited(&mut self) -> Result<bool> {
        Ok(self
            .child
            .as_mut()
            .map(|child| child.0.try_wait())
            .transpose()?
            .flatten()
            .is_some())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            drop(child.0.stdin.take()); // EOF asks the generated server to shut down.
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                if child.0.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(child); // bounded kill/wait fallback for this direct child.
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_cleanup_only_removes_its_known_files() {
        let temp = crate::cli::tests::Temp::new();
        let directory = temp.0.join("run");
        fs::create_dir(&directory).unwrap();
        let executable = directory.join(EXECUTABLE);
        let ready = directory.join("ready-0");
        fs::write(&executable, b"owned").unwrap();
        fs::write(&ready, b"owned").unwrap();
        fs::write(directory.join("foreign"), b"keep").unwrap();
        drop(Binary {
            directory: directory.clone(),
            executable: executable.clone(),
            ready_files: vec![ready.clone()],
            wasm: None,
            revision: 0,
            sources: None,
        });
        assert!(!executable.exists());
        assert!(!ready.exists());
        assert_eq!(fs::read(directory.join("foreign")).unwrap(), b"keep");
    }
}
