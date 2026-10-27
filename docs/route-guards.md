# Server route guards

`RouteGuard` runs on the native server before an App page, endpoint, cache lookup
or loader. It makes one typed decision:

| Decision | Result |
| --- | --- |
| `Allow` | Continue; eligible public server output may be reused. |
| `Private` | Continue with all page cache lookup, storage and refresh bypassed for this request. |
| `Forbidden` | Stop with 403. No page loader or endpoint handler runs. |
| `Unavailable` | Stop with 503. No page loader or endpoint handler runs. |

An unwinding guard panic produces a generic 500. Guarded responses use
`Cache-Control: no-store`, including successful `Allow` responses, so navigation
and prefetch caches cannot retain an earlier access decision. Native public data
caches remain available after an `Allow` decision. `Private` is sticky: a later
`Allow` cannot restore caching. Credentials also bypass public data caches.

## Application and local guards

```rust
# use ruvane::{App, GuardDecision, RouteGuard};
# fn configure(app: &mut App) -> Result<(), ruvane::AppError> {
#[cfg(not(target_family = "wasm"))]
app.guard_branch("/account", RouteGuard::new(|context| {
    // Replace with the application's verified access policy.
    if context.request().header("x-maintenance") == Some("1") {
        GuardDecision::Unavailable
    } else {
        GuardDecision::Private
    }
}))?;
# Ok(())
# }
```

`App::guard` applies to all App pages, endpoints and unmatched GET/HEAD page
requests. `guard_branch` applies to a segment prefix, including its descendants
and unknown URLs under it. Literal segments compare decoded validated values;
`/account` also covers `/%61ccount`, but does not cover `/accounting`.
`{parameter}` scope segments match a single segment. Scopes cannot have a trailing
slash except `/`.

Application guards run first in registration order. Matching branch guards follow
from shallowest to deepest, retaining registration order at equal depth. Local
page or endpoint guards run last. A denial stops the chain.

`App::page_guard(name, guard)` and `FilePage::guard(guard)` apply to that page's
GET/HEAD requests. A POST endpoint at the same URL needs its own guard or an
inherited guard. `FileEndpoint::guard(guard)` applies to all methods declared by
that endpoint. Public assets, framework runtime/development assets and handlers
added manually to the low-level Router are outside App guard scope.

## File routes

Place `guard.rs` in the directory whose entire branch needs guarding:

```text
src/routes/
  account/
    guard.rs
    page.rs
    settings/page.rs
    update/route.rs
```

```rust
// src/routes/account/guard.rs
pub fn guard() -> ruvane::RouteGuard {
    ruvane::RouteGuard::new(|_| ruvane::GuardDecision::Private)
}
```

The generated registry excludes the whole `guard.rs` module and its registration
from Wasm, as it does for `route.rs`. Use native cfg for programmatic guards or
`FilePage::guard` inside shared code: `RouteGuard::new` is native-only. The browser
registry contains routes and components; it does not contain guard callbacks.
`FileRouteRegistry::guard(source, pattern, guard)` supports manual atomic batches.

## Request validation and caching

The transport's bounded HTTP checks and POST origin, content type and body checks
precede guards. Router revision, query, form and selected capture validation also
precede guards. Unsupported methods are rejected by the router. A guard sees the
selected page/endpoint pattern, optional page name and the original read-only
request, including decoded captures, query and form fields. Captures belong to
the selected route, not to guard scope parameter names.

`GuardContext::cache_bypass()` includes credentials and preceding `Private`
decisions. After a successful private chain, `Request::cache_bypass()` also
reports true in page/layout loaders and endpoint handlers. The framework does not
invent Cookie or Authorization headers.

Guards run before immutable pre-render lookup, in-memory data/output lookup,
persistent restoration, coalesced loads and scheduling background refresh. A
private or denied request cannot read, replace or schedule public output. Work
already started by an earlier allowed request may finish; policy changes do not
invalidate data. Use cache invalidation separately when public output changes.

Pre-render declarations still describe **public build artifacts**. Build planning
does not run guards and executes the explicitly selected export loaders. Never
put private content into a pre-render declaration or use a guard to make an
artifact private. Use live loaders with `Private` for user-specific output.
Native `PageMode::Static` export refuses guarded pages before rendering, since a
static server cannot enforce their callbacks.

## Browser failures and limits

HTML and SPA requests receive the same 403/503 status and fixed public message.
SPA denials replace the complete page/layout branch, clear its data and canonical
metadata, and set noindex. Initial denied HTML hydrates the same built-in read-only
component. Application error factories need no new states. The reserved component
ID is `GUARD_COMPONENT_ID` (`u32::MAX`); applications cannot register it as a page,
layout or presentation. Shells retained by SPA navigation must contain public
content: guards protect route branches, not previously displayed shell islands.
Initial denial HTML skips the application shell entirely.

There are at most 64 guards per App and 16 inherited/local guards per request.
Invalid declarations fail atomically without executing callbacks. Guard callbacks
are trusted synchronous code: CPU, allocations and blocking I/O cannot be
preempted. Panics with abort semantics cannot be recovered.

This API supplies access decision hooks. Authentication, verified session lookup,
identity storage, login/logout and CSRF tokens remain application responsibilities.
A header's presence is never proof of identity. Browser navigation is not an
access authority; every protected request must be guarded on the server.

## Native sessions

`SessionStore::guard()` resolves its configured cookie and returns Private for a
live session, Forbidden for invalid/absent credentials and Unavailable for failed
store access. See [memory sessions](sessions.md) for lifecycle and platform limits.
