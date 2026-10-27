# Native request quotas

`RateLimiter` bounds requests reaching selected App guards. It uses only `std`,
with a shared mutex, monotonic time and integer token-bucket credits. It starts no
threads and performs no network or disk I/O. Clones share quotas; a new process
starts empty. The backend and session keys are excluded from Wasm.

## Policies and storage

```rust
use ruvane::{RateLimiter, RateLimitLimits, RateLimitPolicy};
let limiter = RateLimiter::new(
    RateLimitPolicy::new(5, 10).unwrap(),
    RateLimitLimits::new(128, 32 * 1024).unwrap(),
);
```

This permits an initial burst of five requests and replenishes five tokens every
ten seconds, continuously. It is not a fixed or sliding count window. A denied
request consumes no token. Fractional refill credit is retained; repeated denials
do not move the next allowance farther into the future. Retry deltas round up to
whole seconds and describe the next token, not a guaranteed reservation.

Capacity is 1 through 1,000,000 requests; period is 1 through 86,400 seconds. Storage
defaults to 1,024 entries and 256 KiB of route-pattern bytes, with hard maxima of
4,096 entries and 4 MiB. Pattern length is also bounded by the existing route API.
The byte limit measures stored UTF-8 pattern copies; fixed bucket, hash-table and
opaque session-key overhead is bounded separately by the entry count.

Every check removes fully refilled buckets. Outstanding debt is never evicted to
admit a new key. Entry or byte saturation returns `RateLimitError::Full`; guards
map this and internal errors to 503. Existing buckets keep their debt. `stats()`
also performs cleanup and returns aggregate allowed, limited and capacity-rejection
counters. Counters saturate at `u64::MAX`. `clear()` explicitly resets all quotas
while preserving those counters. Neither Debug nor statistics includes patterns,
cookies, session identifiers or payloads.

## Guards

```rust
# use ruvane::*;
# fn configure(app: &mut App, store: SessionStore) -> Result<(), AppError> {
let limiter = RateLimiter::new(
    RateLimitPolicy::new(5, 10).unwrap(),
    RateLimitLimits::default(),
);
app.guard_branch("/account", store.csrf_guard())?;
app.guard_branch("/account", limiter.session_guard(store.clone()))?;
# Ok(())
# }
```

`guard()` shares a quota among all callers of each selected route pattern.
`session_guard(store)` gives each live session family its own quota and groups
anonymous, expired and invalid session identifiers into one shared global quota
across that limiter's guarded routes. Changing routes or cookies cannot reset this
anonymous pool. A valid session allowance returns Private. An anonymous allowance
returns Allow. These decisions are quota checks, not identity verification or
authorization. Keep session/CSRF guards where access requires them.

Session keys contain a native store identity and family number, never the cookie
identifier. Rotation retains the quota even though the cookie and CSRF token
change. A new session family gets a new quota; combine a shared guard with a
session guard where both aggregate traffic and per-session traffic need limits.
Apply the global anonymous quota to session creation as well to bound churn. Native
session resolution by the limiter does not refresh idle time or expose data.
Other session/CSRF guards and application lookups can refresh idle time independently.

Guards use the selected pattern, not the raw path, query, capture or forwarded
headers. Dynamic captures share their declared pattern. All supported methods
at the same pattern share a bucket when using the same limiter. Per-route checks group
unmatched paths into one extra pattern scope; the anonymous session guard keeps
using its global pool. Unknown-path churn cannot create arbitrary buckets.
Different limiter instances have independent state
and can use different policies.

Use `App::guard`, `guard_branch`, local page/endpoint guards or native `guard.rs`.
For a file guard, keep the limiter in shared native application state; constructing
one inside the callback for every request would reset the quota. A limiter held
only by the returned guard is retained through its clone. Guard callbacks stay
out of Wasm through the existing native registration rules.

HTTP and route validation precede guards. Place authorization and CSRF guards
before quota guards when rejected requests should not spend an allowance. A later
guard refusal, cancelled request or handler failure does not refund an allowance.
App guards run before public caches, persistent restoration, loaders, refresh
scheduling and endpoint handlers. Guarded responses remain no-store. Framework
assets and manually added Router handlers are outside App guard scope.

Low-level handlers can use `check("/trusted/pattern")` or
`check_session("/trusted/pattern", &session)`. These return Allowed or Limited with
a typed `RetryAfter`, or an error. A stale/revoked/expired native handle returns
InvalidSession. Never put credentials or arbitrary user-controlled scopes in these
calls. Check before mutation and map capacity/errors conservatively.

## HTTP and browser behavior

Quota guards return status 429 with a bounded positive `Retry-After` delta and
`Cache-Control: no-store`. `GuardDecision::RateLimited(RetryAfter)` supports custom
guards. `RetryAfter::new(seconds)` accepts 1 through 86,400 seconds; raw text and
HTTP-date values are outside this typed profile. `Response::too_many_requests`
and `with_retry_after` build manual 429 responses without arbitrary headers.
Changing the status away from 429 suppresses the retry header.

Navigation uses the reserved read-only root error component with title
"Too many requests", no old layouts, no mutable seed and no cache reuse. SSR,
initial hydration and client navigation accept the same closed error shape. The
Retry-After header is transport metadata and is not embedded in navigation seeds.

For mutation guard failures, the framework additionally emits the fixed
`Ruvane-Action-Rejected: rate-limit` marker. Together with a valid no-store 429 and
retry delta, this identifies a pre-handler refusal. For POST actions, it proves
to the bridge that the route handler was skipped. It delivers `ActionFailure::RateLimited` to the Rust component and `"rate-limited"`
from `controller.whenIdle()`. `controller.retryAfter()` reads the last bounded
retry delta, or null. The runtime does not set uncertain mutation state for this
confirmed refusal and never retries the POST automatically.

A manual 429 returned by an already executed handler has no guard marker. The
bridge keeps the conservative Rejected/uncertain behavior for it, and for malformed
or ambiguous 429 metadata. An ordinary response must never assert non-execution
just because its status is 429. If a CSRF token GET returns a valid no-store 429,
the bridge delivers RateLimited without sending the subsequent POST.

ABI v15 includes action failure kind 5 and the closed 429 navigation error shape. It
keeps the same 22 function exports and four runtime assets. Rebuild the Wasm and
use its matching bridge; ABI v11 modules and hydration roots are rejected.

## Scope

This is process-local admission control after bounded request parsing. It does
not limit connection bandwidth, interrupt callbacks, coordinate multiple workers
in separate processes, persist quotas across restarts or provide DDoS protection.
It does not identify clients from `X-Forwarded-For` or other untrusted headers.
The existing loopback-only transport and production deployment limits still apply.
Protect any application endpoint that calls `clear()` as an administrative action.

The 429 and Retry-After behavior follows
[RFC 6585 section 4](https://www.rfc-editor.org/rfc/rfc6585.html#section-4).
