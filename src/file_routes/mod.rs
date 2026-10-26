//! Bounded route discovery for Cargo build scripts.

mod declarations;
use crate::{PageRoute, Router};
pub use declarations::{
    FileError, FileLayout, FileLoading, FileNotFound, FilePage, FileRouteBoundary, FileRouteLayout,
    FileRouteNotFound, FileRouteRegistry,
};
use std::fmt::{self, Write as _};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Maximum page, layout, loading, error and not-found source files in a route tree.
pub const MAX_FILE_ROUTE_FILES: usize = 128;
/// Maximum source bytes in one route file.
pub const MAX_FILE_ROUTE_SOURCE_BYTES: usize = 256 * 1024;
/// Maximum combined source bytes scanned in a route tree.
pub const MAX_FILE_ROUTE_TOTAL_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bytes in a generated route registry.
pub const MAX_FILE_ROUTE_MODULE_BYTES: usize = 1024 * 1024;
const MARKER: &str = "// ruvane-file-routes\t1\n";
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A route discovery or registration error with its relative source location.
#[derive(Debug)]
pub struct FileRouteError {
    path: String,
    message: String,
}
impl FileRouteError {
    fn at(path: impl AsRef<str>, message: impl fmt::Display) -> Self {
        Self {
            path: path
                .as_ref()
                .chars()
                .flat_map(char::escape_debug)
                .take(1024)
                .collect(),
            message: message.to_string(),
        }
    }
    /// Returns the escaped relative source path associated with this error.
    pub fn path(&self) -> &str {
        &self.path
    }
}
impl fmt::Display for FileRouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}
impl std::error::Error for FileRouteError {}

#[derive(Debug)]
struct Source {
    relative: String,
    path: PathBuf,
    pattern: String,
    parent: Option<usize>,
    kind: SourceKind,
    boundary: Option<usize>,
    not_found: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceKind {
    Page,
    Layout,
    Loading,
    Error,
    NotFound,
}
impl SourceKind {
    fn name(self) -> &'static str {
        match self {
            Self::Page => "page.rs",
            Self::Layout => "layout.rs",
            Self::Loading => "loading.rs",
            Self::Error => "error.rs",
            Self::NotFound => "not_found.rs",
        }
    }
}
#[derive(Debug)]
struct BoundarySource {
    parent: Option<usize>,
    loading: Option<usize>,
    error: Option<usize>,
}

/// A deterministic route tree discovered before compilation, never from HTTP input.
/// Accepts page/layout/loading/error/not_found Rust files and portable literal or `[parameter]` directories.
#[derive(Debug)]
pub struct FileRoutes {
    sources: Vec<Source>,
    pages: usize,
    layouts: usize,
    boundaries: Vec<BoundarySource>,
}
impl FileRoutes {
    /// Scans real directories and bounded UTF-8 source files without parsing Rust.
    pub fn read_dir(root: impl AsRef<Path>) -> Result<Self, FileRouteError> {
        let root = absolute(root.as_ref())?;
        real_directory(&root)?;
        let mut scan = Scan {
            result: Self {
                sources: Vec::new(),
                pages: 0,
                layouts: 0,
                boundaries: Vec::new(),
            },
            directories: 0,
            entries: 0,
            bytes: 0,
            router: Router::new(),
        };
        scan.visit(&root, &mut Vec::new(), None, None, None)?;
        if scan.result.pages == 0 {
            return Err(FileRouteError::at("<routes>", "no page.rs files found"));
        }
        Ok(scan.result)
    }
    /// Returns the number of discovered pages.
    pub fn page_count(&self) -> usize {
        self.pages
    }
    /// Returns the number of discovered layouts.
    pub fn layout_count(&self) -> usize {
        self.layouts
    }

    /// Returns the number of directories declaring loading or error presentations.
    pub fn boundary_count(&self) -> usize {
        self.boundaries.len()
    }

    /// Returns the number of local not_found.rs declarations.
    pub fn not_found_count(&self) -> usize {
        self.sources
            .iter()
            .filter(|source| source.kind == SourceKind::NotFound)
            .count()
    }

