use super::*;
use crate::{ServerCacheDiskLimits, ServerCachePersistence};
use std::{fs, path::PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "ruvane-app-disk-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("cache")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn persistent(temp: &Temp) -> Fixture {
    limited(
        temp,
        ServerCacheLimits::default(),
        ServerCacheDiskLimits::default(),
    )
}
fn limited(temp: &Temp, memory: ServerCacheLimits, disk: ServerCacheDiskLimits) -> Fixture {
    let mut f = Fixture::new();
    f.cache = ServerCache::new(memory)
        .persistent(ServerCachePersistence::open(temp.path(), "application", disk).unwrap())
        .unwrap();
    f.app.server_cache(f.cache.clone());
    f
}

#[test]
fn clean_restart_restores_complete_validated_output_without_page_or_layout_loaders() {
    let temp = Temp::new();
    let f = persistent(&temp);
    let router = f.app.router(None).unwrap();
    let html = request(&router, "/articles/a", false, "");
    assert_eq!(snapshot(&html).page().title(), "Title 2");
    f.cache.close_persistence().unwrap();
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::ServiceUnavailable
    );
    let f = persistent(&temp);
    f.value.store(3, Ordering::SeqCst);
    let router = f.app.router(None).unwrap();
    let restored = request(&router, "/articles/a", false, "");
    assert_eq!(restored.body(), html.body());
    let nav = request(&router, "/articles/a", true, "");
    assert_eq!(seed(&nav).encode(), snapshot(&restored).page().encode());
    assert_eq!(f.loads.load(Ordering::SeqCst), 2);
    assert_eq!(f.cache.stats().unwrap().persistent_restores, 1);
}

#[test]
fn invalidation_is_durable_even_before_any_disk_output_is_restored() {
    for mode in 0..3 {
        let temp = Temp::new();
        let f = persistent(&temp);
        let router = f.app.router(None).unwrap();
        for path in ["/articles/a", "/articles/b"] {
            let _ = request(&router, path, true, "");
        }
        f.cache.close_persistence().unwrap();
        let f = persistent(&temp);
        assert_eq!(f.cache.stats().unwrap().entries, 0);
        match mode {
            0 => {
                f.cache.invalidate_path("/articles/a").unwrap();
            }
            1 => {
                f.cache.invalidate_tag("articles").unwrap();
            }
            _ => {
                f.cache.clear().unwrap();
            }
        }
        f.cache.close_persistence().unwrap();
        let f = persistent(&temp);
        f.value.store(3, Ordering::SeqCst);
        let router = f.app.router(None).unwrap();
        assert_eq!(
            seed(&request(&router, "/articles/a", true, "")).title(),
            "Title 3"
        );
        assert_eq!(
            seed(&request(&router, "/articles/b", true, "")).title(),
            if mode == 0 { "Title 2" } else { "Title 3" }
        );
    }
}

#[test]
fn revision_origin_policy_and_build_declaration_changes_do_not_restore_other_output() {
    for mode in 0..4 {
        let temp = Temp::new();
        let f = persistent(&temp);
        let router = f.app.router(None).unwrap();
        let _ = request(&router, "/articles/a", true, "");
        f.cache.close_persistence().unwrap();
        let mut f = persistent(&temp);
        let origin = if mode == 0 {
            Some(SiteOrigin::parse("https://public.example").unwrap())
        } else {
            None
        };
        if mode == 1 {
            f.app
                .client_page_revalidation(
                    "article",
                    Some(
                        PrerenderRevalidationPolicy::new()
                            .revalidate_after(60)
                            .unwrap(),
                    ),
                )
                .unwrap();
        }
        if mode == 2 {
            f.app.client_runtime.as_mut().unwrap().revision = 124;
        }
        if mode >= 2 {
            let plan = f.app.client_prerender_plan().unwrap();
            prerender::attach(&mut f.app, &plan).unwrap();
        }
        f.value.store(3, Ordering::SeqCst);
        let before = f.loads.load(Ordering::SeqCst);
        let router = f.app.router(origin).unwrap();
        let response = if mode == 2 {
            let bytes=b"GET /articles/a HTTP/1.1\r\nHost: localhost\r\nAccept: application/vnd.ruvane.page\r\nRuvane-Application-Revision: 124\r\n\r\n";
            router.dispatch(crate::http::parse(bytes, 8192, 32).unwrap())
        } else {
            request(&router, "/articles/a", true, "")
        };
        assert_eq!(seed(&response).title(), "Title 3");
        assert_eq!(f.loads.load(Ordering::SeqCst), before + 1);
        assert_eq!(f.cache.stats().unwrap().persistent_restores, 0);
    }
}

