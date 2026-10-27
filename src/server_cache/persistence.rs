use super::*;
use crate::ClientPrerender;
use crate::prerender::PrerenderRow;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(all(test, unix))]
mod tests;
mod wire;

/// Maximum encoded persistent catalog, independent of the memory budget.
pub const MAX_SERVER_CACHE_DISK_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SCOPE_BYTES: usize = 256 * 1024;
const FILES: [&str; 5] = ["identity", "LOCK", "store.bin", "pending", "next.bin"];

/// Bounds the encoded catalog, including its metadata and complete outputs.
#[derive(Clone, Copy, Debug)]
pub struct ServerCacheDiskLimits {
    entries: usize,
    bytes: usize,
}
impl ServerCacheDiskLimits {
    /// Sets 1..=128 entries and 32 bytes..=16 MiB for the complete catalog.
    pub fn new(entries: usize, bytes: usize) -> Result<Self, ServerCacheError> {
        if !(1..=MAX_SERVER_CACHE_ENTRIES).contains(&entries)
            || !(32..=MAX_SERVER_CACHE_DISK_BYTES).contains(&bytes)
        {
            return Err(ServerCacheError::InvalidLimits);
        }
        Ok(Self { entries, bytes })
    }
}
impl Default for ServerCacheDiskLimits {
    fn default() -> Self {
        Self {
            entries: 32,
            bytes: 8 * 1024 * 1024,
        }
    }
}

/// Exclusive persistent storage for public regenerated output on native Unix.
/// Use a trusted private directory and a stable ASCII application identifier.
/// The checksum detects corruption; it does not authenticate HTML from other writers.
pub struct ServerCachePersistence {
    pub(super) store: Arc<Store>,
}
impl ServerCachePersistence {
    /// Opens a new or cleanly closed cache. Refuses symlinks, foreign files and live/stale locks.
    /// An interrupted owner requires recover_empty after all other owners have stopped.
    /// Unsupported platforms, including Wasm, return PersistenceUnavailable.
    pub fn open(
        path: impl AsRef<Path>,
        application: &str,
        limits: ServerCacheDiskLimits,
    ) -> Result<Self, ServerCacheError> {
        if !cfg!(unix) {
            return Err(unavailable());
        }
        let root = directory(path.as_ref(), application)?;
        let mut lock = create(&root.join("LOCK")).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ServerCacheError::PersistenceBusy
            } else {
                unavailable()
            }
        })?;
        lock.write_all(b"ruvane-cache-owner\t1\n")
            .and_then(|_| lock.sync_all())
            .map_err(|_| unavailable())?;
        sync_directory(&root)?;
        if root.join("pending").exists() || root.join("next.bin").exists() {
            return Err(ServerCacheError::PersistenceBusy);
        }
        let now = clock()?;
        let decoded = read_catalog(&root)
            .ok()
            .flatten()
            .and_then(|bytes| wire::decode(&bytes).ok());
        let (saved, records) = decoded.unwrap_or((now, Vec::new()));
        let mut state = State {
            records: if saved <= now { records } else { Vec::new() },
            clock: now,
            healthy: true,
            closed: false,
            restores: 0,
        };
        state.prune(now);
        state.bound(limits);
        let store = Arc::new(Store {
            root,
            limits,
            state: Mutex::new(state),
        });
        {
            let mut state = store.state.lock().map_err(|_| unavailable())?;
            store.commit(&mut state)?;
        }
        Ok(Self { store })
    }
    /// Discards this dedicated cache after an interrupted or failed owner.
    /// The caller must first stop every process using the directory. Never use this to take over a live owner.
    pub fn recover_empty(
        path: impl AsRef<Path>,
        application: &str,
        limits: ServerCacheDiskLimits,
    ) -> Result<Self, ServerCacheError> {
        if !cfg!(unix) {
            return Err(unavailable());
        }
        let root = directory(path.as_ref(), application)?;
        if !root.join("LOCK").exists() {
            create(&root.join("LOCK"))
                .and_then(|file| file.sync_all())
                .map_err(|_| unavailable())?;
            sync_directory(&root)?;
        }
        for name in ["store.bin", "pending", "next.bin"] {
            remove_if_present(&root.join(name))?;
        }
        sync_directory(&root)?;
        fs::remove_file(root.join("LOCK")).map_err(|_| unavailable())?;
        sync_directory(&root)?;
        Self::open(root, application, limits)
    }
}

