use super::*;
use crate::{Html, InternalUrl, NavigationCachePolicy, NavigationPage};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct CachedBranch {
    pub packet: NavigationPage,
    pub fallback: Html,
    pub policy: NavigationCachePolicy,
}
#[derive(Clone, Copy)]
pub(crate) struct Namespace(u64);
#[derive(Clone, Eq, PartialEq)]
struct Key {
    namespace: u64,
    page: usize,
    url: String,
}
struct Entry {
    key: Key,
    tags: Vec<String>,
    branch: Arc<CachedBranch>,
    expires: Instant,
    bytes: usize,
}
struct Flight {
    key: Key,
    tags: Vec<String>,
    result: Mutex<FlightResult>,
    ready: Condvar,
}
enum FlightResult {
    Pending,
    Ready(Arc<CachedBranch>, Instant),
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
}
struct Inner {
    limits: ServerCacheLimits,
    state: Mutex<State>,
}

/// An explicit in-memory public cache and invalidation handle. Clones share state.
/// Each constructed App router gets a separate namespace, including its revision/origin.
#[derive(Clone)]
pub struct ServerCache(Arc<Inner>);
/// Bounded cache counts; no URLs, tags, headers or page data are exposed.
#[derive(Clone, Copy, Debug)]
pub struct ServerCacheStats {
    /// Fresh retained entries.
    pub entries: usize,
    /// Accounted encoded packet, fallback, key and tag bytes.
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
    /// Creates an empty handle with validated limits. No worker or timer is started.
    pub fn new(limits: ServerCacheLimits) -> Self {
        Self(Arc::new(Inner {
            limits,
            state: Mutex::new(State::default()),
        }))
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
    pub fn invalidate_path(&self, path: &str) -> Result<usize, ServerCacheError> {
        let url = InternalUrl::new(path).map_err(|_| ServerCacheError::InvalidPath)?;
        self.invalidate(|key, _| key.url.split('?').next() == Some(url.as_str()))
    }
    /// Invalidates all branches declaring this validated tag, including active loads.
    pub fn invalidate_tag(&self, tag: &str) -> Result<usize, ServerCacheError> {
        if !crate::page_route::valid_name(tag) {
            return Err(ServerCacheError::InvalidTag);
        }
        self.invalidate(|_, tags| tags.iter().any(|value| value == tag))
    }
    /// Invalidates all entries and retires every active public load.
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
        Ok(removed)
    }
    /// Sweeps expired entries and returns aggregate counters.
    pub fn stats(&self) -> Result<ServerCacheStats, ServerCacheError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        state.sweep(Instant::now());
        Ok(ServerCacheStats {
            entries: state.entries.len(),
            bytes: state.bytes,
            active_loads: state.flights.len(),
            waiting_requests: state.waiters,
            hits: state.hits,
            misses: state.misses,
            waits: state.waits,
        })
    }
    pub(crate) fn access(
        &self,
        namespace: Namespace,
        page: usize,
        url: &InternalUrl,
        policy: &ServerCachePolicy,
    ) -> Result<Access, ServerCacheError> {
        let key = Key {
            namespace: namespace.0,
            page,
            url: url.as_str().into(),
        };
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        state.sweep(Instant::now());
        if let Some(index) = state.entries.iter().position(|entry| entry.key == key) {
            let entry = state.entries.remove(index);
            let access = Access::Hit(Arc::clone(&entry.branch), entry.expires);
            state.entries.push(entry);
            state.hits = state.hits.saturating_add(1);
            return Ok(access);
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
                    FlightResult::Ready(branch, expires) if *expires > Instant::now() => {
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
                                && entry.expires > Instant::now()
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
            key,
            tags: policy.tags.clone(),
            result: Mutex::new(FlightResult::Pending),
            ready: Condvar::new(),
        });
        state.flights.push(Arc::clone(&flight));
        state.misses = state.misses.saturating_add(1);
        Ok(Access::Load(Lease {
            cache: self.clone(),
            flight,
            seconds: policy.seconds.unwrap(),
            finished: false,
        }))
    }
}
impl State {
    fn sweep(&mut self, now: Instant) {
        self.entries.retain(|entry| entry.expires > now);
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
    Hit(Arc<CachedBranch>, Instant),
    Load(Lease),
    Bypass,
}
pub(crate) struct Lease {
    cache: ServerCache,
    flight: Arc<Flight>,
    seconds: u32,
    finished: bool,
}
impl Lease {
    pub fn publish(mut self, branch: Option<CachedBranch>) -> Result<(), ServerCacheError> {
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
        *result = FlightResult::Retry;
        if let Some(branch) = branch {
            let bytes = branch.packet.encode().len()
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
                }
                let branch = Arc::new(branch);
                let expires = Instant::now() + Duration::from_secs(u64::from(self.seconds));
                state.entries.push(Entry {
                    key: self.flight.key.clone(),
                    tags: self.flight.tags.clone(),
                    branch: Arc::clone(&branch),
                    expires,
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
impl Drop for Lease {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Ok(mut state) = self.cache.0.state.lock() {
            state
                .flights
                .retain(|flight| !Arc::ptr_eq(flight, &self.flight));
            if let Ok(mut result) = self.flight.result.lock() {
                if matches!(*result, FlightResult::Pending) {
                    *result = FlightResult::Retry;
                }
            }
            self.flight.ready.notify_all();
        }
    }
}
