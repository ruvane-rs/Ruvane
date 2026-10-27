use super::*;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
const LOCK: &[u8] = b"RFL1\n";
const NAMES: [&str; 4] = ["identity", "LOCK", "store.bin", "next.bin"];
static ACTIVE: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
struct Lease(PathBuf);
impl Lease {
    fn acquire(root: &Path) -> Result<Self, FileStoreError> {
        let mut active = ACTIVE
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| FileStoreError::Unavailable)?;
        if !active.insert(root.to_owned()) {
            return Err(FileStoreError::Busy);
        }
        Ok(Self(root.to_owned()))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(active) = ACTIVE.get() {
            if let Ok(mut active) = active.lock() {
                active.remove(&self.0);
            }
        }
    }
}
pub(super) struct Disk {
    root: PathBuf,
    clean: AtomicBool,
    _lease: Lease,
    #[cfg(test)]
    pub(super) fault: std::sync::atomic::AtomicU8,
}
impl Disk {
    pub(super) fn open(
        path: &Path,
        application: &str,
        limits: FileStoreLimits,
        recovery: bool,
    ) -> Result<(Self, Option<Vec<u8>>), FileStoreError> {
        let (root, fresh) = directory(path, application)?;
        let lease = Lease::acquire(&root)?;
        let saved = if fresh {
            None
        } else {
            Some(read(&root.join("store.bin"), MAX_CATALOG)?)
        };
        if recovery {
            if let Some(bytes) = &saved {
                wire::decode(bytes, application, limits)?;
            }
            if root
                .join("LOCK")
                .try_exists()
                .map_err(|_| FileStoreError::Unavailable)?
            {
                let bytes = read(&root.join("LOCK"), LOCK.len())?;
                if !LOCK.starts_with(&bytes) {
                    return Err(FileStoreError::Corrupt);
                }
                fs::remove_file(root.join("LOCK")).map_err(|_| FileStoreError::Unavailable)?;
            }
            if root
                .join("next.bin")
                .try_exists()
                .map_err(|_| FileStoreError::Unavailable)?
            {
                fs::remove_file(root.join("next.bin")).map_err(|_| FileStoreError::Unavailable)?;
            }
            sync(&root).map_err(|_| FileStoreError::Unavailable)?;
        } else if root
            .join("LOCK")
            .try_exists()
            .map_err(|_| FileStoreError::Unavailable)?
            || root
                .join("next.bin")
                .try_exists()
                .map_err(|_| FileStoreError::Unavailable)?
        {
            return Err(FileStoreError::Busy);
        }
        let mut lock = create(&root.join("LOCK")).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                FileStoreError::Busy
            } else {
                FileStoreError::Unavailable
            }
        })?;
        lock.write_all(LOCK)
            .and_then(|_| lock.sync_all())
            .and_then(|_| sync(&root))
            .map_err(|_| FileStoreError::Unavailable)?;
        Ok((
            Self {
                root,
                clean: AtomicBool::new(true),
                _lease: lease,
                #[cfg(test)]
                fault: Default::default(),
            },
            saved,
        ))
    }
    pub(super) fn publish(&self, bytes: &[u8]) -> Result<(), FileStoreError> {
        let mut published = false;
        let result = (|| -> std::io::Result<()> {
            let mut file = create(&self.root.join("next.bin"))?;
            self.fail(1)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            self.fail(2)?;
            fs::rename(self.root.join("next.bin"), self.root.join("store.bin"))?;
            published = true;
            self.fail(3)?;
            sync(&self.root)
        })();
        if result.is_err() {
            self.clean.store(false, Ordering::Relaxed);
            return Err(if published {
                FileStoreError::Uncertain
            } else {
                FileStoreError::Unavailable
            });
        }
        Ok(())
    }
    fn fail(&self, point: u8) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fault.load(Ordering::Relaxed) == point {
            return Err(std::io::Error::other("injected storage failure"));
        }
        let _ = point;
        Ok(())
    }
}
impl Drop for Disk {
    fn drop(&mut self) {
        if self.clean.load(Ordering::Relaxed)
            && read(&self.root.join("LOCK"), LOCK.len()).is_ok_and(|bytes| bytes == LOCK)
        {
            let _ = fs::remove_file(self.root.join("LOCK"));
            let _ = sync(&self.root);
        }
    }
}
#[cfg(unix)]
fn directory(path: &Path, application: &str) -> Result<(PathBuf, bool), FileStoreError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    if !path.is_absolute()
        || !path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(FileStoreError::InvalidInput);
    }
    let parent = path
        .parent()
        .filter(|parent| *parent != path)
        .ok_or(FileStoreError::InvalidInput)?;
    let mut current = PathBuf::new();
    for part in parent.components() {
        current.push(part);
        let metadata = fs::symlink_metadata(&current).map_err(|_| FileStoreError::Unavailable)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(FileStoreError::Corrupt);
        }
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(FileStoreError::Corrupt);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(path)
                .map_err(|_| FileStoreError::Unavailable)?;
            if fs::symlink_metadata(path)
                .map_err(|_| FileStoreError::Unavailable)?
                .mode()
                & 0o777
                != 0o700
            {
                return Err(FileStoreError::Corrupt);
            }
        }
        Err(_) => return Err(FileStoreError::Unavailable),
    }
    let root = path
        .canonicalize()
        .map_err(|_| FileStoreError::Unavailable)?;
    if root != path {
        return Err(FileStoreError::Corrupt);
    }
    let entries = fs::read_dir(&root)
        .map_err(|_| FileStoreError::Unavailable)?
        .take(5)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| FileStoreError::Unavailable)?;
    if entries.len() > NAMES.len() {
        return Err(FileStoreError::Corrupt);
    }
    for entry in &entries {
        if !NAMES.iter().any(|name| entry.file_name() == *name) {
            return Err(FileStoreError::Corrupt);
        }
        private_metadata(&entry.path(), MAX_CATALOG)?;
    }
    let identity = format!("ruvane-files\t1\t{application}\n");
    let fresh = entries.is_empty();
    if fresh {
        sync(parent).map_err(|_| FileStoreError::Unavailable)?;
        let mut marker = create(&root.join("identity")).map_err(|_| FileStoreError::Unavailable)?;
        marker
            .write_all(identity.as_bytes())
            .and_then(|_| marker.sync_all())
            .and_then(|_| sync(&root))
            .map_err(|_| FileStoreError::Unavailable)?;
    } else if read(&root.join("identity"), 80)? != identity.as_bytes() {
        return Err(FileStoreError::Corrupt);
    }
    Ok((root, fresh))
}
#[cfg(not(unix))]
fn directory(_: &Path, _: &str) -> Result<(PathBuf, bool), FileStoreError> {
    Err(FileStoreError::UnsupportedPlatform)
}
#[cfg(unix)]
fn private_metadata(path: &Path, limit: usize) -> Result<fs::Metadata, FileStoreError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).map_err(|_| FileStoreError::Corrupt)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() > limit as u64
    {
        return Err(FileStoreError::Corrupt);
    }
    Ok(metadata)
}
#[cfg(not(unix))]
fn private_metadata(_: &Path, _: usize) -> Result<fs::Metadata, FileStoreError> {
    Err(FileStoreError::UnsupportedPlatform)
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, FileStoreError> {
    let before = private_metadata(path, limit)?;
    let file = File::open(path).map_err(|_| FileStoreError::Unavailable)?;
    let opened = file.metadata().map_err(|_| FileStoreError::Unavailable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev()
            || before.ino() != opened.ino()
            || opened.nlink() != 1
            || opened.mode() & 0o777 != 0o600
        {
            return Err(FileStoreError::Corrupt);
        }
    }
    let _ = (before, opened);
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FileStoreError::Unavailable)?;
    if bytes.len() > limit {
        return Err(FileStoreError::Corrupt);
    }
    Ok(bytes)
}
#[cfg(unix)]
fn create(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o777 != 0o600 {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}
#[cfg(not(unix))]
fn create(_: &Path) -> std::io::Result<File> {
    Err(std::io::ErrorKind::Unsupported.into())
}
fn sync(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}
