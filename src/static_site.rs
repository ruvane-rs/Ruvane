use crate::{CONTENT_SECURITY_POLICY, Html, InternalUrl, MediaType, Response, Router};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Maximum declared pages and assets in a static site (plus three metadata files).
pub const MAX_STATIC_ENTRIES: usize = 256;
/// Maximum bytes in one generated HTML page or asset (1 MiB).
pub const MAX_STATIC_FILE_BYTES: usize = 1024 * 1024;
/// Maximum exported bytes, including the manifest and required headers (16 MiB).
pub const MAX_STATIC_TOTAL_BYTES: usize = 16 * 1024 * 1024;
/// Maximum ASCII bytes in a static URL path.
pub const MAX_STATIC_PATH_BYTES: usize = 240;
/// Maximum path segments, excluding the root and final directory slash.
pub const MAX_STATIC_SEGMENTS: usize = 16;
/// Relative filename of the deterministic, provider-neutral content manifest.
pub const STATIC_MANIFEST_PATH: &str = "_ruvane/manifest.tsv";
/// Relative filename listing required HTTP headers for the hosting server.
pub const STATIC_HEADERS_PATH: &str = "_ruvane/headers.txt";
/// Empty marker created last, only after all export writes have succeeded.
pub const STATIC_COMPLETE_PATH: &str = "_ruvane/complete";

const MANIFEST_HEADER: &str = "ruvane-static\t1\nkind\turl\tfile\tcontent-type\tbytes\n";
// Enough space for 256 manifest rows at their maximum size.
const MAX_MANIFEST_BYTES: usize = 160 * 1024;

/// A bounded static-plan or export failure, with no reflected path or content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportError {
    /// A path does not fit the portable lowercase ASCII profile.
    InvalidPath,
    /// A page needs a directory URL; an asset needs a file URL.
    WrongPathKind,
    /// A URL or output filename is already declared.
    DuplicatePath,
    /// One output file would be an ancestor of another output file.
    PathConflict,
    /// An asset suffix does not match its declared fixed MIME type.
    UnsupportedAsset,
    /// The site exceeds `MAX_STATIC_ENTRIES` content files.
    TooManyEntries,
    /// One page or asset exceeds `MAX_STATIC_FILE_BYTES`.
    FileTooLarge,
    /// Content plus metadata exceeds `MAX_STATIC_TOTAL_BYTES`.
    TotalTooLarge,
    /// At least one content entry is required for export or preview.
    EmptySite,
    /// An existing export is incomplete, malformed, oversized, or inconsistent.
    InvalidExport,
    /// The destination already exists, including a file or dangling symlink.
    DestinationExists,
    /// The destination has no new leaf, uses parent traversal, or has a nondirectory parent.
    InvalidDestination,
    /// A destination parent component is a symbolic link.
    Symlink,
    /// An operating-system I/O failure; a partial export may remain without a completion marker.
    Io(io::ErrorKind),
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "invalid static path",
            Self::WrongPathKind => "wrong static path kind",
            Self::DuplicatePath => "duplicate static path",
            Self::PathConflict => "conflicting static output paths",
            Self::UnsupportedAsset => "unsupported static asset suffix",
            Self::TooManyEntries => "static entry count limit exceeded",
            Self::FileTooLarge => "static file byte limit exceeded",
            Self::TotalTooLarge => "static total byte limit exceeded",
            Self::EmptySite => "static site must not be empty",
            Self::InvalidExport => "invalid or incomplete static export",
            Self::DestinationExists => "static export destination already exists",
            Self::InvalidDestination => "invalid static export destination",
            Self::Symlink => "symbolic link in static export parent",
            Self::Io(_) => "static site I/O failure",
        })
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// An explicit portable URL and its deterministic relative output filename.
///
/// Directory URLs (`/`, `/articles/`) map to `index.html`. File URLs keep
/// their filename. Segments start with a lowercase ASCII letter or digit and
/// contain lowercase letters, digits, `-`, or `_`; the final file segment may
/// also contain single interior dots. Segments are at most 64 bytes.
/// Windows device names, uppercase, Unicode, percent escapes, query strings,
/// fragments, separators inside segments, dot segments, and empty segments
/// are rejected rather than normalized. Text content remains UTF-8.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticPath {
    url: InternalUrl,
    output: String,
    page: bool,
}

