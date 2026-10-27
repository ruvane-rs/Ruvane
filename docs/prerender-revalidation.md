# On-demand pre-render regeneration

Alpha.43 adds an explicit regenerable mode for listed public client URLs.
Immutable build snapshots remain the default. This mode uses the existing
ServerCache handle to keep complete buffered HTML, RVI1 hydration data, typed
navigation, metadata and native fallback together until invalidation or eviction.
There are no new dependencies, runtime assets or Wasm exports.

Alpha.44 adds optional [expiry and background refresh](prerender-refresh.md) through
`revalidate_after(seconds)` and `stale_while_revalidate(seconds)`. The policy below
continues to use explicit invalidation only.

## Declare and attach

```rust,no_run
use ruvane::{App, PageParams, PrerenderRevalidationPolicy, ServerCache};
fn configure(app: &mut App) -> Result<ServerCache, Box<dyn std::error::Error>> {
    let cache = ServerCache::default();
    app.server_cache(cache.clone());
    app.client_page_prerender("article", Some(vec![
        PageParams::new(&[("slug", "hello")])?,
        PageParams::new(&[("slug", "rust")])?,
    ]))?;
    app.client_page_revalidation("article", Some(
        PrerenderRevalidationPolicy::new().tag("articles")?,
    ))?;
    Ok(cache)
}
```

The client page must already exist. File routes use
`FilePage::client(...).prerender(params).revalidation(policy)`. Native-only pages,
revalidation without a pre-render declaration, and combinations with an enabled
ServerCachePolicy are rejected. Shared Wasm declarations need no native handle;
native router construction requires one. Configuration executes no loaders.

Tags use ASCII page-name identifiers, at most 48 bytes, with at most eight
deduplicated tags per policy. A policy without tags supports path invalidation.
`client_page_revalidation(name, None)` restores immutable mode for subsequent
routers. Disabling pre-rendering also removes its regeneration policy.

## Read and invalidate

The production build still prepares and embeds the exact declared public URLs
with the same application/Wasm revision. Regenerable runtime reads never adopt
their embedded build data. The first eligible request in a new router namespace,
including after process restart, runs fresh loaders and renders the whole output.
Later HTML and navigation requests use that same validated generation.

After committing a public data change, invalidate through the attached handle:

```rust,no_run
fn after_commit(cache: &ruvane::ServerCache) -> Result<(), ruvane::ServerCacheError> {
    cache.invalidate_path("/articles/hello")?;
    // Or invalidate all branches carrying this tag.
    cache.invalidate_tag("articles")?;
    Ok(())
}
```

The next eligible request blocks on fresh generation. Without a timed policy,
there is no expiry, stale response or background worker. One owner runs per
URL; concurrent HTML/navigation requests wait within the handle's absolute
deadline. Invalidation retires matching active loads, so a generation invalidated
while running cannot publish its output. An already adopted response cannot be
revoked. Coordinate data commits and invalidation in application mutation code;
the framework does not supply a database transaction or an invalidation endpoint.

The cache publishes only a complete validated 200 response. Errors, panics,
invalid seeds/metadata, inconsistent custom heads and oversized output are not
stored. Page or layout `no_store()` responses remain fresh and unshared. An error
uses the existing error/404 presentation, never an old build or cached snapshot.
Eviction and clear similarly require a fresh load on the next read.

## Request and browser behavior

Only anonymous, query-free GET/HEAD requests for explicitly listed URLs enter
this output cache. A Cookie or Authorization header, including an empty one,
every query variant and unlisted parameters retain normal dynamic rendering.
Progressive navigation to a listed eligible URL receives the complete buffered
packet. Build and runtime use the same component schemas and revision.

Runtime generation reconstructs a GET with validated URL/captures and trusted
origin, without headers, body or query, and `is_exporting() == false`. Build
loaders still receive `is_exporting() == true`. The whole branch must be public,
read-only and independent of identity or unkeyed request properties. Build
artifacts remain public even when runtime generation is enabled.

Regenerated HTML and navigation are `no-store`, including when page/layout
browser policies allow caching. Initial hydration adopts the matching RVI1
snapshot without another page GET. Managed metadata and retained layout state
follow the normal navigation contract. Invalidation does not push updates into
an already displayed page: refresh or navigate to perform the next GET.

## Budgets and deployment

Entry/byte, simultaneous-load, waiter and wait-time budgets are shared with
ordinary server caching on the same handle. Complete HTML bytes count toward
storage in addition to seeds, head, fallback, URL and tags. Entries use LRU
eviction. If an individual output exceeds the storage budget, its current fresh
response can succeed but is not retained or shared. HTML and snapshot size limits
still apply. Waiters whose owner cannot publish a retained output return no-store
503 and can retry; they do not start an unbudgeted wave of duplicate loaders.
Exhausted load/wait budgets and retired publication also return no-store 503.
Running synchronous loaders cannot be forcibly cancelled.

Storage is in memory per process and router namespace. Each replica must receive
its own invalidation; restart and eviction reload fresh. No persistent cache,
cross-process invalidation, runtime compilation or new
build revision is supplied. Development stays dynamic. The
[pre-render build contract](client-prerender.md) still applies.

Alpha.45 adds optional [persistent output storage](persistent-prerender.md) for
fresh restore after clean shutdown. Memory-only startup still loads fresh;
interrupted persistent owners require explicit empty recovery.
