# Background server revalidation

Alpha.41 optionally serves a previously validated public branch while a fixed
native worker pool prepares a replacement. The default server cache still
expires entries and loads synchronously, as in alpha.40. There are no new
dependencies, browser assets, Wasm exports or protocol envelopes.

## Opt in explicitly

Declare a fresh lifetime and an additional stale window on a client page:

```rust,no_run
use ruvane::{App, ServerCache, ServerCacheBackgroundLimits, ServerCacheLimits, ServerCachePolicy};
fn configure(app: &mut App) -> Result<ServerCache, Box<dyn std::error::Error>> {
    let cache = ServerCache::with_background(
        ServerCacheLimits::default(),
        ServerCacheBackgroundLimits::new(1, 8)?,
    )?;
    app.server_cache(cache.clone());
    app.client_page_server_cache("article", ServerCachePolicy::public(30)?
        .stale_while_revalidate(60)?.tag("articles")?)?;
    Ok(cache)
}
```

The route must already exist. `FilePage::client(...).server_cache(policy)` accepts
the same policy. Keep worker construction in native code, for example under
`#[cfg(not(target_family = "wasm"))]` in a shared declaration. Policies compile
for both targets; `with_background` returns Unavailable on Wasm. Native router
construction refuses a stale policy without an attached background-enabled
handle. `ServerCache::new/default` starts no workers and keeps its old behavior.

Freshness and the additional stale window each accept 1..=300 seconds. The
example permits adoption for 30 fresh seconds plus at most 60 stale seconds.
The window starts after successful initial loading/response validation, not
after the most recent stale request. Repeated reads and failures cannot move it.

## Request behavior

| Entry state at lookup | Request behavior |
|---|---|
| Fresh | Reuse the validated branch normally. |
| Inside the stale window | Return old data immediately; reserve at most one refresh for that key. |
| Refresh already queued/running | Return the same permitted stale branch without waiting. |
| Queue or load budget full | Return permitted stale data; do not start another loader. |
| Past the hard deadline | Never adopt old data; load normally or wait within the existing cache deadline. |
| Entry invalidated/removed | Never use the retired data; normal cache-miss rules apply. |

Actual initial document streams and progressive navigation bypass this feature.
Buffered SSR, native fallbacks and typed navigation share the same complete
branch data. Cookie and Authorization requests, including empty headers, bypass
lookup, storage and scheduling. They neither read nor replace public entries.

Stale typed navigation always sends no-store. Fresh hits retain alpha.40's
remaining-lifetime cap on navigation max-age. HTML remains no-store. Completing
a refresh affects later GETs; it does not push a replacement into an existing
browser page. Initial hydration still adopts its embedded snapshot without an
extra page GET.

## Validation and publication

A worker loads layouts and the leaf in their normal order, runs component and
packet validation, checks metadata ownership and constructs a complete buffered
response before publication. Replacing a retained entry happens under one cache
lock. A reader sees either the old complete branch or the new complete branch,
including title, seed, metadata and native fallback.

For buffered custom document profiles, the resolved head is retained with the
branch and counted in the payload budget. Stale SSR uses that head rather than
resolving a newer title against older data. When a stale-enabled loader omits
explicit metadata, its resolved custom head supplies the typed metadata
replacement; the same configured canonical-origin checks apply. Explicit loader
metadata remains authoritative. Existing legacy packets remain readable.

An internal loading, validation or response error keeps the old entry only
until its original hard deadline. Failed/declined refresh attempts have a
one-second retry backoff, bounded by that deadline. A result containing page or
layout no_store, a bad request or a missing page retires the old entry. A new
payload too large for the configured cache also removes the old entry. Errors
and uncacheable results are not shared as new public data.

The public assertion covers every layout, page seed, fallback and head. Values
must be safe for any anonymous reader and depend only on URL, immutable router
configuration and explicitly invalidated public application data. A background
job reconstructs a GET with the same validated URL, query and captures, plus
the trusted configured origin. Request headers and body are not retained. Do
not use a public policy for header-, method- or identity-dependent data.

## Invalidation and lifetime

Existing path/tag invalidation and clear remove matching entries and retire
active refresh reservations. Queued retired jobs skip their loaders; running
jobs cannot publish a late result. LRU eviction also retires the evicted entry's
refresh, preventing it from returning after eviction. Invalidation remains
immediate; it never merely marks data eligible for stale serving.

Call invalidation after committing mutations that must take effect without the
stale window. Revocation or deletion must not rely on background discovery.
Requests that already adopted old public data can finish their response, as
with any concurrent read that preceded a mutation. Server invalidation does not
purge another document's existing browser cache or roll back application data.

The native pool starts a fixed number of threads when its handle is constructed.
There are no per-request threads, timers or polling workers. Idle workers wait on
a condition variable. Jobs hold weak router ownership, so an abandoned queued
router is skipped. A running job temporarily owns its router while using it.

`cache.stop_background()` stops accepting jobs, discards queued work and retires
running background reservations. It does not wait for synchronous callbacks to
finish. Those workers exit after their current callback returns; late results
are rejected. Fresh lookups and foreground cache loading remain usable. Dropping
the last pool owner also stops its queue. Callback execution and allocations
remain trusted/bounded application work; Rust code is not forcibly killed.

## Budgets and observability

| Budget | Default background configuration | Maximum |
|---|---:|---:|
| Worker threads per handle | 1 | 4 |
| Queued refresh jobs | 8 | 32 |
| Additional stale window | explicit | 300 seconds |
| Retry interval after failure/decline | 1 second | fixed |

`ServerCacheBackgroundLimits::new(workers, queue)` validates positive limits.
Background queued and running reservations share the existing active cache
load budget with foreground misses; that budget can limit concurrency below
the pool's worker/queue settings. Entry count and payload budgets include stale
entries and buffered heads. This remains an in-memory, per-handle/process cache.

`stats()` adds stale-entry/hit counts, refresh attempts/failures and aggregate
queued/running/alive-worker counts. Attempts count reservations, including ones
declined by a full/stopped queue; failures exclude explicitly invalidated work.
Counts expose no paths, tags, headers or data. Counter snapshots span storage
and worker locks and are observational, not a synchronization API.
