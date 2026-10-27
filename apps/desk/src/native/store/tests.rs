use super::*;
use std::{fs, sync::Arc};
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        Self(parent.join(format!(
            "ruvane-desk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn input(title: &str) -> TicketInput {
    TicketInput::new(title, "A persistent description\nwith two lines", "open").unwrap()
}
fn condition(row: &Snapshot) -> IfMatch {
    IfMatch::parse(row.tag.as_str()).unwrap()
}
#[test]
fn restart_retains_business_data_and_invalidates_old_tags() {
    let dir = Temp::new();
    let store = Store::open(&dir.0, false).unwrap();
    let a = store.create(input("First")).unwrap();
    let b = store
        .update(
            a.ticket.id,
            TicketInput::new("Updated", "details", "in_progress").unwrap(),
            Some(&condition(&a)),
        )
        .unwrap();
    assert_eq!(b.ticket.revision, 2);
    assert!(!a.tag.strong_eq(&b.tag));
    drop(store);
    assert!(!dir.0.join("LOCK").exists());
    let store = Store::open(&dir.0, false).unwrap();
    let loaded = store.get(1).unwrap();
    assert!(loaded.ticket == b.ticket);
    assert!(!loaded.tag.strong_eq(&b.tag));
    assert!(matches!(
        store.update(1, input("stale"), Some(&condition(&b))),
        Err(StoreError::Conflict)
    ));
}
#[test]
fn two_writers_cannot_overwrite_one_version() {
    let dir = Temp::new();
    let store = Arc::new(Store::open(&dir.0, false).unwrap());
    let row = store.create(input("Initial")).unwrap();
    let gate = Arc::new(std::sync::Barrier::new(3));
    let handles = ["Tab A", "Tab B"].map(|title| {
        let s = store.clone();
        let c = condition(&row);
        let g = gate.clone();
        std::thread::spawn(move || {
            g.wait();
            s.update(1, input(title), Some(&c))
        })
    });
    gate.wait();
    let results = handles.map(|h| h.join().unwrap());
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StoreError::Conflict)))
            .count(),
        1
    );
    assert_eq!(store.get(1).unwrap().ticket.revision, 2);
}
#[test]
fn exact_precondition_noop_and_capacity() {
    let dir = Temp::new();
    let store = Store::open(&dir.0, false).unwrap();
    let row = store.create(input("One")).unwrap();
    for c in [None, Some(IfMatch::parse("*").unwrap())] {
        assert!(matches!(
            store.update(1, input("Two"), c.as_ref()),
            Err(StoreError::Precondition)
        ));
    }
    let weak = IfMatch::parse(&format!("W/{}", row.tag.as_str())).unwrap();
    assert!(matches!(
        store.update(1, input("Two"), Some(&weak)),
        Err(StoreError::Conflict)
    ));
    let same = store
        .update(1, input("One"), Some(&condition(&row)))
        .unwrap();
    assert_eq!(same.ticket.revision, 1);
    assert!(same.tag.strong_eq(&row.tag));
    for i in 2..=MAX_TICKETS {
        assert_eq!(
            store
                .create(input(&format!("Ticket {i}")))
                .unwrap()
                .ticket
                .id,
            i as u32
        );
    }
    assert!(matches!(store.create(input("Full")), Err(StoreError::Full)));
    assert_eq!(store.list().unwrap().len(), MAX_TICKETS);
}
#[test]
fn wire_is_bounded_and_corruption_is_preserved() {
    let rows = vec![Ticket {
        id: 1,
        revision: 1,
        input: input("Hello"),
    }];
    let bytes = encode(&rows).unwrap();
    assert!(decode(&bytes).unwrap() == rows);
    for i in 0..bytes.len() {
        let mut broken = bytes.clone();
        broken[i] ^= 1;
        assert!(decode(&broken).is_err());
    }
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end]).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode(&extra).is_err());
    let mut duplicate = rows.clone();
    duplicate.push(rows[0].clone());
    assert!(encode(&duplicate).is_err());
    let dir = Temp::new();
    drop(Store::open(&dir.0, false).unwrap());
    fs::write(dir.0.join("store.bin"), b"bad").unwrap();
    assert!(matches!(
        Store::open(&dir.0, true),
        Err(StoreError::Corrupt)
    ));
    assert_eq!(fs::read(dir.0.join("store.bin")).unwrap(), b"bad");
}
#[test]
fn publication_failures_require_offline_recovery() {
    for point in 1..=3 {
        let dir = Temp::new();
        let store = Store::open(&dir.0, false).unwrap();
        let row = store.create(input("Before")).unwrap();
        store.disk.fault.store(point, Ordering::Relaxed);
        let failure = store.update(1, input("After"), Some(&condition(&row)));
        assert!(matches!(
            failure,
            Err(StoreError::Unavailable | StoreError::Uncertain)
        ));
        assert!(matches!(
            store.create(input("Blocked")),
            Err(StoreError::Uncertain)
        ));
        drop(store);
        assert!(dir.0.join("LOCK").exists());
        assert!(matches!(Store::open(&dir.0, false), Err(StoreError::Busy)));
        let recovered = Store::open(&dir.0, true).unwrap();
        assert_eq!(
            recovered.get(1).unwrap().ticket.input.title,
            if point == 3 { "After" } else { "Before" }
        );
        assert!(!dir.0.join("next.bin").exists());
    }
}
#[test]
fn rejects_live_recovery_and_foreign_files() {
    let dir = Temp::new();
    let store = Store::open(&dir.0, false).unwrap();
    assert!(matches!(Store::open(&dir.0, false), Err(StoreError::Busy)));
    assert!(matches!(Store::open(&dir.0, true), Err(StoreError::Busy)));
    drop(store);
    fs::write(dir.0.join("foreign.txt"), b"leave me").unwrap();
    assert!(matches!(
        Store::open(&dir.0, true),
        Err(StoreError::Corrupt)
    ));
    assert_eq!(fs::read(dir.0.join("foreign.txt")).unwrap(), b"leave me");
}
#[cfg(unix)]
#[test]
fn private_files_and_link_rejection() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    let dir = Temp::new();
    drop(Store::open(&dir.0, false).unwrap());
    assert_eq!(fs::metadata(&dir.0).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(dir.0.join("store.bin")).unwrap().mode() & 0o777,
        0o600
    );
    fs::set_permissions(dir.0.join("store.bin"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Store::open(&dir.0, false),
        Err(StoreError::Corrupt)
    ));
    fs::set_permissions(dir.0.join("store.bin"), fs::Permissions::from_mode(0o600)).unwrap();
    let other = Temp::new();
    fs::hard_link(dir.0.join("store.bin"), &other.0).unwrap();
    assert!(matches!(
        Store::open(&dir.0, false),
        Err(StoreError::Corrupt)
    ));
    fs::remove_file(&other.0).unwrap();
    fs::rename(dir.0.join("store.bin"), dir.0.join("original")).unwrap();
    symlink(dir.0.join("original"), dir.0.join("store.bin")).unwrap();
    assert!(matches!(
        Store::open(&dir.0, false),
        Err(StoreError::Corrupt)
    ));
}
