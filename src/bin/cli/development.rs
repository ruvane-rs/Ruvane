use super::{Result, project};
use ruvane::{Development, Server, ShutdownHandle};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::thread::JoinHandle;

pub(super) struct Session {
    directory: PathBuf,
    pub path: PathBuf,
    pub id: u64,
    sequence: u64,
    pending_file: bool,
}

impl Session {
    pub fn new(project: &project::Project) -> Result<Self> {
        let id = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
        )?;
        let directory = project
            .work
            .join(format!("dev-{}-{id}", std::process::id()));
        fs::create_dir(&directory)?;
        let mut value = Self {
            path: directory.join("status"),
            directory,
            id,
            sequence: 0,
            pending_file: false,
        };
        value.publish("building", 0, "")?;
        Ok(value)
    }

    pub fn publish(&mut self, state: &str, revision: u64, message: &str) -> Result<()> {
        if !matches!(state, "building" | "ready" | "error") || state == "ready" && revision == 0 {
            return Err("invalid development state".into());
        }
        project::checked_dir(&self.directory)?;
        if self.sequence > 0 {
            let old = project::read(&self.path, ruvane::MAX_DEVELOPMENT_STATUS_BYTES)?;
            if !old.starts_with(format!("RVD1\n{}\n{}\n", self.id, self.sequence).as_bytes()) {
                return Err("development status ownership changed".into());
            }
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("development sequence exhausted")?;
        let message = diagnostics(message);
        let text = format!(
            "RVD1\n{}\n{sequence}\n{state}\n{revision}\n\n{message}",
            self.id
        );
        let temporary = self.directory.join("status-next");
        project::write_new(&temporary, text.as_bytes())?;
        self.pending_file = true;
        fs::rename(&temporary, &self.path)?;
        self.pending_file = false;
        self.sequence = sequence;
        Ok(())
    }

    pub fn waiting(&self, port: u16) -> Result<Waiting> {
        let development = Development::new(&self.path, self.id)?;
        let server = Server::bind(([127, 0, 0, 1], port).into(), development.waiting_router()?)?;
        let address = server.local_addr()?;
        let shutdown = server.shutdown_handle();
        let thread = std::thread::spawn(move || server.run());
        Ok(Waiting {
            address,
            shutdown,
            thread: Some(thread),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.sequence > 0 {
            let _ = fs::remove_file(&self.path);
        }
        if self.pending_file {
            let _ = fs::remove_file(self.directory.join("status-next"));
        }
        let _ = fs::remove_dir(&self.directory);
    }
}

pub(super) struct Waiting {
    pub address: SocketAddr,
    shutdown: ShutdownHandle,
    thread: Option<JoinHandle<std::io::Result<()>>>,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.shutdown.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn diagnostics(message: &str) -> String {
    let text: String = message
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let mut start = text.len().saturating_sub(24 * 1024);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_bounded_utf8_without_terminal_controls() {
        let message = format!("{}\r\u{1b}\0end", "é".repeat(20000));
        let text = diagnostics(&message);
        assert!(text.len() <= 24 * 1024);
        assert!(text.ends_with("end"));
        assert!(!text.chars().any(char::is_control));
    }
    #[test]
    fn session_cleanup_preserves_unknown_files_and_refuses_replaced_status() {
        let temp = crate::cli::tests::Temp::new();
        let project = project::Project {
            root: temp.0.clone(),
            work: temp.0.clone(),
            interactive: true,
        };
        let mut session = Session::new(&project).unwrap();
        let directory = session.directory.clone();
        fs::write(directory.join("status-next"), b"keep").unwrap();
        assert!(session.publish("ready", 1, "").is_err());
        drop(session);
        assert_eq!(fs::read(directory.join("status-next")).unwrap(), b"keep");
        let mut session = Session::new(&project).unwrap();
        fs::write(&session.path, b"unrecognized").unwrap();
        assert!(session.publish("error", 0, "").is_err());
    }
}
