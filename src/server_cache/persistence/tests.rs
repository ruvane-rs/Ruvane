use super::*;
use crate::{
    ActionData, ComponentProps, Document, InitialState, InternalUrl, NavigationPage,
    NavigationSnapshot, StatusCode,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "ruvane-disk-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("cache")
    }
    fn open(&self) -> ServerCachePersistence {
        ServerCachePersistence::open(self.path(), "fixture", ServerCacheDiskLimits::default())
            .unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn identity() -> OutputIdentity {
    OutputIdentity {
        scope: b"scope-one".to_vec(),
        name: "page".into(),
        revision: 9,
    }
}
fn branch(path: &str) -> store::CachedBranch {
    let packet = NavigationPage::new(
        InternalUrl::new(path).unwrap(),
        "Public",
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap();
    let html = Document::new(
        "Public",
        NavigationSnapshot::new(9, packet.clone()).unwrap().html(),
    )
    .render();
    store::CachedBranch {
        head: None,
        output: Some(Arc::from(html.as_str().as_bytes())),
        packet,
        fallback: crate::Html::default(),
        policy: crate::NavigationCachePolicy::no_store(),
    }
}
fn put(disk: &ServerCachePersistence, path: &str, seconds: Option<u32>) {
    disk.store
        .put(
            &identity(),
            path,
            &["articles".into()],
            seconds,
            0,
            &branch(path),
        )
        .unwrap();
}

#[test]
fn catalog_roundtrip_bounds_corruption_and_every_truncated_prefix_are_checked() {
    let temp = Temp::new();
    let disk = temp.open();
    put(&disk, "/a", None);
    let state = disk.store.state.lock().unwrap();
    let bytes = wire::encode(state.clock, &state.records);
    assert_eq!(bytes.len(), state.bytes());
    assert_eq!(wire::decode(&bytes).unwrap().1.len(), 1);
    for end in 0..bytes.len() {
        assert!(wire::decode(&bytes[..end]).is_err());
    }
    for n in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[n] ^= 1;
        assert!(wire::decode(&changed).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(wire::decode(&trailing).is_err());
    let mut duplicate = state.records.clone();
    duplicate.push(duplicate[0].clone());
    assert!(wire::decode(&wire::encode(state.clock, &duplicate)).is_err());
    duplicate.truncate(1);
    duplicate[0].scope.clear();
    assert!(wire::decode(&wire::encode(state.clock, &duplicate)).is_err());
    duplicate[0].scope = b"scope".to_vec();
    duplicate[0].stale = 1;
    assert!(wire::decode(&wire::encode(state.clock, &duplicate)).is_err());
}

#[test]
fn private_directory_identity_symlinks_and_foreign_files_are_refused() {
    let temp = Temp::new();
    for app in ["", "a b", "a/b"] {
        assert!(
            ServerCachePersistence::open(temp.path(), app, ServerCacheDiskLimits::default())
                .is_err()
        );
    }
    assert!(
        ServerCachePersistence::open("relative", "fixture", ServerCacheDiskLimits::default())
            .is_err()
    );
    let foreign = temp.0.join("foreign");
    fs::create_dir(&foreign).unwrap();
    use std::os::unix::fs::{PermissionsExt, symlink};
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(foreign.join("user-data"), b"keep").unwrap();
    assert!(
        ServerCachePersistence::open(&foreign, "fixture", ServerCacheDiskLimits::default())
            .is_err()
    );
    assert_eq!(fs::read(foreign.join("user-data")).unwrap(), b"keep");
    let disk = temp.open();
    drop(disk);
    let alias = temp.0.join("alias");
    symlink(temp.path(), &alias).unwrap();
    assert!(
        ServerCachePersistence::open(alias, "fixture", ServerCacheDiskLimits::default()).is_err()
    );
    let store = temp.path().join("store.bin");
    fs::remove_file(&store).unwrap();
    symlink(foreign.join("user-data"), &store).unwrap();
    assert!(
        ServerCachePersistence::open(temp.path(), "fixture", ServerCacheDiskLimits::default())
            .is_err()
    );
    assert_eq!(fs::read(foreign.join("user-data")).unwrap(), b"keep");
}

#[test]
fn directory_ownership_is_exclusive_and_clean_close_allows_restoration() {
    let temp = Temp::new();
    let disk = temp.open();
    put(&disk, "/a", None);
    assert_eq!(
        ServerCachePersistence::open(temp.path(), "fixture", ServerCacheDiskLimits::default())
            .err(),
        Some(ServerCacheError::PersistenceBusy)
    );
    assert!(
        ServerCachePersistence::open(temp.path(), "other", ServerCacheDiskLimits::default())
            .is_err()
    );
    drop(disk);
    assert!(!temp.path().join("LOCK").exists());
    let disk = temp.open();
    assert!(disk.store.get(&identity(), "/a").unwrap().is_some());
    disk.store.close().unwrap();
    let next = temp.open();
    disk.store.close().unwrap();
    drop(disk);
    assert!(temp.path().join("LOCK").is_file());
    assert!(next.store.get(&identity(), "/a").unwrap().is_some());
}

#[test]
fn disk_limits_are_independent_and_evict_old_publications_without_exceeding_bytes() {
    for (entries, bytes) in [
        (0, 32),
        (129, 32),
        (1, 31),
        (1, MAX_SERVER_CACHE_DISK_BYTES + 1),
    ] {
        assert!(ServerCacheDiskLimits::new(entries, bytes).is_err());
    }
    let temp = Temp::new();
    let disk = ServerCachePersistence::open(
        temp.path(),
        "fixture",
        ServerCacheDiskLimits::new(2, 4096).unwrap(),
    )
    .unwrap();
    for path in ["/a", "/b", "/c"] {
        put(&disk, path, None);
    }
    assert!(disk.store.get(&identity(), "/a").unwrap().is_none());
    assert_eq!(disk.store.stats().unwrap().0, 2);
    assert_eq!(
        fs::metadata(temp.path().join("store.bin")).unwrap().len() as usize,
        disk.store.stats().unwrap().1
    );
    drop(disk);
    let disk = ServerCachePersistence::open(
        temp.path(),
        "fixture",
        ServerCacheDiskLimits::new(1, 32).unwrap(),
    )
    .unwrap();
    put(&disk, "/a", None);
    assert_eq!(disk.store.stats().unwrap().0, 0);
    assert_eq!(disk.store.stats().unwrap().1, 24);
}

#[test]
fn acknowledged_path_and_tag_removal_survives_reopening_without_loading_records() {
    for tag in [false, true] {
        let temp = Temp::new();
        let disk = temp.open();
        put(&disk, "/a", None);
        put(&disk, "/b", None);
        drop(disk);
        let disk = temp.open();
        disk.store
            .invalidate(|url, tags| {
                if tag {
                    tags.iter().any(|tag| tag == "articles")
                } else {
                    url == "/a"
                }
            })
            .unwrap();
        drop(disk);
        let disk = temp.open();
        assert!(disk.store.get(&identity(), "/a").unwrap().is_none());
        assert_eq!(disk.store.get(&identity(), "/b").unwrap().is_none(), tag);
    }
}

#[test]
fn a_failed_transaction_leaves_an_unavailable_owner_and_requires_empty_recovery() {
    let temp = Temp::new();
    let disk = temp.open();
    put(&disk, "/a", None);
    create(&temp.path().join("next.bin")).unwrap();
    assert!(disk.store.invalidate(|_, _| true).is_err());
    assert!(!disk.store.available());
    assert!(disk.store.close().is_err());
    drop(disk);
    assert!(temp.path().join("LOCK").exists());
    assert_eq!(
        ServerCachePersistence::open(temp.path(), "fixture", ServerCacheDiskLimits::default())
            .err(),
        Some(ServerCacheError::PersistenceBusy)
    );
    let disk = ServerCachePersistence::recover_empty(
        temp.path(),
        "fixture",
        ServerCacheDiskLimits::default(),
    )
    .unwrap();
    assert!(disk.store.get(&identity(), "/a").unwrap().is_none());
    assert!(!temp.path().join("pending").exists());
}

#[test]
fn expired_future_and_corrupt_catalogs_are_reloaded_conservatively() {
    let temp = Temp::new();
    let disk = temp.open();
    put(&disk, "/a", Some(1));
    {
        let mut state = disk.store.state.lock().unwrap();
        state.records[0].published = clock().unwrap() - 2000;
        disk.store.commit(&mut state).unwrap();
    }
    assert!(disk.store.get(&identity(), "/a").unwrap().is_none());
    drop(disk);
    let disk = temp.open();
    put(&disk, "/a", None);
    drop(disk);
    let file = temp.path().join("store.bin");
    let bytes = fs::read(&file).unwrap();
    let (saved, records) = wire::decode(&bytes).unwrap();
    fs::write(&file, wire::encode(saved + 60_000, &records)).unwrap();
    let disk = temp.open();
    assert_eq!(disk.store.stats().unwrap().0, 0);
    drop(disk);
    fs::write(&file, b"torn catalog").unwrap();
    let disk = temp.open();
    assert_eq!(disk.store.stats().unwrap().0, 0);
}

#[test]
fn restored_freshness_uses_the_remaining_lifetime_and_clock_rollback_clears_entries() {
    let temp = Temp::new();
    let disk = temp.open();
    put(&disk, "/a", Some(3));
    {
        let mut state = disk.store.state.lock().unwrap();
        state.records[0].published = clock().unwrap() - 2000;
    }
    let restored = disk.store.get(&identity(), "/a").unwrap().unwrap();
    assert!(
        restored
            .expires
            .unwrap()
            .saturating_duration_since(Instant::now())
            <= Duration::from_secs(1)
    );
    {
        disk.store.state.lock().unwrap().clock = clock().unwrap() + 10_000;
    }
    assert!(disk.store.get(&identity(), "/a").unwrap().is_none());
}
