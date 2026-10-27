use super::*;
use crate::{Html, InternalUrl, NavigationCachePolicy, NavigationPage};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct CachedBranch {
    pub head: Option<crate::ClientDocument>,
    pub output: Option<Arc<[u8]>>,
    pub packet: NavigationPage,
    pub fallback: Html,
    pub policy: NavigationCachePolicy,
}
#[derive(Clone, Copy)]
pub(crate) struct Namespace(u64);
#[derive(Clone, Eq, PartialEq)]
struct Key {
    output: bool,
    namespace: u64,
    page: usize,
    url: String,
}
struct Entry {
    key: Key,
    tags: Vec<String>,
    branch: Arc<CachedBranch>,
    expires: Option<Instant>,
    stale_until: Option<Instant>,
    retry_at: Instant,
    bytes: usize,
}
struct Flight {
    background: bool,
    key: Key,
    tags: Vec<String>,
    persistence: Option<OutputIdentity>,
    result: Mutex<FlightResult>,
    ready: Condvar,
}
enum FlightResult {
    Pending,
    Ready(Arc<CachedBranch>, Option<Instant>),
    Retry,
    Invalidated,
}
#[derive(Default)]
struct State {
    namespace: u64,
    entries: Vec<Entry>,
    flights: Vec<Arc<Flight>>,
    bytes: usize,
    waiters: usize,
    hits: u64,
    misses: u64,
    waits: u64,
    stale_hits: u64,
    refresh_attempts: u64,
    refresh_failures: u64,
}
struct Inner {
    limits: ServerCacheLimits,
    state: Mutex<State>,
    disk: Option<Arc<super::persistence::Store>>,
}

