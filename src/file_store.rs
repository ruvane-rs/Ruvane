//! Bounded private file snapshots for one native writer and a trusted directory.
use crate::sessions::entropy::Entropy;
use crate::{
    DownloadName, FileId, FileInfo, FileUploadId, FileUploadState, FileUploadStatus,
    MAX_FILE_UPLOAD_OPERATIONS, MAX_OWNER_FILE_UPLOAD_OPERATIONS, UploadedFile,
};
use std::{
    collections::BTreeMap,
    fmt,
    path::Path,
    sync::{Arc, Mutex},
};
mod disk;
mod operations;
#[cfg(all(
    test,
    any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )
))]
mod tests;
mod wire;
const MAX_FILES: usize = 256;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_CATALOG: usize = MAX_BYTES + MAX_FILES * 358 + MAX_FILE_UPLOAD_OPERATIONS * 137 + 160;

/// A fixed storage failure, without paths, owners, filenames or submitted bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileStoreError {
    /// The owner, identifier, application name or uploaded name is invalid.
    InvalidInput,
    /// Configured limits are invalid or an operation exceeds a quota.
    Limit,
    /// A live or interrupted writer owns this directory. Never steal its lock automatically.
    Busy,
    /// The directory or committed catalog does not satisfy the storage contract.
    Corrupt,
    /// I/O, entropy or locking failed. The handle may no longer be used.
    Unavailable,
    /// Publication occurred but durable synchronization failed. Inspect after offline recovery.
    Uncertain,
    /// The native entropy/filesystem profile is unavailable on this platform.
    UnsupportedPlatform,
    /// The upload identity is expired or dated in the future.
    OperationExpired,
    /// This operation already refers to different input or a deleted file.
    OperationConflict,
}
impl fmt::Display for FileStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid file storage input",
            Self::Limit => "file storage limit exceeded",
            Self::Busy => "file storage requires exclusive ownership",
            Self::Corrupt => "invalid file storage directory or catalog",
            Self::Unavailable => "file storage unavailable",
            Self::Uncertain => "file storage publication uncertain",
            Self::UnsupportedPlatform => "file storage platform unsupported",
            Self::OperationExpired => "file upload operation expired",
            Self::OperationConflict => "file upload operation conflict",
        })
    }
}
impl std::error::Error for FileStoreError {}

