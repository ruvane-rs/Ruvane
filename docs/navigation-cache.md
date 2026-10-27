# Navigation cache and prefetch

Alpha.20 adds explicit public page caching and speculative GET loading without
third-party crates or browser packages. Cache storage is private to one navigator
in one document and holds only validated final navigation bytes. No live component, DOM tree,
localStorage, sessionStorage or database is used as a page cache.

## Server policy

Client pages default to `no-store`. Opt in by registered name:

```rust,ignore
app.client_page_cache("article", NavigationCachePolicy::public(30)?)?;
```

Public freshness accepts 1..=300 seconds. Use `NavigationCachePolicy::no_store()`
to restore the default. The policy is only valid on a registered client page.
It declares that the page's title and seed are public and safe to request without
user intent beyond a link hover/focus. GET loaders, component factories, seed
validators and initial view construction must be bounded and read-only trusted
code. The framework cannot sandbox their side effects.

Only successful typed navigation responses opt in. They send:

```text
Cache-Control: public, max-age=30, must-revalidate
Vary: Accept, Ruvane-Application-Revision
```

HTML, native fallbacks, actions, assets and errors keep `no-store`. App responses
to requests carrying Cookie or Authorization also stay `no-store`. A loader can
tighten a public declaration for a particular response:

```rust,ignore
Ok(ClientPageData::new(title, data).no_store())
```

Never opt personalized or authorization-dependent seeds into public caching.
The revision header is public build metadata, not access control. Check resource
access in every loader and action. The low-level
`Response::navigation_with_cache(&page, policy)` is an explicit public-data
assertion by its caller; unlike App, it has no Request from which to infer a
credential guard. `Response::navigation` remains no-store.

The browser accepts only this public profile, supported freshness and the exact
Vary fields. It rejects no-store/private/unsupported directives for storage,
accounts for Age when supplied, and subtracts time spent fetching/reading from
freshness. Each actual GET uses `cache: "no-cache"`, so invalidated or expired
application entries require network revalidation instead of silently reusing
an independently fresh HTTP-cache entry. This navigator is not an HTTP cache
implementation for arbitrary response policies.

## Eligible links

`App::prefetch_link(name, params, label)` builds a normal named navigation link
only when the route has an explicit public declaration. The lower-level
`prefetch_link(&url, label)` and `View::prefetch_link(parent, key, &url, label)`
mark trusted read-only destinations directly. They add `data-ruvane-prefetch` to
`data-ruvane-nav` links. The response still controls whether data can be stored.
A conditional no-store response can therefore decline storage for a marked link.

Delegated pointerover and focusin handlers schedule only marked internal links.
Targets, downloads, external links, fragments and unmarked links retain their
existing behavior. Native links work before activation or with scripts blocked.
The generated application opts its shared in-memory demo pages in explicitly;
remove those declarations for private application data.

## Navigator API and limits

```js
const navigator = createNavigator(runtime, {
  outlet, status, scope: document,
  maxCacheEntries: 32,
  maxCacheBytes: 2097152,
  cacheTtlMs: 30000,
  prefetchConcurrency: 2,
});
await navigator.start();
await navigator.prefetch("/articles/rust");
await navigator.navigate("/articles/rust");
navigator.invalidate("/articles/rust");
await navigator.refresh();
navigator.invalidate();
```

Maximums are 32 entries, 2 MiB of encoded page bytes, two active prefetches and
32 total active/queued prefetch jobs. Configuration can lower these bounds.
Setting the entry or byte budget to zero disables caching and speculation.
Client freshness defaults to 30 seconds, accepts 1..=300000 ms, and is capped by
the server's remaining freshness. Every packet remains limited to 64 KiB and
fetch/read time to 10 seconds (or a lower `timeoutMs`). A complete valid packet
is checked by Rust before prefetch storage; it is checked again when mounted.

Keys include the origin, application revision and exact encoded target/query.
Insertion evicts least recently used entries until both budgets hold. Expiration
is checked lazily, without one timer per stored page. There is no cross-tab or
persistent cache. Other processes or users can change public data during its
allowed lifetime; TTL caching does not promise globally current snapshots.

Concurrent prefetch calls for the same target share one promise. Queue overflow
returns `skipped`. Foreground navigation cancels background work and takes
priority; clicking during an unfinished prefetch can issue a fresh foreground
GET. A completed warm entry avoids another GET. No view IDs or retained scopes
are allocated during prefetch validation.