impl StaticPath {
    /// Validates a portable path without decoding or filesystem access.
    ///
    /// ```
    /// let path = ruvane::StaticPath::new("/articles/page/2/").unwrap();
    /// assert_eq!(path.output_path(), "articles/page/2/index.html");
    /// assert_eq!(path.url().as_str(), "/articles/page/2/");
    /// ```
    pub fn new(value: &str) -> Result<Self, ExportError> {
        if value.is_empty() || value.len() > MAX_STATIC_PATH_BYTES || !value.starts_with('/') {
            return Err(ExportError::InvalidPath);
        }
        let page = value.ends_with('/');
        let body = value.strip_prefix('/').unwrap_or("");
        let body = if page {
            body.strip_suffix('/').unwrap_or(body)
        } else {
            body
        };
        if value != "/" {
            let segments: Vec<_> = body.split('/').collect();
            if segments.len() > MAX_STATIC_SEGMENTS {
                return Err(ExportError::InvalidPath);
            }
            for (index, segment) in segments.iter().enumerate() {
                let file = !page && index + 1 == segments.len();
                if !valid_segment(segment, file) {
                    return Err(ExportError::InvalidPath);
                }
            }
        }
        let url = InternalUrl::new(value).map_err(|_| ExportError::InvalidPath)?;
        let output = if page {
            if body.is_empty() {
                String::from("index.html")
            } else {
                format!("{body}/index.html")
            }
        } else {
            body.to_owned()
        };
        Ok(Self { url, output, page })
    }

    /// Borrows the URL for links, metadata, and exact preview routing.
    pub fn url(&self) -> &InternalUrl {
        &self.url
    }

    /// Borrows the relative filename derived by this type, with `/` separators.
    pub fn output_path(&self) -> &str {
        &self.output
    }

    /// Reports whether this path is a directory URL for an HTML page.
    pub fn is_page(&self) -> bool {
        self.page
    }
}

fn valid_segment(value: &str, file: bool) -> bool {
    let Some(first) = value.as_bytes().first() else {
        return false;
    };
    if value.len() > 64
        || !first.is_ascii_lowercase() && !first.is_ascii_digit()
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_')
                || file && byte == b'.'
        })
    {
        return false;
    }
    let stem = value.split('.').next().unwrap_or("");
    !matches!(stem, "con" | "prn" | "aux" | "nul")
        && !(stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit())
}

pub(crate) fn valid_suffix(path: &str, kind: MediaType) -> bool {
    match kind {
        MediaType::PlainText => path.ends_with(".txt"),
        MediaType::Css => path.ends_with(".css"),
        MediaType::JavaScript => path.ends_with(".js") || path.ends_with(".mjs"),
        MediaType::WebAssembly => path.ends_with(".wasm"),
        MediaType::Action => false,
        MediaType::Png => path.ends_with(".png"),
        MediaType::Jpeg => path.ends_with(".jpg") || path.ends_with(".jpeg"),
        MediaType::WebP => path.ends_with(".webp"),
        MediaType::OctetStream => path.ends_with(".bin"),
    }
}

#[derive(Clone, Debug)]
struct Entry {
    path: StaticPath,
    bytes: Arc<[u8]>,
    content_type: &'static str,
}

fn manifest_row(path: &StaticPath, content_type: &str, bytes: usize) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\n",
        if path.page { "page" } else { "asset" },
        path.url.as_str(),
        path.output,
        content_type,
        bytes
    )
}

