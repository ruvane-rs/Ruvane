use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
#[test]
fn dropping_the_last_pool_owner_stops_idle_workers() {
    let pool = Pool::new(ServerCacheBackgroundLimits::new(2, 1).unwrap()).unwrap();
    let observer = Arc::clone(&pool.shared);
    until(|| pool.stats().unwrap().2 == 2);
    drop(pool);
    until(|| observer.state.lock().unwrap().alive == 0);
    until(|| Arc::strong_count(&observer) == 1);
}
#[test]
fn stop_discards_queued_work_without_waiting_for_a_running_callback() {
    let pool = Pool::new(ServerCacheBackgroundLimits::new(1, 1).unwrap()).unwrap();
    let (release, blocked) = mpsc::channel();
    let (entered, receive) = mpsc::channel();
    pool.submit(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    }))
    .unwrap();
    receive.recv_timeout(Duration::from_secs(2)).unwrap();
    let (queued, receipt) = mpsc::channel();
    pool.submit(Box::new(move || {
        let _ = queued.send(());
    }))
    .unwrap();
    assert!(
        pool.submit(Box::new(|| panic!("full queue executed")))
            .is_err()
    );
    pool.stop();
    assert_eq!(pool.stats().unwrap().0, 0);
    assert!(receipt.recv_timeout(Duration::from_millis(20)).is_err());
    assert_eq!(pool.stats().unwrap().1, 1);
    release.send(()).unwrap();
    until(|| pool.stats().unwrap().2 == 0);
    assert!(
        pool.submit(Box::new(|| panic!("stopped queue executed")))
            .is_err()
    );
}
#[test]
fn panicking_jobs_do_not_destroy_the_fixed_worker() {
    let pool = Pool::new(ServerCacheBackgroundLimits::new(1, 2).unwrap()).unwrap();
    pool.submit(Box::new(|| panic!("fixture job panic")))
        .unwrap();
    let (send, receive) = mpsc::channel();
    pool.submit(Box::new(move || send.send(()).unwrap()))
        .unwrap();
    receive.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(pool.stats().unwrap().2, 1);
}
