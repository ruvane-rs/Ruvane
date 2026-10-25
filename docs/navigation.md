# Client navigation

Alpha.18 adds explicit client navigation between shared Rust pages. The server
renders every direct URL, and ordinary links still work before JavaScript starts
or when Wasm is unavailable. Navigation retains the common layout and replaces
only its page outlet. There are no third-party Cargo or browser packages.

Build and run the example:

```sh
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked --example server_forms -- 3002
```

Open `/app`, `/app/tasks` or `/app/tasks/1`. Create a task before opening its detail
URL. The counter above the outlet keeps its value across client transitions.
The task list and detail page use the existing typed server actions. Direct
requests for missing tasks return HTTP 404. `/navigation-tests` runs the browser
checks; `/navigation-tests?pop=yes` additionally forces a failed history pop
back to a normal SSR document. Adding `?native=yes` serves a script-blocking representation for testing
native links, forms and Post/Redirect/Get. Native mutations redirect to `/tasks`.
Task state is bounded, shared and held in memory until the server stops.

## Shared declarations

`NavigationRoutes` registers existing `PageRoute` declarations with component
IDs. Server and Wasm compile the same registry. Fixed routes take precedence
over dynamic captures; overlapping dynamic patterns, duplicate names/paths and
zero component IDs are rejected. The registry allows 128 routes. Captures decode
once, query values use URI decoding, and named URL generation verifies route
ownership after encoding. Queries do not become path captures.

