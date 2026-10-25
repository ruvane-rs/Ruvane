//! Local content and a small Markdown renderer.
mod files;
mod markdown;

use crate::{Html, PageMode, PageParams, StaticPath};
use std::fmt;

pub use files::ContentBundle;
pub use markdown::markdown;

/// Maximum documents in one content collection.
pub const MAX_CONTENT_FILES: usize = 128;
/// Maximum UTF-8 bytes in one complete document or standalone Markdown input.
pub const MAX_CONTENT_FILE_BYTES: usize = 64 * 1024;
/// Maximum aggregate source bytes in a collection (4 MiB).
pub const MAX_CONTENT_TOTAL_BYTES: usize = 4 * 1024 * 1024;
/// Maximum HTML bytes from one document.
pub const MAX_CONTENT_HTML_BYTES: usize = 1024 * 1024;
/// Maximum aggregate rendered body bytes in a collection (8 MiB).
pub const MAX_CONTENT_TOTAL_HTML_BYTES: usize = 8 * 1024 * 1024;
/// Maximum bytes in a content line.
pub const MAX_CONTENT_LINE_BYTES: usize = 8192;
/// Maximum lines per document.
pub const MAX_CONTENT_LINES: usize = 2048;
/// Maximum nested inline markup levels.
pub const MAX_MARKDOWN_DEPTH: usize = 8;
/// Maximum emitted block/inline tokens per document.
pub const MAX_MARKDOWN_TOKENS: usize = 8192;
/// Maximum parser byte-scan steps per document.
pub const MAX_MARKDOWN_WORK: usize = 1024 * 1024;
/// Maximum bytes in an escaped generated Rust content module (16 MiB).
pub const MAX_CONTENT_MODULE_BYTES: usize = 16 * 1024 * 1024;

/// Content errors. Messages contain no source text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContentErrorKind {
    /// A source name is not a portable basename ending in .md.
    InvalidName,
    /// The required strict Ruvane header is absent or malformed.
    InvalidHeader,
    /// A required metadata field is absent or unknown.
    InvalidField,
    /// A field or source name occurs twice.
    DuplicateField,
    /// A slug is not a portable single directory segment.
    InvalidSlug,
    /// Two documents declare the same slug.
    DuplicateSlug,
    /// The collection has no documents.
    EmptyCollection,
    /// Input is not UTF-8 or contains forbidden control characters.
    InvalidText,
    /// A source count, input byte, line or HTML byte limit is exceeded.
    Limit,
    /// Markdown nesting, token or byte-scan work exceeds its budget.
    Complexity,
    /// A link is not a query-free validated internal path.
    InvalidLink,
    /// The limited profile does not support this construct.
    UnsupportedSyntax,
    /// A fenced code block is not closed.
    UnclosedFence,
    /// A filesystem entry is a symlink, directory or special file where forbidden.
    InvalidFile,
    /// A local filesystem I/O operation failed.
    Io(std::io::ErrorKind),
}
impl fmt::Display for ContentErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidName => "invalid content filename",
            Self::InvalidHeader => "expected a strict Ruvane content header",
            Self::InvalidField => "invalid or missing content field",
            Self::DuplicateField => "duplicate content field or filename",
            Self::InvalidSlug => "invalid portable content slug",
            Self::DuplicateSlug => "duplicate content slug",
            Self::EmptyCollection => "content collection must not be empty",
            Self::InvalidText => "invalid content text",
            Self::Limit => "content size/count limit exceeded",
            Self::Complexity => "Markdown analysis budget exceeded",
            Self::InvalidLink => "invalid internal Markdown link",
            Self::UnsupportedSyntax => "unsupported Markdown construct",
            Self::UnclosedFence => "unclosed Markdown code fence",
            Self::InvalidFile => "expected real content files without symbolic links",
            Self::Io(_) => "content filesystem operation failed",
        })
    }
}

