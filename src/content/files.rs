use super::*;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

fn io(error: std::io::Error) -> ContentError {
    ContentError::new(ContentErrorKind::Io(error.kind()), None)
}
fn invalid() -> ContentError {
    ContentError::new(ContentErrorKind::InvalidFile, None)
}

/// A validated, bounded snapshot of flat local .md files for build-time embedding.
/// Directory parents must be trusted and protected from concurrent hostile
/// mutation. Observed symlinks/special files and all subdirectories are refused.
/// No filesystem sandbox or integrity signature is provided.
#[derive(Clone, Debug)]
pub struct ContentBundle {
    sources: Vec<(String, String)>,
}
impl ContentBundle {
    /// Reads a flat directory into one immutable snapshot, validating the same
    /// headers, slugs and Markdown used by `ContentCollection::new`. Entries are
    /// sorted by filename. Extra non-.md files are errors, never silently served.
    pub fn read_dir(directory: impl AsRef<Path>) -> Result<Self, ContentError> {
        let directory = directory.as_ref();
        if directory
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(invalid());
        }
        let absolute = if directory.is_absolute() {
            directory.to_owned()
        } else {
            std::env::current_dir().map_err(io)?.join(directory)
        };
        let mut current = std::path::PathBuf::new();
        for component in absolute.components() {
            current.push(component.as_os_str());
            let metadata = fs::symlink_metadata(&current).map_err(io)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(invalid());
            }
        }
        let mut paths = Vec::new();
        for entry in fs::read_dir(&absolute).map_err(io)? {
            if paths.len() == MAX_CONTENT_FILES {
                return Err(ContentError::new(ContentErrorKind::Limit, None));
            }
            let entry = entry.map_err(io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ContentError::new(ContentErrorKind::InvalidName, None))?;
            if !valid_source_name(&name) {
                return Err(ContentError::new(ContentErrorKind::InvalidName, None));
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(io)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(invalid().at(&name));
            }
            if metadata.len() > MAX_CONTENT_FILE_BYTES as u64 {
                return Err(error(ContentErrorKind::Limit, 1).at(&name));
            }
            paths.push((name, entry.path()));
        }
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        let mut sources = Vec::new();
        let mut total = 0;
        for (name, path) in paths {
            let mut bytes = Vec::new();
            fs::File::open(path)
                .map_err(io)?
                .take(MAX_CONTENT_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            total += bytes.len();
            if bytes.len() > MAX_CONTENT_FILE_BYTES || total > MAX_CONTENT_TOTAL_BYTES {
                return Err(error(ContentErrorKind::Limit, 1).at(&name));
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| error(ContentErrorKind::InvalidText, 1).at(&name))?;
            sources.push((name, text));
        }
        let bundle = Self { sources };
        let _ = ContentCollection::new(&bundle.sources().collect::<Vec<_>>())?;
        Ok(bundle)
    }
    /// Iterates over this snapshot's owned source text in filename order.
    pub fn sources(&self) -> impl Iterator<Item = ContentSource<'_>> {
        self.sources
            .iter()
            .map(|(name, text)| ContentSource::new(name, text))
    }
    /// Produces one bounded Rust module containing escaped string literals.
    /// Source text never supplies Rust tokens outside those literals. This is
    /// intended for a trusted build script's OUT_DIR, included by the app.
    /// The module declares `pub const SOURCES: &[ruvane::ContentSource<'static>]`.
    pub fn rust_module(&self) -> Result<String, ContentError> {
        let mut result = String::from(
            "// ruvane-content\t1\npub const SOURCES: &[ruvane::ContentSource<'static>] = &[\n",
        );
        for (name, source) in &self.sources {
            let line = format!("    ruvane::ContentSource::new({name:?}, {source:?}),\n");
            if result.len() + line.len() + 3 > MAX_CONTENT_MODULE_BYTES {
                return Err(ContentError::new(ContentErrorKind::Limit, None));
            }
            result.push_str(&line);
        }
        result.push_str("];\n");
        Ok(result)
    }
}