```rust
use ruvane::{NavigationRoutes, PageRoute, InternalUrl};
let mut routes = NavigationRoutes::new();
routes.register(PageRoute::new("task", "/app/tasks/{id}")?, 10)?;
let url = routes.url("task", &[("id", "42")])?;
let matched = routes.resolve(&InternalUrl::from_target("/app/tasks/42?q=a+b")?)?.unwrap();
assert_eq!(matched.param("id"), Some("42"));
assert_eq!(matched.query().get("q"), Some("a+b"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`navigation_link(&url, label)` emits an escaped HTML link marked
`data-ruvane-nav`. `View::link(parent, key, &url, label)` provides the same native
link in shared component trees. Links carry no per-node event binding. Their
targets can be updated while retaining the keyed DOM node. Unmarked links,
modified or middle clicks, targets, downloads, external links and fragments
retain native behavior.

## Server and page models

The same GET handler selects its representation explicitly. The example returns
`Response::navigation(&page)` for the exact Accept value
`application/vnd.ruvane.page`, and SSR HTML otherwise. A descriptor carries the
exact internal target, document title, component props, public `ActionData` seed
and actual HTTP status. Supported statuses are 200, 400 and 404; the packet and
HTTP status must agree. All responses retain the server's no-store policy.

Implement `Component::page_data` to check the application schema and initialize
the model before its first view. It rejects seeds by default. Register a shared
validator with `BrowserRuntime::new(factory).with_navigation(validate_page)`.
That function must check URL ownership, status, component props and seed schema.
The example additionally checks task IDs and all task fields. A seeded mount
that queues an initial action is refused: preparing a page must not start a
server mutation. Later user events can use the normal action API.

Descriptors are public page data. Put authorization checks in the GET and action
handlers; component IDs and route names do not authorize access. Keep secrets
out of seeds and titles. Application Rust code remains trusted code.

## Browser controller

Serve the bridge and navigation modules next to each other as `bridge.mjs` and
`navigation.mjs`; export their source with `BROWSER_BRIDGE_MODULE` and
`BROWSER_NAVIGATION_MODULE`. Serve the application module and Wasm separately.
Use `Response::interactive_html` for the document's fixed module/Wasm CSP.

```js
import { createRuntime } from "./bridge.mjs";
import { createNavigator } from "./navigation.mjs";
const runtime = await createRuntime("/assets/app.wasm");
const navigator = createNavigator(runtime, { outlet, status, scope: document });
await navigator.start();
await navigator.navigate("/app/tasks");
await navigator.navigate("/app", { replace: true });
await navigator.whenIdle();
// Release the navigator before disposing the shared runtime.
navigator.dispose();
runtime.dispose();
```

`start()` first leaves SSR intact, then prepares a seeded Rust page in a detached
empty host. This initial replacement does not hydrate arbitrary SSR HTML.
Use `start({ accept: () => boolean })` to decline activation if native form
fields have focus or contain drafts. A declined start releases the prepared
owner and returns `native`. The example checks both focus and changed fields.
Sibling layout islands can use normal hydration and remain independent.

Successful navigation prepares and validates the next page before changing
history or disposing the previous page. `pushState`, `replaceState` and
`popstate` manage the URL; a newer request aborts the previous one, and stale
completions cannot commit. The title is plain text. The new host receives focus
with `preventScroll`, new entries scroll to the top, and back/forward restore
saved positions. Initial activation retains focus and scroll. History entries
use the namespaced `__ruvane_navigation_v1` object; existing object fields are
preserved when replacing the current entry. Non-object or array state is refused
at startup. The navigator temporarily selects manual scroll restoration.

Loading sets `aria-busy` on the outlet and updates the supplied status element
with a polite status announcement. Disposal restores its previous role/live attributes.
A failed link/programmatic transition leaves the current page and URL in place.
A failed pop transition or commit uses a normal document request at the target
URL. Disposal aborts fetches, removes the five delegated handlers, cancels scroll
callbacks, restores scroll restoration and drops the current page. Disposing a
page also releases its action requests and listeners. The shared runtime and
other islands remain alive until explicitly disposed.

`whenIdle()` returns the current transition promise. Outcomes are `success`,
`failed`, `native`, `invalidated`, `superseded` or `disposed`; before any transition it resolves
to `idle`. `current()` exposes the current component controller, or null.
`stats()` reports navigator state, pending status, current target and listener
count. Only one navigator can own a window at a time.

## RVP1 wire format and limits

All integers are little-endian. A complete packet contains:

| Field | Encoding |
| --- | --- |
| Magic | Four bytes `RVP1` |
| Actual status | u16, 200/400/404 |
| Exact internal target | u32 byte length, encoded ASCII path and optional query |
| Plain title | u32 byte length, strict UTF-8 |
| Component props | component u32, state kind u32, value u32 |
| Public seed | u32 byte length, complete successful RVA1 frame |

The whole decoded response is limited to 64 KiB, the target to 4096 bytes and
the title to 1024 bytes. Titles cannot be blank or contain controls. RVA1 retains
its field/UTF-8/type budgets. No trailing bytes, validation-kind seeds or unknown
statuses are accepted. The bridge checks metadata and the exact requested target;
Rust checks the complete packet, shared route validator and component schema.
No HTML, selectors or raw DOM attributes cross this format.

Fetch uses same-origin GET, fixed Accept, same-origin credentials, no redirects,
and an absolute 10-second response/read deadline. `timeoutMs` may lower that
deadline to 1..=10000 ms. There is one in-flight foreground navigation. Alpha.20 adds an optional public
page cache and two bounded speculative requests; no-store remains the default.
At most two page owners overlap during preparation, within the runtime's
16-view budget; reserve space for the prepared page alongside layout islands.

Browser ABI v8 provides `ruvane_page_begin`, `ruvane_page_chunk`,
`ruvane_page_mount` and `ruvane_page_validate`. Input arrives in exact-length
chunks of 1..=4 bytes, without input pointers or imports. Error 11 means an invalid
page/route/seed. RVN6 carries link tag 15, opcode 12 (node u32, target length u32,
encoded target bytes) and opcode 13 (node u32, prefetch boolean u8).
Rebuild Wasm with the matching bridge; older versions are rejected.

Alpha.19 integrates this layer through `App::client_page` and application-owned
factories. See [interactive applications](interactive-apps.md). Ordinary
`App::page` handlers, static exports and arbitrary HTML are not automatically
converted into client pages. See [navigation cache](navigation-cache.md) for prefetch, expiry and invalidation.
Nested changing layouts, streaming and richer props remain separate work.