#[test]
fn private_query_and_unlisted_requests_neither_restore_nor_persist_private_output() {
    let temp = Temp::new();
    let f = persistent(&temp);
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    f.cache.close_persistence().unwrap();
    let f = persistent(&temp);
    f.value.store(3, Ordering::SeqCst);
    let router = f.app.router(None).unwrap();
    for header in [
        "Cookie: fixture=present\r\n",
        "Cookie:\r\n",
        "Authorization: Bearer fixture\r\n",
        "Authorization:\r\n",
    ] {
        assert_eq!(
            seed(&request(&router, "/articles/a", true, header)).title(),
            "Title 999"
        );
    }
    for path in ["/articles/a?q=1", "/articles/a?", "/articles/c"] {
        assert_eq!(seed(&request(&router, path, true, "")).title(), "Title 3");
    }
    assert_eq!(f.cache.stats().unwrap().persistent_restores, 0);
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    let bytes = fs::read(temp.path().join("store.bin")).unwrap();
    assert!(
        !bytes
            .windows(b"Article 999".len())
            .any(|part| part == b"Article 999")
    );
}

#[test]
fn memory_eviction_can_restore_disk_and_a_small_disk_budget_keeps_loaders_fresh() {
    let temp = Temp::new();
    let f = limited(
        &temp,
        ServerCacheLimits::new(1, 2 * 1024 * 1024, 8, 1000).unwrap(),
        ServerCacheDiskLimits::default(),
    );
    let router = f.app.router(None).unwrap();
    for path in ["/articles/a", "/articles/b", "/articles/a"] {
        assert_eq!(seed(&request(&router, path, true, "")).title(), "Title 2");
    }
    assert_eq!(f.loads.load(Ordering::SeqCst), 4);
    assert_eq!(f.cache.stats().unwrap().persistent_restores, 1);
    f.cache.close_persistence().unwrap();
    let f = limited(
        &temp,
        ServerCacheLimits::default(),
        ServerCacheDiskLimits::new(1, 32).unwrap(),
    );
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    assert_eq!(f.cache.stats().unwrap().persistent_entries, 0);
    f.cache.close_persistence().unwrap();
    let f = persistent(&temp);
    f.value.store(3, Ordering::SeqCst);
    assert_eq!(
        seed(&request(
            &f.app.router(None).unwrap(),
            "/articles/a",
            true,
            ""
        ))
        .title(),
        "Title 3"
    );
}

#[test]
fn invalidation_of_a_blocked_owner_prevents_durable_late_publication() {
    let temp = Temp::new();
    let f = persistent(&temp);
    let router = Arc::new(f.app.router(None).unwrap());
    let _ = request(&router, "/articles/a", true, "");
    f.cache.invalidate_tag("articles").unwrap();
    f.value.store(3, Ordering::SeqCst);
    let release = f.block();
    let owner = {
        let router = router.clone();
        std::thread::spawn(move || request(&router, "/articles/a", true, ""))
    };
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    assert!(f.cache.close_persistence().is_err());
    f.cache.invalidate_path("/articles/a").unwrap();
    release.send(()).unwrap();
    assert_eq!(
        owner.join().unwrap().status(),
        StatusCode::ServiceUnavailable
    );
    f.cache.close_persistence().unwrap();
    let f = persistent(&temp);
    f.value.store(4, Ordering::SeqCst);
    assert_eq!(
        seed(&request(
            &f.app.router(None).unwrap(),
            "/articles/a",
            true,
            ""
        ))
        .title(),
        "Title 4"
    );
}

#[test]
fn errors_and_no_store_after_invalidation_cannot_reappear_after_restart() {
    for mode in 1..=8 {
        let temp = Temp::new();
        let f = persistent(&temp);
        let router = f.app.router(None).unwrap();
        let _ = request(&router, "/articles/a", true, "");
        f.cache.invalidate_path("/articles/a").unwrap();
        f.mode.store(mode, Ordering::SeqCst);
        let _ = request(&router, "/articles/a", true, "");
        assert_eq!(f.cache.stats().unwrap().persistent_entries, 0);
        f.cache.close_persistence().unwrap();
        let f = persistent(&temp);
        f.value.store(3, Ordering::SeqCst);
        assert_eq!(
            seed(&request(
                &f.app.router(None).unwrap(),
                "/articles/a",
                true,
                ""
            ))
            .title(),
            "Title 3"
        );
    }
}

#[test]
fn persistent_io_failure_fails_public_cache_closed_and_empty_recovery_discards_old_data() {
    let temp = Temp::new();
    let f = persistent(&temp);
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temp.path().join("next.bin"))
        .unwrap();
    assert!(f.cache.invalidate_path("/articles/a").is_err());
    assert_eq!(
        request(&router, "/articles/a", true, "").status(),
        StatusCode::ServiceUnavailable
    );
    assert_eq!(
        seed(&request(
            &router,
            "/articles/a",
            true,
            "Cookie: fixture=present\r\n"
        ))
        .title(),
        "Title 999"
    );
    assert!(f.cache.close_persistence().is_err());
    drop(router);
    drop(f);
    let disk = ServerCachePersistence::recover_empty(
        temp.path(),
        "application",
        ServerCacheDiskLimits::default(),
    )
    .unwrap();
    let mut next = Fixture::new();
    next.cache = ServerCache::default().persistent(disk).unwrap();
    next.app.server_cache(next.cache.clone());
    next.value.store(3, Ordering::SeqCst);
    assert_eq!(
        seed(&request(
            &next.app.router(None).unwrap(),
            "/articles/a",
            true,
            ""
        ))
        .title(),
        "Title 3"
    );
}