/// A bounded public cache and invalidation handle, with optional persistent output.
/// Clones share state.
/// Each constructed App router gets a separate namespace, including its revision/origin.
#[derive(Clone)]
pub struct ServerCache(Arc<Inner>, Option<Arc<Pool>>);
/// Bounded cache counts; no URLs, tags, headers or page data are exposed.
#[derive(Clone, Copy, Debug)]
pub struct ServerCacheStats {
    /// Records in the persistent catalog; zero when persistence is disabled.
    pub persistent_entries: usize,
    /// Complete encoded catalog bytes, independent of the memory budget.
    pub persistent_bytes: usize,
    /// Outputs restored and validated in this process.
    pub persistent_restores: u64,
    /// Retained entries past freshness but within their allowed stale window.
    pub stale_entries: usize,
    /// Requests served permitted stale data without waiting for a refresh.
    pub stale_hits: u64,
    /// Background refresh reservations, including attempts declined by the queue.
    pub refresh_attempts: u64,
    /// Failed or declined refreshes, excluding explicit invalidation.
    pub refresh_failures: u64,
    /// Background jobs waiting in the fixed queue.
    pub background_queued: usize,
    /// Background jobs currently executing.
    pub background_running: usize,
    /// Background worker threads still alive, including workers stopping cooperatively.
    pub background_workers: usize,
    /// Retained fresh and permitted stale entries.
    pub entries: usize,
    /// Accounted complete HTML, packet, fallback, buffered head, key and tag bytes.
    pub bytes: usize,
    /// Loaders still running, including invalidated ones awaiting completion.
    pub active_loads: usize,
    /// Requests waiting for an existing loader.
    pub waiting_requests: usize,
    /// Successful retained-entry lookups.
    pub hits: u64,
    /// Public loads started through this cache.
    pub misses: u64,
    /// Requests that waited for an existing loader.
    pub waits: u64,
}
impl Default for ServerCache {
    fn default() -> Self {
        Self::new(ServerCacheLimits::default())
    }
}
impl ServerCache {
    #[cfg(test)]
    pub(crate) fn expire_for_test(&self, hard: bool) {
        let mut state = self.0.state.lock().unwrap();
        for entry in &mut state.entries {
            if entry.expires.is_none() {
                continue;
            }
            entry.expires = Some(Instant::now());
            entry.retry_at = entry.expires.unwrap();
            if hard {
                entry.stale_until = entry.expires;
            }
        }
    }
    /// Creates an empty handle with validated limits. No worker or timer is started.
    pub fn new(limits: ServerCacheLimits) -> Self {
        Self(
            Arc::new(Inner {
                limits,
                state: Mutex::new(State::default()),
                disk: None,
            }),
            None,
        )
    }
    /// Attaches exclusive disk storage before cloning or using this handle.
    /// Only regenerated pre-render output is persisted; memory caching stays independent.
    pub fn persistent(
        mut self,
        persistence: ServerCachePersistence,
    ) -> Result<Self, ServerCacheError> {
        let inner = Arc::get_mut(&mut self.0).ok_or(ServerCacheError::PersistenceUnavailable)?;
        if inner.disk.is_some()
            || inner
                .state
                .get_mut()
                .map_err(|_| ServerCacheError::Unavailable)?
                .namespace
                != 0
        {
            return Err(ServerCacheError::PersistenceUnavailable);
        }
        inner.disk = Some(persistence.store);
        Ok(self)
    }
    /// Closes persistence after the server has stopped and all cache work has settled.
    /// Required for static/global handles. Further public cache access is unavailable.
    pub fn close_persistence(&self) -> Result<(), ServerCacheError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        if !state.flights.is_empty() || state.waiters != 0 {
            return Err(ServerCacheError::Unavailable);
        }
        if let Some(disk) = &self.0.disk {
            disk.close()?;
        }
        Ok(())
    }
    pub(crate) fn has_persistence(&self) -> bool {
        self.0.disk.is_some()
    }
    /// Starts a fixed native worker pool with a bounded refresh queue.
    /// Worker creation failure is reported; Wasm cannot start server workers.
    pub fn with_background(
        limits: ServerCacheLimits,
        background: ServerCacheBackgroundLimits,
    ) -> Result<Self, ServerCacheError> {
        let mut cache = Self::new(limits);
        cache.1 = Some(Arc::new(Pool::new(background)?));
        Ok(cache)
    }
    /// Stops queuing refreshes, drops queued jobs and retires active background loads.
    /// Running synchronous callbacks finish cooperatively; this call does not join them.
    /// Fresh lookup and foreground loading remain available.
    pub fn stop_background(&self) -> Result<(), ServerCacheError> {
        if let Some(pool) = &self.1 {
            pool.stop();
        }
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        for flight in &state.flights {
            if flight.background {
                *flight
                    .result
                    .lock()
                    .map_err(|_| ServerCacheError::Unavailable)? = FlightResult::Invalidated;
                flight.ready.notify_all();
            }
        }
        Ok(())
    }
    pub(crate) fn has_background(&self) -> bool {
        self.1.is_some()
    }
    pub(crate) fn detached(&self) -> Self {
        Self(Arc::clone(&self.0), None)
    }
    pub(crate) fn submit(&self, job: Job) -> Result<(), ServerCacheError> {
        self.1
            .as_ref()
            .ok_or(ServerCacheError::Unavailable)?
            .submit(job)
    }
    pub(crate) fn namespace(&self) -> Result<Namespace, ServerCacheError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        state.namespace = state
            .namespace
            .checked_add(1)
            .ok_or(ServerCacheError::Unavailable)?;
        Ok(Namespace(state.namespace))
    }
    /// Invalidates this exact path and every query variant across the handle's routers.
    /// Active matching loads are retired before this call returns.
    /// The count covers memory entries; persistent removal is acknowledged even when zero.
    pub fn invalidate_path(&self, path: &str) -> Result<usize, ServerCacheError> {
        let url = InternalUrl::new(path).map_err(|_| ServerCacheError::InvalidPath)?;
        self.invalidate(|key, _| key.url.split('?').next() == Some(url.as_str()))
    }
    /// Invalidates all branches declaring this validated tag, including active loads.
    /// The returned count covers memory entries, not separate disk records.
    pub fn invalidate_tag(&self, tag: &str) -> Result<usize, ServerCacheError> {
        if !crate::page_route::valid_name(tag) {
            return Err(ServerCacheError::InvalidTag);
        }
        self.invalidate(|_, tags| tags.iter().any(|value| value == tag))
    }
    /// Invalidates all entries and retires every active public load.
    /// The returned count covers memory entries, not separate disk records.
    pub fn clear(&self) -> Result<usize, ServerCacheError> {
        self.invalidate(|_, _| true)
    }
    fn invalidate(
        &self,
        matches: impl Fn(&Key, &[String]) -> bool,
    ) -> Result<usize, ServerCacheError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        let before = state.entries.len();
        state
            .entries
            .retain(|entry| !matches(&entry.key, &entry.tags));
        state.bytes = state.entries.iter().map(|entry| entry.bytes).sum();
        let removed = before - state.entries.len();
        for flight in &state.flights {
            if matches(&flight.key, &flight.tags) {
                *flight
                    .result
                    .lock()
                    .map_err(|_| ServerCacheError::Unavailable)? = FlightResult::Invalidated;
                flight.ready.notify_all();
            }
        }
        if let Some(disk) = &self.0.disk {
            disk.invalidate(|url, tags| {
                matches(
                    &Key {
                        output: true,
                        namespace: 0,
                        page: 0,
                        url: url.to_owned(),
                    },
                    tags,
                )
            })?;
        }
        Ok(removed)
    }
    /// Sweeps expired entries and returns aggregate counters.
    pub fn stats(&self) -> Result<ServerCacheStats, ServerCacheError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        let now = Instant::now();
        state.sweep(now);
        let (persistent_entries, persistent_bytes, persistent_restores) = self
            .0
            .disk
            .as_ref()
            .map(|disk| disk.stats())
            .transpose()?
            .unwrap_or((0, 0, 0));
        let mut stats = ServerCacheStats {
            persistent_entries,
            persistent_bytes,
            persistent_restores,
            entries: state.entries.len(),
            bytes: state.bytes,
            active_loads: state.flights.len(),
            waiting_requests: state.waiters,
            hits: state.hits,
            misses: state.misses,
            waits: state.waits,
            stale_entries: state
                .entries
                .iter()
                .filter(|entry| entry.expires.is_some_and(|expires| expires <= now))
                .count(),
            stale_hits: state.stale_hits,
            refresh_attempts: state.refresh_attempts,
            refresh_failures: state.refresh_failures,
            background_queued: 0,
            background_running: 0,
            background_workers: 0,
        };
        drop(state);
        if let Some(pool) = &self.1 {
            (
                stats.background_queued,
                stats.background_running,
                stats.background_workers,
            ) = pool.stats()?;
        }
        Ok(stats)
    }

    pub(crate) fn access(
        &self,
        namespace: Namespace,
        page: usize,
        url: &InternalUrl,
        policy: &ServerCachePolicy,
    ) -> Result<Access, ServerCacheError> {
        if policy.seconds.is_none() {
            return Err(ServerCacheError::Unavailable);
        }
        self.access_inner(namespace, page, url, policy, false, None)
    }
    pub(crate) fn access_output(
        &self,
        namespace: Namespace,
        page: usize,
        url: &InternalUrl,
        policy: &crate::PrerenderRevalidationPolicy,
        identity: Option<OutputIdentity>,
    ) -> Result<Access, ServerCacheError> {
        let policy = ServerCachePolicy {
            seconds: policy.max_age_seconds(),
            tags: policy.tags.clone(),
            stale_seconds: policy.stale_seconds(),
        };
        self.access_inner(namespace, page, url, &policy, true, identity)
    }
    fn access_inner(
        &self,
        namespace: Namespace,
        page: usize,
        url: &InternalUrl,
        policy: &ServerCachePolicy,
        output: bool,
        identity: Option<OutputIdentity>,
    ) -> Result<Access, ServerCacheError> {
        if self.0.disk.as_ref().is_some_and(|disk| !disk.available()) {
            return Err(ServerCacheError::PersistenceUnavailable);
        }
        let key = Key {
            output,
            namespace: namespace.0,
            page,
            url: url.as_str().into(),
        };
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        let now = Instant::now();
        state.sweep(now);
        if let Some(index) = state.entries.iter().position(|entry| entry.key == key) {
            let entry = state.entries.remove(index);
            if entry.expires.is_none_or(|expires| expires > now) {
                let access = Access::Hit(Arc::clone(&entry.branch), entry.expires);
                state.entries.push(entry);
                state.hits = state.hits.saturating_add(1);
                return Ok(access);
            }
            state.stale_hits = state.stale_hits.saturating_add(1);
            let access = (
                Arc::clone(&entry.branch),
                entry.expires.ok_or(ServerCacheError::Unavailable)?,
            );
            let reserve = self.1.is_some()
                && entry.retry_at <= now
                && state.flights.len() < self.0.limits.loads
                && !state.flights.iter().any(|flight| flight.key == key);
            state.entries.push(entry);
            let lease = reserve.then(|| {
                let flight = Arc::new(Flight {
                    background: true,
                    key,
                    tags: policy.tags.clone(),
                    persistence: identity.clone(),
                    result: Mutex::new(FlightResult::Pending),
                    ready: Condvar::new(),
                });
                state.flights.push(Arc::clone(&flight));
                state.refresh_attempts = state.refresh_attempts.saturating_add(1);
                Lease {
                    cache: self.detached(),
                    flight,
                    seconds: policy.seconds,
                    stale_seconds: policy.stale_seconds,
                    finished: false,
                }
            });
            return Ok(Access::Stale(access.0, access.1, lease));
        }
        if let Some(flight) = state
            .flights
            .iter()
            .find(|flight| flight.key == key)
            .cloned()
        {
            if state.waiters == MAX_SERVER_CACHE_WAITERS {
                return Err(ServerCacheError::Unavailable);
            }
            state.waiters += 1;
            state.waits = state.waits.saturating_add(1);
            drop(state);
            let _waiting = Waiting(self.clone());
            let deadline = Instant::now() + Duration::from_millis(self.0.limits.wait_ms);
            let mut result = flight
                .result
                .lock()
                .map_err(|_| ServerCacheError::Unavailable)?;
            loop {
                if Instant::now() >= deadline {
                    return Err(ServerCacheError::Unavailable);
                }
                match &*result {
                    FlightResult::Ready(branch, expires)
                        if expires.is_none_or(|expires| expires > Instant::now()) =>
                    {
                        let (branch, expires) = (Arc::clone(branch), *expires);
                        drop(result);
                        let state = self
                            .0
                            .state
                            .lock()
                            .map_err(|_| ServerCacheError::Unavailable)?;
                        if !state.entries.iter().any(|entry| {
                            entry.key == key
                                && Arc::ptr_eq(&entry.branch, &branch)
                                && entry.expires.is_none_or(|expires| expires > Instant::now())
                        }) {
                            return Err(ServerCacheError::Unavailable);
                        }
                        drop(state);
                        return Ok(Access::Hit(branch, expires));
                    }
                    FlightResult::Retry => return Ok(Access::Bypass),
                    FlightResult::Ready(..) | FlightResult::Invalidated => {
                        return Err(ServerCacheError::Unavailable);
                    }
                    FlightResult::Pending => {}
                }
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or(ServerCacheError::Unavailable)?;
                result = flight
                    .ready
                    .wait_timeout(result, remaining)
                    .map_err(|_| ServerCacheError::Unavailable)?
                    .0;
            }
        }
        if state.flights.len() >= self.0.limits.loads {
            return Err(ServerCacheError::Unavailable);
        }
        let flight = Arc::new(Flight {
            background: false,
            key,
            tags: policy.tags.clone(),
            persistence: identity,
            result: Mutex::new(FlightResult::Pending),
            ready: Condvar::new(),
        });
        state.flights.push(Arc::clone(&flight));
        state.misses = state.misses.saturating_add(1);
        Ok(Access::Load(Lease {
            cache: self.clone(),
            flight,
            seconds: policy.seconds,
            stale_seconds: policy.stale_seconds,
            finished: false,
        }))
    }
}
impl State {
    fn sweep(&mut self, now: Instant) {
        self.entries
            .retain(|entry| entry.stale_until.is_none_or(|until| until > now));
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
    }
}
struct Waiting(ServerCache);
impl Drop for Waiting {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.0.state.lock() {
            state.waiters -= 1;
        }
    }
}
pub(crate) enum Access {
    Hit(Arc<CachedBranch>, Option<Instant>),
    Stale(Arc<CachedBranch>, Instant, Option<Lease>),
    Load(Lease),
    Bypass,
}
pub(crate) struct Lease {
    cache: ServerCache,
    flight: Arc<Flight>,
    seconds: Option<u32>,
    stale_seconds: u32,
    finished: bool,
}
impl Lease {
    pub fn publish(mut self, branch: Option<CachedBranch>) -> Result<(), ServerCacheError> {
        self.publish_inner(branch, None)
    }
    pub fn restore(&self) -> Result<Option<RestoredOutput>, ServerCacheError> {
        match (&self.cache.0.disk, &self.flight.persistence) {
            (Some(disk), Some(identity)) => disk.get(identity, &self.flight.key.url),
            _ => Ok(None),
        }
    }
    pub fn reject_restore(&self) -> Result<(), ServerCacheError> {
        if let (Some(disk), Some(identity)) = (&self.cache.0.disk, &self.flight.persistence) {
            disk.remove(identity, &self.flight.key.url)?;
        }
        Ok(())
    }
    pub fn publish_restored(
        mut self,
        branch: CachedBranch,
        expires: Option<Instant>,
    ) -> Result<(), ServerCacheError> {
        self.publish_inner(Some(branch), Some(expires))
    }
    fn publish_inner(
        &mut self,
        branch: Option<CachedBranch>,
        restored: Option<Option<Instant>>,
    ) -> Result<(), ServerCacheError> {
        let mut state = self
            .cache
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        let mut result = self
            .flight
            .result
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        state
            .flights
            .retain(|flight| !Arc::ptr_eq(flight, &self.flight));
        self.finished = true;
        if matches!(*result, FlightResult::Invalidated) {
            self.flight.ready.notify_all();
            return Err(ServerCacheError::Unavailable);
        }
        if restored
            .flatten()
            .is_some_and(|expiry| expiry <= Instant::now())
        {
            *result = FlightResult::Retry;
            self.flight.ready.notify_all();
            return Err(ServerCacheError::Unavailable);
        }
        if let (Some(disk), Some(identity)) = (&self.cache.0.disk, &self.flight.persistence) {
            let saved = if restored.is_some() {
                if !disk.available() {
                    Err(ServerCacheError::PersistenceUnavailable)
                } else {
                    disk.restored();
                    Ok(())
                }
            } else if let Some(branch) = &branch {
                disk.put(
                    identity,
                    &self.flight.key.url,
                    &self.flight.tags,
                    self.seconds,
                    self.stale_seconds,
                    branch,
                )
            } else {
                disk.remove(identity, &self.flight.key.url)
            };
            if let Err(error) = saved {
                *result = FlightResult::Retry;
                self.flight.ready.notify_all();
                return Err(error);
            }
        }
        *result = FlightResult::Retry;
        state.entries.retain(|entry| entry.key != self.flight.key);
        state.bytes = state.entries.iter().map(|entry| entry.bytes).sum();
        if let Some(branch) = branch {
            let bytes = branch.output.as_ref().map_or(0, |bytes| bytes.len())
                + branch.packet.encode().len()
                + branch
                    .head
                    .as_ref()
                    .map_or(0, crate::ClientDocument::cache_bytes)
                + branch.fallback.as_str().len()
                + self.flight.key.url.len()
                + self.flight.tags.iter().map(String::len).sum::<usize>();
            if bytes <= self.cache.0.limits.bytes {
                state.sweep(Instant::now());
                while state.entries.len() >= self.cache.0.limits.entries
                    || state.bytes + bytes > self.cache.0.limits.bytes
                {
                    let removed = state.entries.remove(0);
                    state.bytes -= removed.bytes;
                    for flight in &state.flights {
                        if flight.key == removed.key {
                            if let Ok(mut result) = flight.result.lock() {
                                *result = FlightResult::Invalidated;
                            }
                            flight.ready.notify_all();
                        }
                    }
                }
                let branch = Arc::new(branch);
                let now = Instant::now();
                let expires = restored.unwrap_or_else(|| {
                    self.seconds
                        .map(|seconds| now + Duration::from_secs(u64::from(seconds)))
                });
                state.entries.push(Entry {
                    key: self.flight.key.clone(),
                    tags: self.flight.tags.clone(),
                    branch: Arc::clone(&branch),
                    expires,
                    stale_until: expires.map(|expires| {
                        expires + Duration::from_secs(u64::from(self.stale_seconds))
                    }),
                    retry_at: expires.unwrap_or(now),
                    bytes,
                });
                state.bytes += bytes;
                *result = FlightResult::Ready(branch, expires);
            }
        }
        self.flight.ready.notify_all();
        Ok(())
    }
}
impl Lease {
    pub fn is_output(&self) -> bool {
        self.flight.key.output
    }
    pub fn is_current(&self) -> bool {
        self.flight
            .result
            .lock()
            .is_ok_and(|result| matches!(*result, FlightResult::Pending))
    }
    pub fn fail(mut self, error: crate::PageError) {
        self.release(matches!(
            error,
            crate::PageError::NotFound | crate::PageError::BadRequest
        ));
    }
    fn release(&mut self, retire: bool) {
        if self.finished {
            return;
        }
        if let Ok(mut state) = self.cache.0.state.lock() {
            state
                .flights
                .retain(|flight| !Arc::ptr_eq(flight, &self.flight));
            if let Ok(mut result) = self.flight.result.lock() {
                if matches!(*result, FlightResult::Pending) {
                    if self.flight.background {
                        state.refresh_failures = state.refresh_failures.saturating_add(1);
                        if retire {
                            state.entries.retain(|entry| entry.key != self.flight.key);
                            if let (Some(disk), Some(identity)) =
                                (&self.cache.0.disk, &self.flight.persistence)
                            {
                                let _ = disk.remove(identity, &self.flight.key.url);
                            }
                        } else if let Some(entry) = state
                            .entries
                            .iter_mut()
                            .find(|entry| entry.key == self.flight.key)
                        {
                            let retry = Instant::now() + Duration::from_secs(1);
                            entry.retry_at = retry.min(entry.stale_until.unwrap_or(retry));
                        }
                        state.bytes = state.entries.iter().map(|entry| entry.bytes).sum();
                    }
                    *result = FlightResult::Retry;
                }
            }
            self.flight.ready.notify_all();
            self.finished = true;
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.release(false);
    }
}
