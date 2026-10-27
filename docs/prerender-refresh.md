# Timed pre-render output refresh

Alpha.44 adds expiry and optional background refresh to regenerable public client
pages. It uses the existing ServerCache workers, storage and invalidation. The
replacement contains complete HTML, hydration/navigation data, metadata, layout
seeds and native fallback. No dependencies, runtime assets or Wasm exports change.

## Declare the policy

```rust,no_run
use ruvane::{App, PrerenderRevalidationPolicy, ServerCache,
    ServerCacheBackgroundLimits, ServerCacheLimits};
fn configure(app: &mut App) -> Result<ServerCache, Box<dyn std::error::Error>> {
    let cache = ServerCache::with_background(
        ServerCacheLimits::default(),
        ServerCacheBackgroundLimits::new(1, 8)?,
    )?;
    app.server_cache(cache.clone());
    // Register the client page and enumerate its pre-render parameters first.
    app.client_page_revalidation("article", Some(
        PrerenderRevalidationPolicy::new()
            .revalidate_after(60)?
            .stale_while_revalidate(120)?
            .tag("articles")?,
    ))?;
    Ok(cache)
}
```

`FilePage::client(...).prerender(params).revalidation(policy)` accepts the same
policy. Build artifact validation and the explicit public-URL list are unchanged.
Both durations accept 1..=300 seconds. The stale window requires a fresh lifetime
and a native handle created with `with_background`. Keep worker construction in
native code with `#[cfg(not(target_family = "wasm"))]` when sharing declarations.
Native router construction rejects a stale policy without background support.
Wasm shares the policy without starting workers.

`revalidate_after(seconds)` alone uses blocking expiry and needs no workers.
The original `PrerenderRevalidationPolicy::new()` still retains fresh output until
invalidation or eviction. No revalidation policy still means immutable build
output. `max_age_seconds()` and `stale_seconds()` expose the declared durations.

## Read behavior

For a policy with 60 fresh seconds and 120 additional stale seconds:

| State at lookup | Behavior |
|---|---|
| No retained output, including first read after restart | Generate fresh output or wait for the current owner. |
| First 60 seconds after successful publication | Serve the retained complete output. |
| Next 120 seconds | Serve old output immediately and reserve at most one background refresh for that URL. |
| Refresh already queued/running, or queue/load budget full | Serve permitted old output without starting another loader. |
| Past the hard deadline | Never adopt old output; generate fresh or wait within the handle deadline. |
| Explicitly invalidated/evicted | Never adopt the retired output; fence any old active publication. |

The first eligible GET/HEAD after expiry triggers refreshing. There is no polling
thread, timer job or periodic crawl. Durations start after complete output has
been validated and published, not after lookup or build time. A successful
replacement starts its own lifetime. Reads and failed refreshes cannot extend an
old deadline. If a background refresh is still running at hard expiry, readers
wait for that generation within the existing absolute wait deadline; they do not
start a duplicate loader.

HTML and navigation are always no-store, whether fresh or stale. A listed
progressive navigation receives a complete buffered packet. Initial hydration
uses its matching RVI1 snapshot without a second page GET. A completed refresh
affects later reads; it does not push updates to an already displayed document.
The Wasm module and application revision remain those of the production build.

## Failure and invalidation

A transient loader, component, metadata or rendering failure leaves the old
generation available only inside its original stale window. A one-second retry
backoff prevents every stale read from retrying; this never extends the hard
deadline. No error response or partial output replaces the valid generation.

404/400 results and page/layout no_store results retire old output. A failure
after a successful no_store ancestor also retires it. If the replacement cannot
fit in the storage budget, the old entry is removed and no new entry is retained.
A later request must generate fresh data. Fresh owners whose output is not
retained can respond normally; their waiters receive no-store 503 rather than
run duplicate unbudgeted loaders.

Path/tag invalidation after a committed mutation takes precedence over the
optional stale window. It removes matching entries and retires active or queued
work. Queued retired jobs skip loaders; running retired jobs cannot publish
late. An already adopted response cannot be revoked, including one adopted before
a refresh discovers a missing/no_store page. Use explicit invalidation for changes
that must not use the stale window.

## Bounds and scope

The fixed worker/queue, entry/byte, active-load and waiter budgets are shared with
server data caching on the same handle. HTML counts toward storage. A full queue
declines additional refreshing without blocking an allowed stale response. Hard
expiry, exhausted wait/load budgets and retired publication return no-store 503
when fresh work cannot safely proceed. Existing cache statistics count stale
entries/hits, refresh attempts/failures and queued/running workers for both uses.
Worker shutdown is cooperative; running trusted Rust callbacks cannot be forcibly
cancelled. Router ownership in queued jobs stays weak.

Only anonymous, query-free, explicitly listed URLs use this mode. Cookie or
Authorization, including empty headers, queries and unlisted parameters remain
dynamic and do not schedule output refreshes. Regeneration uses a reconstructed
headerless/bodyless GET, validated captures, trusted origin and `is_exporting()
== false`. The entire page/layout/head/fallback branch must remain public.

Storage is per process/router namespace and never falls back to embedded build
data for regenerable pages. Restart reloads fresh; each replica needs its own
invalidation. Development stays dynamic. There is no persistent cache, distributed
invalidation, runtime compilation or automatic push to browsers. See
[on-demand regeneration](prerender-revalidation.md) and the
[pre-render build contract](client-prerender.md).

Alpha.45 adds optional [persistent output storage](persistent-prerender.md).
Fresh records can survive clean restart with their original expiry. Stale records
are reloaded, and interrupted owners require explicit empty recovery.
