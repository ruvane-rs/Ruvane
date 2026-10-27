use crate::{Cookie, GuardDecision, Request, RouteGuard, SameSite};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
mod entropy;
use entropy::Entropy;

/// Maximum live sessions in one store.
pub const MAX_SESSION_ENTRIES: usize = 1024;
/// Maximum retained session payload bytes in one store.
pub const MAX_SESSION_STORE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum logical bytes in one session's data.
pub const MAX_SESSION_DATA_BYTES: usize = 4096;
/// Maximum fields in one session's data.
pub const MAX_SESSION_FIELDS: usize = 16;
/// Maximum UTF-8 bytes in one field value.
pub const MAX_SESSION_VALUE_BYTES: usize = 1024;
/// Maximum idle or absolute lifetime, in seconds.
pub const MAX_SESSION_LIFETIME_SECONDS: u32 = 7 * 24 * 60 * 60;
/// Number of OS-generated random bytes in a session identifier.
pub const SESSION_ID_BYTES: usize = 32;

/// Session errors contain no credentials, identifiers, paths or payload values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionError {
    /// The requested count, byte or lifetime bounds are invalid.
    InvalidLimits,
    /// A field name, value, duplicate or payload budget is invalid.
    InvalidData,
    /// The cookie name, scope or attributes are outside the session profile.
    InvalidCookie,
    /// The live-entry or aggregate payload capacity is exhausted.
    Full,
    /// The OS random source cannot provide a complete fresh identifier.
    EntropyUnavailable,
    /// The native OS has no supported random source in this release.
    UnsupportedPlatform,
    /// A handle belongs to a different store.
    InvalidHandle,
    /// A store lock or counter is unavailable. Access must fail closed.
    Unavailable,
}
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "invalid session limits",
            Self::InvalidData => "invalid session data",
            Self::InvalidCookie => "invalid session cookie policy",
            Self::Full => "session store capacity exhausted",
            Self::EntropyUnavailable => "session random source unavailable",
            Self::UnsupportedPlatform => "session random source unsupported on this platform",
            Self::InvalidHandle => "session handle belongs to another store",
            Self::Unavailable => "session store unavailable",
        })
    }
}
impl std::error::Error for SessionError {}

/// Bounded server-only string fields. Debug omits keys and values.
/// Creating a session with these fields does not verify their application identity.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct SessionData {
    fields: Vec<(String, String)>,
    bytes: usize,
}
impl fmt::Debug for SessionData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionData")
            .field("fields", &self.fields.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}
impl SessionData {
    /// Adds a unique ASCII form-name field with bounded UTF-8 text, atomically.
    pub fn insert(&mut self, name: &str, value: &str) -> Result<(), SessionError> {
        if crate::forms::valid_name(name).is_err()
            || value.len() > MAX_SESSION_VALUE_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(SessionError::InvalidData);
        }
        let bytes = 6 + name.len() + value.len();
        if self.get(name).is_some()
            || self.fields.len() == MAX_SESSION_FIELDS
            || self.bytes + bytes > MAX_SESSION_DATA_BYTES
        {
            return Err(SessionError::InvalidData);
        }
        self.fields.push((name.into(), value.into()));
        self.bytes += bytes;
        Ok(())
    }
    /// Reads server data. These values are never sent to Wasm implicitly.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
    /// Returns the logical payload budget used, including field framing.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    /// Returns the field count.
    pub fn len(&self) -> usize {
        self.fields.len()
    }
    /// Returns whether no fields are stored.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Validated payload, capacity and monotonic expiration bounds.
#[derive(Clone, Copy, Debug)]
pub struct SessionLimits {
    entries: usize,
    bytes: usize,
    data_bytes: usize,
    idle: u32,
    absolute: u32,
}
impl SessionLimits {
    /// Sets positive budgets. Idle lifetime cannot exceed absolute lifetime.
    pub fn new(
        entries: usize,
        bytes: usize,
        data_bytes: usize,
        idle_seconds: u32,
        absolute_seconds: u32,
    ) -> Result<Self, SessionError> {
        if !(1..=MAX_SESSION_ENTRIES).contains(&entries)
            || !(1..=MAX_SESSION_STORE_BYTES).contains(&bytes)
            || !(1..=MAX_SESSION_DATA_BYTES).contains(&data_bytes)
            || idle_seconds == 0
            || idle_seconds > absolute_seconds
            || absolute_seconds > MAX_SESSION_LIFETIME_SECONDS
        {
            return Err(SessionError::InvalidLimits);
        }
        Ok(Self {
            entries,
            bytes,
            data_bytes,
            idle: idle_seconds,
            absolute: absolute_seconds,
        })
    }
}
impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            entries: 128,
            bytes: 1024 * 1024,
            data_bytes: MAX_SESSION_DATA_BYTES,
            idle: 1800,
            absolute: 28800,
        }
    }
}