/// A bounded plan of explicitly rendered pages and assets for static export.
///
/// Registration is independent of the request router: applications enumerate
/// dynamic-page data explicitly. No request crawling, filesystem discovery,
/// arbitrary HTML decoding, or network access is performed.
#[derive(Clone, Debug)]
pub struct StaticSite {
    entries: Vec<Entry>,
    content_bytes: usize,
    manifest_bytes: usize,
}

impl Default for StaticSite {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticSite {
    /// Creates an empty static plan.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            content_bytes: 0,
            manifest_bytes: MANIFEST_HEADER.len(),
        }
    }

    /// Adds rendered HTML at a directory URL. Caller allocations occur before
    /// these output limits; strings cannot be registered as raw HTML.
    pub fn page(&mut self, path: StaticPath, html: Html) -> Result<(), ExportError> {
        if !path.page {
            return Err(ExportError::WrongPathKind);
        }
        self.register(path, html.as_str().as_bytes(), "text/html; charset=utf-8")
    }

    /// Adds trusted bytes at a file URL. Its suffix must match the fixed MIME
    /// declaration: txt, css, png, jpg/jpeg, webp, or bin. Bytes are copied once
    /// into shared storage, not decoded, sanitized, or opened as a file.
    pub fn asset(
        &mut self,
        path: StaticPath,
        bytes: impl AsRef<[u8]>,
        kind: MediaType,
    ) -> Result<(), ExportError> {
        if path.page {
            return Err(ExportError::WrongPathKind);
        }
        if !valid_suffix(path.output_path(), kind) {
            return Err(ExportError::UnsupportedAsset);
        }
        self.register(path, bytes.as_ref(), kind.as_str())
    }

    fn register(
        &mut self,
        path: StaticPath,
        bytes: &[u8],
        content_type: &'static str,
    ) -> Result<(), ExportError> {
        for existing in &self.entries {
            if existing.path.url == path.url || existing.path.output == path.output {
                return Err(ExportError::DuplicatePath);
            }
            if existing
                .path
                .output
                .strip_prefix(path.output.as_str())
                .is_some_and(|tail| tail.starts_with('/'))
                || path
                    .output
                    .strip_prefix(existing.path.output.as_str())
                    .is_some_and(|tail| tail.starts_with('/'))
            {
                return Err(ExportError::PathConflict);
            }
        }
        if self.entries.len() == MAX_STATIC_ENTRIES {
            return Err(ExportError::TooManyEntries);
        }
        if bytes.len() > MAX_STATIC_FILE_BYTES {
            return Err(ExportError::FileTooLarge);
        }
        let content_bytes = self
            .content_bytes
            .checked_add(bytes.len())
            .ok_or(ExportError::TotalTooLarge)?;
        let manifest_bytes =
            self.manifest_bytes + manifest_row(&path, content_type, bytes.len()).len();
        if content_bytes + manifest_bytes + self.required_headers().len() > MAX_STATIC_TOTAL_BYTES {
            return Err(ExportError::TotalTooLarge);
        }
        self.entries.push(Entry {
            path,
            bytes: Arc::from(bytes),
            content_type,
        });
        self.content_bytes = content_bytes;
        self.manifest_bytes = manifest_bytes;
        Ok(())
    }

    /// Returns the number of declared content files, excluding three metadata files.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Reports whether no content files have been declared.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the exact planned bytes, including the manifest and headers.
    /// The final completion marker is empty.
    pub fn planned_bytes(&self) -> usize {
        self.content_bytes + self.manifest_bytes + self.required_headers().len()
    }

    /// Produces a sorted TSV manifest with no timestamps, absolute filenames,
    /// or nondeterministic fields. It is not an integrity signature.
    pub fn manifest(&self) -> String {
        let mut result = String::with_capacity(self.manifest_bytes);
        result.push_str(MANIFEST_HEADER);
        for entry in self.sorted_entries() {
            result.push_str(&manifest_row(
                &entry.path,
                entry.content_type,
                entry.bytes.len(),
            ));
        }
        result
    }

    /// Lists required global HTTP headers in a provider-neutral TSV format.
    /// This is documentation for hosting configuration, not a universal host
    /// configuration file. Per-file MIME types are listed in the manifest.
    pub fn required_headers(&self) -> String {
        format!(
            "ruvane-static-headers\t1\nContent-Security-Policy\t{CONTENT_SECURITY_POLICY}\nX-Content-Type-Options\tnosniff\nCache-Control\tno-store\n"
        )
    }

    fn sorted_entries(&self) -> Vec<&Entry> {
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by(|a, b| a.path.url.as_str().cmp(b.path.url.as_str()));
        entries
    }

    /// Builds exact GET/HEAD preview routes from shared, immutable planned bytes.
    /// No disk file or manifest is read at request time. Valid queries do not
    /// change the response; encoded aliases and undeclared paths return 404.
    /// The normal loopback server, CSP, query validation, and output limits apply.
    pub fn preview_router(&self) -> Result<Router, ExportError> {
        if self.is_empty() {
            return Err(ExportError::EmptySite);
        }
        let mut router = Router::new();
        for entry in self.sorted_entries() {
            let body = Arc::clone(&entry.bytes);
            let content_type = entry.content_type;
            router
                .get(entry.path.url.as_str(), move |_| {
                    Response::shared(Arc::clone(&body), content_type)
                })
                .map_err(|_| ExportError::InvalidPath)?;
        }
        Ok(router)
    }

    /// Loads an existing, trusted completed export into exact preview routes.
    ///
    /// This reads the actual HTML/assets once, validates the bounded canonical
    /// manifest, paths, lengths, MIME types, required headers and final marker,
    /// and rejects symlinks and non-regular files. It never rewrites the export.
    /// Extra files are ignored and never served. Subsequent requests use shared
    /// in-memory bytes; restart the preview to load another validated build.
    ///
    /// Exported HTML/assets are trusted application code, not sanitized input.
    /// The directory and ancestors must remain protected from hostile concurrent
    /// changes. A completion marker does not authenticate file contents.
    pub fn preview_export(directory: impl AsRef<Path>) -> Result<Router, ExportError> {
        Self::load_export(directory.as_ref())?.preview_router()
    }

    fn load_export(directory: &Path) -> Result<Self, ExportError> {
        let root = checked_destination(directory)?;
        let metadata = fs::symlink_metadata(&root)?;
        if metadata.file_type().is_symlink() {
            return Err(ExportError::Symlink);
        }
        if !metadata.is_dir() {
            return Err(ExportError::InvalidExport);
        }
        read_export_file(&root, STATIC_COMPLETE_PATH, 0)?;
        let mut site = Self::new();
        let required_headers = site.required_headers();
        if read_export_file(&root, STATIC_HEADERS_PATH, required_headers.len())?
            != required_headers.as_bytes()
        {
            return Err(ExportError::InvalidExport);
        }
        let manifest_bytes = read_export_file(&root, STATIC_MANIFEST_PATH, MAX_MANIFEST_BYTES)?;
        let manifest =
            std::str::from_utf8(&manifest_bytes).map_err(|_| ExportError::InvalidExport)?;
        let rows = manifest
            .strip_prefix(MANIFEST_HEADER)
            .ok_or(ExportError::InvalidExport)?;
        let mut declared = Vec::new();
        let mut total = manifest_bytes.len() + required_headers.len();
        let mut canonical = String::from(MANIFEST_HEADER);
        let mut previous = "";
        for row in rows.lines() {
            if declared.len() == MAX_STATIC_ENTRIES {
                return Err(ExportError::InvalidExport);
            }
            let fields: Vec<_> = row.splitn(6, '\t').collect();
            if fields.len() != 5 {
                return Err(ExportError::InvalidExport);
            }
            let path = StaticPath::new(fields[1]).map_err(|_| ExportError::InvalidExport)?;
            if fields[1] <= previous
                || fields[2] != path.output_path()
                || fields[4].is_empty()
                || !fields[4].bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(ExportError::InvalidExport);
            }
            previous = fields[1];
            let content_type =
                if fields[0] == "page" && path.is_page() && fields[3] == "text/html; charset=utf-8"
                {
                    "text/html; charset=utf-8"
                } else if fields[0] == "asset" && !path.is_page() {
                    let kind = match fields[3] {
                        "text/plain; charset=utf-8" => MediaType::PlainText,
                        "text/css; charset=utf-8" => MediaType::Css,
                        "text/javascript; charset=utf-8" => MediaType::JavaScript,
                        "application/wasm" => MediaType::WebAssembly,
                        "image/png" => MediaType::Png,
                        "image/jpeg" => MediaType::Jpeg,
                        "image/webp" => MediaType::WebP,
                        "application/octet-stream" => MediaType::OctetStream,
                        _ => return Err(ExportError::InvalidExport),
                    };
                    if !valid_suffix(path.output_path(), kind) {
                        return Err(ExportError::InvalidExport);
                    }
                    kind.as_str()
                } else {
                    return Err(ExportError::InvalidExport);
                };
            let size = fields[4]
                .parse::<usize>()
                .map_err(|_| ExportError::InvalidExport)?;
            if size > MAX_STATIC_FILE_BYTES {
                return Err(ExportError::InvalidExport);
            }
            total = total
                .checked_add(size)
                .filter(|size| *size <= MAX_STATIC_TOTAL_BYTES)
                .ok_or(ExportError::InvalidExport)?;
            canonical.push_str(&manifest_row(&path, content_type, size));
            declared.push((path, content_type, size));
        }
        if declared.is_empty() || canonical != manifest {
            return Err(ExportError::InvalidExport);
        }
        // Validate the whole manifest before reading files.
        for (path, content_type, size) in declared {
            let body = read_export_file(&root, path.output_path(), size)?;
            if body.len() != size || path.is_page() && std::str::from_utf8(&body).is_err() {
                return Err(ExportError::InvalidExport);
            }
            site.register(path, &body, content_type)
                .map_err(|_| ExportError::InvalidExport)?;
        }
        Ok(site)
    }

    /// Writes this plan to a new directory under an existing trusted parent.
    ///
    /// All ancestor directories must be real directories, without symlinks.
    /// Existing destinations are never reused. Files use exclusive creation;
    /// no recursive deletion or overwrite is performed. On I/O failure partial
    /// files may remain, without a completion marker. The marker is created last.
    /// The caller must prevent concurrent hostile modification of the parent
    /// and output tree: portable safe std APIs are not a filesystem sandbox.
    /// Completion is not a power-loss durability or atomic deployment guarantee.
    pub fn export_to(&self, destination: impl AsRef<Path>) -> Result<ExportReport, ExportError> {
        self.export_with(destination.as_ref(), write_new)
    }

    fn export_with(
        &self,
        destination: &Path,
        mut write: impl FnMut(&Path, &[u8]) -> Result<(), ExportError>,
    ) -> Result<ExportReport, ExportError> {
        if self.is_empty() {
            return Err(ExportError::EmptySite);
        }
        let manifest = self.manifest();
        let headers = self.required_headers();
        let report = ExportReport {
            files: self.len() + 3,
            bytes: self.planned_bytes(),
        };
        let destination = checked_destination(destination)?;
        fs::create_dir(&destination).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                ExportError::DestinationExists
            } else {
                error.into()
            }
        })?;
        let mut directories = BTreeSet::from([String::from("_ruvane")]);
        for entry in &self.entries {
            let mut path = entry.path.output.as_str();
            while let Some((parent, _)) = path.rsplit_once('/') {
                directories.insert(parent.to_owned());
                path = parent;
            }
        }
        for directory in directories {
            fs::create_dir(destination.join(directory))?;
        }
        for entry in self.sorted_entries() {
            write(&destination.join(&entry.path.output), &entry.bytes)?;
        }
        write(&destination.join(STATIC_HEADERS_PATH), headers.as_bytes())?;
        write(&destination.join(STATIC_MANIFEST_PATH), manifest.as_bytes())?;
        // Create the completion marker only after every write succeeds.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination.join(STATIC_COMPLETE_PATH))?;
        Ok(report)
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn read_export_file(root: &Path, relative: &str, limit: usize) -> Result<Vec<u8>, ExportError> {
    let mut path = root.to_owned();
    let parts: Vec<_> = relative.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        let metadata = fs::symlink_metadata(&path).map_err(export_read_error)?;
        if metadata.file_type().is_symlink() {
            return Err(ExportError::Symlink);
        }
        if index + 1 == parts.len() {
            if !metadata.is_file() || metadata.len() > limit as u64 {
                return Err(ExportError::InvalidExport);
            }
        } else if !metadata.is_dir() {
            return Err(ExportError::InvalidExport);
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(export_read_error)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(ExportError::InvalidExport);
    }
    Ok(bytes)
}