    /// Generates typed module imports and one atomic `register(&mut App)` function.
    pub fn rust_module(&self) -> Result<String, FileRouteError> {
        let mut result = String::from(MARKER);
        let mut bytes = 0;
        for (index, source) in self.sources.iter().enumerate() {
            bytes += read_source(&source.path, &source.relative)?.len();
            if bytes > MAX_FILE_ROUTE_TOTAL_BYTES {
                return Err(FileRouteError::at(
                    &source.relative,
                    "route source total byte limit exceeded",
                ));
            }
            let path = source
                .path
                .to_str()
                .ok_or_else(|| FileRouteError::at(&source.relative, "source path is not UTF-8"))?;
            writeln!(result, "#[path = {path:?}]\nmod __ruvane_file_{index};").unwrap();
        }
        result.push_str("pub fn register(app: &mut ruvane::App) -> Result<(), ruvane::FileRouteError> {\n    let mut registry = ruvane::FileRouteRegistry::new(app);\n");
        for (index, source) in self.sources.iter().enumerate() {
            if source.kind == SourceKind::NotFound {
                writeln!(result, "    let not_found_{index} = registry.not_found({:?}, __ruvane_file_{index}::not_found(), {})?;", source.relative, source.relative == "not_found.rs").unwrap();
            }
        }
        for (index, boundary) in self.boundaries.iter().enumerate() {
            let parent = boundary
                .parent
                .map_or_else(|| "None".into(), |index| format!("Some(&boundary_{index})"));
            let declaration = |source: Option<usize>, function: &str| {
                source.map_or_else(
                    || "None".to_owned(),
                    |index| {
                        format!(
                            "Some(({:?}, __ruvane_file_{index}::{function}()))",
                            self.sources[index].relative
                        )
                    },
                )
            };
            writeln!(
                result,
                "    let boundary_{index} = registry.boundary({parent}, {}, {})?;",
                declaration(boundary.loading, "loading"),
                declaration(boundary.error, "error")
            )
            .unwrap();
        }
        for (index, source) in self.sources.iter().enumerate() {
            let parent = source.parent.map_or_else(
                || "None".to_owned(),
                |index| format!("Some(&layout_{index})"),
            );
            let boundary = source.boundary.map_or_else(String::new, |index| {
                format!(".with_boundary(&boundary_{index})")
            });
            let not_found = source.not_found.map_or_else(String::new, |index| {
                format!(".with_not_found(&not_found_{index})")
            });
            match source.kind {
                SourceKind::Layout => {
                    writeln!(result, "    let layout_{index} = registry.layout({:?}, {:?}, {parent}, __ruvane_file_{index}::layout(){boundary}{not_found})?;", source.relative, source.pattern).unwrap();
                }
                SourceKind::Page => {
                    writeln!(result, "    registry.page({:?}, {:?}, {parent}, __ruvane_file_{index}::page(){boundary}{not_found})?;", source.relative, source.pattern).unwrap();
                }
                SourceKind::Loading | SourceKind::Error | SourceKind::NotFound => {}
            }
        }
        result.push_str("    registry.finish()\n}\n");
        if result.len() > MAX_FILE_ROUTE_MODULE_BYTES {
            return Err(FileRouteError::at(
                "<registry>",
                "generated module byte limit exceeded",
            ));
        }
        Ok(result)
    }