/// Host-only HttpOnly session cookie policy. Defaults to SameSite=Strict and Path=/.
#[derive(Clone, Debug)]
pub struct SessionCookie(Cookie);
impl SessionCookie {
    /// Configures a Secure cookie. The current loopback HTTP server does not provide TLS.
    pub fn secure(name: &str) -> Result<Self, SessionError> {
        Cookie::new(name, "")
            .and_then(|cookie| cookie.same_site(SameSite::Strict))
            .map(Self)
            .map_err(|_| SessionError::InvalidCookie)
    }
    /// Explicitly configures the local loopback HTTP profile without Secure.
    pub fn local_http(name: &str) -> Result<Self, SessionError> {
        Cookie::local_http(name, "")
            .and_then(|cookie| cookie.same_site(SameSite::Strict))
            .map(Self)
            .map_err(|_| SessionError::InvalidCookie)
    }
    /// Narrows the path, using the cookie API's prefix and path validation.
    pub fn path(mut self, path: &str) -> Result<Self, SessionError> {
        self.0 = self.0.path(path).map_err(|_| SessionError::InvalidCookie)?;
        Ok(self)
    }
    /// Selects Strict or Lax. Cross-site None is outside this session profile.
    pub fn same_site(mut self, policy: SameSite) -> Result<Self, SessionError> {
        if policy == SameSite::None {
            return Err(SessionError::InvalidCookie);
        }
        self.0 = self
            .0
            .same_site(policy)
            .map_err(|_| SessionError::InvalidCookie)?;
        Ok(self)
    }
    /// Returns the case-sensitive cookie name used by request lookups.
    pub fn name(&self) -> &str {
        self.0.name()
    }
    /// Creates the scoped removal cookie without revoking server state.
    pub fn removal(&self) -> Cookie {
        self.0.removal()
    }
    fn issue(&self, token: &[u8; SESSION_ID_BYTES], seconds: u32) -> Result<Cookie, SessionError> {
        let mut value = String::with_capacity(SESSION_ID_BYTES * 2);
        for byte in token {
            use std::fmt::Write;
            write!(value, "{byte:02x}").map_err(|_| SessionError::Unavailable)?;
        }
        let cookie = if self.0.is_secure() {
            Cookie::new(self.name(), &value)
        } else {
            Cookie::local_http(self.name(), &value)
        };
        cookie
            .and_then(|cookie| cookie.path(self.0.cookie_path()))
            .and_then(|cookie| cookie.same_site(self.0.same_site_policy()))
            .and_then(|cookie| cookie.max_age(seconds))
            .map_err(|_| SessionError::InvalidCookie)
    }
}

struct Record {
    family: u64,
    data: SessionData,
    touched: Instant,
    expires: Instant,
}
struct State {
    records: HashMap<[u8; SESSION_ID_BYTES], Record>,
    bytes: usize,
    next: u64,
    entropy: Entropy,
    #[cfg(test)]
    now: Option<Instant>,
}
impl State {
    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(now) = self.now {
            return now;
        }
        Instant::now()
    }
    fn prune(&mut self, now: Instant, limits: SessionLimits) {
        self.records.retain(|_, record| {
            now < record.expires
                && now.saturating_duration_since(record.touched)
                    < Duration::from_secs(limits.idle.into())
        });
        self.bytes = self.records.values().map(|record| record.data.bytes).sum();
    }
    fn fresh_token(&mut self) -> Result<[u8; SESSION_ID_BYTES], SessionError> {
        for _ in 0..4 {
            let token = self.entropy.read()?;
            if token != [0; SESSION_ID_BYTES] && !self.records.contains_key(&token) {
                return Ok(token);
            }
        }
        Err(SessionError::EntropyUnavailable)
    }
}
struct Inner {
    state: Mutex<State>,
    limits: SessionLimits,
}