#[derive(Clone)]
pub(crate) struct OutputIdentity {
    pub scope: Vec<u8>,
    pub name: String,
    pub revision: u64,
}
pub(crate) struct RestoredOutput {
    pub row: PrerenderRow,
    pub expires: Option<Instant>,
}
#[derive(Clone)]
struct Record {
    scope: Vec<u8>,
    published: u64,
    seconds: Option<u32>,
    stale: u32,
    tags: Vec<String>,
    plan: ClientPrerender,
    payload_len: usize,
}
impl Record {
    fn bytes(&self) -> usize {
        28 + self.scope.len()
            + self.payload_len
            + self.tags.iter().map(|tag| 4 + tag.len()).sum::<usize>()
    }
    fn matches(&self, identity: &OutputIdentity, url: &str) -> bool {
        self.scope == identity.scope
            && self.plan.revision == identity.revision
            && self.plan.rows[0].name == identity.name
            && self.plan.rows[0].url.as_str() == url
    }
    fn fresh(&self, now: u64) -> bool {
        self.published <= now
            && self.seconds.is_none_or(|seconds| {
                self.published
                    .checked_add(u64::from(seconds) * 1000)
                    .is_some_and(|end| end > now)
            })
    }
}
struct State {
    records: Vec<Record>,
    clock: u64,
    healthy: bool,
    closed: bool,
    restores: u64,
}
impl State {
    fn prune(&mut self, now: u64) {
        self.records.retain(|record| record.fresh(now));
    }
    fn bytes(&self) -> usize {
        24 + self.records.iter().map(Record::bytes).sum::<usize>()
    }
    fn bound(&mut self, limits: ServerCacheDiskLimits) {
        while self.records.len() > limits.entries || self.bytes() > limits.bytes {
            self.records.remove(0);
        }
    }
    fn observe(&mut self) -> Result<u64, ServerCacheError> {
        if !self.healthy || self.closed {
            return Err(unavailable());
        }
        let now = clock().inspect_err(|_| self.healthy = false)?;
        if now < self.clock {
            self.records.clear();
        }
        self.clock = now;
        Ok(now)
    }
}
pub(super) struct Store {
    root: PathBuf,
    limits: ServerCacheDiskLimits,
    state: Mutex<State>,
}
impl Store {
    pub fn available(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.healthy && !state.closed)
    }
    pub fn stats(&self) -> Result<(usize, usize, u64), ServerCacheError> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        Ok((state.records.len(), state.bytes(), state.restores))
    }
    pub fn get(
        &self,
        identity: &OutputIdentity,
        url: &str,
    ) -> Result<Option<RestoredOutput>, ServerCacheError> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        let now = state.observe()?;
        state.prune(now);
        let Some(record) = state
            .records
            .iter()
            .find(|record| record.matches(identity, url))
        else {
            return Ok(None);
        };
        let expires = record.seconds.map(|seconds| {
            let end = record.published + u64::from(seconds) * 1000;
            Instant::now() + Duration::from_millis(end - now)
        });
        Ok(Some(RestoredOutput {
            row: record.plan.rows[0].clone(),
            expires,
        }))
    }
    pub fn restored(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.restores = state.restores.saturating_add(1);
        }
    }
    pub fn put(
        &self,
        identity: &OutputIdentity,
        url: &str,
        tags: &[String],
        seconds: Option<u32>,
        stale: u32,
        branch: &store::CachedBranch,
    ) -> Result<(), ServerCacheError> {
        let html = String::from_utf8(branch.output.as_ref().ok_or_else(unavailable)?.to_vec())
            .map_err(|_| unavailable())?;
        let plan = ClientPrerender {
            revision: identity.revision,
            rows: vec![PrerenderRow {
                name: identity.name.clone(),
                url: branch.packet.url().clone(),
                packet: branch.packet.clone(),
                html,
            }],
        };
        let encoded = plan.encode();
        ClientPrerender::decode(&encoded).map_err(|_| unavailable())?;
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        let now = state.observe()?;
        state.prune(now);
        state.records.retain(|record| {
            record.scope != identity.scope || record.plan.rows[0].url.as_str() != url
        });
        let record = Record {
            scope: identity.scope.clone(),
            published: now,
            seconds,
            stale,
            tags: tags.to_vec(),
            plan,
            payload_len: encoded.len(),
        };
        if record.scope.len() <= MAX_SCOPE_BYTES && record.bytes() + 24 <= self.limits.bytes {
            state.records.push(record);
        }
        state.bound(self.limits);
        self.commit(&mut state)
    }
    pub fn invalidate(
        &self,
        matches: impl Fn(&str, &[String]) -> bool,
    ) -> Result<(), ServerCacheError> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        let now = state.observe()?;
        state.prune(now);
        state
            .records
            .retain(|record| !matches(record.plan.rows[0].url.as_str(), &record.tags));
        self.commit(&mut state)
    }
    pub fn remove(&self, identity: &OutputIdentity, url: &str) -> Result<(), ServerCacheError> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        state.observe()?;
        state
            .records
            .retain(|record| !record.matches(identity, url));
        self.commit(&mut state)
    }
    fn commit(&self, state: &mut State) -> Result<(), ServerCacheError> {
        let result = (|| {
            let bytes = wire::encode(state.clock, &state.records);
            if bytes.len() > self.limits.bytes {
                return Err(unavailable());
            }
            let mut pending = create(&self.root.join("pending")).map_err(|_| unavailable())?;
            pending
                .write_all(b"ruvane-cache-transaction\t1\n")
                .and_then(|_| pending.sync_all())
                .map_err(|_| unavailable())?;
            sync_directory(&self.root)?;
            let mut next = create(&self.root.join("next.bin")).map_err(|_| unavailable())?;
            next.write_all(&bytes)
                .and_then(|_| next.sync_all())
                .map_err(|_| unavailable())?;
            fs::rename(self.root.join("next.bin"), self.root.join("store.bin"))
                .map_err(|_| unavailable())?;
            sync_directory(&self.root)?;
            fs::remove_file(self.root.join("pending")).map_err(|_| unavailable())?;
            sync_directory(&self.root)
        })();
        if result.is_err() {
            state.healthy = false;
        }
        result
    }
    pub fn close(&self) -> Result<(), ServerCacheError> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.closed {
            return Ok(());
        }
        let now = state.observe()?;
        state.prune(now);
        self.commit(&mut state)?;
        let result = fs::remove_file(self.root.join("LOCK"))
            .map_err(|_| unavailable())
            .and_then(|_| sync_directory(&self.root));
        state.healthy = result.is_ok();
        state.closed = true;
        result
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
fn unavailable() -> ServerCacheError {
    ServerCacheError::PersistenceUnavailable
}
fn clock() -> Result<u64, ServerCacheError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| u64::try_from(time.as_millis()).ok())
        .ok_or_else(unavailable)
}
fn create(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn sync_directory(root: &Path) -> Result<(), ServerCacheError> {
    File::open(root)
        .and_then(|file| file.sync_all())
        .map_err(|_| unavailable())
}
fn remove_if_present(path: &Path) -> Result<(), ServerCacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(unavailable()),
    }
}
fn regular(path: &Path) -> Result<(), ServerCacheError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(unavailable());
        }
    }
    Ok(())
}
fn directory(path: &Path, application: &str) -> Result<PathBuf, ServerCacheError> {
    if !path.is_absolute()
        || !crate::page_route::valid_name(application)
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(unavailable());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(unavailable());
            }
        } else if current != path {
            return Err(unavailable());
        }
    }
    if !path.exists() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(|_| unavailable())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)
            .map_err(|_| unavailable())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(unavailable());
        }
    }
    let entries = fs::read_dir(path).map_err(|_| unavailable())?;
    let mut count = 0;
    for entry in entries {
        count += 1;
        let entry = entry.map_err(|_| unavailable())?;
        if count > FILES.len() || !FILES.iter().any(|name| entry.file_name() == *name) {
            return Err(unavailable());
        }
        regular(&entry.path())?;
    }
    let identity = format!("ruvane-server-cache\t1\t{application}\n");
    let marker = path.join("identity");
    if count == 0 {
        create(&marker)
            .and_then(|mut file| {
                file.write_all(identity.as_bytes())
                    .and_then(|_| file.sync_all())
            })
            .map_err(|_| unavailable())?;
        sync_directory(path)?;
    } else {
        regular(&marker)?;
        let mut bytes = Vec::new();
        File::open(&marker)
            .and_then(|file| file.take(512).read_to_end(&mut bytes))
            .map_err(|_| unavailable())?;
        if bytes != identity.as_bytes() {
            return Err(unavailable());
        }
    }
    Ok(path.to_owned())
}
fn read_catalog(root: &Path) -> Result<Option<Vec<u8>>, ServerCacheError> {
    let path = root.join("store.bin");
    if !path.exists() {
        return Ok(None);
    }
    regular(&path)?;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_SERVER_CACHE_DISK_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| unavailable())?;
    if bytes.len() > MAX_SERVER_CACHE_DISK_BYTES {
        return Err(unavailable());
    }
    Ok(Some(bytes))
}