/// A content error with a basename and one-based line, when available.
/// Messages contain no source text or full filesystem paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentError {
    kind: ContentErrorKind,
    source: Option<String>,
    line: Option<usize>,
}
impl ContentError {
    pub(crate) fn new(kind: ContentErrorKind, line: Option<usize>) -> Self {
        Self {
            kind,
            source: None,
            line,
        }
    }
    pub(crate) fn at(mut self, name: &str) -> Self {
        self.source = Some(name.into());
        self
    }
    /// Returns the fixed failure category.
    pub fn kind(&self) -> ContentErrorKind {
        self.kind
    }
    /// Returns a validated source basename, when available.
    pub fn source_name(&self) -> Option<&str> {
        self.source.as_deref()
    }
    /// Returns the one-based source line, when available.
    pub fn line(&self) -> Option<usize> {
        self.line
    }
}
impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = &self.source {
            write!(f, "{name}")?;
            if let Some(line) = self.line {
                write!(f, ":{line}")?;
            }
            f.write_str(": ")?;
        }
        self.kind.fmt(f)
    }
}
impl std::error::Error for ContentError {}

pub(crate) fn error(kind: ContentErrorKind, line: usize) -> ContentError {
    ContentError::new(kind, Some(line))
}

/// A validated portable content slug, at most 64 lowercase ASCII bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentSlug(String);
impl ContentSlug {
    /// Validates one portable static path segment, including device-name checks.
    pub fn new(value: &str) -> Result<Self, ContentError> {
        if value.is_empty()
            || value.len() > 64
            || value.contains('/')
            || StaticPath::new(&format!("/{value}/")).is_err()
        {
            return Err(ContentError::new(ContentErrorKind::InvalidSlug, None));
        }
        Ok(Self(value.into()))
    }
    /// Borrows the validated slug.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One named text source, typically embedded in the executable.
#[derive(Clone, Copy, Debug)]
pub struct ContentSource<'a> {
    name: &'a str,
    text: &'a str,
}
impl<'a> ContentSource<'a> {
    /// Declares an input; validation occurs when constructing a collection.
    pub const fn new(name: &'a str, text: &'a str) -> Self {
        Self { name, text }
    }
    /// Borrows the input basename.
    pub fn name(&self) -> &'a str {
        self.name
    }
    /// Borrows the complete source text.
    pub fn text(&self) -> &'a str {
        self.text
    }
}

/// Validated metadata and rendered typed HTML for one content document.
#[derive(Clone, Debug)]
pub struct ContentEntry {
    source: String,
    slug: ContentSlug,
    title: String,
    description: String,
    body: Html,
}
impl ContentEntry {
    /// Borrows the validated source basename.
    pub fn source_name(&self) -> &str {
        &self.source
    }
    /// Borrows the portable URL slug.
    pub fn slug(&self) -> &ContentSlug {
        &self.slug
    }
    /// Borrows the control-free page title.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Borrows the control-free metadata description.
    pub fn description(&self) -> &str {
        &self.description
    }
    /// Borrows HTML generated exclusively through typed Ruvane elements.
    pub fn body(&self) -> &Html {
        &self.body
    }
}

pub(crate) fn valid_source_name(name: &str) -> bool {
    name.strip_suffix(".md")
        .is_some_and(|stem| ContentSlug::new(stem).is_ok())
}

pub(crate) fn lines(text: &str) -> Result<Vec<&str>, ContentError> {
    if text.len() > MAX_CONTENT_FILE_BYTES {
        return Err(error(ContentErrorKind::Limit, 1));
    }
    let mut result = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if result.len() == MAX_CONTENT_LINES || line.len() > MAX_CONTENT_LINE_BYTES {
            return Err(error(ContentErrorKind::Limit, i + 1));
        }
        if line.chars().any(|c| c.is_control() && c != '\t') {
            return Err(error(ContentErrorKind::InvalidText, i + 1));
        }
        result.push(line);
    }
    Ok(result)
}

