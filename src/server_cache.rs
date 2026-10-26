//! Bounded public server data caching, shared through explicit application handles.
mod store;
use std::fmt;
pub(crate) use store::{Access, CachedBranch, Namespace};
pub use store::{ServerCache, ServerCacheStats};

/// Maximum retained entries in a server cache.
pub const MAX_SERVER_CACHE_ENTRIES: usize = 128;
/// Maximum accounted payload bytes retained in a server cache.
pub const MAX_SERVER_CACHE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum simultaneous public cache loaders per handle.
pub const MAX_SERVER_CACHE_LOADS: usize = 16;
/// Maximum waiting requests per handle.
pub const MAX_SERVER_CACHE_WAITERS: usize = 64;
/// Maximum time a request can wait for another cache loader.
pub const MAX_SERVER_CACHE_WAIT_MS: u64 = 1000;
/// Maximum tags attached to one cached branch.
pub const MAX_SERVER_CACHE_TAGS: usize = 8;

/// Invalid configuration/input, or a cache operation that cannot proceed safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerCacheError {
    /// A count, byte, duration or freshness limit is invalid.
    InvalidLimits,
    /// A tag is not an ASCII page-name identifier, or the tag budget is exhausted.
    InvalidTag,
    /// An invalidation path is not a validated query-free internal path.
    InvalidPath,
    /// A wait/load budget is exhausted, invalidation retired a load, or state is unavailable.
    Unavailable,
}
impl fmt::Display for ServerCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "invalid server cache limits",
            Self::InvalidTag => "invalid server cache tag",
            Self::InvalidPath => "invalid server cache path",
            Self::Unavailable => "server cache unavailable",
        })
    }
}
impl std::error::Error for ServerCacheError {}

/// Validated storage, simultaneous-loader and wait budgets.
#[derive(Clone, Copy, Debug)]
pub struct ServerCacheLimits {
    entries: usize,
    bytes: usize,
    loads: usize,
    wait_ms: u64,
}
impl ServerCacheLimits {
    /// Sets positive budgets below the exported maxima. Waits use an absolute deadline.
    pub fn new(
        entries: usize,
        bytes: usize,
        loads: usize,
        wait_ms: u64,
    ) -> Result<Self, ServerCacheError> {
        if !(1..=MAX_SERVER_CACHE_ENTRIES).contains(&entries)
            || !(1..=MAX_SERVER_CACHE_BYTES).contains(&bytes)
            || !(1..=MAX_SERVER_CACHE_LOADS).contains(&loads)
            || !(1..=MAX_SERVER_CACHE_WAIT_MS).contains(&wait_ms)
        {
            return Err(ServerCacheError::InvalidLimits);
        }
        Ok(Self {
            entries,
            bytes,
            loads,
            wait_ms,
        })
    }
}
impl Default for ServerCacheLimits {
    fn default() -> Self {
        Self {
            entries: 32,
            bytes: 2 * 1024 * 1024,
            loads: 8,
            wait_ms: 1000,
        }
    }
}

/// Opt-in assertion that an entire client page branch contains public data.
/// This is independent of browser navigation caching and HTTP freshness.
#[derive(Clone, Debug, Default)]
pub struct ServerCachePolicy {
    seconds: Option<u32>,
    tags: Vec<String>,
}
impl ServerCachePolicy {
    /// Keeps server data caching disabled.
    pub fn no_store() -> Self {
        Self::default()
    }
    /// Allows public branch data to remain fresh for 1..=300 seconds after loading.
    pub fn public(seconds: u32) -> Result<Self, ServerCacheError> {
        if !(1..=300).contains(&seconds) {
            return Err(ServerCacheError::InvalidLimits);
        }
        Ok(Self {
            seconds: Some(seconds),
            tags: Vec::new(),
        })
    }
    /// Adds a unique invalidation tag. Tags use the page-name identifier profile.
    pub fn tag(mut self, tag: &str) -> Result<Self, ServerCacheError> {
        if !crate::page_route::valid_name(tag) || self.seconds.is_none() {
            return Err(ServerCacheError::InvalidTag);
        }
        if !self.tags.iter().any(|value| value == tag) {
            if self.tags.len() == MAX_SERVER_CACHE_TAGS {
                return Err(ServerCacheError::InvalidTag);
            }
            self.tags.push(tag.into());
        }
        Ok(self)
    }
    /// Returns the declared freshness, or None when disabled.
    pub fn max_age_seconds(&self) -> Option<u32> {
        self.seconds
    }
    /// Returns the declared invalidation tags.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }
}