fn key(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
/// A stable application owner assigned by trusted server code, not a credential.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct FileOwner(String);
impl FileOwner {
    /// Accepts 1..=64 ASCII letters, digits, underscores and hyphens.
    /// Resolve this key from authorized server state, never directly from a request field.
    pub fn new(value: &str) -> Result<Self, FileStoreError> {
        if !key(value, 64) {
            return Err(FileStoreError::InvalidInput);
        }
        Ok(Self(value.into()))
    }
}
impl fmt::Debug for FileOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FileOwner([redacted])")
    }
}
/// One authorized opaque payload, capped at 32 KiB.
#[derive(Clone)]
pub struct StoredFile {
    info: FileInfo,
    bytes: Vec<u8>,
}
impl StoredFile {
    /// Metadata already checked against the supplied owner.
    pub fn info(&self) -> &FileInfo {
        &self.info
    }
    /// The exact uploaded bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Consumes this file into a fixed no-store attachment response.
    pub fn into_response(self) -> crate::Response {
        crate::Response::download(self.bytes, self.info.name)
    }
}
impl fmt::Debug for StoredFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.info.fmt(f)
    }
}
/// Global and per-owner quotas. Neither count nor byte capacity may be exceeded.
#[derive(Clone, Copy, Debug)]
pub struct FileStoreLimits {
    files: usize,
    bytes: usize,
    owner_files: usize,
    owner_bytes: usize,
}
impl FileStoreLimits {
    /// Sets up to 256 files and 8 MiB globally, with positive per-owner limits no larger than the global limits.
    pub fn new(
        files: usize,
        bytes: usize,
        owner_files: usize,
        owner_bytes: usize,
    ) -> Result<Self, FileStoreError> {
        if !(1..=MAX_FILES).contains(&files)
            || !(1..=MAX_BYTES).contains(&bytes)
            || !(1..=files).contains(&owner_files)
            || !(1..=bytes).contains(&owner_bytes)
        {
            return Err(FileStoreError::Limit);
        }
        Ok(Self {
            files,
            bytes,
            owner_files,
            owner_bytes,
        })
    }
    fn check(&self, records: &Records) -> Result<(), FileStoreError> {
        let mut owners = BTreeMap::<&FileOwner, (usize, usize)>::new();
        let mut total = 0;
        for record in records.values() {
            total += record.file.bytes.len();
            let entry = owners.entry(&record.owner).or_default();
            entry.0 += 1;
            entry.1 += record.file.bytes.len();
            if entry.0 > self.owner_files || entry.1 > self.owner_bytes {
                return Err(FileStoreError::Limit);
            }
        }
        if records.len() > self.files || total > self.bytes {
            return Err(FileStoreError::Limit);
        }
        Ok(())
    }
}
impl Default for FileStoreLimits {
    fn default() -> Self {
        Self {
            files: 64,
            bytes: 2 * 1024 * 1024,
            owner_files: 8,
            owner_bytes: 256 * 1024,
        }
    }
}
#[derive(Clone)]
struct Record {
    owner: FileOwner,
    file: StoredFile,
}
type Records = BTreeMap<FileId, Record>;
type Operations = BTreeMap<(FileOwner, FileUploadId), FileId>;
struct Snapshot {
    records: Records,
    operations: Operations,
    clock: u64,
}
struct State {
    records: Records,
    operations: Operations,
    pending: BTreeMap<FileUploadId, FileOwner>,
    clock: u64,
    entropy: Entropy,
    healthy: bool,
}
struct Store {
    state: Mutex<State>,
    disk: disk::Disk,
    application: String,
    limits: FileStoreLimits,
    #[cfg(test)]
    time: std::sync::atomic::AtomicU64,
}
/// A process-local handle to an exclusively owned private disk snapshot.
/// Requires a trusted local filesystem, a stable application key and one writer process.
/// Every write replaces the bounded snapshot; this is not a large-file or database backend.
#[derive(Clone)]
pub struct FileStore(Arc<Store>);
impl fmt::Debug for FileStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FileStore([private])")
    }
}
impl FileStore {
    /// Opens or initializes an absolute private directory under a trusted existing parent.
    /// Refuses foreign files, symlinks, unsafe permissions and live or stale writer locks.
    pub fn open(
        path: impl AsRef<Path>,
        application: &str,
        limits: FileStoreLimits,
    ) -> Result<Self, FileStoreError> {
        Self::start(path.as_ref(), application, limits, false)
    }
    /// Recovers the last complete snapshot after all writer processes have stopped.
    /// The caller must guarantee exclusive offline access: other processes cannot be detected by std.
    /// Validates the committed snapshot before removing a stale lock or unfinished next snapshot.
    /// This is an administrative startup operation, never an HTTP recovery endpoint.
    pub fn recover(
        path: impl AsRef<Path>,
        application: &str,
        limits: FileStoreLimits,
    ) -> Result<Self, FileStoreError> {
        Self::start(path.as_ref(), application, limits, true)
    }
    fn start(
        path: &Path,
        application: &str,
        limits: FileStoreLimits,
        recovery: bool,
    ) -> Result<Self, FileStoreError> {
        if !key(application, 48) {
            return Err(FileStoreError::InvalidInput);
        }
        let entropy = Entropy::open().map_err(|error| {
            if error == crate::SessionError::UnsupportedPlatform {
                FileStoreError::UnsupportedPlatform
            } else {
                FileStoreError::Unavailable
            }
        })?;
        let (disk, saved) = disk::Disk::open(path, application, limits, recovery)?;
        let snapshot = if let Some(bytes) = saved {
            wire::decode_snapshot(&bytes, application, limits)?
        } else {
            let empty = Snapshot {
                records: Records::new(),
                operations: Operations::new(),
                clock: 0,
            };
            disk.publish(&wire::encode_snapshot(
                &empty.records,
                &empty.operations,
                empty.clock,
                application,
            ))?;
            empty
        };
        Ok(Self(Arc::new(Store {
            state: Mutex::new(State {
                records: snapshot.records,
                operations: snapshot.operations,
                pending: BTreeMap::new(),
                clock: snapshot.clock,
                entropy,
                healthy: true,
            }),
            disk,
            application: application.into(),
            limits,
            #[cfg(test)]
            time: Default::default(),
        })))
    }
    /// Saves one parsed upload for an authorized application owner, returning a new opaque ID.
    /// The original MIME hint is discarded. I/O failures never trigger an automatic retry.
    pub fn save(
        &self,
        owner: &FileOwner,
        upload: &UploadedFile,
    ) -> Result<FileInfo, FileStoreError> {
        self.write_file(owner, upload, None)
    }
    /// Lists only this owner's metadata, in opaque ID order.
    pub fn list(&self, owner: &FileOwner) -> Result<Vec<FileInfo>, FileStoreError> {
        Ok(self
            .state()?
            .records
            .values()
            .filter(|record| &record.owner == owner)
            .map(|record| record.file.info.clone())
            .collect())
    }
    /// Returns exact bytes only for this owner. Missing and foreign IDs both return None.
    pub fn read(
        &self,
        owner: &FileOwner,
        id: FileId,
    ) -> Result<Option<StoredFile>, FileStoreError> {
        Ok(self
            .state()?
            .records
            .get(&id)
            .filter(|record| &record.owner == owner)
            .map(|record| record.file.clone()))
    }
    /// Durably removes this owner's file. Missing and foreign IDs both return false.
    pub fn remove(&self, owner: &FileOwner, id: FileId) -> Result<bool, FileStoreError> {
        let mut state = self.state()?;
        if state
            .records
            .get(&id)
            .is_none_or(|record| &record.owner != owner)
        {
            return Ok(false);
        }
        let mut records = state.records.clone();
        records.remove(&id);
        self.commit(&mut state, records)?;
        Ok(true)
    }
    fn state(&self) -> Result<std::sync::MutexGuard<'_, State>, FileStoreError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| FileStoreError::Unavailable)?;
        if !state.healthy {
            return Err(FileStoreError::Unavailable);
        }
        Ok(state)
    }
    fn commit(&self, state: &mut State, records: Records) -> Result<(), FileStoreError> {
        let now = self.now(state)?;
        let mut operations = state.operations.clone();
        operations.retain(|(_, operation), _| operation.expires_at() > now);
        self.publish(state, records, operations, now)
    }
    fn publish(
        &self,
        state: &mut State,
        records: Records,
        operations: Operations,
        clock: u64,
    ) -> Result<(), FileStoreError> {
        if let Err(error) = self.0.disk.publish(&wire::encode_snapshot(
            &records,
            &operations,
            clock,
            &self.0.application,
        )) {
            state.healthy = false;
            return Err(error);
        }
        state.pending.retain(|operation, owner| {
            operation.expires_at() > clock && !operations.contains_key(&(owner.clone(), *operation))
        });
        state.records = records;
        state.operations = operations;
        state.clock = clock;
        Ok(())
    }
}