fn parse(source: ContentSource<'_>) -> Result<ContentEntry, ContentError> {
    let input = lines(source.text)?;
    if input.first() != Some(&"---ruvane") {
        return Err(error(ContentErrorKind::InvalidHeader, 1));
    }
    let mut title = None;
    let mut slug = None;
    let mut description = None;
    let mut end = None;
    for (index, line) in input.iter().enumerate().skip(1) {
        if *line == "---" {
            end = Some(index + 1);
            break;
        }
        if index > 3 {
            return Err(error(ContentErrorKind::InvalidField, index + 1));
        }
        let (key, value) = line
            .split_once(": ")
            .ok_or_else(|| error(ContentErrorKind::InvalidField, index + 1))?;
        if value.is_empty()
            || value.trim() != value
            || value.len() > 1024
            || value.chars().any(char::is_control)
        {
            return Err(error(ContentErrorKind::InvalidField, index + 1));
        }
        let slot = match key {
            "title" => &mut title,
            "slug" => &mut slug,
            "description" => &mut description,
            _ => return Err(error(ContentErrorKind::InvalidField, index + 1)),
        };
        if slot.is_some() {
            return Err(error(ContentErrorKind::DuplicateField, index + 1));
        }
        *slot = Some(value.to_owned());
    }
    let end = end.ok_or_else(|| error(ContentErrorKind::InvalidHeader, input.len()))?;
    let title = title.ok_or_else(|| error(ContentErrorKind::InvalidField, end))?;
    let description = description.ok_or_else(|| error(ContentErrorKind::InvalidField, end))?;
    let slug = ContentSlug::new(&slug.ok_or_else(|| error(ContentErrorKind::InvalidField, end))?)
        .map_err(|mut e| {
        e.line = input
            .iter()
            .position(|l| l.starts_with("slug: "))
            .map(|i| i + 1);
        e
    })?;
    let body = markdown::render_lines(&input[end..], end)?;
    Ok(ContentEntry {
        source: source.name.into(),
        slug,
        title,
        description,
        body,
    })
}

/// An immutable collection sorted by source filename, validated before serving.
/// No source parsing or filesystem access occurs during requests.
#[derive(Clone, Debug)]
pub struct ContentCollection {
    entries: Vec<ContentEntry>,
}
impl ContentCollection {
    /// Validates strict headers, unique portable slugs, Markdown and aggregate
    /// budgets. Headers contain exactly title, slug and description between
    /// `---ruvane` and `---`. HTML in the body remains escaped text.
    ///
    /// ```
    /// use ruvane::{ContentCollection, ContentSource};
    /// let collection = ContentCollection::new(&[ContentSource::new("hello.md",
    ///     "---ruvane\ntitle: Hello\nslug: hello\ndescription: A first page.\n---\n## Welcome\n\nHello **Rust**!\n")]).unwrap();
    /// assert_eq!(collection.get("hello").unwrap().title(), "Hello");
    /// ```
    pub fn new(sources: &[ContentSource<'_>]) -> Result<Self, ContentError> {
        if sources.is_empty() {
            return Err(ContentError::new(ContentErrorKind::EmptyCollection, None));
        }
        if sources.len() > MAX_CONTENT_FILES {
            return Err(ContentError::new(ContentErrorKind::Limit, None));
        }
        let mut ordered = sources.to_vec();
        for source in &ordered {
            if !valid_source_name(source.name) {
                return Err(ContentError::new(ContentErrorKind::InvalidName, None));
            }
        }
        ordered.sort_by_key(|s| s.name);
        let mut entries: Vec<ContentEntry> = Vec::new();
        let mut bytes = 0usize;
        let mut html = 0usize;
        for source in ordered {
            if entries.iter().any(|e| e.source == source.name) {
                return Err(error(ContentErrorKind::DuplicateField, 1).at(source.name));
            }
            bytes = bytes
                .checked_add(source.text.len())
                .filter(|n| *n <= MAX_CONTENT_TOTAL_BYTES)
                .ok_or_else(|| error(ContentErrorKind::Limit, 1).at(source.name))?;
            let entry = parse(source).map_err(|e| e.at(source.name))?;
            if entries.iter().any(|e| e.slug == entry.slug) {
                return Err(error(ContentErrorKind::DuplicateSlug, 1).at(source.name));
            }
            html += entry.body.as_str().len();
            if html > MAX_CONTENT_TOTAL_HTML_BYTES {
                return Err(error(ContentErrorKind::Limit, 1).at(source.name));
            }
            entries.push(entry);
        }
        Ok(Self { entries })
    }
    /// Iterates in deterministic source filename order.
    pub fn iter(&self) -> impl Iterator<Item = &ContentEntry> {
        self.entries.iter()
    }
    /// Returns the number of documents.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Reports whether there are no documents (validated collections are nonempty).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Looks up a decoded slug without interpreting it as a filesystem path.
    pub fn get(&self, slug: &str) -> Option<&ContentEntry> {
        self.entries.iter().find(|e| e.slug.as_str() == slug)
    }
    /// Enumerates static parameters for a page pattern with one `{slug}` capture.
    pub fn page_mode(&self) -> PageMode {
        PageMode::Static(
            self.entries
                .iter()
                .map(|e| PageParams(vec![("slug".into(), e.slug.as_str().into())]))
                .collect(),
        )
    }
}