`prefetch()` resolves to `prefetched`, `cached`, `skipped`, `failed`,
`invalidated`, `superseded` or `disposed`. It does not change the current page,
URL, title, focus or scroll. A background 409 clears entries, stops speculation
and asks for a document reload. A fresh runtime starts with an empty cache.

`invalidate(url)` removes that target; without an argument it clears all entries.
It also retires matching pending work. `refresh()` invalidates the active target
and performs a fresh GET without pushing history, retaining scroll. It replaces
the page model and can discard its drafts, so it is explicit rather than an
automatic reaction to mutations. Normal typed action callbacks continue updating
their own component as before.

`stats()` adds cacheEntries, cacheBytes, pendingPrefetches, queuedPrefetches,
cachePaused, hits and misses. The navigator owns five delegated DOM handlers and
one runtime action subscription. Disposal aborts jobs, resolves their waiters,
clears the cache and subscription, cancels timers/scroll callbacks and releases
its page. Independent layout islands remain owned by the shared runtime.

## Mutation ordering and uncertain outcomes

The runtime publishes a bounded action lifecycle through `actionState()` and
`subscribeActions(callback)`. At most 16 subscribers are accepted. State exposes
the current epoch, pending count, uncertain latch, subscriber count and runtime
state; it contains no action body or user fields. Unsubscribe with the returned
function. Callbacks are synchronous notifications and should not call the
numeric runtime ABI recursively.

Each action start and end changes the epoch. The navigator invalidates all
entries and background requests on both events, and suspends caching while any
action is pending. A response from an earlier generation cannot populate the
cache after invalidation. Foreground requests invalidated before commit retain
the current page; a failed pop falls back to a document request. A commit already
in progress can finish, but its data is not cached across a mutation boundary.

Network, timeout, malformed-reply, rejected-response and aborted-owner outcomes
are uncertain: the server may have applied a mutation even when its reply was
lost. They clear the cache and latch speculation/storage off until a document
reload creates a new runtime. Fresh foreground navigations and explicit refresh
remain available and are not retained. Refresh does not confirm that an unknown
server mutation has finished. No automatic retries or rollback are implied.
Mutations made outside this runtime require explicit invalidation by application
code; even then, in-flight server side effects are not a transaction.

## ABI and verification

Browser ABI v13 provides `ruvane_page_validate() -> u32` after normal bounded page
staging. It checks framing, registered route/props, application seed schema,
view/resource budgets and absence of initial actions, then drops temporary state.
It emits no commands, consumes no view IDs and retains no instances or scopes.
The core still forbids unsafe code and the adapter isolates 22 numeric exports.

RVN7 adds opcode 13: view u32, node u32, boolean u8. Every initial link includes
its prefetch eligibility; updates can toggle it while keeping the keyed node.
Leaf RVP1 and RVA1 remain supported; RVP2 carries complete layout branches.
All layouts must permit storage. See [client layouts](client-layouts.md). Rebuild Wasm with its matching bridge.

`/cache-tests` on the server_forms example checks warm reuse, native resource
counts, link intent, no-store, credential guards, bounded concurrency/queueing,
LRU/TTL, explicit refresh, mutation races, uncertain outcomes and disposal.
Node checks HTTP freshness parsing and real Wasm validation; Rust checks policy
bounds and response/route behavior. Existing browser, action and navigation
suites still apply. Run suites sharing the example task store sequentially.

Progressive RVS1 responses are always no-store. Their prefixes and final packets
are not inserted into this cache. Prefetch still requests a complete public
packet, and a validated warm packet can satisfy an opted-in navigation without
streaming. See [progressive navigation](progressive-navigation.md).

## Server data freshness

The separate opt-in [server data cache](server-cache.md) reuses validated branch
data between buffered requests. A server cache hit caps navigation max-age at
the remaining whole seconds of server freshness; less than one second produces
no-store. Server path/tag invalidation does not purge another document's browser
cache. Existing action notifications still invalidate the requesting document;
other documents refresh explicitly or wait for their bounded freshness to end.

Opt-in server background revalidation returns permitted stale data with
no-store navigation headers. The browser cannot extend its stale lifetime.
A completed refresh replaces the typed seed and metadata together for later
GETs; it does not push a replacement into an already displayed browser page.
