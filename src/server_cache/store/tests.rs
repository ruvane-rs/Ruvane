use super::*;
use crate::{ActionData, ComponentProps, InitialState, StatusCode};
mod background;

fn policy() -> ServerCachePolicy {
    ServerCachePolicy::public(30)
        .unwrap()
        .tag("articles")
        .unwrap()
}
fn url(path: &str) -> InternalUrl {
    InternalUrl::from_target(path).unwrap()
}
fn branch(path: &str) -> CachedBranch {
    CachedBranch {
        head: None,
        packet: NavigationPage::new(
            url(path),
            "Title",
            ComponentProps::new(1, InitialState::Number(0)).unwrap(),
            ActionData::default(),
            StatusCode::Ok,
        )
        .unwrap(),
        fallback: Html::default(),
        policy: NavigationCachePolicy::no_store(),
    }
}
fn load(cache: &ServerCache, ns: Namespace, path: &str) -> Lease {
    match cache.access(ns, 0, &url(path), &policy()).unwrap() {
        Access::Load(lease) => lease,
        _ => panic!("expected loader"),
    }
}
fn until_waiting(cache: &ServerCache) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while cache.stats().unwrap().waiting_requests == 0 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn invalidation_retires_waiters_and_prevents_late_publication() {
    for tag in [false, true] {
        let cache = ServerCache::default();
        let ns = cache.namespace().unwrap();
        let lease = load(&cache, ns, "/articles/a?q=1");
        let waiting = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                cache
                    .access(ns, 0, &url("/articles/a?q=1"), &policy())
                    .is_err()
            })
        };
        until_waiting(&cache);
        if tag {
            cache.invalidate_tag("articles").unwrap();
        } else {
            cache.invalidate_path("/articles/a").unwrap();
        }
        assert!(waiting.join().unwrap());
        assert_eq!(cache.stats().unwrap().active_loads, 1);
        assert!(lease.publish(Some(branch("/articles/a?q=1"))).is_err());
        assert_eq!(cache.stats().unwrap().entries, 0);
        assert_eq!(cache.stats().unwrap().active_loads, 0);
        load(&cache, ns, "/articles/a?q=1")
            .publish(Some(branch("/articles/a?q=1")))
            .unwrap();
        assert_eq!(cache.stats().unwrap().entries, 1);
    }
}

#[test]
fn load_and_wait_budgets_fail_without_duplicate_loaders() {
    let cache = ServerCache::new(ServerCacheLimits::new(2, 1024, 1, 5).unwrap());
    let ns = cache.namespace().unwrap();
    let owner = load(&cache, ns, "/a");
    assert!(cache.access(ns, 0, &url("/b"), &policy()).is_err());
    let began = Instant::now();
    assert!(cache.access(ns, 0, &url("/a"), &policy()).is_err());
    assert!(began.elapsed() < Duration::from_secs(1));
    assert_eq!(cache.stats().unwrap().active_loads, 1);
    assert_eq!(cache.stats().unwrap().waiting_requests, 0);
    cache.0.state.lock().unwrap().waiters = MAX_SERVER_CACHE_WAITERS;
    assert!(cache.access(ns, 0, &url("/a"), &policy()).is_err());
    cache.0.state.lock().unwrap().waiters = 0;
    drop(owner);
    assert_eq!(cache.stats().unwrap().active_loads, 0);
    load(&cache, ns, "/a").publish(Some(branch("/a"))).unwrap();
}

#[test]
fn uncacheable_or_panicking_loads_release_waiters_without_sharing_data() {
    for panic in [false, true] {
        let cache = ServerCache::default();
        let ns = cache.namespace().unwrap();
        let owner = load(&cache, ns, "/a");
        let waiting = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                matches!(
                    cache.access(ns, 0, &url("/a"), &policy()),
                    Ok(Access::Bypass)
                )
            })
        };
        until_waiting(&cache);
        if panic {
            let _ = std::panic::catch_unwind(|| {
                let _owner = owner;
                panic!("fixture");
            });
        } else {
            owner.publish(None).unwrap();
        }
        assert!(waiting.join().unwrap());
        assert_eq!(cache.stats().unwrap().entries, 0);
        assert_eq!(cache.stats().unwrap().waiting_requests, 0);
        assert_eq!(cache.stats().unwrap().active_loads, 0);
    }
}

#[test]
fn lru_byte_budgets_expiry_and_clear_bound_retained_data() {
    let cache = ServerCache::new(ServerCacheLimits::new(2, 1024, 1, 10).unwrap());
    let ns = cache.namespace().unwrap();
    for path in ["/a", "/b"] {
        load(&cache, ns, path).publish(Some(branch(path))).unwrap();
    }
    assert!(matches!(
        cache.access(ns, 0, &url("/a"), &policy()).unwrap(),
        Access::Hit(..)
    ));
    load(&cache, ns, "/c").publish(Some(branch("/c"))).unwrap();
    let evicted = load(&cache, ns, "/b");
    drop(evicted);
    assert_eq!(cache.stats().unwrap().entries, 2);
    cache.0.state.lock().unwrap().entries[0].stale_until = Instant::now();
    assert_eq!(cache.stats().unwrap().entries, 1);
    assert_eq!(cache.clear().unwrap(), 1);
    assert_eq!(cache.stats().unwrap().bytes, 0);
    let cache = ServerCache::new(ServerCacheLimits::new(3, 100, 1, 10).unwrap());
    let ns = cache.namespace().unwrap();
    for path in ["/a", "/b", "/c"] {
        load(&cache, ns, path).publish(Some(branch(path))).unwrap();
        assert!(cache.stats().unwrap().bytes <= 100);
    }
    assert!(cache.stats().unwrap().entries < 3);
}

#[test]
fn invalid_configuration_tags_paths_and_namespace_exhaustion_fail_closed() {
    for seconds in [0, 301, u32::MAX] {
        assert!(ServerCachePolicy::public(seconds).is_err());
    }
    for limits in [
        (0, 1, 1, 1),
        (129, 1, 1, 1),
        (1, 0, 1, 1),
        (1, MAX_SERVER_CACHE_BYTES + 1, 1, 1),
        (1, 1, 0, 1),
        (1, 1, 17, 1),
        (1, 1, 1, 0),
        (1, 1, 1, 1001),
    ] {
        assert!(ServerCacheLimits::new(limits.0, limits.1, limits.2, limits.3).is_err());
    }
    let cache = ServerCache::default();
    for tag in ["", "a b", "../x", "é", &"a".repeat(49)] {
        assert!(policy().tag(tag).is_err());
        assert!(cache.invalidate_tag(tag).is_err());
    }
    let mut tags = ServerCachePolicy::public(1).unwrap();
    for i in 0..MAX_SERVER_CACHE_TAGS {
        tags = tags.tag(&format!("tag{i}")).unwrap();
    }
    assert!(tags.clone().tag("extra").is_err());
    assert!(tags.tag("tag0").is_ok());
    assert!(ServerCachePolicy::no_store().tag("tag").is_err());
    for path in [
        "https://example.com/a",
        "/a?q=1",
        "/a#x",
        "/../a",
        "//example.com",
    ] {
        assert!(cache.invalidate_path(path).is_err());
    }
    cache.0.state.lock().unwrap().namespace = u64::MAX;
    assert!(cache.namespace().is_err());
}
