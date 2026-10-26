use super::*;
fn cache(entries: usize) -> ServerCache {
    ServerCache::with_background(
        ServerCacheLimits::new(entries, 1024, 4, 20).unwrap(),
        ServerCacheBackgroundLimits::new(1, 2).unwrap(),
    )
    .unwrap()
}
fn stale_policy() -> ServerCachePolicy {
    policy().stale_while_revalidate(60).unwrap()
}
fn warm(cache: &ServerCache, ns: Namespace, path: &str) {
    let Access::Load(lease) = cache.access(ns, 0, &url(path), &stale_policy()).unwrap() else {
        panic!("cold loader");
    };
    lease.publish(Some(branch(path))).unwrap();
}
#[test]
fn eviction_retires_the_old_refresh_instead_of_resurrecting_its_entry() {
    let cache = cache(1);
    let ns = cache.namespace().unwrap();
    warm(&cache, ns, "/a");
    cache.expire_for_test(false);
    let Access::Stale(_, _, Some(lease)) =
        cache.access(ns, 0, &url("/a"), &stale_policy()).unwrap()
    else {
        panic!("refresh");
    };
    warm(&cache, ns, "/b");
    assert!(!lease.is_current());
    assert!(lease.publish(Some(branch("/a"))).is_err());
    assert_eq!(cache.stats().unwrap().entries, 1);
    assert!(matches!(
        cache.access(ns, 0, &url("/b"), &stale_policy()),
        Ok(Access::Hit(..))
    ));
}
#[test]
fn transient_refresh_failure_preserves_the_original_hard_deadline() {
    let cache = cache(2);
    let ns = cache.namespace().unwrap();
    warm(&cache, ns, "/a");
    cache.expire_for_test(false);
    let deadline = cache.0.state.lock().unwrap().entries[0].stale_until;
    let Access::Stale(_, _, Some(lease)) =
        cache.access(ns, 0, &url("/a"), &stale_policy()).unwrap()
    else {
        panic!("refresh");
    };
    lease.fail(crate::PageError::InternalServerError);
    let state = cache.0.state.lock().unwrap();
    assert_eq!(state.entries[0].stale_until, deadline);
    assert!(state.entries[0].retry_at > Instant::now());
    drop(state);
    assert!(matches!(
        cache.access(ns, 0, &url("/a"), &stale_policy()),
        Ok(Access::Stale(_, _, None))
    ));
    cache.expire_for_test(true);
    assert!(matches!(
        cache.access(ns, 0, &url("/a"), &stale_policy()),
        Ok(Access::Load(_))
    ));
    assert_eq!(cache.stats().unwrap().entries, 0);
}
#[test]
fn background_and_stale_limits_are_explicit_and_bounded() {
    for (workers, queue) in [(0, 1), (5, 1), (1, 0), (1, 33)] {
        assert!(ServerCacheBackgroundLimits::new(workers, queue).is_err());
    }
    for seconds in [0, 301, u32::MAX] {
        assert!(policy().stale_while_revalidate(seconds).is_err());
    }
    assert!(
        ServerCachePolicy::no_store()
            .stale_while_revalidate(1)
            .is_err()
    );
    assert_eq!(policy().stale_seconds(), 0);
    assert_eq!(stale_policy().stale_seconds(), 60);
    let cache = ServerCache::default();
    assert_eq!(cache.stats().unwrap().background_workers, 0);
    assert!(!cache.has_background());
}

#[test]
fn failed_output_refreshes_preserve_the_exact_hard_deadline_and_retry_backoff() {
    let cache = cache(2);
    let ns = cache.namespace().unwrap();
    let policy = crate::PrerenderRevalidationPolicy::new()
        .revalidate_after(30)
        .unwrap()
        .stale_while_revalidate(60)
        .unwrap();
    let Access::Load(lease) = cache.access_output(ns, 0, &url("/a"), &policy).unwrap() else {
        panic!("cold output loader");
    };
    let mut branch = branch("/a");
    branch.output = Some(Arc::from(&b"complete HTML"[..]));
    lease.publish(Some(branch)).unwrap();
    cache.expire_for_test(false);
    let deadline = cache.0.state.lock().unwrap().entries[0].stale_until;
    let Access::Stale(_, _, Some(lease)) = cache.access_output(ns, 0, &url("/a"), &policy).unwrap()
    else {
        panic!("output refresh");
    };
    assert!(lease.is_output());
    lease.fail(crate::PageError::InternalServerError);
    let state = cache.0.state.lock().unwrap();
    assert_eq!(state.entries[0].stale_until, deadline);
    assert!(state.entries[0].retry_at > Instant::now());
    drop(state);
    assert!(matches!(
        cache.access_output(ns, 0, &url("/a"), &policy),
        Ok(Access::Stale(_, _, None))
    ));
    cache.expire_for_test(true);
    assert!(matches!(
        cache.access_output(ns, 0, &url("/a"), &policy),
        Ok(Access::Load(_))
    ));
}
