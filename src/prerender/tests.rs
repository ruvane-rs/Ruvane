use super::*;
use crate::{ActionData, ComponentProps, Document, InitialState};
fn plan() -> ClientPrerender {
    let url = InternalUrl::new("/frozen").unwrap();
    let packet = NavigationPage::new(
        url.clone(),
        "Frozen",
        ComponentProps::new(1, InitialState::Number(0)).unwrap(),
        ActionData::default(),
        StatusCode::Ok,
    )
    .unwrap();
    let html = Document::new(
        "Frozen",
        NavigationSnapshot::new(9, packet.clone()).unwrap().html(),
    )
    .render()
    .as_str()
    .into();
    ClientPrerender {
        revision: 9,
        rows: vec![PrerenderRow {
            name: "frozen".into(),
            url,
            packet,
            html,
        }],
    }
}
fn fix_checksum(bytes: &mut [u8]) {
    let end = bytes.len() - 8;
    let sum = checksum(&bytes[..end]);
    bytes[end..].copy_from_slice(&sum.to_le_bytes());
}
#[test]
fn artifact_roundtrip_truncations_and_byte_corruption_are_rejected() {
    let plan = plan();
    let bytes = plan.encode();
    let decoded = ClientPrerender::decode(&bytes).unwrap();
    assert_eq!(decoded.revision(), 9);
    assert_eq!(decoded.urls().next().unwrap().as_str(), "/frozen");
    assert_eq!(decoded.encode(), bytes);
    for end in 0..bytes.len() {
        assert!(ClientPrerender::decode(&bytes[..end]).is_err());
    }
    for position in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[position] ^= 1;
        assert!(ClientPrerender::decode(&damaged).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(ClientPrerender::decode(&trailing).is_err());
    let empty = ClientPrerender {
        revision: 9,
        rows: Vec::new(),
    };
    assert!(ClientPrerender::decode(&empty.encode()).unwrap().is_empty());
}
#[test]
fn closed_format_checks_budget_counts_urls_and_snapshot_ownership() {
    let mut bytes = plan().encode();
    bytes[12..16].copy_from_slice(&129u32.to_le_bytes());
    fix_checksum(&mut bytes);
    assert!(ClientPrerender::decode(&bytes).is_err());
    let mut duplicate = plan();
    duplicate.rows.push(duplicate.rows[0].clone());
    assert!(ClientPrerender::decode(&duplicate.encode()).is_err());
    let mut wrong = plan();
    wrong.rows[0].url = InternalUrl::from_target("/frozen?q=1").unwrap();
    assert!(ClientPrerender::decode(&wrong.encode()).is_err());
    let mut wrong = plan();
    wrong.rows[0].html = "<!doctype html><html></html>".into();
    assert!(ClientPrerender::decode(&wrong.encode()).is_err());
    let mut wrong = plan();
    wrong.rows[0].packet = NavigationPage::new(
        wrong.rows[0].url.clone(),
        "Error",
        ComponentProps::new(1, InitialState::Number(404)).unwrap(),
        ActionData::default(),
        StatusCode::NotFound,
    )
    .unwrap();
    assert!(ClientPrerender::decode(&wrong.encode()).is_err());
    assert!(ClientPrerender::decode(&vec![0; MAX_CLIENT_PRERENDER_BYTES + 1]).is_err());
    let mut wrong = plan();
    wrong.rows[0].html = "x".repeat(crate::MAX_STATIC_FILE_BYTES + 1);
    assert!(ClientPrerender::decode(&wrong.encode()).is_err());
}
#[test]
fn build_output_never_overwrites_existing_files_and_refuses_symlinks() {
    let root = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("ruvane-prerender-unit-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let file = root.join("pages.bin");
    plan().write_new(&file).unwrap();
    let before = std::fs::read(&file).unwrap();
    assert!(plan().write_new(&file).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), before);
    #[cfg(unix)]
    {
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert!(plan().write_new(alias.join("new.bin")).is_err());
        let link = root.join("link.bin");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(plan().write_new(&link).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}