fn export_read_error(error: io::Error) -> ExportError {
    if error.kind() == io::ErrorKind::NotFound {
        ExportError::InvalidExport
    } else {
        error.into()
    }
}

fn checked_destination(destination: &Path) -> Result<PathBuf, ExportError> {
    if destination.as_os_str().is_empty()
        || destination
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || destination.file_name().is_none()
    {
        return Err(ExportError::InvalidDestination);
    }
    let absolute = if destination.is_absolute() {
        destination.to_owned()
    } else {
        std::env::current_dir()?.join(destination)
    };
    let parent = absolute.parent().ok_or(ExportError::InvalidDestination)?;
    let mut cursor = PathBuf::new();
    for part in parent.components() {
        cursor.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&cursor)?;
        if metadata.file_type().is_symlink() {
            return Err(ExportError::Symlink);
        }
        if !metadata.is_dir() {
            return Err(ExportError::InvalidDestination);
        }
    }
    Ok(absolute)
}

/// Counts for a successfully completed export, including three metadata files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportReport {
    files: usize,
    bytes: usize,
}

impl ExportReport {
    /// Returns the number of exported files, including metadata and the marker.
    pub fn files(&self) -> usize {
        self.files
    }
    /// Returns the total exported bytes, including metadata.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temporary_parent() -> PathBuf {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        loop {
            let path = base.join(format!(
                "ruvane-export-failure-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test parent: {error}"),
            }
        }
    }

    #[test]
    fn io_failure_keeps_existing_data_and_does_not_publish_completion() {
        let parent = temporary_parent();
        let destination = parent.join("out");
        fs::write(parent.join("sentinel"), b"keep").unwrap();
        let mut site = StaticSite::new();
        site.page(
            StaticPath::new("/").unwrap(),
            Document::new("x", "page").render(),
        )
        .unwrap();
        let result = site.export_with(&destination, |path, bytes| {
            if path.ends_with("manifest.tsv") {
                // Simulate a write that fails midway.
                write_new(path, &bytes[..3])?;
                return Err(ExportError::Io(io::ErrorKind::WriteZero));
            }
            write_new(path, bytes)
        });
        assert_eq!(result, Err(ExportError::Io(io::ErrorKind::WriteZero)));
        assert_eq!(fs::read(parent.join("sentinel")).unwrap(), b"keep");
        assert_eq!(
            fs::read(destination.join(STATIC_MANIFEST_PATH)).unwrap(),
            b"ruv"
        );
        assert!(!destination.join(STATIC_COMPLETE_PATH).exists());
        assert_eq!(
            site.export_to(&destination),
            Err(ExportError::DestinationExists)
        );
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn exclusive_file_writer_never_truncates_existing_files() {
        let parent = temporary_parent();
        let file = parent.join("file");
        fs::write(&file, b"keep").unwrap();
        assert_eq!(
            write_new(&file, b"replacement"),
            Err(ExportError::Io(io::ErrorKind::AlreadyExists))
        );
        assert_eq!(fs::read(file).unwrap(), b"keep");
        fs::remove_dir_all(parent).unwrap();
    }
}
