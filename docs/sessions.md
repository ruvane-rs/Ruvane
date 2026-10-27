# Native memory sessions

Ruvane supplies a bounded native session store with OS-generated opaque identifiers,
idle and absolute expiration, rotation and revocation. It builds on the typed
[cookie API](cookies.md) and [route guards](route-guards.md), using std only.
Session creation does not authenticate a user: applications must verify identity
and authorization before supplying session data.

## Create and resolve

```rust
use ruvane::{SessionCookie, SessionData, SessionLimits, SessionStore};

let store = SessionStore::new(
    SessionLimits::default(),
    SessionCookie::local_http("example_session")?,
)?;
let mut data = SessionData::default();
data.insert("user_id", "verified-application-user")?;
let cookie = store.create(data)?;
let response = ruvane::Response::text("created").with_cookie(cookie)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `SessionCookie::secure` for Secure cookies; use `local_http` explicitly with
the current loopback HTTP server. Policies are host-only, HttpOnly, SameSite=Strict,
Path=/ and a Max-Age aligned with the absolute deadline. `path` narrows scope and
`same_site` allows Strict or Lax. Cross-site None is outside this profile. Cookies
are bearer credentials: anyone holding a valid identifier can present it.

`store.lookup(request)` accepts only the configured cookie, as exactly 64 lowercase
hexadecimal characters representing 32 random bytes. Cookie syntax/duplicate checks
run first. Absent, malformed, unknown, expired and rotated identifiers return None.
URL/query/form fields and Authorization headers are never session-ID sources.

A successful lookup returns a native `Session` handle and refreshes its idle time.
`session.data()` clones bounded data only while that generation remains current and
live; it does not refresh idle time. Failed store locks return an error. Handles
are not serializable and do not provide a permanent authorization grant.

## Guard application routes

```rust
# fn protect(app: &mut ruvane::App, store: &ruvane::SessionStore) -> Result<(), ruvane::AppError> {
app.guard_branch("/account", store.guard())?;
# Ok(())
# }
```

`SessionStore::guard` returns Private for a live session, Forbidden for an absent
or invalid one, and Unavailable when a required store lookup fails. In a file tree,
a native-only guard.rs can obtain the application's shared store and return its
guard. This protects pages, endpoints and unmatched descendants in that branch.
Mutation origin, body-profile and revision checks still precede guard callbacks.

The handle and payload types, store and entropy provider are excluded entirely
from Wasm. Place store construction and session-using handlers in native modules
or native cfg blocks. Shared page declarations can use separate native/Wasm loader
functions; the browser validates page schemas and never executes server loaders.
Explicitly select public presentation fields when building client page data.
Identifiers and server payloads are not added to hydration or navigation frames.

## Rotate and revoke

```rust
# fn rotate(request: &ruvane::Request, store: &ruvane::SessionStore) -> Result<Option<ruvane::Cookie>, ruvane::SessionError> {
let Some(session) = store.lookup(request)? else { return Ok(None) };
store.rotate(&session)
# }
```

Rotation generates a fresh identifier atomically and retires the previous one.
The original absolute deadline is preserved; successful rotation refreshes idle
activity. Cookie Max-Age is rounded up to whole seconds of remaining absolute
lifetime, while the server remains authoritative. `rotate_with_data` also replaces
bounded application-verified data, for example after a privilege change. Only the
current generation can rotate. A stale generation returns None.

```rust
# fn logout(request: &ruvane::Request, store: &ruvane::SessionStore) -> Result<ruvane::Response, Box<dyn std::error::Error>> {
if let Some(session) = store.lookup(request)? {
    store.revoke(&session)?;
}
Ok(ruvane::Response::see_other(ruvane::InternalUrl::new("/")?)
    .with_cookie(store.removal_cookie())?)
# }
```

A resolved native handle can revoke its family even if a concurrent rotation
already changed its identifier. Revocation and rotation serialize under the same
store lock, so a rotation using that handle cannot resurrect a revoked family.
A retired cookie cannot resolve a new handle. A logout request arriving with an
already retired token can remove that browser cookie, but cannot identify a newer
generation's family without a previously resolved native handle.

`removal_cookie()` only constructs the scoped Max-Age=0 cookie; removing a browser
cookie alone does not revoke server state. `clear()` revokes all retained families
without resetting internal family numbers. Store clones share memory; a new store
or process restart has no previous sessions. There is no disk or distributed store.

Return cookies with native POST/303 responses or `post_action_response` and
`Response::action`. Cookie changes retain no-store and browser cache reset. Private
session guards bypass all page cache layers. Cookies modified outside the framework
still need the explicit invalidation/reload described in the cookie guide.

## Bounds and expiration

| Budget | Default | Maximum |
| --- | --- | --- |
| Live entries | 128 | 1024 |
| Aggregate payload bytes | 1 MiB | 4 MiB |
| Per-session payload bytes | 4096 | 4096 |
| Fields per payload | 16 | 16 |
| UTF-8 bytes per value | 1024 | 1024 |
| Idle lifetime | 30 minutes | Absolute lifetime |
| Absolute lifetime | 8 hours | 7 days |

`SessionLimits::new(entries, bytes, data_bytes, idle_seconds, absolute_seconds)`
sets positive bounds. Field names follow the form-name profile, values reject
controls, and duplicate fields fail atomically. Payload byte accounting includes
six framing bytes per field plus names and values. Entry limits separately bound
record/map overhead. These are store budgets, not a Rust allocation sandbox.

Expiry uses monotonic Instant deadlines; wall-clock changes do not extend sessions.
Idle/absolute checks occur during lookup, handle data access, rotation, revocation
and stats. Expired entries are pruned lazily and their capacity reclaimed. A full
store rejects new work instead of evicting live credentials. Entropy or capacity
failure during rotation preserves the live generation. `stats()` exposes only
aggregate live entry and logical data-byte counts.

Expiration is based on Instant. Whether suspend time counts follows the platform's
Instant behavior; no wall-clock durability is promised across process restarts.

## OS source and security scope

macOS reads the OS random device at /dev/urandom. Linux x86_64 and aarch64 read
/dev/random with O_NONBLOCK, and both providers require a character device and a
successful startup probe. Reads must provide every byte. Other native platforms
return UnsupportedPlatform; failed reads return EntropyUnavailable. All-zero or
live-colliding candidates are retried at most four times, then refused. There is
no time/counter/PRNG fallback and no public entropy injection API. Deterministic
providers exist only in private tests. The implementation contains no unsafe code.

Linux's kernel can still wait for early-boot initialization despite O_NONBLOCK.
OS reads and trusted synchronous Rust callbacks are not preempted by the framework.
The device directory and kernel are trusted. See [Linux random(4)](https://man7.org/linux/man-pages/man4/random.4.html)
and the installed Darwin random(4) manual for platform behavior.

Session identifiers and data remain in process memory in plaintext. The store
assumes process memory is protected; it does not provide verifier hashing, memory
zeroization, protection against memory disclosure or persistent encrypted storage.
Debug and errors omit credentials and data. The returned Cookie still intentionally
contains a bearer value; do not log it. See [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).

Previously cloned data or an already authorized in-flight response cannot be
recalled by revocation. Applications must check resource-level authorization, and
must keep retained shells public. Session guards do not make pre-rendered artifacts
private. Add the opt-in [session CSRF guard](csrf.md) for mutations.
Accounts, password verification, rate limiting and production
TLS remain separate work. The included local demo opens an explicitly labeled
demo session and does not authenticate real users.
