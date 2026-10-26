# Public server data cache

Alpha.40 adds an explicit in-memory cache for complete client page branches,
without third-party dependencies. Buffered SSR, native fallback rendering and
typed navigation share validated layout/page seeds, title, metadata and fallback
data. Buffered custom document heads are retained with the validated branch. Complete HTML documents and live components are constructed separately
for each response; the native fallback fragment is retained with its data.

Listed pre-rendered URLs can instead opt into
[on-demand output regeneration](prerender-revalidation.md). That separate policy
shares this handle's budgets and invalidation while retaining the complete HTML.
It also supports [timed output refresh](prerender-refresh.md) through the same
fixed worker pool.

## Opt in

Attach a handle before building the native router and declare public routes:

```rust,no_run
use ruvane::{App, ServerCache, ServerCachePolicy};
fn configure(app: &mut App) -> Result<ServerCache, Box<dyn std::error::Error>> {
    let cache = ServerCache::default();
    app.server_cache(cache.clone());
    app.client_page_server_cache("article",
        ServerCachePolicy::public(30)?.tag("articles")?)?;
    Ok(cache)
}
```

The named client page must already exist. Server caching defaults to disabled;
`ServerCachePolicy::no_store()` restores that default. An enabled policy without
an attached handle refuses native router construction. Ordinary native pages,
static export, endpoint handlers, actions and assets are outside this cache.
`FilePage::client(...).server_cache(policy)` supplies the same declaration for
file routes. Native-only `FilePage::server` rejects an enabled policy atomically.
The shared declaration compiles for Wasm, but browser activation does not access
the server cache. No browser ABI, protocol envelope or runtime asset is added.

A public declaration covers the entire branch, including every ancestor layout
and the native fallback. All those values must be public and depend only on the
full URL, immutable application configuration and explicitly invalidated public
application data. They must not vary with locale/identity/custom headers, method
or an unkeyed authorization decision. The cache is not a permission system.
GET loaders and component factories remain bounded, read-only trusted code.

Cookie or Authorization headers, including empty values, bypass lookup, waiting
and publication. A page or layout loader's `no_store()` declines publication;
its result is not shared with waiting requests. Errors, panics, invalid seeds,
foreign canonicals, oversized packets and failed response construction are not
stored. These loader checks run on a miss, so a public declaration must not rely
on a header-dependent no_store decision to make an otherwise private route safe.

## Keys and lifetime

Each constructed router receives a separate namespace, even when built from the
same App or sharing one handle. The namespace isolates its immutable declaration,
public origin and runtime revision. Within it, the key is the registered page
and complete validated URL, including the raw query. Different query encodings
remain distinct. SSR and typed navigation for the same URL share data. A native
query variant such as `?native=yes` has its own key.

Freshness is 1..=300 seconds, starting after successful response construction.
By default entries expire without stale fallback or background regeneration.
Alpha.41 optionally adds [background revalidation](background-revalidation.md)
with a bounded stale window and explicit native worker pool. Cache hits
move entries to the most recently used position; insertion evicts the least
recently used entries when a storage budget is reached. An entry larger than
the payload budget is returned normally but is neither retained nor shared.

Browser navigation caching remains independently opt-in. Successful navigation
max-age is capped by server freshness, using the remaining whole seconds on a
hit. Below one second, navigation sends no-store. HTML keeps its existing
no-store policy. A server hit cannot mint another full navigation lifetime for
old data. Errors and private responses retain their existing no-store policy.

Actual progressive navigation and initial document streams bypass this cache,
including when a complete entry already exists. Buffered requests for a route
that also supports streaming can still use the cache. No partial branch or
stream is retained. Initial hydration still adopts the embedded final snapshot.

## Invalidation after a mutation

Clone the handle into trusted native action/endpoint closures. After validating
the mutation and committing its data, invalidate the relevant public data:

```rust,no_run
use ruvane::{ServerCache, ServerCacheError};
fn article_saved(cache: &ServerCache) -> Result<(), ServerCacheError> {
    cache.invalidate_path("/articles/hello")?;
    cache.invalidate_tag("articles")?;
    Ok(())
}
```

`invalidate_path` matches the exact query-free path and all its query variants;
it does not invalidate child paths. `invalidate_tag` matches declared tags.
`clear` invalidates the whole handle. All three affect every router sharing the
handle and return the number of retained entries removed. Tags use the existing
ASCII page-name profile (1..=48 bytes, beginning with a letter); a policy has at
most eight unique tags. Paths use InternalUrl validation and reject queries,
fragments, external URLs and traversal. Untrusted mutation input must not select
arbitrary invalidation scope without application checks.

Matching active loads are retired and waiting requests wake. Their late results
cannot enter the cache or complete as successful cache loads. A retired owner or
waiter receives a fixed no-store 503. Already adopted cache hits can finish their
response, as with any request that read data before a mutation. Invalidation
does not roll back the application's committed mutation or cancel synchronous
Rust code. Handle clones must refer to the same cache used by the server.

Server invalidation does not purge caches in other browser documents. Existing
action notifications invalidate the acting document; other documents refresh
explicitly or reach their bounded freshness. There is no cross-process sharing,
disk storage, distributed invalidation or durable transaction coupling.

## Concurrent loading and bounds

For an eligible miss, one request loads and validates the branch outside cache
locks. Requests for the same key wait for that result; distinct keys can load
concurrently. Successful, storable results are shared. Uncacheable results wake
waiters to execute their own uncached load once. No private seed or error reply
is passed between requests. Dropping or unwinding an owner releases its slot.

Waits have an absolute deadline and a fixed waiter budget. A full load/waiter
budget, timeout, unavailable state or retired load returns a fixed no-store 503,
without launching a duplicate public cache loader. The browser treats this as
a transport failure and retains the displayed page and head. These limits do
not interrupt an executing synchronous loader; application callbacks must stay
bounded and must not recursively wait on the same cache key.

| Budget | Default | Maximum |
|---|---:|---:|
| Retained entries | 32 | 128 |
| Accounted payload bytes | 2 MiB | 8 MiB |
| Active cache loaders | 8 | 16 |
| Waiting requests per handle | 64 | 64 |
| Wait deadline | 1000 ms | 1000 ms |
| Freshness per policy | explicit | 300 s |
| Unique tags per policy | 0 | 8 |

Use `ServerCacheLimits::new(entries, bytes, loads, wait_ms)` with positive
budgets to lower limits, then `ServerCache::new(limits)`. `stats()` sweeps expiry
and returns aggregate entry, byte, load, waiter, hit, miss and wait counts. It
does not expose paths, tags or cached data. Counters saturate rather than wrap.

The byte budget accounts for encoded packet, fallback, buffered head, URL and tag bytes, not
exact allocator overhead or total process memory. Fixed count bounds constrain
structural overhead; active responses and waiters can retain bounded shared
payloads after eviction. Existing packet, HTML, view and server worker limits
still apply. Cache budgets do not sandbox callback CPU or allocations.
