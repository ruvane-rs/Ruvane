use super::{Result, process, project, watch};
use ruvane::{Development, Server, ShutdownHandle};
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::thread::JoinHandle;

pub(super) struct Session {
    directory: PathBuf,
    pub path: PathBuf,
    pub id: u64,
    sequence: u64,
    pending_file: bool,
    css: VecDeque<CssFiles>,
    css_generation: u64,
}

struct CssFiles {
    paths: Vec<PathBuf>,
    revision: u64,
    generation: u64,
    urls: Vec<String>,
}
impl Drop for CssFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
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
            css: VecDeque::new(),
            css_generation: 0,
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
            if !["RVD1", "RVD2"].iter().any(|magic| {
                old.starts_with(format!("{magic}\n{}\n{}\n", self.id, self.sequence).as_bytes())
            }) {
                return Err("development status ownership changed".into());
            }
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("development sequence exhausted")?;
        let message = diagnostics(message);
        let text = if let Some(css) = self.css.back().filter(|css| css.revision == revision) {
            format!(
                "RVD2\n{}\n{sequence}\n{state}\n{revision}\n{}\n{}\n{}\n\n{message}",
                self.id,
                css.generation,
                css.urls.len(),
                css.urls.join("\n")
            )
        } else {
            format!(
                "RVD1\n{}\n{sequence}\n{state}\n{revision}\n\n{message}",
                self.id
            )
        };
        let temporary = self.directory.join("status-next");
        project::write_new(&temporary, text.as_bytes())?;
        self.pending_file = true;
        fs::rename(&temporary, &self.path)?;
        self.pending_file = false;
        self.sequence = sequence;
        Ok(())
    }

    pub fn capture_styles(
        &mut self,
        project: &project::Project,
        app: &process::App,
        sources: &watch::Snapshot,
    ) -> Result<()> {
        if app.styles.is_empty() {
            return Ok(());
        }
        project::checked_dir(&self.directory)?;
        let mut total = 0;
        let mut bodies = Vec::new();
        for (_, source) in &app.styles {
            let bytes = project::read(&project.root.join(source), ruvane::MAX_ASSET_BYTES)?;
            total += bytes.len();
            if total > ruvane::MAX_TOTAL_ASSET_BYTES
                || sources.get(std::path::Path::new(source)) != Some(&watch::signature(&bytes))
            {
                return Err("stylesheet sources changed or exceed the byte budget".into());
            }
            bodies.push(bytes);
        }
        if watch::snapshot(&project.root)? != *sources {
            return Err("sources changed while preparing stylesheets".into());
        }
        self.css_generation = self
            .css_generation
            .checked_add(1)
            .ok_or("stylesheet generation exhausted")?;
        if self.css.len() == 8 {
            self.css.pop_front();
        }
        let mut css = CssFiles {
            paths: Vec::new(),
            revision: app.revision(),
            generation: self.css_generation,
            urls: app.styles.iter().map(|(url, _)| url.clone()).collect(),
        };
        for (index, bytes) in bodies.iter().enumerate() {
            let path = self.directory.join(format!(
                "css-{}-{}-{index}",
                app.revision(),
                self.css_generation
            ));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            css.paths.push(path);
            file.write_all(bytes)?;
        }
        self.css.push_back(css);
        Ok(())
    }

    pub fn reload_styles(
        &mut self,
        project: &project::Project,
        app: &mut process::App,
        sources: &watch::Snapshot,
    ) -> Result<bool> {
        let Some(before) = app.sources() else {
            return Ok(false);
        };
        if app.styles.is_empty() || before.keys().ne(sources.keys()) {
            return Ok(false);
        }
        let changed: Vec<_> = sources
            .iter()
            .filter(|(path, value)| before.get(*path) != Some(*value))
            .map(|(path, _)| path)
            .collect();
        if changed.is_empty()
            || changed.iter().any(|path| {
                !app.styles
                    .iter()
                    .any(|(_, source)| std::path::Path::new(source) == path.as_path())
            })
        {
            return Ok(false);
        }
        self.capture_styles(project, app, sources)?;
        self.publish("ready", app.revision(), "")?;
        app.accept_style_sources(sources.clone());
        Ok(true)
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
        self.css.clear();
        if self.sequence > 0 {
            let _ = fs::remove_file(&self.path);
        }
        if self.pending_file {
            let _ = fs::remove_file(self.directory.join("status-next"));
        }
        let _ = fs::remove_dir(&self.directory);
    }
}

pub(super) fn read_styles(path: &std::path::Path, revision: u64) -> Result<Vec<(String, String)>> {
    let bytes = project::read(path, 16 * 1024)?;
    let text = std::str::from_utf8(&bytes)?;
    let mut lines = text.lines();
    if !text.ends_with('\n')
        || lines.next() != Some("RVDS1")
        || lines.next() != Some(revision.to_string().as_str())
    {
        return Err("invalid development stylesheet declaration".into());
    }
    let mut styles = Vec::<(String, String)>::new();
    for line in lines {
        let (url, source) = line
            .split_once('\t')
            .ok_or("invalid stylesheet declaration")?;
        if styles.len() == ruvane::MAX_PAGE_STYLESHEETS
            || url.len() > 256
            || source.len() > 255
            || !url.ends_with(".css")
            || url.starts_with("/_ruvane/")
            || ruvane::StaticPath::new(url).is_err()
            || !source.starts_with("assets/")
            || !source.ends_with(".css")
            || ruvane::StaticPath::new(&format!("/{source}")).is_err()
            || styles
                .iter()
                .any(|(old_url, old_source)| old_url == url || old_source == source)
        {
            return Err("invalid stylesheet declaration".into());
        }
        styles.push((url.into(), source.into()));
    }
    Ok(styles)
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
    #[test]
    fn stylesheet_records_reject_unknown_revisions_paths_duplicates_and_limits() {
        let temp = crate::cli::tests::Temp::new();
        let path = temp.0.join("styles");
        fs::write(&path, "RVDS1\n123\n/assets/site.css\tassets/site.css\n").unwrap();
        assert_eq!(
            read_styles(&path, 123).unwrap(),
            vec![("/assets/site.css".into(), "assets/site.css".into())]
        );
        assert!(read_styles(&path, 124).is_err());
        for text in [
            "RVDS1\n123\n/assets/site.css\t../secret.css\n",
            "RVDS1\n123\n/assets/site.css\tsrc/style.css\n",
            "RVDS1\n123\n/assets/site.css?x=1\tassets/site.css\n",
            "RVDS1\n123\n/assets/site.css\tassets/site.css\n/assets/other.css\tassets/site.css\n",
            "RVDS1\n123\n/assets/site.css\tassets/site.css",
        ] {
            fs::write(&path, text).unwrap();
            assert!(read_styles(&path, 123).is_err(), "{text}");
        }
        use std::fmt::Write as _;
        let mut declarations = String::from("RVDS1\n123\n");
        for index in 0..17 {
            writeln!(
                declarations,
                "/assets/site{index}.css\tassets/site{index}.css"
            )
            .unwrap();
        }
        fs::write(&path, declarations).unwrap();
        assert!(read_styles(&path, 123).is_err());
    }
}