/// Native-only handle for one resolved session generation. Debug redacts all data.
/// Data access rechecks expiry and the current generation; a handle is not a permanent grant.
#[derive(Clone)]
pub struct Session {
    inner: Weak<Inner>,
    family: u64,
    token: [u8; SESSION_ID_BYTES],
}
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}
impl Session {
    /// Clones bounded server data if this generation is still current and live.
    /// This does not extend idle lifetime. Previously cloned data cannot be recalled.
    pub fn data(&self) -> Result<Option<SessionData>, SessionError> {
        let Some(inner) = self.inner.upgrade() else {
            return Ok(None);
        };
        let mut state = inner.state.lock().map_err(|_| SessionError::Unavailable)?;
        let now = state.now();
        state.prune(now, inner.limits);
        Ok(state
            .records
            .get(&self.token)
            .filter(|record| record.family == self.family)
            .map(|record| record.data.clone()))
    }
}

/// Aggregate live-state counters with no identifiers or payload values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionStats {
    /// Live session families after expiry cleanup.
    pub entries: usize,
    /// Logical data bytes retained by those families.
    pub data_bytes: usize,
}

/// Bounded native memory store. Clones share state; restarts discard every session.
/// The only production entropy provider is the supported OS random device.
#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Inner>,
    cookie: SessionCookie,
}
impl fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionStore")
            .field("limits", &self.inner.limits)
            .finish_non_exhaustive()
    }
}
impl SessionStore {
    /// Opens the OS random source and probes it before creating an empty store.
    /// Supported providers are macOS and Linux x86_64/aarch64; failure never selects a fallback generator.
    ///
    /// ```no_run
    /// use ruvane::{SessionCookie, SessionData, SessionLimits, SessionStore};
    /// let store=SessionStore::new(SessionLimits::default(),SessionCookie::local_http("example_session")?)?;
    /// let mut data=SessionData::default();data.insert("user_id","demo-user")?;
    /// let cookie=store.create(data)?;
    /// let response=ruvane::Response::text("session created").with_cookie(cookie)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(limits: SessionLimits, cookie: SessionCookie) -> Result<Self, SessionError> {
        let entropy = Entropy::open()?;
        Ok(Self::from_entropy(limits, cookie, entropy))
    }
    fn from_entropy(limits: SessionLimits, cookie: SessionCookie, entropy: Entropy) -> Self {
        Self {
            inner: Arc::new(Inner {
                limits,
                state: Mutex::new(State {
                    records: HashMap::new(),
                    bytes: 0,
                    next: 0,
                    entropy,
                    #[cfg(test)]
                    now: None,
                }),
            }),
            cookie,
        }
    }
    /// Stores application-verified data under a fresh 256-bit identifier and returns its cookie.
    /// Live entries are never evicted to make room. Construct the response before applying other mutations.
    pub fn create(&self, data: SessionData) -> Result<Cookie, SessionError> {
        let limits = self.inner.limits;
        if data.bytes > limits.data_bytes {
            return Err(SessionError::InvalidData);
        }
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let now = state.now();
        state.prune(now, limits);
        if state.records.len() == limits.entries || state.bytes + data.bytes > limits.bytes {
            return Err(SessionError::Full);
        }
        let family = state.next.checked_add(1).ok_or(SessionError::Unavailable)?;
        let expires = now
            .checked_add(Duration::from_secs(limits.absolute.into()))
            .ok_or(SessionError::Unavailable)?;
        let token = state.fresh_token()?;
        let cookie = self.cookie.issue(&token, limits.absolute)?;
        state.next = family;
        state.bytes += data.bytes;
        state.records.insert(
            token,
            Record {
                family,
                data,
                touched: now,
                expires,
            },
        );
        Ok(cookie)
    }
    /// Resolves only the configured cookie and refreshes idle time on a valid lookup.
    /// Absent, malformed, unknown, rotated and expired identifiers return None.
    pub fn lookup(&self, request: &Request) -> Result<Option<Session>, SessionError> {
        let Some(token) = request
            .cookies()
            .get(self.cookie.name())
            .and_then(parse_token)
        else {
            return Ok(None);
        };
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let now = state.now();
        state.prune(now, self.inner.limits);
        let Some(record) = state.records.get_mut(&token) else {
            return Ok(None);
        };
        record.touched = now;
        Ok(Some(Session {
            inner: Arc::downgrade(&self.inner),
            family: record.family,
            token,
        }))
    }
    /// Atomically replaces the current identifier, retaining data and the original absolute deadline.
    /// A stale generation cannot rotate again. Failed entropy/capacity checks preserve the live record.
    pub fn rotate(&self, session: &Session) -> Result<Option<Cookie>, SessionError> {
        self.rotate_inner(session, None)
    }
    /// Rotates while replacing bounded data, for an application-verified privilege change.
    pub fn rotate_with_data(
        &self,
        session: &Session,
        data: SessionData,
    ) -> Result<Option<Cookie>, SessionError> {
        if data.bytes > self.inner.limits.data_bytes {
            return Err(SessionError::InvalidData);
        }
        self.rotate_inner(session, Some(data))
    }
    fn rotate_inner(
        &self,
        session: &Session,
        data: Option<SessionData>,
    ) -> Result<Option<Cookie>, SessionError> {
        self.check_owner(session)?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let now = state.now();
        state.prune(now, self.inner.limits);
        let Some(current) = state
            .records
            .get(&session.token)
            .filter(|record| record.family == session.family)
        else {
            return Ok(None);
        };
        let bytes = state.bytes - current.data.bytes
            + data.as_ref().map_or(current.data.bytes, |data| data.bytes);
        let expires = current.expires;
        if bytes > self.inner.limits.bytes {
            return Err(SessionError::Full);
        }
        let remaining = expires.saturating_duration_since(now);
        let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0);
        let token = state.fresh_token()?;
        let cookie = self.cookie.issue(
            &token,
            seconds.min(self.inner.limits.absolute.into()) as u32,
        )?;
        let mut record = state
            .records
            .remove(&session.token)
            .ok_or(SessionError::Unavailable)?;
        record.touched = now;
        if let Some(data) = data {
            record.data = data;
        }
        state.records.insert(token, record);
        state.bytes = bytes;
        Ok(Some(cookie))
    }
    /// Revokes a resolved native session family, including a generation rotated since lookup.
    /// A retired cookie cannot obtain such a handle; already resolved handles can still revoke.
    pub fn revoke(&self, session: &Session) -> Result<bool, SessionError> {
        self.check_owner(session)?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let now = state.now();
        state.prune(now, self.inner.limits);
        let Some(token) = state
            .records
            .iter()
            .find_map(|(token, record)| (record.family == session.family).then_some(*token))
        else {
            return Ok(false);
        };
        let record = state
            .records
            .remove(&token)
            .ok_or(SessionError::Unavailable)?;
        state.bytes -= record.data.bytes;
        Ok(true)
    }
    /// Builds the matching removal cookie. Revoke server state separately when a handle resolves.
    pub fn removal_cookie(&self) -> Cookie {
        self.cookie.removal()
    }
    /// Creates a native guard: live session -> Private, invalid/absent -> 403, unavailable -> 503.
    /// Identity and credential verification remain application responsibilities.
    pub fn guard(&self) -> RouteGuard {
        let store = self.clone();
        RouteGuard::new(move |context| match store.lookup(context.request()) {
            Ok(Some(_)) => GuardDecision::Private,
            Ok(None) => GuardDecision::Forbidden,
            Err(_) => GuardDecision::Unavailable,
        })
    }
    /// Revokes all families and preserves the monotonically increasing internal family counter.
    pub fn clear(&self) -> Result<usize, SessionError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let count = state.records.len();
        state.records.clear();
        state.bytes = 0;
        Ok(count)
    }
    /// Returns live aggregate state after removing expired entries.
    pub fn stats(&self) -> Result<SessionStats, SessionError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let now = state.now();
        state.prune(now, self.inner.limits);
        Ok(SessionStats {
            entries: state.records.len(),
            data_bytes: state.bytes,
        })
    }
    fn check_owner(&self, session: &Session) -> Result<(), SessionError> {
        if Weak::ptr_eq(&session.inner, &Arc::downgrade(&self.inner)) {
            Ok(())
        } else {
            Err(SessionError::InvalidHandle)
        }
    }
}
fn parse_token(value: &str) -> Option<[u8; SESSION_ID_BYTES]> {
    if value.len() != SESSION_ID_BYTES * 2 {
        return None;
    }
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut token = [0; SESSION_ID_BYTES];
    for (slot, pair) in token.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *slot = (hex(pair[0])? << 4) | hex(pair[1])?;
    }
    Some(token)
}
#[cfg(test)]
mod tests;