struct Strict(Box<dyn Component>);
impl Component for Strict {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.text("value") == Some("Article 2") {
            return Err(ComponentError::InvalidProps);
        }
        self.0.page_data(data)
    }
    fn view(&self) -> Result<View, ComponentError> {
        self.0.view()
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        self.0.dispatch(event)
    }
}
fn strict_factory(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    Ok(Box::new(Strict(factory(props)?)))
}

#[test]
fn restored_data_is_checked_against_live_component_schemas_before_adoption() {
    let temp = Temp::new();
    let f = persistent(&temp);
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    f.cache.close_persistence().unwrap();
    let mut f = persistent(&temp);
    f.app.client_factory = Some(strict_factory);
    f.value.store(3, Ordering::SeqCst);
    let router = f.app.router(None).unwrap();
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 3"
    );
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
    assert_eq!(f.cache.stats().unwrap().persistent_restores, 0);
}

#[test]
fn reopening_keeps_the_original_expiry_instead_of_starting_a_new_lifetime() {
    let temp = Temp::new();
    let configure = |f: &mut Fixture| {
        f.app
            .client_page_revalidation(
                "article",
                Some(
                    PrerenderRevalidationPolicy::new()
                        .revalidate_after(2)
                        .unwrap()
                        .tag("articles")
                        .unwrap(),
                ),
            )
            .unwrap();
    };
    let mut f = persistent(&temp);
    configure(&mut f);
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    std::thread::sleep(Duration::from_millis(700));
    f.cache.close_persistence().unwrap();
    let mut f = persistent(&temp);
    configure(&mut f);
    f.value.store(3, Ordering::SeqCst);
    let router = f.app.router(None).unwrap();
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    assert_eq!(f.loads.load(Ordering::SeqCst), 2);
    std::thread::sleep(Duration::from_millis(1400));
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 3"
    );
    assert_eq!(f.loads.load(Ordering::SeqCst), 3);
}

fn background(temp: &Temp) -> Fixture {
    let mut f = Fixture::new();
    let disk =
        ServerCachePersistence::open(temp.path(), "application", ServerCacheDiskLimits::default())
            .unwrap();
    f.cache = ServerCache::with_background(
        ServerCacheLimits::default(),
        crate::ServerCacheBackgroundLimits::new(1, 2).unwrap(),
    )
    .unwrap()
    .persistent(disk)
    .unwrap();
    f.app.server_cache(f.cache.clone());
    f.app
        .client_page_revalidation(
            "article",
            Some(
                PrerenderRevalidationPolicy::new()
                    .revalidate_after(60)
                    .unwrap()
                    .stale_while_revalidate(120)
                    .unwrap()
                    .tag("articles")
                    .unwrap(),
            ),
        )
        .unwrap();
    f
}
#[test]
fn a_complete_background_replacement_is_durable_and_restores_without_loaders() {
    let temp = Temp::new();
    let f = background(&temp);
    let router = f.app.router(None).unwrap();
    let _ = request(&router, "/articles/a", true, "");
    f.value.store(3, Ordering::SeqCst);
    f.cache.expire_for_test(false);
    let release = f.block();
    assert_eq!(
        seed(&request(&router, "/articles/a", true, "")).title(),
        "Title 2"
    );
    until(|| f.loads.load(Ordering::SeqCst) == 4);
    release.send(()).unwrap();
    until(|| {
        let stats = f.cache.stats().unwrap();
        stats.active_loads == 0 && stats.background_running == 0
    });
    f.cache.close_persistence().unwrap();
    let f = background(&temp);
    f.value.store(4, Ordering::SeqCst);
    let router = f.app.router(None).unwrap();
    let html = request(&router, "/articles/a", false, "");
    assert_eq!(snapshot(&html).page().title(), "Title 3");
    assert!(html.body_text().unwrap().contains("Description 3"));
    assert_eq!(f.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn persistence_must_be_attached_before_handles_are_cloned_or_namespaces_created() {
    for used in [false, true] {
        let temp = Temp::new();
        let disk = ServerCachePersistence::open(
            temp.path(),
            "application",
            ServerCacheDiskLimits::default(),
        )
        .unwrap();
        let cache = ServerCache::default();
        let clone = if used {
            cache.namespace().unwrap();
            None
        } else {
            Some(cache.clone())
        };
        assert!(cache.persistent(disk).is_err());
        drop(clone);
        assert!(!temp.path().join("LOCK").exists());
    }
}
