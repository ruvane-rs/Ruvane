//! Immutable interactive page snapshots prepared by a trusted application build.
mod files;
#[cfg(test)]
mod tests;
use crate::{InternalUrl, NavigationPage, NavigationSnapshot, StatusCode};
use std::fmt;

/// Maximum explicitly enumerated interactive pre-rendered pages.
pub const MAX_CLIENT_PRERENDER_PAGES: usize = 128;
/// Maximum encoded pre-render artifact, including HTML and navigation packets.
pub const MAX_CLIENT_PRERENDER_BYTES: usize = 16 * 1024 * 1024;
/// Project capability marker for the first-party two-stage native build.
pub const CLIENT_PRERENDER_MARKER: &str = "ruvane-prerender\t1\n";

/// Keeps regenerated public output until explicit invalidation or cache eviction.
/// The first request in each router namespace loads fresh data, including after restart.
#[derive(Clone, Debug, Default)]
pub struct PrerenderRevalidationPolicy {
    pub(crate) tags: Vec<String>,
}
impl PrerenderRevalidationPolicy {
    /// Creates a policy without tags. Exact-path invalidation remains available.
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a deduplicated ASCII page-name tag, up to MAX_SERVER_CACHE_TAGS.
    pub fn tag(mut self, tag: &str) -> Result<Self, PrerenderError> {
        if !crate::page_route::valid_name(tag) {
            return Err(PrerenderError::InvalidDeclaration);
        }
        if !self.tags.iter().any(|existing| existing == tag) {
            if self.tags.len() == crate::MAX_SERVER_CACHE_TAGS {
                return Err(PrerenderError::Limit);
            }
            self.tags.push(tag.to_owned());
        }
        Ok(self)
    }
    /// Lists declared invalidation tags.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }
}

/// A bounded pre-render declaration, artifact or build failure, without reflected data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrerenderError {
    /// A page, parameter list or combined cache policy is invalid.
    InvalidDeclaration,
    /// Artifact syntax, checksum, snapshot, ownership or revision is invalid.
    InvalidArtifact,
    /// A page/count/HTML/aggregate byte budget was exceeded.
    Limit,
    /// A loader, component, metadata or buffered response could not be validated.
    RenderFailed,
    /// A trusted build path, environment or filesystem operation was refused.
    BuildIo,
}
impl fmt::Display for PrerenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDeclaration => "invalid client pre-render declaration",
            Self::InvalidArtifact => "invalid client pre-render artifact",
            Self::Limit => "client pre-render budget exceeded",
            Self::RenderFailed => "client pre-render failed",
            Self::BuildIo => "client pre-render build I/O failed",
        })
    }
}
impl std::error::Error for PrerenderError {}

#[derive(Clone)]
pub(crate) struct PrerenderRow {
    pub name: String,
    pub url: InternalUrl,
    pub packet: NavigationPage,
    pub html: String,
}
/// A complete, bounded set of immutable HTML and typed navigation snapshots.
/// Artifacts are trusted build output, not an authenticated or public upload format.
#[derive(Clone)]
pub struct ClientPrerender {
    pub(crate) revision: u64,
    pub(crate) rows: Vec<PrerenderRow>,
}
impl ClientPrerender {
    /// Returns the exact application/Wasm revision required by this artifact.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Returns the number of concrete URLs.
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// Reports an empty plan.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// Lists validated, query-free URLs in declaration order.
    pub fn urls(&self) -> impl Iterator<Item = &InternalUrl> {
        self.rows.iter().map(|row| &row.url)
    }
    /// Encodes RPR1 with the revision, complete rows and an accidental-corruption checksum.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = b"RPR1".to_vec();
        bytes.extend(self.revision.to_le_bytes());
        bytes.extend((self.rows.len() as u32).to_le_bytes());
        for row in &self.rows {
            put(&mut bytes, row.name.as_bytes());
            put(&mut bytes, row.url.as_str().as_bytes());
            put(&mut bytes, &row.packet.encode());
            put(&mut bytes, row.html.as_bytes());
        }
        bytes.extend(checksum(&bytes).to_le_bytes());
        bytes
    }
    /// Checks structure, byte budgets, duplicate URLs, packets and embedded snapshots.
    /// The checksum is not a signature. Never treat caller-supplied HTML as trusted build output.
    pub fn decode(bytes: &[u8]) -> Result<Self, PrerenderError> {
        if bytes.len() > MAX_CLIENT_PRERENDER_BYTES {
            return Err(PrerenderError::Limit);
        }
        if bytes.len() < 24 || &bytes[..4] != b"RPR1" {
            return Err(PrerenderError::InvalidArtifact);
        }
        let end = bytes.len() - 8;
        if checksum(&bytes[..end]) != u64::from_le_bytes(bytes[end..].try_into().unwrap()) {
            return Err(PrerenderError::InvalidArtifact);
        }
        let mut input = Input {
            bytes: &bytes[4..end],
            offset: 0,
        };
        let revision = u64::from_le_bytes(input.take(8)?.try_into().unwrap());
        let count = input.number()? as usize;
        if count > MAX_CLIENT_PRERENDER_PAGES {
            return Err(PrerenderError::Limit);
        }
        let mut rows: Vec<PrerenderRow> = Vec::with_capacity(count);
        for _ in 0..count {
            let name = input.string(crate::MAX_PAGE_NAME_BYTES)?;
            if !crate::page_route::valid_name(&name) {
                return Err(PrerenderError::InvalidArtifact);
            }
            let url = InternalUrl::new(&input.string(crate::MAX_INTERNAL_URL_BYTES)?)
                .map_err(|_| PrerenderError::InvalidArtifact)?;
            if rows.iter().any(|row| row.url == url) {
                return Err(PrerenderError::InvalidArtifact);
            }
            let packet = NavigationPage::decode(input.field(crate::MAX_NAVIGATION_BYTES)?)
                .map_err(|_| PrerenderError::InvalidArtifact)?;
            if packet.url() != &url
                || packet.status() != StatusCode::Ok
                || packet.selected_segment() != 0
                || packet.pending_layouts().is_some()
                || packet.failed_segment().is_some()
            {
                return Err(PrerenderError::InvalidArtifact);
            }
            let html = input.string(crate::MAX_STATIC_FILE_BYTES)?;
            let snapshot = NavigationSnapshot::new(revision, packet.clone())
                .map_err(|_| PrerenderError::InvalidArtifact)?
                .html();
            if !html.starts_with("<!doctype html>")
                || !html.ends_with("</html>")
                || html.matches(snapshot.as_str()).count() != 1
            {
                return Err(PrerenderError::InvalidArtifact);
            }
            rows.push(PrerenderRow {
                name,
                url,
                packet,
                html,
            });
        }
        if input.offset != input.bytes.len() {
            return Err(PrerenderError::InvalidArtifact);
        }
        Ok(Self { revision, rows })
    }
}
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(bytes);
}
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
struct Input<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Input<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], PrerenderError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(PrerenderError::InvalidArtifact)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(PrerenderError::InvalidArtifact)?;
        self.offset = end;
        Ok(value)
    }
    fn number(&mut self) -> Result<u32, PrerenderError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn field(&mut self, max: usize) -> Result<&'a [u8], PrerenderError> {
        let len = self.number()? as usize;
        if len > max {
            return Err(PrerenderError::Limit);
        }
        self.take(len)
    }
    fn string(&mut self, max: usize) -> Result<String, PrerenderError> {
        String::from_utf8(self.field(max)?.to_vec()).map_err(|_| PrerenderError::InvalidArtifact)
    }
}
