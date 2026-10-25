use super::{Result, project};
use ruvane::SiteOrigin;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const EXECUTABLE: &str = if cfg!(windows) {
    "ruvane-app.exe"
} else {
    "ruvane-app"
};

pub(super) struct Binary {
    directory: PathBuf,
    executable: PathBuf,
    ready_files: Vec<PathBuf>,
}
impl Drop for Binary {
    fn drop(&mut self) {
        for path in &self.ready_files {
            let _ = fs::remove_file(path);
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
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .current_dir(&project.root)
        .arg("build")
        .arg("--bin")
        .arg("ruvane-app")
        .arg("--manifest-path")
        .arg(project.root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // A revision forces the generated build script to rerun for equal-mtime
    // edits detected by content polling. It is not an integrity token.
    command.env(
        "RUVANE_CONTENT_REVISION",
        format!(
            "{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ),
    );
    if offline {
        command.arg("--offline");
    }
    let mut child = ChildGuard(command.spawn()?);
    wait(&mut child, quit, Duration::from_secs(120))?;
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
    let binary = Binary {
        executable: directory.join(EXECUTABLE),
        directory,
        ready_files: Vec::new(),
    };
    let compiled = target.join("debug").join(EXECUTABLE);
    let bytes = project::read(&compiled, 64 * 1024 * 1024)?;
    project::write_new(&binary.executable, &bytes)?;
    fs::set_permissions(&binary.executable, fs::metadata(compiled)?.permissions())?;
    Ok(binary)
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
        });
        assert!(!executable.exists());
        assert!(!ready.exists());
        assert_eq!(fs::read(directory.join("foreign")).unwrap(), b"keep");
    }
}
