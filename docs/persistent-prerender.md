# Persistent regenerated output

Alpha.45 optionally persists public regenerated client pages across clean restarts.
Memory caching stays the default. This backend uses only the Rust standard library
and requires native Unix plus a trusted filesystem supporting file/directory sync
and atomic rename. Wasm and other platforms return PersistenceUnavailable.

## Attach before use

```rust,no_run
use ruvane::{ServerCache, ServerCacheDiskLimits, ServerCachePersistence};
fn cache() -> Result<ServerCache, ruvane::ServerCacheError> {
    let disk = ServerCachePersistence::open(
        "/srv/ruvane/cache/site", "my-app", ServerCacheDiskLimits::default(),
    )?;
    ServerCache::default().persistent(disk)
}
```

The parent directory must already exist and be trusted, with no concurrent
tampering. Supply an absolute real path without symlinks or parent traversal.
The backend creates its own private directory with mode 0700 and files with mode
0600. Existing directories must be private and either empty or marked for this
same application. Foreign files, subdirectories and symlinks are refused. Do not
derive paths from requests, point at user documents or accept uploaded catalogs.

The application identifier is a stable ASCII page-name identifier, at most 48
bytes. Use a distinct private directory for each application and one process per
directory. Attach persistence before cloning the handle or creating a router.
Then attach the handle through `app.server_cache(cache.clone())` as usual.
`ServerCache::with_background(...)?` also accepts `.persistent(disk)?` before use.
Keep this setup behind `#[cfg(unix)]` in declarations shared with Wasm.

Only explicitly listed client URLs with a PrerenderRevalidationPolicy are
persisted. App and FilePage declarations need no new page option. Immutable build
snapshots, ordinary server data-cache entries, actions, private/query requests
and unlisted URLs keep their existing behavior.

## What is restored

The bounded RDC1 catalog contains complete HTML, the matching RPR1/RVI1 packet,
metadata and native fallback, publication times, policies, tags and exact scope
bytes. Restoration requires the same application directory identity, build
revision, trusted router/metadata origins, route, build seed, layout declarations,
stylesheets and cache policy. Component props, page/layout seeds, route ownership,
metadata origin and module URL are validated again before adoption.

A successful restore reuses the complete HTML and navigation packet without
running page/layout loaders. Component validation still executes trusted factory,
page_data and view callbacks. There is no Rust/Wasm compilation at runtime.
The build revision must identify immutable application code and assets; change it
when they change. Opaque Rust callbacks cannot be fingerprinted automatically.

Only fresh entries are restored. Timed entries keep their original remaining
lifetime; reopening does not grant a new TTL. An entry already stale at startup
or a later memory miss is discarded from the disk tier and loaded fresh, even
if its old stale window has not ended. The normal in-process background-refresh
behavior remains available after adoption. Explicit-invalidation-only entries
remain valid until invalidated or evicted.

Disk deadlines use wall-clock milliseconds. Future publication/checkpoint times
and observed backwards movement discard old entries. Runtime memory deadlines
still use Instant. A trustworthy system clock is required across restarts;
undetectable clock changes and external rollback of filesystem snapshots are
outside this local cache's guarantees.

## Durable changes

Path/tag invalidation and clear update the persistent catalog even before any
entry has been restored into memory. Matching active work is retired first; a
retired owner cannot publish late. A successful invalidation means the new catalog
and its directory updates have been synchronized before the call returns.
Handle errors after a data commit; a failed invalidation is not an acknowledgment.
The returned count still counts memory entries. Ok(0) can successfully remove
persistent records that have not yet been restored.
There is no database transaction coupling or multi-process invalidation.

Publication writes a transaction marker and a new complete catalog, syncs it,
renames it over the prior catalog, syncs the directory and removes the marker.
Only then can the memory publication complete. Errors, no_store and retired
generations cannot restore an older disk output. A storage failure makes public
cache access unavailable with no-store 503 and leaves ownership for explicit
empty recovery; private dynamic routes retain their usual behavior.

RDC1 has fixed count/field/byte bounds, exact parsing and an accidental-corruption
checksum. Embedded RPR1 envelopes check matching complete snapshots. Neither
checksum authenticates data. Treat the directory and stored HTML as trusted
application output, with the same trust obligations as embedded pre-render data.
Corrupt or incompatible catalogs are discarded conservatively.

## Close and recover

After stopping request handling, settle cache work and close persistence:

```rust,no_run
fn after_server_stops(cache: &ruvane::ServerCache) -> Result<(), ruvane::ServerCacheError> {
    cache.close_persistence()
}
```

The method refuses active loads/waiters. Background callbacks finish
cooperatively; stop queuing and allow running work to settle before closing.
Closing synchronizes the final checkpoint and releases the owner marker. Later
public cache access through that closed handle is unavailable. The last ordinary
handle/router drop also closes the backend. Static/global handles must explicitly
close because Rust statics do not run destructors at process exit.

A crash, killed process or failed transaction leaves LOCK in place. Normal open
refuses it instead of guessing whether another owner is alive. After stopping
every owner, an operator can deliberately reset this dedicated cache:

```rust,no_run
use ruvane::{ServerCacheDiskLimits, ServerCachePersistence};
fn recover() -> Result<ServerCachePersistence, ruvane::ServerCacheError> {
    ServerCachePersistence::recover_empty(
        "/srv/ruvane/cache/site", "my-app", ServerCacheDiskLimits::default(),
    )
}
```

Recovery discards the old catalog and partial transaction files before opening
an empty cache. Never invoke it automatically to take over a potentially live
owner. Acknowledged invalidations cannot be undone by this recovery. Storage
sync/durability depends on the filesystem; hardware failure, hostile writers and
external backup rollback are outside the backend's trust model.

## Budgets and operations

ServerCacheDiskLimits accepts 1..=128 records and a complete encoded catalog
budget of 32 bytes..=16 MiB; defaults are 32 records and 8 MiB. This is independent
of ServerCacheLimits for memory. Oldest publications are removed to fit disk
budgets. Expired entries are pruned on opening, restore, mutation and close.
An oversized record is not retained, and an old matching disk record is removed.
The current fresh response may still succeed if durable removal succeeds.

The bounded catalog is decoded into memory for lookup; its payload memory is in
addition to the ordinary memory cache budget. Memory hits do no disk I/O.
Writes and invalidations serialize synchronously with cache publication. Memory
LRU eviction can later restore a fresh disk record through the same bounded
owner/waiter path. Disk eviction does not invalidate an already adopted memory
response. Statistics expose persistent_entries, persistent_bytes and
persistent_restores without URLs or page data.

See [on-demand regeneration](prerender-revalidation.md),
[timed refresh](prerender-refresh.md) and the [build contract](client-prerender.md).