    /// Publishes a generated module atomically, replacing only a recognized file.
    /// Parent directories must exist and remain protected from concurrent writers.
    pub fn write_module(&self, destination: impl AsRef<Path>) -> Result<(), FileRouteError> {
        let text = self.rust_module()?;
        let destination = absolute(destination.as_ref())?;
        let parent = destination
            .parent()
            .ok_or_else(|| FileRouteError::at("<output>", "missing parent directory"))?;
        real_directory(parent)?;
        match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > MAX_FILE_ROUTE_MODULE_BYTES as u64
                {
                    return Err(FileRouteError::at(
                        "<output>",
                        "expected a regular generated module",
                    ));
                }
                let mut prefix = [0; MARKER.len()];
                let mut file = fs::File::open(&destination)
                    .map_err(|error| FileRouteError::at("<output>", error))?;
                file.read_exact(&mut prefix)
                    .map_err(|error| FileRouteError::at("<output>", error))?;
                if prefix != MARKER.as_bytes() {
                    return Err(FileRouteError::at(
                        "<output>",
                        "refusing to replace an unknown file",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(FileRouteError::at("<output>", error)),
        }
        let path = parent.join(format!(
            ".ruvane-routes-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| FileRouteError::at("<output>", error))?;
        let temporary = Temporary(path);
        file.write_all(text.as_bytes())
            .map_err(|error| FileRouteError::at("<output>", error))?;
        drop(file);
        fs::rename(&temporary.0, destination)
            .map_err(|error| FileRouteError::at("<output>", error))?;
        Ok(())
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn absolute(path: &Path) -> Result<PathBuf, FileRouteError> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(FileRouteError::at(
            "<routes>",
            "parent traversal is unsupported",
        ));
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| FileRouteError::at("<routes>", error))?
            .join(path)
    };
    let value = path
        .to_str()
        .ok_or_else(|| FileRouteError::at("<routes>", "path is not UTF-8"))?;
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(FileRouteError::at(
            "<routes>",
            "invalid root or output path",
        ));
    }
    Ok(path)
}
fn real_directory(path: &Path) -> Result<(), FileRouteError> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| FileRouteError::at("<routes>", error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(FileRouteError::at(
                "<routes>",
                "expected real directories without symbolic links",
            ));
        }
    }
    Ok(())
}
fn read_source(path: &Path, relative: &str) -> Result<Vec<u8>, FileRouteError> {
    real_directory(path.parent().unwrap())?;
    let metadata =
        fs::symlink_metadata(path).map_err(|error| FileRouteError::at(relative, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FileRouteError::at(
            relative,
            "expected a regular source file without symbolic links",
        ));
    }
    if metadata.len() > MAX_FILE_ROUTE_SOURCE_BYTES as u64 {
        return Err(FileRouteError::at(
            relative,
            "source file byte limit exceeded",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_FILE_ROUTE_SOURCE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| FileRouteError::at(relative, error))?;
    if bytes.len() > MAX_FILE_ROUTE_SOURCE_BYTES || std::str::from_utf8(&bytes).is_err() {
        return Err(FileRouteError::at(relative, "source must be bounded UTF-8"));
    }
    Ok(bytes)
}

struct Scan {
    result: FileRoutes,
    directories: usize,
    entries: usize,
    bytes: usize,
    router: Router,
}
impl Scan {
    fn visit(
        &mut self,
        directory: &Path,
        segments: &mut Vec<String>,
        inherited: Option<usize>,
        inherited_boundary: Option<usize>,
        inherited_not_found: Option<usize>,
    ) -> Result<(), FileRouteError> {
        self.directories += 1;
        let label = if segments.is_empty() {
            "<routes>".into()
        } else {
            segments.join("/")
        };
        if self.directories > 128 || segments.len() > 16 || label.len() > 512 {
            return Err(FileRouteError::at(
                &label,
                "route directory budget exceeded",
            ));
        }
        real_directory(directory)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(directory).map_err(|error| FileRouteError::at(&label, error))? {
            self.entries += 1;
            if self.entries > 512 {
                return Err(FileRouteError::at(&label, "route entry limit exceeded"));
            }
            entries.push(entry.map_err(|error| FileRouteError::at(&label, error))?);
        }
        entries.sort_by_key(|entry| entry.file_name());
        let mut children = Vec::new();
        let mut page = false;
        let mut layout = false;
        let mut loading = false;
        let mut error = false;
        let mut local_not_found = false;
        for entry in entries {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| FileRouteError::at(&label, "entry name is not UTF-8"))?;
            let kind = entry
                .file_type()
                .map_err(|error| FileRouteError::at(&label, error))?;
            if kind.is_symlink() {
                return Err(FileRouteError::at(
                    format!("{label}/{name}"),
                    "symbolic links are unsupported",
                ));
            }
            if kind.is_dir() {
                segment(&name)
                    .map_err(|error| FileRouteError::at(format!("{label}/{name}"), error))?;
                if segments.is_empty() && matches!(name.as_str(), "ruvane-runtime" | "_ruvane") {
                    return Err(FileRouteError::at(&name, "reserved framework route prefix"));
                }
                children.push(name);
            } else if kind.is_file()
                && matches!(
                    name.as_str(),
                    "page.rs" | "layout.rs" | "loading.rs" | "error.rs" | "not_found.rs"
                )
            {
                match name.as_str() {
                    "page.rs" => page = true,
                    "layout.rs" => layout = true,
                    "loading.rs" => loading = true,
                    "error.rs" => error = true,
                    "not_found.rs" => local_not_found = true,
                    _ => unreachable!(),
                }
            } else {
                return Err(FileRouteError::at(
                    format!("{label}/{name}"),
                    "expected page.rs, layout.rs, loading.rs, error.rs, not_found.rs or a route directory",
                ));
            }
        }
        let pattern = format!(
            "/{}",
            segments
                .iter()
                .map(|part| segment(part).unwrap())
                .collect::<Vec<_>>()
                .join("/")
        );
        PageRoute::new("file", &pattern).map_err(|error| FileRouteError::at(&label, error))?;
        let not_found = if local_not_found {
            Some(self.add(
                directory,
                segments,
                &pattern,
                inherited,
                SourceKind::NotFound,
                (None, None),
            )?)
        } else {
            inherited_not_found
        };
        let mut boundary = inherited_boundary;
        if loading || error {
            let loading = if loading {
                Some(self.add(
                    directory,
                    segments,
                    &pattern,
                    inherited,
                    SourceKind::Loading,
                    (None, None),
                )?)
            } else {
                None
            };
            let error = if error {
                Some(self.add(
                    directory,
                    segments,
                    &pattern,
                    inherited,
                    SourceKind::Error,
                    (None, None),
                )?)
            } else {
                None
            };
            boundary = Some(self.result.boundaries.len());
            self.result.boundaries.push(BoundarySource {
                parent: inherited_boundary,
                loading,
                error,
            });
        }
        let mut parent = inherited;
        if layout {
            parent = Some(self.add(
                directory,
                segments,
                &pattern,
                inherited,
                SourceKind::Layout,
                (boundary, not_found),
            )?);
        }
        let pages_before = self.result.pages;
        if page {
            self.router
                .get(&pattern, |_| crate::Response::text(""))
                .map_err(|error| FileRouteError::at(format!("{label}/page.rs"), error))?;
            self.add(
                directory,
                segments,
                &pattern,
                parent,
                SourceKind::Page,
                (boundary, not_found),
            )?;
        }
        for name in children {
            segments.push(name.clone());
            self.visit(&directory.join(name), segments, parent, boundary, not_found)?;
            segments.pop();
        }
        if boundary != inherited_boundary && self.result.pages == pages_before {
            let scope = &self.result.boundaries[boundary.unwrap()];
            let source = &self.result.sources[scope.loading.or(scope.error).unwrap()];
            return Err(FileRouteError::at(
                &source.relative,
                "presentation has no descendant pages",
            ));
        }
        if local_not_found && self.result.pages == pages_before {
            return Err(FileRouteError::at(
                &self.result.sources[not_found.unwrap()].relative,
                "not-found presentation has no descendant pages",
            ));
        }
        if layout && self.result.pages == pages_before {
            return Err(FileRouteError::at(
                format!("{label}/layout.rs"),
                "layout has no descendant pages",
            ));
        }
        Ok(())
    }
    fn add(
        &mut self,
        directory: &Path,
        segments: &[String],
        pattern: &str,
        parent: Option<usize>,
        kind: SourceKind,
        (boundary, not_found): (Option<usize>, Option<usize>),
    ) -> Result<usize, FileRouteError> {
        let name = kind.name();
        let relative = if segments.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", segments.join("/"))
        };
        if self.result.sources.len() == MAX_FILE_ROUTE_FILES
            || kind == SourceKind::Layout && self.result.layouts == crate::MAX_APP_LAYOUTS
        {
            return Err(FileRouteError::at(
                &relative,
                "route source count limit exceeded",
            ));
        }
        let path = directory.join(name);
        self.bytes += read_source(&path, &relative)?.len();
        if self.bytes > MAX_FILE_ROUTE_TOTAL_BYTES {
            return Err(FileRouteError::at(
                &relative,
                "route source total byte limit exceeded",
            ));
        }
        let index = self.result.sources.len();
        self.result.sources.push(Source {
            relative,
            path,
            pattern: pattern.into(),
            parent,
            kind,
            boundary,
            not_found,
        });
        match kind {
            SourceKind::Layout => self.result.layouts += 1,
            SourceKind::Page => self.result.pages += 1,
            SourceKind::Loading | SourceKind::Error | SourceKind::NotFound => {}
        }
        Ok(index)
    }
}
fn segment(name: &str) -> Result<String, &'static str> {
    if let Some(parameter) = name
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        let mut chars = parameter.bytes();
        if parameter.len() > 64
            || !chars
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte == b'_')
            || !chars.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err("invalid dynamic route directory");
        }
        Ok(format!("{{{parameter}}}"))
    } else if crate::StaticPath::new(&format!("/{name}/")).is_ok() {
        Ok(name.to_owned())
    } else {
        Err("invalid literal route directory")
    }
}
