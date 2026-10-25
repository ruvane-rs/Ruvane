use super::{Result, project};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) type Snapshot = BTreeMap<PathBuf, (usize, u64)>;
const MAX_FILES: usize = 1024;
const MAX_BYTES: usize = 64 * 1024 * 1024;

fn fingerprint(root: &Path, path: &Path, result: &mut Snapshot, bytes: &mut usize) -> Result<()> {
    if result.len() == MAX_FILES {
        return Err("watch file count exceeded".into());
    }
    let body = project::read(path, 16 * 1024 * 1024)?;
    *bytes = bytes
        .checked_add(body.len())
        .filter(|size| *size <= MAX_BYTES)
        .ok_or("watch byte budget exceeded")?;
    // For change detection, not integrity checks.
    let hash = body.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    result.insert(path.strip_prefix(root)?.to_owned(), (body.len(), hash));
    Ok(())
}

fn visit(
    root: &Path,
    directory: &Path,
    depth: usize,
    directories: &mut usize,
    result: &mut Snapshot,
    bytes: &mut usize,
) -> Result<()> {
    *directories += 1;
    if depth > 16 || *directories > 128 {
        return Err("watch directory budget exceeded".into());
    }
    project::checked_dir(directory)?;
    let mut count = 0;
    for entry in fs::read_dir(directory)? {
        count += 1;
        if count > MAX_FILES {
            return Err("watch directory entry budget exceeded".into());
        }
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err("symbolic links in watched sources are unsupported".into());
        }
        if kind.is_dir() {
            visit(root, &path, depth + 1, directories, result, bytes)?;
        } else if kind.is_file() {
            fingerprint(root, &path, result, bytes)?;
        } else {
            return Err("non-regular watched source".into());
        }
    }
    Ok(())
}

pub(super) fn snapshot(root: &Path) -> Result<Snapshot> {
    let mut result = Snapshot::new();
    let mut bytes = 0;
    let mut directories = 0;
    for name in ["src", "assets"] {
        visit(
            root,
            &root.join(name),
            0,
            &mut directories,
            &mut result,
            &mut bytes,
        )?;
    }
    if fs::symlink_metadata(root.join("browser")).is_ok() {
        visit(
            root,
            &root.join("browser"),
            0,
            &mut directories,
            &mut result,
            &mut bytes,
        )?;
    }
    // Projects created before alpha.10 may lack content and build scripts.
    if fs::symlink_metadata(root.join("content")).is_ok() {
        visit(
            root,
            &root.join("content"),
            0,
            &mut directories,
            &mut result,
            &mut bytes,
        )?;
    }
    if fs::symlink_metadata(root.join("build.rs")).is_ok() {
        fingerprint(root, &root.join("build.rs"), &mut result, &mut bytes)?;
    }
    for name in ["Cargo.toml", "Ruvane.project"] {
        fingerprint(root, &root.join(name), &mut result, &mut bytes)?;
    }
    refresh_lock(root, &mut result)?;
    Ok(result)
}

pub(super) fn refresh_lock(root: &Path, snapshot: &mut Snapshot) -> Result<()> {
    let path = root.join("Cargo.lock");
    if fs::symlink_metadata(&path).is_ok() {
        let body = project::read(&path, 1024 * 1024)?;
        let key = Path::new("Cargo.lock");
        if !snapshot.contains_key(key) && snapshot.len() == MAX_FILES {
            return Err("watch file count exceeded".into());
        }
        let other_bytes: usize = snapshot
            .iter()
            .filter(|(path, _)| path.as_path() != key)
            .map(|(_, value)| value.0)
            .sum();
        if other_bytes + body.len() > MAX_BYTES {
            return Err("watch byte budget exceeded".into());
        }
        let hash = body.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        snapshot.insert(PathBuf::from("Cargo.lock"), (body.len(), hash));
    } else if fs::symlink_metadata(&path)
        .err()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        snapshot.remove(Path::new("Cargo.lock"));
    } else {
        return Err("cannot inspect Cargo.lock".into());
    }
    Ok(())
}
