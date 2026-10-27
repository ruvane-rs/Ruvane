use super::*;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        loop {
            let path = parent.join(format!(
                "ruvane-files-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{error}"),
            }
        }
    }
    fn root(&self) -> PathBuf {
        self.0.join("files")
    }
    fn open(&self) -> FileStore {
        FileStore::open(self.root(), "test", FileStoreLimits::default()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn owner(value: &str) -> FileOwner {
    FileOwner::new(value).unwrap()
}
fn upload(name: &str, bytes: &[u8]) -> UploadedFile {
    let name = name.replace('"', "\\\"");
    let mut body = format!("--x\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"{name}\"\r\nContent-Type: image/png\r\n\r\n").into_bytes();
    body.extend(bytes);
    body.extend(b"\r\n--x--\r\n");
    crate::MultipartData::parse("multipart/form-data; boundary=x", &body)
        .unwrap()
        .file()
        .unwrap()
        .clone()
}
#[test]
fn identifiers_owners_and_debug_never_become_paths_or_credentials() {
    for value in ["", "../x", "a/b", "é", "account id", &"x".repeat(65)] {
        assert_eq!(FileOwner::new(value), Err(FileStoreError::InvalidInput));
    }
    for value in [
        "",
        "../x",
        &"A".repeat(64),
        &"0".repeat(64),
        &"f".repeat(65),
    ] {
        assert_eq!(FileId::parse(value), Err(crate::FileMetadataError));
    }
    let id = FileId::parse(&"a1".repeat(32)).unwrap();
    assert_eq!(id.to_string(), "a1".repeat(32));
    assert!(!format!("{:?}{id:?}", owner("PRIVATE_OWNER")).contains("PRIVATE_OWNER"));
    assert!(!format!("{id:?}").contains("a1a1"));
}
#[test]
fn private_binary_empty_files_and_deletions_survive_clean_restarts() {
    let temp = Temp::new();
    let a = owner("alpha");
    let b = owner("beta");
    let store = temp.open();
    let original = upload("PRIVATE 🦀 \".bin", b"\0\xff\r\n\x80");
    let info = store.save(&a, &original).unwrap();
    let empty = store.save(&b, &upload("empty.bin", b"")).unwrap();
    assert_eq!(
        store.read(&a, info.id()).unwrap().unwrap().bytes(),
        original.bytes()
    );
    assert!(store.read(&b, info.id()).unwrap().is_none());
    assert!(!store.remove(&b, info.id()).unwrap());
    assert!(empty.is_empty());
    assert_eq!(store.list(&a).unwrap(), vec![info.clone()]);
    let loaded = store.read(&a, info.id()).unwrap().unwrap();
    assert!(!format!("{loaded:?}{store:?}{info:?}").contains("PRIVATE"));
    assert_eq!(fs::metadata(temp.root()).unwrap().mode() & 0o777, 0o700);
    for name in ["identity", "LOCK", "store.bin"] {
        assert_eq!(
            fs::metadata(temp.root().join(name)).unwrap().mode() & 0o777,
            0o600
        );
    }
    let cloned = store.clone();
    drop(store);
    assert!(matches!(
        FileStore::open(temp.root(), "test", FileStoreLimits::default()),
        Err(FileStoreError::Busy)
    ));
    assert!(matches!(
        FileStore::recover(temp.root(), "test", FileStoreLimits::default()),
        Err(FileStoreError::Busy)
    ));
    drop(cloned);
    let store = temp.open();
    assert_eq!(
        store.read(&a, info.id()).unwrap().unwrap().bytes(),
        original.bytes()
    );
    assert!(store.remove(&a, info.id()).unwrap());
    assert!(!store.remove(&a, info.id()).unwrap());
    drop(store);
    let store = temp.open();
    assert!(store.list(&a).unwrap().is_empty());
    assert_eq!(store.list(&b).unwrap(), vec![empty]);
}
#[test]
fn global_and_owner_quotas_are_checked_before_publication() {
    let temp = Temp::new();
    let limits = FileStoreLimits::new(2, 5, 1, 4).unwrap();
    let store = FileStore::open(temp.root(), "test", limits).unwrap();
    store.save(&owner("a"), &upload("one", b"1234")).unwrap();
    let before = fs::read(temp.root().join("store.bin")).unwrap();
    assert_eq!(
        store.save(&owner("a"), &upload("two", b"")).unwrap_err(),
        FileStoreError::Limit
    );
    assert_eq!(
        store.save(&owner("b"), &upload("two", b"12")).unwrap_err(),
        FileStoreError::Limit
    );
    assert_eq!(fs::read(temp.root().join("store.bin")).unwrap(), before);
    store.save(&owner("b"), &upload("two", b"1")).unwrap();
    assert_eq!(
        store.save(&owner("c"), &upload("three", b"")).unwrap_err(),
        FileStoreError::Limit
    );
    drop(store);
    assert!(matches!(
        FileStore::open(
            temp.root(),
            "test",
            FileStoreLimits::new(1, 5, 1, 4).unwrap()
        ),
        Err(FileStoreError::Limit)
    ));
    let store = FileStore::open(temp.root(), "test", limits).unwrap();
    assert_eq!(store.list(&owner("b")).unwrap().len(), 1);
    for args in [
        (0, 1, 1, 1),
        (257, 1, 1, 1),
        (1, 0, 1, 1),
        (1, MAX_BYTES + 1, 1, 1),
        (1, 1, 2, 1),
        (1, 1, 1, 2),
    ] {
        assert!(FileStoreLimits::new(args.0, args.1, args.2, args.3).is_err());
    }
}
#[test]
fn concurrent_writers_cannot_exceed_a_shared_owner_quota() {
    let temp = Temp::new();
    let store = FileStore::open(
        temp.root(),
        "test",
        FileStoreLimits::new(8, 1024, 2, 1024).unwrap(),
    )
    .unwrap();
    let threads = (0..16)
        .map(|_| {
            let store = store.clone();
            std::thread::spawn(move || store.save(&owner("a"), &upload("x", b"binary")))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        2
    );
    assert_eq!(store.list(&owner("a")).unwrap().len(), 2);
}
#[test]
fn unsafe_directories_foreign_entries_and_links_are_refused_without_cleanup() {
    let temp = Temp::new();
    assert!(FileStore::open("relative", "test", FileStoreLimits::default()).is_err());
    fs::write(temp.0.join("foreign"), b"keep").unwrap();
    assert!(FileStore::open(&temp.0, "test", FileStoreLimits::default()).is_err());
    assert_eq!(fs::read(temp.0.join("foreign")).unwrap(), b"keep");
    let store = temp.open();
    drop(store);
    let original = fs::read(temp.root().join("store.bin")).unwrap();
    assert!(FileStore::open(temp.root(), "other", FileStoreLimits::default()).is_err());
    fs::set_permissions(temp.root(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(FileStore::open(temp.root(), "test", FileStoreLimits::default()).is_err());
    fs::set_permissions(temp.root(), fs::Permissions::from_mode(0o700)).unwrap();
    symlink(temp.root(), temp.0.join("alias")).unwrap();
    assert!(FileStore::open(temp.0.join("alias"), "test", FileStoreLimits::default()).is_err());
    fs::hard_link(temp.root().join("store.bin"), temp.0.join("linked")).unwrap();
    assert!(FileStore::open(temp.root(), "test", FileStoreLimits::default()).is_err());
    fs::remove_file(temp.0.join("linked")).unwrap();
    fs::rename(temp.root().join("store.bin"), temp.0.join("saved")).unwrap();
    symlink(temp.0.join("saved"), temp.root().join("store.bin")).unwrap();
    assert!(FileStore::open(temp.root(), "test", FileStoreLimits::default()).is_err());
    fs::remove_file(temp.root().join("store.bin")).unwrap();
    fs::rename(temp.0.join("saved"), temp.root().join("store.bin")).unwrap();
    fs::write(temp.root().join("foreign"), b"keep").unwrap();
    assert!(FileStore::recover(temp.root(), "test", FileStoreLimits::default()).is_err());
    fs::remove_file(temp.root().join("foreign")).unwrap();
    assert_eq!(fs::read(temp.root().join("store.bin")).unwrap(), original);
    let _store = temp.open();
}
#[test]
fn damaged_committed_data_never_becomes_an_empty_store() {
    let temp = Temp::new();
    let store = temp.open();
    store.save(&owner("a"), &upload("x", b"PRIVATE")).unwrap();
    drop(store);
    let path = temp.root().join("store.bin");
    let good = fs::read(&path).unwrap();
    for length in 0..good.len() {
        fs::write(&path, &good[..length]).unwrap();
        assert!(matches!(
            FileStore::open(temp.root(), "test", FileStoreLimits::default()),
            Err(FileStoreError::Corrupt)
        ));
        assert_eq!(fs::read(&path).unwrap(), good[..length]);
    }
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        FileStore::open(temp.root(), "test", FileStoreLimits::default()),
        Err(FileStoreError::Corrupt)
    ));
    fs::write(&path, &good).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let _store = temp.open();
}
#[test]
fn codec_checks_crc_structure_and_all_metadata_before_adoption() {
    assert_eq!(wire::crc32(b"123456789"), 0xcbf43926);
    let temp = Temp::new();
    let store = temp.open();
    store.save(&owner("a"), &upload("x", b"\0\xff")).unwrap();
    let bytes = fs::read(temp.root().join("store.bin")).unwrap();
    for index in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[index] ^= 1;
        assert!(wire::decode(&bad, "test", FileStoreLimits::default()).is_err());
    }
    for (index, value) in [
        (9, 255),
        (45, 255),
        (46, b'/'),
        (47, 0),
        (49, 255),
        (50, 255),
    ] {
        let mut bad = bytes.clone();
        bad[index] = value;
        let end = bad.len() - 4;
        let crc = wire::crc32(&bad[..end]);
        bad[end..].copy_from_slice(&crc.to_le_bytes());
        assert!(wire::decode(&bad, "test", FileStoreLimits::default()).is_err());
    }
    assert!(wire::decode(&bytes, "foreign", FileStoreLimits::default()).is_err());
}
#[test]
fn publication_failures_poison_the_handle_and_recovery_selects_only_the_committed_snapshot() {
    for point in 1..=3 {
        let temp = Temp::new();
        let store = temp.open();
        let a = owner("a");
        store.save(&a, &upload("old", b"old")).unwrap();
        store.0.disk.fault.store(point, Ordering::Relaxed);
        assert_eq!(
            store.save(&a, &upload("new", b"new")).unwrap_err(),
            if point == 3 {
                FileStoreError::Uncertain
            } else {
                FileStoreError::Unavailable
            }
        );
        assert_eq!(store.list(&a).unwrap_err(), FileStoreError::Unavailable);
        assert!(matches!(
            FileStore::recover(temp.root(), "test", FileStoreLimits::default()),
            Err(FileStoreError::Busy)
        ));
        drop(store);
        assert!(matches!(
            FileStore::open(temp.root(), "test", FileStoreLimits::default()),
            Err(FileStoreError::Busy)
        ));
        let store = FileStore::recover(temp.root(), "test", FileStoreLimits::default()).unwrap();
        assert_eq!(
            store.list(&a).unwrap().len(),
            if point == 3 { 2 } else { 1 }
        );
        assert!(!temp.root().join("next.bin").exists());
    }
}
#[test]
fn recovery_preserves_corruption_and_rejected_names_instead_of_deleting_them() {
    let temp = Temp::new();
    let store = temp.open();
    store.save(&owner("a"), &upload("x", b"x")).unwrap();
    store.0.disk.fault.store(2, Ordering::Relaxed);
    let _ = store.save(&owner("a"), &upload("y", b"y"));
    drop(store);
    let path = temp.root().join("store.bin");
    fs::write(&path, b"broken").unwrap();
    assert!(matches!(
        FileStore::recover(temp.root(), "test", FileStoreLimits::default()),
        Err(FileStoreError::Corrupt)
    ));
    assert!(temp.root().join("LOCK").exists() && temp.root().join("next.bin").exists());
    assert_eq!(fs::read(path).unwrap(), b"broken");
}
#[test]
fn uncertain_deletion_does_not_resurrect_after_recovery() {
    let temp = Temp::new();
    let store = temp.open();
    let a = owner("a");
    let info = store.save(&a, &upload("x", b"x")).unwrap();
    store.0.disk.fault.store(3, Ordering::Relaxed);
    assert!(!store.remove(&owner("b"), info.id()).unwrap());
    assert_eq!(store.list(&a).unwrap().len(), 1);
    assert_eq!(store.remove(&a, info.id()), Err(FileStoreError::Uncertain));
    drop(store);
    let store = FileStore::recover(temp.root(), "test", FileStoreLimits::default()).unwrap();
    assert!(store.list(&a).unwrap().is_empty());
}
#[test]
fn random_source_failure_and_repeated_identifiers_never_overwrite_an_existing_file() {
    use std::collections::VecDeque;
    let temp = Temp::new();
    let store = temp.open();
    let a = owner("a");
    let info = store.save(&a, &upload("x", b"original")).unwrap();
    for candidate in [[0; 32], info.id.0] {
        store.0.state.lock().unwrap().entropy =
            Entropy::Scripted(VecDeque::from(vec![Ok(candidate); 8]));
        assert_eq!(
            store.save(&a, &upload("y", b"replace")).unwrap_err(),
            FileStoreError::Unavailable
        );
        assert_eq!(
            store.read(&a, info.id()).unwrap().unwrap().bytes(),
            b"original"
        );
    }
    store.0.state.lock().unwrap().entropy = Entropy::Scripted(VecDeque::from([Err(
        crate::SessionError::EntropyUnavailable,
    )]));
    assert_eq!(
        store.save(&a, &upload("z", b"z")).unwrap_err(),
        FileStoreError::Unavailable
    );
}

fn at(store: &FileStore, seconds: u64) {
    store.0.time.store(seconds, Ordering::Relaxed);
}
const OP_TIME: u64 = 2_000_000_000;
#[test]
fn concurrent_replays_return_one_durable_file_and_no_foreign_result() {
    let temp = Temp::new();
    let store = temp.open();
    at(&store, OP_TIME);
    let a = owner("alpha");
    let operation = store.upload_id(&a).unwrap();
    let before = fs::read(temp.root().join("store.bin")).unwrap();
    assert!(matches!(
        store.upload_status(&a, operation).unwrap().state(),
        FileUploadState::Unconfirmed
    ));
    assert_eq!(fs::read(temp.root().join("store.bin")).unwrap(), before);
    let file = upload("exact.bin", b"\0\xff\r\n");
    let handles = (0..12)
        .map(|_| {
            let store = store.clone();
            let a = a.clone();
            let file = file.clone();
            std::thread::spawn(move || store.save_once(&a, operation, &file).unwrap())
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().all(|info| info == &results[0]));
    assert_eq!(store.list(&a).unwrap().len(), 1);
    let bytes = fs::read(temp.root().join("store.bin")).unwrap();
    assert_eq!(store.save_once(&a, operation, &file).unwrap(), results[0]);
    assert_eq!(fs::read(temp.root().join("store.bin")).unwrap(), bytes);
    assert!(matches!(
        store
            .upload_status(&owner("beta"), operation)
            .unwrap()
            .state(),
        FileUploadState::Expired
    ));
    drop(store);
    let store = temp.open();
    at(&store, OP_TIME + 10);
    assert_eq!(store.save_once(&a, operation, &file).unwrap(), results[0]);
    assert_eq!(
        store.upload_status(&a, operation).unwrap().state(),
        &FileUploadState::Stored(results[0].clone())
    );
}
#[test]
fn changed_input_and_deleted_results_cannot_reuse_an_operation() {
    let temp = Temp::new();
    let store = temp.open();
    at(&store, OP_TIME);
    let a = owner("alpha");
    let operation = store.upload_id(&a).unwrap();
    let file = upload("x.bin", b"abc");
    let info = store.save_once(&a, operation, &file).unwrap();
    let snapshot = fs::read(temp.root().join("store.bin")).unwrap();
    for file in [
        upload("other.bin", b"abc"),
        upload("x.bin", b"abd"),
        upload("x.bin", b"ab"),
    ] {
        assert_eq!(
            store.save_once(&a, operation, &file),
            Err(FileStoreError::OperationConflict)
        );
    }
    assert_eq!(fs::read(temp.root().join("store.bin")).unwrap(), snapshot);
    store.remove(&a, info.id()).unwrap();
    assert_eq!(
        store.upload_status(&a, operation).unwrap().state(),
        &FileUploadState::Removed
    );
    assert_eq!(
        store.save_once(&a, operation, &file),
        Err(FileStoreError::OperationConflict)
    );
    assert!(store.list(&a).unwrap().is_empty());
    drop(store);
    let store = temp.open();
    at(&store, OP_TIME + 5);
    assert_eq!(
        store.save_once(&a, operation, &file),
        Err(FileStoreError::OperationConflict)
    );
}
#[test]
fn expiration_pruning_and_durable_clock_floor_prevent_old_key_reexecution() {
    let temp = Temp::new();
    let store = temp.open();
    at(&store, OP_TIME);
    let a = owner("alpha");
    let operation = store.upload_id(&a).unwrap();
    let file = upload("x", b"");
    store.save_once(&a, operation, &file).unwrap();
    let future = FileUploadId {
        issued: OP_TIME + 1,
        random: [1; 32],
    };
    assert_eq!(
        store.save_once(&a, future, &file),
        Err(FileStoreError::OperationExpired)
    );
    at(&store, operation.expires_at());
    assert_eq!(
        store.save_once(&a, operation, &file),
        Err(FileStoreError::OperationExpired)
    );
    assert_eq!(
        store.upload_status(&a, operation).unwrap().state(),
        &FileUploadState::Expired
    );
    store.save(&a, &file).unwrap();
    assert!(store.state().unwrap().operations.is_empty());
    at(&store, OP_TIME);
    assert_eq!(
        store.save_once(&a, operation, &file),
        Err(FileStoreError::Unavailable)
    );
    drop(store);
    let store = temp.open();
    at(&store, OP_TIME);
    assert_eq!(store.upload_id(&a), Err(FileStoreError::Unavailable));
    at(&store, operation.expires_at() + 1);
    assert_eq!(
        store.save_once(&a, operation, &file),
        Err(FileStoreError::OperationExpired)
    );
    assert_eq!(store.list(&a).unwrap().len(), 2);
}
#[test]
fn operation_quotas_never_evict_live_results_and_expiry_releases_capacity() {
    let temp = Temp::new();
    let store = temp.open();
    at(&store, OP_TIME);
    let file = upload("x", b"");
    let mut first = None;
    for account in 0..8 {
        let owner = owner(&format!("account{account}"));
        for n in 0..32 {
            let operation = store.upload_id(&owner).unwrap();
            let info = store.save_once(&owner, operation, &file).unwrap();
            if account == 0 && n == 0 {
                first = Some((owner.clone(), operation, info));
            } else {
                store.remove(&owner, info.id()).unwrap();
            }
        }
        if account == 0 {
            assert_eq!(store.upload_id(&owner), Err(FileStoreError::Limit));
        }
    }
    let (a, operation, info) = first.unwrap();
    assert_eq!(store.save_once(&a, operation, &file).unwrap(), info);
    assert_eq!(
        store.upload_id(&owner("new-owner")),
        Err(FileStoreError::Limit)
    );
    at(&store, OP_TIME + crate::FILE_UPLOAD_RETENTION_SECONDS);
    store
        .save_once(&a, store.upload_id(&a).unwrap(), &file)
        .unwrap();
    assert_eq!(store.state().unwrap().operations.len(), 1);
}
#[test]
fn file_and_operation_recover_from_the_same_committed_snapshot() {
    for point in 1..=3 {
        let temp = Temp::new();
        let store = temp.open();
        at(&store, OP_TIME);
        let a = owner("alpha");
        let operation = store.upload_id(&a).unwrap();
        let file = upload("x", b"abc");
        store.0.disk.fault.store(point, Ordering::Relaxed);
        assert_eq!(
            store.save_once(&a, operation, &file),
            Err(if point == 3 {
                FileStoreError::Uncertain
            } else {
                FileStoreError::Unavailable
            })
        );
        drop(store);
        let store = FileStore::recover(temp.root(), "test", Default::default()).unwrap();
        at(&store, OP_TIME + 1);
        assert_eq!(store.list(&a).unwrap().len(), usize::from(point == 3));
        assert_eq!(
            matches!(
                store.upload_status(&a, operation).unwrap().state(),
                FileUploadState::Stored(_)
            ),
            point == 3
        );
        let operation = if point == 3 {
            operation
        } else {
            assert_eq!(
                store.save_once(&a, operation, &file),
                Err(FileStoreError::OperationExpired)
            );
            store.upload_id(&a).unwrap()
        };
        let info = store.save_once(&a, operation, &file).unwrap();
        assert_eq!(store.save_once(&a, operation, &file).unwrap(), info);
        assert_eq!(store.list(&a).unwrap().len(), 1);
    }
}
#[test]
fn legacy_snapshots_upgrade_only_on_a_successful_write_and_operation_codec_fails_closed() {
    let temp = Temp::new();
    let store = temp.open();
    let a = owner("alpha");
    let original = store.save(&a, &upload("old.bin", b"old")).unwrap();
    let old = wire::encode(&store.state().unwrap().records, "test");
    drop(store);
    fs::write(temp.root().join("store.bin"), &old).unwrap();
    let store = temp.open();
    assert_eq!(store.list(&a).unwrap(), vec![original.clone()]);
    assert_eq!(fs::read(temp.root().join("store.bin")).unwrap(), old);
    let operation = store.upload_id(&a).unwrap();
    store
        .save_once(&a, operation, &upload("new.bin", b"new"))
        .unwrap();
    let bytes = fs::read(temp.root().join("store.bin")).unwrap();
    assert_eq!(&bytes[..4], b"RFS2");
    for n in 0..bytes.len() {
        assert!(wire::decode(&bytes[..n], "test", Default::default()).is_err());
    }
    let snapshot = wire::decode_snapshot(&bytes, "test", Default::default()).unwrap();
    assert_eq!(snapshot.operations.len(), 1);
    let check = |operations: &Operations, clock| {
        assert!(
            wire::decode(
                &wire::encode_snapshot(&snapshot.records, operations, clock, "test"),
                "test",
                Default::default()
            )
            .is_err()
        )
    };
    check(&snapshot.operations, 0);
    check(&Operations::new(), 0);
    let mut operations = snapshot.operations.clone();
    let id = *operations.values().next().unwrap();
    operations.insert((owner("beta"), operation), id);
    check(&operations, snapshot.clock);
    let mut operations = Operations::new();
    operations.insert(
        (
            a,
            FileUploadId {
                issued: 0,
                random: [1; 32],
            },
        ),
        id,
    );
    check(&operations, snapshot.clock);
}

#[test]
fn pending_operations_cannot_be_stolen_or_overwritten_by_repeated_entropy() {
    use std::collections::VecDeque;
    let temp = Temp::new();
    let store = temp.open();
    at(&store, OP_TIME);
    let a = owner("alpha");
    let b = owner("beta");
    let operation = store.upload_id(&a).unwrap();
    assert_eq!(
        store.save_once(&b, operation, &upload("x", b"abc")),
        Err(FileStoreError::OperationExpired)
    );
    assert_eq!(
        store.upload_status(&b, operation).unwrap().state(),
        &FileUploadState::Expired
    );
    for candidate in [[0; 32], operation.random] {
        store.state().unwrap().entropy = Entropy::Scripted(VecDeque::from(vec![Ok(candidate); 8]));
        assert_eq!(store.upload_id(&b), Err(FileStoreError::Unavailable));
        assert_eq!(
            store.upload_status(&a, operation).unwrap().state(),
            &FileUploadState::Unconfirmed
        );
    }
    let forged = FileUploadId {
        issued: OP_TIME,
        random: [255; 32],
    };
    assert_eq!(
        store.save_once(&a, forged, &upload("x", b"abc")),
        Err(FileStoreError::OperationExpired)
    );
    assert!(store.list(&a).unwrap().is_empty());
}
