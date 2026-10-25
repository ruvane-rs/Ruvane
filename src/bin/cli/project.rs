use super::{INTERACTIVE_MARKER, PROJECT_MARKER, Result, WORK_MARKER};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub(super) struct Project {
    pub root: PathBuf,
    pub work: PathBuf,
    pub interactive: bool,
}

impl Project {
    pub fn open() -> Result<Self> {
        let root = std::env::current_dir()?;
        checked_dir(&root)?;
        let marker = read(&root.join("Ruvane.project"), 256)?;
        let interactive = marker == INTERACTIVE_MARKER.as_bytes();
        if !interactive && marker != PROJECT_MARKER.as_bytes() {
            return Err("not a supported Ruvane project root".into());
        }
        regular(&root.join("Cargo.toml"))?;
        let work = root.join(".ruvane");
        managed(&work, "owner", WORK_MARKER)?;
        Ok(Self {
            root,
            work,
            interactive,
        })
    }

    pub fn lock(&self) -> Result<Lock> {
        let path = self.work.join("lock");
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|_| "project is locked; stop its other CLI process first (a crashed process may leave .ruvane/lock)")?;
        let lock = Lock(path);
        file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
        Ok(lock)
    }
}

pub(super) struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn checked_dir(path: &Path) -> Result<()> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("parent traversal is unsupported".into());
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for part in absolute.components() {
        current.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("expected real directories without symbolic links".into());
        }
    }
    Ok(())
}

pub(super) fn regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("expected a regular file without a symbolic link".into());
    }
    Ok(())
}

pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    regular(path)?;
    if fs::metadata(path)?.len() > limit as u64 {
        return Err("file exceeds CLI budget".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err("file exceeds CLI budget".into());
    }
    Ok(bytes)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

pub(super) fn managed(path: &Path, marker: &str, contents: &str) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => write_new(&path.join(marker), contents.as_bytes())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    check_managed(path, marker, contents)
}

pub(super) fn check_managed(path: &Path, marker: &str, contents: &str) -> Result<()> {
    checked_dir(path)?;
    if read(&path.join(marker), 256)? != contents.as_bytes() {
        return Err("refusing an unrecognized managed directory".into());
    }
    Ok(())
}

pub(super) fn build_id(value: &str) -> bool {
    value.len() == 12
        && value.starts_with("build-")
        && value[6..].bytes().all(|b| b.is_ascii_digit())
        && value != "build-000000"
}

pub(super) fn next_build(root: &Path) -> Result<String> {
    for index in 1..=128 {
        let name = format!("build-{index:06}");
        match fs::symlink_metadata(root.join(&name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(name),
            _ => {}
        }
    }
    Err("128 build slots are occupied; choose another --out directory".into())
}

pub(super) fn current(root: &Path) -> Result<String> {
    let bytes = read(&root.join("current"), 32)?;
    let value = std::str::from_utf8(&bytes)?;
    let id = value
        .strip_suffix('\n')
        .ok_or("invalid current build pointer")?;
    if !build_id(id) {
        return Err("invalid current build pointer".into());
    }
    Ok(id.to_owned())
}

pub(super) fn publish(root: &Path, id: &str) -> Result<()> {
    if !build_id(id) {
        return Err("invalid build identifier".into());
    }
    if fs::symlink_metadata(root.join("current")).is_ok() {
        current(root)?;
    }
    let temporary = root.join(format!("current-{}.tmp", &id[6..]));
    write_new(&temporary, format!("{id}\n").as_bytes())?;
    fs::rename(&temporary, root.join("current"))?;
    Ok(())
}
