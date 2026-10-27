use super::*;
fn owner() -> Actor {
    Actor::parse("alex").unwrap()
}
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
    let a = store.create(owner(), 1, input("First")).unwrap();
    let b = store
        .update(
            owner(),
            1,
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
    let loaded = store.get(owner(), 1, 1).unwrap();
    assert!(loaded.ticket == b.ticket);
    assert!(!loaded.tag.strong_eq(&b.tag));
    assert!(matches!(
        store.update(owner(), 1, 1, input("stale"), Some(&condition(&b))),
        Err(StoreError::Conflict)
    ));
}
#[test]
fn two_writers_cannot_overwrite_one_version() {
    let dir = Temp::new();
    let store = Arc::new(Store::open(&dir.0, false).unwrap());
    let row = store.create(owner(), 1, input("Initial")).unwrap();
    let gate = Arc::new(std::sync::Barrier::new(3));
    let handles = ["Tab A", "Tab B"].map(|title| {
        let s = store.clone();
        let c = condition(&row);
        let g = gate.clone();
        std::thread::spawn(move || {
            g.wait();
            s.update(owner(), 1, 1, input(title), Some(&c))
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
    assert_eq!(store.get(owner(), 1, 1).unwrap().ticket.revision, 2);
}
#[test]
fn exact_precondition_noop_and_capacity() {
    let dir = Temp::new();
    let store = Store::open(&dir.0, false).unwrap();
    let row = store.create(owner(), 1, input("One")).unwrap();
    for c in [None, Some(IfMatch::parse("*").unwrap())] {
        assert!(matches!(
            store.update(owner(), 1, 1, input("Two"), c.as_ref()),
            Err(StoreError::Precondition)
        ));
    }
    let weak = IfMatch::parse(&format!("W/{}", row.tag.as_str())).unwrap();
    assert!(matches!(
        store.update(owner(), 1, 1, input("Two"), Some(&weak)),
        Err(StoreError::Conflict)
    ));
    let same = store
        .update(owner(), 1, 1, input("One"), Some(&condition(&row)))
        .unwrap();
    assert_eq!(same.ticket.revision, 1);
    assert!(same.tag.strong_eq(&row.tag));
    for i in 2..=MAX_TICKETS {
        assert_eq!(
            store
                .create(owner(), 1, input(&format!("Ticket {i}")))
                .unwrap()
                .ticket
                .id,
            i as u32
        );
    }
    assert!(matches!(
        store.create(owner(), 1, input("Full")),
        Err(StoreError::Full)
    ));
    assert_eq!(store.list(owner(), 1).unwrap().len(), MAX_TICKETS);
}
#[test]
fn wire_is_bounded_and_corruption_is_preserved() {
    let rows = vec![Ticket {
        id: 1,
        workspace: 1,
        revision: 1,
        input: input("Hello"),
    }];
    let bytes = wire::legacy_encode(&rows).unwrap();
    assert!(wire::decode(&bytes).unwrap().1 == rows);
    for i in 0..bytes.len() {
        let mut broken = bytes.clone();
        broken[i] ^= 1;
        assert!(wire::decode(&broken).is_err());
    }
    for end in 0..bytes.len() {
        assert!(wire::decode(&bytes[..end]).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(wire::decode(&extra).is_err());
    let mut duplicate = rows.clone();
    duplicate.push(rows[0].clone());
    assert!(wire::legacy_encode(&duplicate).is_err());
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
        let row = store.create(owner(), 1, input("Before")).unwrap();
        store.disk.fault.store(point, Ordering::Relaxed);
        let failure = store.update(owner(), 1, 1, input("After"), Some(&condition(&row)));
        assert!(matches!(
            failure,
            Err(StoreError::Unavailable | StoreError::Uncertain)
        ));
        assert!(matches!(
            store.create(owner(), 1, input("Blocked")),
            Err(StoreError::Uncertain)
        ));
        drop(store);
        assert!(dir.0.join("LOCK").exists());
        assert!(matches!(Store::open(&dir.0, false), Err(StoreError::Busy)));
        let recovered = Store::open(&dir.0, true).unwrap();
        assert_eq!(
            recovered.get(owner(), 1, 1).unwrap().ticket.input.title,
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

fn actor(key: &str) -> Actor {
    Actor::parse(key).unwrap()
}
#[test]
fn workspace_and_role_checks_cover_every_business_operation() {
    let dir = Temp::new();
    let s = Store::open(&dir.0, false).unwrap();
    let row = s
        .create(actor("blair"), 1, input("Ruvane private"))
        .unwrap();
    assert_eq!(
        s.get(actor("casey"), 1, row.ticket.id)
            .unwrap()
            .ticket
            .input
            .title,
        "Ruvane private"
    );
    assert!(matches!(
        s.create(actor("casey"), 1, input("Denied")),
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        s.update(
            actor("casey"),
            1,
            row.ticket.id,
            input("Denied"),
            Some(&condition(&row))
        ),
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(s.list(actor("drew"), 1), Err(StoreError::Missing)));
    assert!(matches!(
        s.get(owner(), 2, row.ticket.id),
        Err(StoreError::Missing)
    ));
    assert!(matches!(
        s.members(actor("blair"), 1),
        Err(StoreError::Forbidden)
    ));
    assert_eq!(
        s.spaces(actor("drew"))
            .unwrap()
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert!(matches!(
        s.with_ticket(
            actor("casey"),
            1,
            row.ticket.id,
            Access::Edit,
            |_, _| Ok(())
        ),
        Err(StoreError::Forbidden)
    ));
    assert!(
        s.with_ticket(actor("casey"), 1, row.ticket.id, Access::Read, |_, role| {
            assert_eq!(role, Role::Viewer);
            Ok(())
        })
        .is_ok()
    );
}
#[test]
fn membership_versions_last_owner_and_live_revocation_survive_restart() {
    let dir = Temp::new();
    let s = Store::open(&dir.0, false).unwrap();
    let row = s.create(owner(), 1, input("Protected")).unwrap();
    let current = s.members(owner(), 1).unwrap();
    assert!(matches!(
        s.set_members(
            owner(),
            1,
            [None; 4],
            Some(&IfMatch::parse(current.tag.as_str()).unwrap())
        ),
        Err(StoreError::InvalidInput)
    ));
    let mut members = current.space.members;
    members[1] = Some(Role::Viewer);
    members[2] = None;
    let changed = s
        .set_members(
            owner(),
            1,
            members,
            Some(&IfMatch::parse(current.tag.as_str()).unwrap()),
        )
        .unwrap();
    assert_eq!(changed.space.revision, 2);
    assert!(matches!(
        s.set_members(
            owner(),
            1,
            current.space.members,
            Some(&IfMatch::parse(current.tag.as_str()).unwrap())
        ),
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        s.update(
            actor("blair"),
            1,
            row.ticket.id,
            input("Denied"),
            Some(&condition(&row))
        ),
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        s.get(actor("casey"), 1, row.ticket.id),
        Err(StoreError::Missing)
    ));
    drop(s);
    let s = Store::open(&dir.0, false).unwrap();
    assert_eq!(
        s.space(actor("blair"), 1, Access::Read)
            .unwrap()
            .space
            .role(actor("blair")),
        Some(Role::Viewer)
    );
    assert!(matches!(
        s.get(actor("casey"), 1, row.ticket.id),
        Err(StoreError::Missing)
    ));
}
#[test]
fn file_authorization_and_revocation_are_serialized() {
    use std::sync::mpsc;
    let dir = Temp::new();
    let s = Arc::new(Store::open(&dir.0, false).unwrap());
    let row = s.create(owner(), 1, input("Attachment ticket")).unwrap();
    let initial = s.members(owner(), 1).unwrap();
    let tag = IfMatch::parse(initial.tag.as_str()).unwrap();
    let mut members = initial.space.members;
    members[1] = None;
    let (entered, seen) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let files = s.clone();
    let file = std::thread::spawn(move || {
        files.with_ticket(actor("blair"), 1, row.ticket.id, Access::Edit, |_, _| {
            entered.send(()).unwrap();
            resume.recv().unwrap();
            Ok(())
        })
    });
    seen.recv().unwrap();
    let (started, starting) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let roles = s.clone();
    let revoke = std::thread::spawn(move || {
        started.send(()).unwrap();
        let result = roles.set_members(owner(), 1, members, Some(&tag));
        done.send(()).unwrap();
        result
    });
    starting.recv().unwrap();
    assert!(
        finished
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err()
    );
    release.send(()).unwrap();
    file.join().unwrap().unwrap();
    revoke.join().unwrap().unwrap();
    let called = std::sync::atomic::AtomicBool::new(false);
    assert!(matches!(
        s.with_ticket(actor("blair"), 1, 1, Access::Edit, |_, _| {
            called.store(true, Ordering::Relaxed);
            Ok(())
        }),
        Err(StoreError::Missing)
    ));
    assert!(!called.load(Ordering::Relaxed));
}
#[test]
fn workspace_creation_is_bounded_and_preserves_ownership() {
    let dir = Temp::new();
    let s = Store::open(&dir.0, false).unwrap();
    assert!(matches!(
        s.create_space(actor("casey"), WorkspaceInput::new("No").unwrap()),
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        s.create_space(owner(), WorkspaceInput::new("Ruvane").unwrap()),
        Err(StoreError::InvalidInput)
    ));
    for id in 3..=MAX_WORKSPACES {
        let space = s
            .create_space(
                owner(),
                WorkspaceInput::new(&format!("Space {id}")).unwrap(),
            )
            .unwrap();
        assert_eq!(space.id, id);
        assert_eq!(space.role(owner()), Some(Role::Owner));
        assert_eq!(space.role(actor("drew")), None);
    }
    assert!(matches!(
        s.create_space(owner(), WorkspaceInput::new("Full").unwrap()),
        Err(StoreError::Full)
    ));
}
#[test]
fn migration_is_lazy_and_failed_publication_retains_legacy_data() {
    for fault in 0..=3 {
        let dir = Temp::new();
        drop(Store::open(&dir.0, false).unwrap());
        let old = vec![Ticket {
            id: 1,
            workspace: 1,
            revision: 17,
            input: input("Legacy ticket"),
        }];
        let bytes = wire::legacy_encode(&old).unwrap();
        fs::write(dir.0.join("store.bin"), &bytes).unwrap();
        let s = Store::open(&dir.0, false).unwrap();
        assert!(s.list(owner(), 1).unwrap() == old);
        assert_eq!(fs::read(dir.0.join("store.bin")).unwrap(), bytes);
        let current = s.get(owner(), 1, 1).unwrap();
        let condition = condition(&current);
        s.update(
            owner(),
            1,
            1,
            current.ticket.input.clone(),
            Some(&condition),
        )
        .unwrap();
        assert_eq!(fs::read(dir.0.join("store.bin")).unwrap(), bytes);
        s.disk.fault.store(fault, Ordering::Relaxed);
        let result = s.update(owner(), 1, 1, input("Migrated"), Some(&condition));
        if fault == 0 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
        drop(s);
        let recovered = Store::open(&dir.0, fault != 0).unwrap();
        let row = recovered.get(owner(), 1, 1).unwrap();
        assert_eq!(
            row.ticket.input.title,
            if fault == 0 || fault == 3 {
                "Migrated"
            } else {
                "Legacy ticket"
            }
        );
        assert_eq!(
            row.ticket.revision,
            if fault == 0 || fault == 3 { 18 } else { 17 }
        );
        assert_eq!(
            &fs::read(dir.0.join("store.bin")).unwrap()[..4],
            if fault == 0 || fault == 3 {
                b"RDS2"
            } else {
                b"RDS1"
            }
        );
    }
}
#[test]
fn rds2_rejects_corruption_invalid_memberships_and_foreign_ticket_associations() {
    let dir = Temp::new();
    let s = Store::open(&dir.0, false).unwrap();
    s.create(owner(), 2, input("Sandbox ticket")).unwrap();
    let state = s.locked().unwrap();
    let bytes = wire::encode(&state).unwrap();
    let decoded = wire::decode(&bytes).unwrap();
    assert_eq!(decoded.1[0].workspace, 2);
    for n in 0..bytes.len() {
        let mut broken = bytes.clone();
        broken[n] ^= 1;
        assert!(wire::decode(&broken).is_err());
    }
    let mut invalid = state.clone();
    invalid.spaces[0].space.members = [None; 4];
    assert!(wire::encode(&invalid).is_err());
    let mut invalid = state.clone();
    invalid.tickets[0].ticket.workspace = 4;
    assert!(wire::encode(&invalid).is_err());
}

#[test]
fn ticket_ids_are_local_to_each_workspace() {
    let dir = Temp::new();
    let s = Store::open(&dir.0, false).unwrap();
    let a = s.create(owner(), 1, input("First Ruvane")).unwrap();
    let b = s.create(owner(), 2, input("First Sandbox")).unwrap();
    let next = s.create(owner(), 1, input("Second Ruvane")).unwrap();
    assert_eq!((a.ticket.id, b.ticket.id, next.ticket.id), (1, 1, 2));
    s.update(
        owner(),
        2,
        1,
        input("Sandbox changed"),
        Some(&condition(&b)),
    )
    .unwrap();
    assert_eq!(
        s.get(owner(), 1, 1).unwrap().ticket.input.title,
        "First Ruvane"
    );
    drop(s);
    let s = Store::open(&dir.0, false).unwrap();
    assert_eq!(
        s.get(owner(), 2, 1).unwrap().ticket.input.title,
        "Sandbox changed"
    );
}

#[test]
fn workspace_creation_does_not_reveal_names_in_invisible_spaces() {
    let dir = Temp::new();
    let s = Store::open(&dir.0, false).unwrap();
    let sandbox = s.members(owner(), 2).unwrap();
    let mut roles = sandbox.space.members;
    roles[3] = Some(Role::Owner);
    s.set_members(
        owner(),
        2,
        roles,
        Some(&IfMatch::parse(sandbox.tag.as_str()).unwrap()),
    )
    .unwrap();
    let space = s
        .create_space(actor("drew"), WorkspaceInput::new("Ruvane").unwrap())
        .unwrap();
    assert_eq!(space.id, 3);
    assert!(matches!(
        s.space(actor("drew"), 1, Access::Read),
        Err(StoreError::Missing)
    ));
    assert_eq!(s.spaces(actor("drew")).unwrap().len(), 2);
}
