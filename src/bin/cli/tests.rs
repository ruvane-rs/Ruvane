use super::*;
use std::sync::atomic::AtomicUsize;

static NEXT: AtomicUsize = AtomicUsize::new(0);
pub(super) struct Temp(pub(super) PathBuf);
impl Temp {
    pub(super) fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        loop {
            let root = base.join(format!(
                "ruvane-cli-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("test directory: {error}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn managed_roots_and_locks_never_replace_existing_data() {
    let temp = Temp::new();
    let work = temp.0.join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("sentinel"), b"keep").unwrap();
    assert!(project::managed(&work, "owner", WORK_MARKER).is_err());
    assert_eq!(fs::read(work.join("sentinel")).unwrap(), b"keep");
    let work = temp.0.join("managed");
    project::managed(&work, "owner", WORK_MARKER).unwrap();
    let project = project::Project {
        root: temp.0.clone(),
        work: work.clone(),
    };
    let lock = project.lock().unwrap();
    assert!(project.lock().is_err());
    drop(lock);
    assert!(!work.join("lock").exists());
    assert!(project.lock().is_ok());
}

#[test]
fn version_pointers_reject_paths_and_do_not_overwrite_unknown_records() {
    let temp = Temp::new();
    for value in [
        "../../outside\n",
        "build-000001/../x\n",
        "build-000000\n",
        "build-000001\nextra",
        "build-000001",
    ] {
        fs::write(temp.0.join("current"), value).unwrap();
        assert!(project::current(&temp.0).is_err());
        assert!(project::publish(&temp.0, "build-000002").is_err());
        assert_eq!(fs::read_to_string(temp.0.join("current")).unwrap(), value);
    }
    fs::remove_file(temp.0.join("current")).unwrap();
    project::publish(&temp.0, "build-000001").unwrap();
    project::publish(&temp.0, "build-000002").unwrap();
    assert_eq!(project::current(&temp.0).unwrap(), "build-000002");
    for index in 1..=128 {
        fs::create_dir(temp.0.join(format!("build-{index:06}"))).unwrap();
    }
    assert!(project::next_build(&temp.0).is_err());
}

fn watched(root: &Path) {
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("assets")).unwrap();
    fs::write(root.join("Cargo.toml"), b"manifest").unwrap();
    fs::write(root.join("Ruvane.project"), PROJECT_MARKER).unwrap();
    fs::write(root.join("src/main.rs"), b"aaaa").unwrap();
}

#[test]
fn watcher_detects_same_size_content_and_excludes_generated_trees() {
    let temp = Temp::new();
    watched(&temp.0);
    let before = watch::snapshot(&temp.0).unwrap();
    for directory in ["target", "dist", ".git", ".ruvane"] {
        fs::create_dir(temp.0.join(directory)).unwrap();
        fs::write(temp.0.join(directory).join("ignored"), b"ignored").unwrap();
    }
    assert_eq!(watch::snapshot(&temp.0).unwrap(), before);
    let path = temp.0.join("src/main.rs");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, b"bbbb").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_ne!(watch::snapshot(&temp.0).unwrap(), before);
}

#[test]
fn watcher_file_and_depth_budgets_fail_closed() {
    let temp = Temp::new();
    watched(&temp.0);
    for index in 0..1024 {
        fs::write(temp.0.join("assets").join(format!("f{index}")), []).unwrap();
    }
    assert!(watch::snapshot(&temp.0).is_err());
    let other = Temp::new();
    watched(&other.0);
    let mut path = other.0.join("assets");
    for _ in 0..17 {
        path.push("deep");
        fs::create_dir(&path).unwrap();
    }
    assert!(watch::snapshot(&other.0).is_err());
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_rejected_in_sources_and_managed_roots() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    watched(&temp.0);
    symlink(temp.0.join("src"), temp.0.join("assets/alias")).unwrap();
    assert!(watch::snapshot(&temp.0).is_err());
    symlink(temp.0.join("src"), temp.0.join("alias")).unwrap();
    assert!(project::managed(&temp.0.join("alias"), "owner", WORK_MARKER).is_err());
}

#[test]
fn watcher_tracks_content_and_build_script_with_equal_size_equal_time_edits() {
    let temp = Temp::new();
    watched(&temp.0);
    fs::create_dir(temp.0.join("content")).unwrap();
    fs::write(temp.0.join("content/post.md"), b"aaaa").unwrap();
    fs::write(temp.0.join("build.rs"), b"build").unwrap();
    let before = watch::snapshot(&temp.0).unwrap();
    let post = temp.0.join("content/post.md");
    let modified = fs::metadata(&post).unwrap().modified().unwrap();
    fs::write(&post, b"bbbb").unwrap();
    fs::File::options()
        .write(true)
        .open(&post)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let changed = watch::snapshot(&temp.0).unwrap();
    assert_ne!(before, changed);
    fs::write(temp.0.join("build.rs"), b"other").unwrap();
    assert_ne!(watch::snapshot(&temp.0).unwrap(), changed);
    fs::remove_file(post).unwrap();
    assert_ne!(watch::snapshot(&temp.0).unwrap(), changed);
}
