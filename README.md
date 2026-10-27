# Ruvane

Ruvane is an experimental web framework written from scratch in Rust. Its goal
is to bring server-side and browser-side development into a single stack,
without third-party dependencies.

The current alpha provides HTML rendering, a bounded loopback HTTP server,
fixed and dynamic GET/HEAD routes, explicit form/multipart/JSON mutations, typed parameter conversion, validated
internal links, embedded CSS/image assets, typed native GET forms, page
metadata, canonical URLs, a fixed browser Content Security Policy, and bounded
static export with a local preview. The `ruvane` CLI creates projects, watches
sources, runs the development server, and manages versioned static builds.
The application layer declares named pages and assets once for server rendering
and static builds, with bounded nested layouts and private error pages.
Typed local content collections add a bounded Markdown profile and embedded
articles shared by the server and static export. The independent reactive
runtime provides owner-scoped signals, derived memo values, explicit effects,
batching, cleanup and bounded propagation. A first-party Wasm adapter and small
JavaScript bridge run shared Rust components in the browser. Typed views render
server HTML and hydrate its existing nodes with versioned initial state. Keyed
subtrees support conditional rendering, insertion, removal and reordering while
retaining surviving DOM nodes and component state.
Typed text inputs, textareas and checkboxes send bounded values to Rust.
Client forms support Rust validation, reset tokens and selection/composition handling.
Native POST forms validate bounded fields on the server, check their source origin
and support internal 303 redirects. Interactive HTTP pages explicitly opt into a
fixed module/Wasm CSP. Wasm components can call explicit server actions with
bounded typed replies, pending state and lifetime-checked completions.
Shared page routes now support client navigation, persistent layouts, typed page
seeds, back/forward history and native fallbacks. `App` and the CLI now generate,
compile and bundle interactive applications with their own Wasm registries and
matching build revisions. Explicit public policies now add bounded navigation
caching, hover/focus prefetch and action-aware invalidation. Nested client layouts
keep matching parent components and state during navigation. See
[segment boundaries](docs/segment-boundaries.md) for local loading, typed errors
and explicit GET retry. See
[client layouts](docs/client-layouts.md),
[interactive applications](docs/interactive-apps.md) and [navigation cache](docs/navigation-cache.md).
Explicit [progressive HTML responses](docs/streaming.md) can send a native
document header before deferred content, with bounded pull-based production.
Opted-in App pages also support [progressive client navigation](docs/progressive-navigation.md):
validated layouts become usable while the leaf page is loading, with rollback
on interruption and history committed only after complete delivery.
The initial App document now [hydrates from its embedded server snapshot](docs/initial-hydration.md),
reusing page and layout nodes without another page GET.
Opted-in pages can also [stream their initial App document](docs/document-streaming.md):
the shell and layouts arrive first, followed by the leaf and final snapshot.
Pages can also opt into [early layout hydration](docs/layout-hydration.md):
local layout events work while the leaf loads, and final activation keeps their state.
[Streamed branch errors](docs/streamed-leaf-errors.md) can retain healthy parents
when the leaf or an un-emitted nested layout fails. An explicit GET retry builds
the missing descendants without reloading the document. Ancestor boundaries can
also retire already emitted descendants after the complete error is validated.
An explicit App boundary can recover root layout failures and retain the shell.
Opted-in [shell islands](docs/shell-hydration.md) can activate independently while
the initial page loads, preserving local state through final adoption and recovery.
New interactive starters provide [browser development feedback](docs/development.md):
compiler diagnostics, first-build waiting pages and guarded full-document reloads
after the replacement server is ready. Declared CSS updates in place without
compilation or server restart, keeping the current components and drafts.
Opt-in [file routes](docs/file-routes.md) generate a shared Rust registry from
`page.rs`, `layout.rs`, `loading.rs`, `error.rs`, `not_found.rs` and `[parameter]` directories
during Cargo builds. Loading, error and not-found screens inherit through the
route tree; unknown URLs use the root 404 without guessing parent layouts.
Native-only [file endpoints](docs/file-endpoints.md) add `route.rs` GET/HEAD,
POST, PUT, PATCH and DELETE handlers, sharing HTTP protections and atomic registration.

Native [route guards](docs/route-guards.md) add application, inherited `guard.rs`,
page and endpoint access checks before caches and loaders. Typed allow/private/403/503
decisions stay server-side; guarded navigation always checks the server again.

[Typed HTTP cookies](docs/cookies.md) add bounded request parsing, host-only response
cookies, explicit local HTTP mode, native form/action integration and browser cache
invalidation without exposing Set-Cookie values to the runtime.

[Native memory sessions](docs/sessions.md) add OS-generated opaque identifiers,
bounded server-only data, idle/absolute expiration, atomic rotation and revocation.
Session guards integrate with private page delivery and typed cookies.
[Session CSRF tokens](docs/csrf.md) protect native forms and Wasm actions with
independent OS-generated tokens, generation checks and automatic same-origin retrieval.

[Native request quotas](docs/rate-limits.md) add bounded per-route and per-session
token buckets, rotation-stable session quotas and typed 429 responses across
native forms, navigation and Wasm actions. Browser ABI v14 keeps 22 exports.

[Bounded JSON APIs](docs/json.md) add immutable values, exact number text,
checked construction and explicit native POST/PUT/PATCH JSON routes plus bodyless DELETE. Fixed no-store
responses and the existing session/CSRF/quota guards support the task API example.

[Native multipart forms](docs/multipart.md) add explicit multipart POST routes,
UTF-8 text fields and one opaque file up to 32 KiB within the existing 64 KiB
request budget. Native file controls integrate with session/CSRF/quota guards;
the bounded demo keeps uploads in process memory.

[Component file uploads](docs/browser-uploads.md) add a typed file picker and
explicit multipart requests from Rust/Wasm without a page reload. Selections stay
in the browser, survive validation, and expire on reset, removal or navigation.
The runtime shares the form/JSON pending slot, aborts retired transports and
never retries an upload automatically. ABI v14 requires matching Wasm and bridge.

[Component JSON requests](docs/json-client.md) add explicit GET/POST/PUT/PATCH/DELETE
from Rust/Wasm, exact response decoding, retained versions, bounded transport and
late-response disposal. Typed failures preserve mutation uncertainty without
retries; the task editor now keeps its complete model and requests in Rust.

[Conditional resource updates](docs/conditional-requests.md) add bounded ETag and
If-Match types, atomic version checks and explicit conflicts that preserve browser
drafts. Native OS-random version namespaces prevent accidental reuse after restart.

[Typed JSON validation](docs/validation.md) adds explicit input conversions,
shared native/Wasm rules, exact presence/type checks and bounded field errors.
The task editor preserves drafts and resource versions after invalid input.

Typed [client metadata](docs/client-metadata.md) keeps descriptions, canonical
URLs, robots and PNG icons aligned across SSR, hydration, navigation and history.

Opt-in [server data caching](docs/server-cache.md) shares validated public page
branches between buffered SSR and navigation. Bounded storage, coalesced loads
and explicit path/tag invalidation keep reuse under application control.
Opt-in [background regeneration](docs/background-revalidation.md) serves public
data within a bounded stale window while a fixed worker pool refreshes it.

Opt-in [interactive pre-rendering](docs/client-prerender.md) freezes explicitly
listed public URLs during production builds, with matching HTML, metadata and
Wasm hydration snapshots embedded in the native executable.
Listed URLs can opt into [on-demand regeneration](docs/prerender-revalidation.md):
fresh output on first read by default, retained until path/tag invalidation or eviction.
Optional [expiry and background refresh](docs/prerender-refresh.md) keeps an old
generation available within a bounded window while preparing its full replacement.
Optional [persistent output caching](docs/persistent-prerender.md) restores fresh
generations after a clean restart, with durable path/tag invalidation and bounded
storage in a private directory owned by one native Unix process.

Create an interactive project from a local checkout:

```sh
cargo install --path . --offline --locked
ruvane new my-app --interactive --framework-path /absolute/path/to/Ruvane
cd my-app
ruvane dev --offline
```

Add `--file-routes` to `new --interactive` for the directory-based starter.

The toolchain needs `wasm32-unknown-unknown`. `ruvane build --offline` produces a
versioned server bundle. The adapter is not published, so interactive generation
currently requires a local framework path. [Application guide](docs/interactive-apps.md).

## Principles

- **Zero third-party dependencies**, including build and test dependencies.
- **Explicit composition** through typed elements and attributes.
- **Escaped text by default**, with no raw HTML constructor.
- **Bounded local transport**, with explicit limits and absolute I/O deadlines.
- **No unsafe code in the core crate**. The browser crate isolates reviewed
  `unsafe(no_mangle)` export attributes, with no unsafe blocks or pointer inputs.

## Rust in the browser

From a repository checkout with the Wasm target installed:

```sh
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked -p ruvane --example browser
```

Open the printed address. The server renders a counter at 5/10, a boolean toggle,
an item list and a task form. Wasm hydrates those same DOM nodes and connects their events.
The list supports add/remove, per-item counters, ordering and filtering. Keys keep
surviving rows and state attached to their identities. Both targets compile
`examples/support/widgets.rs`; the registry contains no widget-specific rules.

The `/ssr` page serves the same content with scripts blocked and controls
disabled. The `/tests` page checks node and text identity, state transfer,
mismatches, isolation, stale handles, focus during reorder, 1,000 mount/hydration
cycles each, composition events, dirty field hydration and 1,000 additions/removals
for both lists and typed task forms, with resource counts returning to baseline.
The bridge also supports client mounts with explicit component props.
See [shared views](docs/views.md) for the component API.
See [client forms](docs/client-forms.md) for fields, validation and resets.

The workspace contains only two explicit first-party packages. The browser crate
depends locally on Ruvane; no registry, Git, build or test dependency is added.
No wasm-bindgen, web-sys or npm package is required. Run
`python3 tools/check_first_party.py` to check the complete Cargo graph and ABI
attribute boundary. Node's built-in test runner validates the bridge and compiled
Wasm; a real browser validates DOM and CSP. See [browser](docs/browser.md).

## Dynamic views

Use nonzero sibling keys when an element must survive position changes:

```rust
use ruvane::{BrowserTag, View};

let mut view = View::new();
let list = view.keyed_element(0, 1, BrowserTag::List).unwrap();
for key in [3, 1] {
    let row = view.keyed_element(list, key, BrowserTag::ListItem).unwrap();
    view.keyed_button(row, 1, "Run", key, false).unwrap();
}
let snapshot = view.snapshot().unwrap();
assert!(snapshot.node_for_event(3).is_some());
```

The browser registry retains a `ViewSnapshot` between updates. Removed handles
are never reused, including when a hidden key returns. Unkeyed nodes keep
positional identity. See [shared views](docs/views.md) for keys and lifecycle.

## Server forms

```sh
cargo run --offline --locked --example server_forms
```

Open the printed `/tasks` address. Native POST forms create, complete and remove
tasks without JavaScript. State is shared by the server and survives page reloads
until that process stops. Successful submissions redirect with 303; validation
errors retain escaped fields with status 400.

`PostForm` builds labeled controls with an internal action. `Router::post_form`
registers an explicit mutation handler, independent of GET/HEAD routes on the
same path. `Request::form_data` provides bounded, unique UTF-8 fields separately
from URL queries. Origin is mandatory and must match the server-validated local
Host; Fetch Metadata, when present, must also indicate same-origin. Body size,
framing, MIME type and deadlines are checked before an action runs.

Build the Wasm target and open `/interactive` for server actions without page
navigation. A Rust component sends typed requests, handles validation/pending
state and reconciles replies. Native forms remain available as a fallback.
See [Wasm actions](docs/actions.md) for the protocol and lifecycle.

Open `/app` for the shared navigation demo. The layout counter stays alive across
home, task list and detail pages; direct URLs render on the server. Native links
and forms remain usable when enhancement is unavailable. `/navigation-tests`
checks transitions, history, scroll, malformed replies and owner cleanup.

The server keeps its loopback-only profile; `/tasks` retains the script-blocking CSP.
There are no sessions or database. See
[server POST forms](docs/post-forms.md) for the API, origin policy and limits.

## Progressive HTML

```sh
cargo run --offline --locked --example streaming -- 3004
```

`HtmlStream` starts with a typed `Document` and lazily supplies escaped `Html`
fragments. `Response::stream_html` writes each chunk before requesting the next,
under per-response and server byte, yield and deadline limits. HEAD never starts
the producer. Failures before headers select 500; failures after headers close an
incomplete response. The default script-blocking CSP and no-store policy apply.

Open `/stream` for the report and `/tests` for browser checks. New interactive
projects include the same native `/stream` route. Client navigation packets still
commit complete validated branches. See [streaming](docs/streaming.md) for the
API, cooperative cancellation, limits and protocol scope.

## Reactive state

```rust
use ruvane::{ReactiveError, ReactiveRuntime};

let runtime = ReactiveRuntime::new();
let scope = runtime.scope()?;
let count = scope.signal(1u32)?;
let input = count.clone();
let doubled = scope.memo(&[count.source()], move || Ok(input.get()? * 2))?;

runtime.batch(|| {
    count.set(2)?;
    count.set(3)
})?;
assert_eq!(doubled.get()?, 6);

drop(scope);
assert_eq!(count.get(), Err(ReactiveError::Disposed));
# Ok::<(), ReactiveError>(())
```

Dependencies are declared explicitly; reads do not implicitly subscribe. Each
runtime is local to one thread and has no global state. Scopes own values and
computations, while signal/memo/effect handles are weak. Memos settle before
effects; nested batches coalesce pending work. Propagation has a configurable
run budget and stops permanently after a callback failure. Callbacks and their
external side effects are trusted application code, without rollback or a
resource sandbox.

Run `cargo run --example reactivity --offline --locked` for a native counter,
derived state, batched notifications and explicit cleanup. This native example
exercises the engine directly without loading the browser adapter or selecting
the interactive CSP. See [reactivity](docs/reactivity.md) for ordering, lifetimes, budgets,
failure behavior and the complete example.

## CLI and generated projects

From a source checkout:

```sh
cargo install --path . --locked
ruvane new my-site --framework-path .
cd my-site
ruvane dev
```

`--framework-path` selects a local first-party Ruvane checkout. Without it,
the generated Cargo manifest pins this exact Ruvane version from crates.io;
that version must have been published. The generated application has no
third-party dependencies. Rust/Cargo are required for compilation.

Open the HTTP address printed by `dev`. It watches `src/`, `assets/`, the Cargo
manifest and lockfile, consolidates changes, and recompiles one application at
a time. A successful compilation restarts the server at the same address.
Compilation errors keep the previous server running; startup failures attempt
to restore its executable. New interactive starters show compiler errors and
reload ready versions when fields and actions are safe; otherwise an explicit
Reload button is offered. See [development feedback](docs/development.md).
Static starters still require manual refresh. Type `q` then Enter
to stop dev before running another build command in that project.

```sh
ruvane build --origin https://site.example
ruvane preview dist
```

Builds go into new `dist/build-NNNNNN` directories. The `dist/current` record
changes only after a completed export has been validated. Previous builds are
retained, and unrecognized existing output directories are refused. Preview
also accepts flat alpha.7 exports. It loads the actual selected files once
with CSP, rather than rebuilding them or opening `index.html` as `file://`.

The CLI uses bounded standard-library polling and direct process arguments.
Local project code, Cargo configuration, and the toolchain are trusted; this
is not an execution sandbox. See [the CLI contract](docs/cli.md) for limits,
process lifecycle, project layout, versioned output, and offline operation.

## Pages and layouts

```rust
use ruvane::{App, Page, PageMode, PageRoute, Tag, element};

let mut app = App::new();
app.layout("site", None, |_, title, children| {
    element(Tag::Main)
        .child(element(Tag::H1).child(title))
        .child(children)
        .render()
}).unwrap();
app.page(
    PageRoute::new("home", "/").unwrap(),
    Some("site"),
    PageMode::fixed(),
    |context| {
        Ok(Page::new("Hello from Ruvane", "One declaration, two render targets.")
            .metadata(context.metadata("A page built with Ruvane.").unwrap()))
    },
).unwrap();
assert_eq!(app.url("home", &[]).unwrap().as_str(), "/");
let router = app.router(None).unwrap();
let origin = ruvane::SiteOrigin::parse("https://site.example").unwrap();
let site = app.static_site(&origin).unwrap();
assert_eq!(site.len(), 1);
```

Layouts wrap typed HTML fragments from the leaf to its registered parents.
`Page` supplies document properties; `App` renders the outer document once.
`PageMode::Static` enumerates dynamic export parameters. Static-capable pages
receive no HTTP request, including on the server. `ServerOnly(QueryMode)`
explicitly selects request-dependent pages and excludes them from export.
`App::url` and `PageContext::url` check registered names, complete parameter
sets, single encoding, and ownership of the constructed path.

`PageError` maps deliberate failures to fixed statuses. Custom 404/500 pages
are cached at router construction with noindex and no canonical; protocol
errors keep their fixed responses. Static page errors abort planning before
export. Limits apply to route/layout counts, nesting, parameters and each
rendering stage. Existing low-level APIs remain available. See
[pages and layouts](docs/pages-and-layouts.md) for the full contract and examples.

## Local content and Markdown

Generated projects include `content/welcome.md`. Add a valid `.md` file to
create another article, without editing the Rust page declaration:

````text
---ruvane
title: A first article
slug: first-article
description: Notes from a small beginning.
---
## Welcome

Write **clear words** and *new ideas*. See [the home page](/).
````

The strict Ruvane header requires title, slug and description. The limited
Markdown renderer supports headings, paragraphs, flat lists, emphasis, code and
internal links. Raw HTML remains escaped text. All documents are validated and
embedded as string data in each compiled executable. `dev` watches content and
recompiles it; invalid content keeps the previous server and static build valid.

`ContentCollection` provides immutable typed entries. `App::content_pages`
registers one `{slug}` page from a collection for both serving and export.
`ContentBundle` supports bounded flat-directory snapshots for trusted build
tools. Generated applications use Ruvane itself as their first-party runtime
and build dependency; no third-party package is added. See
[content and Markdown](docs/content-and-markdown.md) for the exact syntax,
budgets, diagnostics, embedding and filesystem trust contract.

## Local server

```rust,no_run
use ruvane::{Document, Response, Router, Server, Tag, element};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut router = Router::new();
    router.get("/", |_| {
        let body = element(Tag::Main)
            .child(element(Tag::H1).child("Hello from Ruvane"));
        Response::html(Document::new("Ruvane", body).render())
    })?;
    router.get("/health", |_| Response::text("ok\n"))?;

    Server::bind("127.0.0.1:3000".parse()?, router)?.run()?;
    Ok(())
}
```

From a checkout, run `cargo run --example server` and open
`http://127.0.0.1:3000`. The `/health` route returns plain text. Unknown paths
return 404. Each GET route also serves HEAD, with response headers and no body.
Query strings are available through `Request::query()` and do not affect routing.
Decoded values are available through `Request::query_params()`; invalid or
duplicate query keys are rejected before a handler is called.

The server accepts loopback IP addresses only. Host must identify `localhost`
or the bound IP and port. The default configuration uses four worker threads,
32 queued connections, 16 KiB of request headers, and five-second total read
and write budgets. Configure budgets with `ServerConfig` and
`Server::bind_with_config`. A `ShutdownHandle` allows another thread to stop
socket I/O and join the workers.

This is a development server with a limited HTTP/1.1 profile. It closes each
connection after one response and does not support request bodies, TLS,
keep-alive, chunked transfer, upgrades, or filesystem directory serving. See the
[HTTP profile](docs/http-profile.md) for supported syntax, errors, and limits.

## Dynamic routes and parameters

```rust
use ruvane::{Response, Router, StatusCode};

let mut router = Router::new();
router.get("/articles/{id}", |request| {
    match request.param_as::<u64>("id") {
        Ok(id) => Response::text(format!("Article {id}")),
        Err(_) => Response::text("Invalid article ID")
            .with_status(StatusCode::BadRequest),
    }
}).unwrap();
```

A `{name}` captures one nonempty path segment. Fixed routes win independently
of registration order. Overlapping dynamic patterns are rejected during
registration. Captures are decoded once as UTF-8; decoded separators, dot
segments, and controls are rejected. `Request::path()` retains the original
encoded path. `Request::param()` returns a decoded string.

`request.query_params().get_as::<usize>("page")` parses a decoded query value.
`ParamError` distinguishes missing values from failed conversions. Handlers
choose their error responses and validate application-specific ranges.

Queries use `&` pairs and the first `=` separator. A bare key has an empty
value. `Query::parse` and `Router::get` preserve literal `+` characters;
`Router::get_form` explicitly selects form decoding. Keys are unique after
the selected decoding and are case-sensitive. See
[routing and URLs](docs/routing-and-urls.md) for the full contract and limits.

## Typed GET forms

```rust
use ruvane::{Document, GetForm, InternalUrl, Response, Router};

let mut router = Router::new();
router.get_form("/search", |request| {
    let search = request.query_params().get("q").unwrap_or("");
    let form = GetForm::new(InternalUrl::new("/search").unwrap()).unwrap()
        .search("q", "Search articles", search).unwrap()
        .hidden("page", "1").unwrap()
        .submit_label("Search").unwrap();
    Response::html(Document::new("Search", form).render())
}).unwrap();
```

`GetForm` renders a native GET form with labeled search inputs, hidden fields,
and an unnamed submit button. Actions require a query-free `InternalUrl`;
field names are bounded, unique ASCII identifiers. Labels and values are
plain text escaped by the renderer. The browser submits the form without
JavaScript. Query conversions still use `get_as::<T>` and application range
validation.

On a `get_form` route, `q=Rust+%2B+HTML` decodes to `Rust + HTML`.
`Query::parse_with_mode(raw, QueryMode::Form)` exposes the same explicit
decoding without a server. `Query::parse` keeps its existing literal-plus
behavior. Both modes decode once and reject duplicates, malformed UTF-8,
controls, and oversized queries before the handler runs. Hidden values are
untrusted, visible URL data. GET handlers must be read-only. See
[GET forms](docs/get-forms.md) for limits, compatibility, and security scope.

## Internal links

```rust
use ruvane::{InternalUrl, link};

let url = InternalUrl::new("/articles").unwrap()
    .segment("🦀 & Rust").unwrap()
    .query("page", "2").unwrap();
let anchor = link(&url, "Read <article>");

assert_eq!(anchor.as_str(),
    "<a href=\"/articles/%F0%9F%A6%80%20%26%20Rust?page=2\">Read &lt;article&gt;</a>");
```

`InternalUrl::new` validates an encoded path. `segment` and `query` accept
unencoded values and encode them as URL data. `link` then escapes the quoted
HTML attribute and its text label. Schemes, external authorities, fragments,
and browser path aliases are outside this link API. URL validity does not
guarantee that a matching route exists.

## Page metadata and canonical URLs

```rust
use ruvane::{Document, InternalUrl, Metadata, Robots, SiteOrigin};

let origin = SiteOrigin::parse("https://journal.example").unwrap();
let path = InternalUrl::new("/articles").unwrap().query("page", "2").unwrap();
let metadata = Metadata::new()
    .description("Notes on Rust and the web.").unwrap()
    .robots(Robots::IndexFollow)
    .canonical(origin.canonical(&path))
    .favicon_png(InternalUrl::new("/assets/mark.png").unwrap());
let page = Document::new("Journal", "Read the latest articles.")
    .metadata(metadata).render();
assert!(page.as_str().contains("https://journal.example/articles?page=2"));
```

Every document includes one responsive viewport that permits zoom. Metadata
uses unique slots: repeated setters replace the prior value, with fixed output
order and HTML escaping. Descriptions have a 1024-byte UTF-8 budget and reject
controls or empty text. A favicon declares an internal PNG URL; the helper
does not decode its bytes. Robots settings are crawler hints, not access control.

`SiteOrigin` parses a restricted HTTP(S) authority profile with ASCII DNS,
IPv4, or bracketed IPv6 and an optional port. It performs no network access.
Canonical URLs combine that configured origin with an `InternalUrl`, retaining
its encoded path/query. Request headers never supply an origin automatically.
See [metadata and CSP](docs/metadata-and-csp.md) for the exact accepted syntax.

The HTTP server sends a fixed `Content-Security-Policy` on every response:
scripts and unspecified resource types are blocked, CSS/images and form
submissions are restricted to the same origin, and base elements and frame
embedding are blocked. This policy supplements escaping and does not replace
application authorization. Standalone rendered HTML does not carry an HTTP
policy by itself.

## Mini-blog example

Run `cargo run --example blog` and open `http://127.0.0.1:3000`.
Use `cargo run --example blog -- 3001` to select another port.
To demonstrate canonical metadata, supply an explicit origin:
`cargo run --example blog -- 3000 https://journal.example`.
This sets metadata only; the listener remains local HTTP. Without a configured
origin, the example emits no canonical URL and requests no indexing. With an
origin, each article and pagination page has its own canonical URL. Searches
and application-rendered errors remain `noindex` without a canonical URL.

The example contains article lists, `/articles/{slug}` pages, UTF-8 slugs,
pagination with `?page=2`, a labeled GET search form, reusable Rust layouts,
responsive CSS, and original PNG illustrations. A new search resets pagination;
page links preserve its filter, and validation errors retain escaped search
text. Styles and images are embedded
at compile time and served locally. No external fonts or scripts are required.
Article text is stored in embedded Markdown files under `examples/content/`. Its handlers and assets are
also exercised by the integration tests.

## Static export

```rust
use ruvane::{Document, MediaType, StaticPath, StaticSite};

let mut site = StaticSite::new();
let articles = ["hello-rust", "typed-urls"];
for slug in articles {
    let path = StaticPath::new(&format!("/articles/{slug}/")).unwrap();
    site.page(path, Document::new(slug, "Rendered at build time.").render()).unwrap();
}
site.asset(StaticPath::new("/assets/site.css").unwrap(),
    b"body { color: #193e33; }", MediaType::Css).unwrap();
assert_eq!(site.len(), 3);
```

`site.export_to("dist")` writes the plan to a **new** directory under an
existing trusted parent. Directory URLs map to `index.html`. Paths follow a
portable lowercase ASCII profile; encoded aliases, Unicode filenames, device
names, traversal, and collisions are rejected. Page text remains UTF-8.
Assets require suffixes consistent with their fixed MIME declaration.

Plans allow up to 256 content entries, 1 MiB per entry, and 16 MiB total,
including generated metadata. Failed registration consumes no budget. Existing
destinations are refused. An I/O failure may leave partial files, without the
completion marker; the exporter performs no recursive deletion or overwrite.
The caller must keep the parent and output tree free from hostile concurrent
modification. This is not a filesystem sandbox or an atomic deployment system.

To export the mini-blog from a checkout:

```sh
cargo run --example blog -- export dist https://journal.example
cargo run --example blog -- preview dist 3001
```

Open `http://127.0.0.1:3001` while the preview command is running. `dist` is
the generated website: HTML, CSS, and images for HTTP hosting. Opening
`dist/index.html` directly as `file://` does not reproduce the site's root-relative
links or HTTP headers. Preview reads the existing export without rewriting it;
`export` continues to refuse an existing destination.

The static edition exports the home page, five articles, three listing pages,
About, CSS, images, and metadata: 17 files. Pagination uses paths such as
`/articles/page/2/`. The static slug `summer-rust` uses the same article title
and text as the server edition. GET search belongs to the server edition;
the static edition uses lists and pagination without a search form. Alpha.9
registers those portable pages once for both modes. The original search and
Unicode article URLs remain explicit server-only pages.

`site.preview_router()` serves a snapshot of the planned bytes through the
normal loopback server and CSP, with no per-request disk lookup or body copy.
It does not reload edited output files. External hosting must configure the
headers listed in `_ruvane/headers.txt` and MIME types in the sorted manifest.
Those files are provider-neutral instructions, not automatically applied
hosting configuration. See [static export](docs/static-export.md) for the
path contract, completion marker, hosting requirements, and security limits.

`StaticSite::preview_export("dist")` loads an existing trusted export once.
It validates the completion marker, bounded canonical manifest, file lengths,
MIME types, and symlink-free file paths before creating preview routes. Extra
files are never served. This reads trusted generated HTML, not sanitized uploads.

## Embedded assets and typed images

```rust
use ruvane::{Document, InternalUrl, MediaType, Router, image};

let mut router = Router::new();
router.asset("/assets/site.css", b"body { color: #193e33; }", MediaType::Css).unwrap();
// For a file in an application: include_bytes!("assets/site.css").
let stylesheet = InternalUrl::new("/assets/site.css").unwrap();
let source = InternalUrl::new("/assets/cover.png").unwrap();
let cover = image(&source, "A green landscape", 960, 480).unwrap();
let page = Document::new("Journal", cover).stylesheet(stylesheet).render();
assert!(page.as_str().contains("<link rel=\"stylesheet\""));
```

`Router::asset` registers a static byte slice on one fixed GET/HEAD path,
sharing the namespace with ordinary routes. Use `include_bytes!` for checked-in
CSS and already encoded images. Response bytes borrow that slice; requests do
not open files or copy the asset body. Assets are limited to 64 registrations,
1 MiB per asset, and 8 MiB total declared bytes per router. The server's
configured response limit also applies, including when it is lower.

`MediaType` declares CSS, PNG, JPEG, WebP, plain text, or opaque bytes. MIME
types are fixed values, never inferred from URL extensions. Assets are trusted
application source: Ruvane does not sanitize CSS, decode images, or validate
that bytes match their declared type. Changing embedded files requires a rebuild.

`Document::stylesheet` renders a quoted, escaped internal URL in a void `link`
element, deduplicating identical URLs in insertion order. `image` renders a
void `img` element with escaped alternative text and numeric width/height in
the range 1–8192. Those dimensions describe layout, not verified file metadata.
An empty alternative is allowed for decorative images.

For a dynamically constructed binary body, use `Response::bytes(data, media_type)`.
Starting with alpha.4, **`Response::body()` returns `&[u8]`**. Use the fallible
`Response::body_text()` accessor for UTF-8. Text and HTML constructors retain
their existing behavior. For lazy streams these accessors expose only the initial
prefix; `buffered_body()` returns `None`. See [assets and media](docs/assets-and-media.md)
and [streaming](docs/streaming.md).

## HTML rendering

```rust
use ruvane::{Attribute, Document, Tag, element};

let message = "Hello <world> & Rust";

let body = element(Tag::Main)
    .attr(Attribute::Class, "page")
    .child(element(Tag::H1).child("Ruvane"))
    .child(element(Tag::P).child(message));

let page = Document::new("Ruvane", body)
    .language("en")
    .render();

println!("{page}");
```

Strings passed as children are rendered as text. The message above becomes
`Hello &lt;world&gt; &amp; Rust`. Existing `Html` fragments retain their markup
when composed. Serialized output is available through `as_str()`,
`into_string()`, or `Display`.

## Rendering API

| Item | Purpose |
| --- | --- |
| `text` | Render a string as escaped HTML text. |
| `element` | Build an element selected from the `Tag` enum. |
| `Attribute` | Set `id`, `class`, or `title` with quoted, escaped values. |
| `fragment` | Join rendered fragments in order. |
| `link` | Render an anchor to an `InternalUrl`, escaping its attribute and label. |
| `image` | Render an image with an internal URL, escaped alternative text, and typed dimensions. |
| `ContentCollection`, `ContentEntry`, `ContentSlug` | Validate immutable local content and enumerate article pages. |
| `ContentBundle`, `ContentSource`, `markdown` | Snapshot, embed and render a bounded Markdown profile. |
| `App` | Declare common pages, layouts and assets, then create a server router or static plan. |
| `PageRoute` | Validate a named pattern and construct URLs from decoded parameters. |
| `Page`, `PageContext`, `PageMode` | Compose document content with explicit static/server input policies. |
| `GetForm` | Render a native GET form with an internal action, labeled search inputs, and hidden fields. |
| `PostForm`, `FormData` | Render native POST controls and parse bounded fields for explicit server mutations. |
| `Document` | Render a complete document with a doctype, language, UTF-8 declaration, title, and body. |
| `Metadata` | Add bounded descriptions, robots hints, a canonical URL, and a PNG favicon. |
| `SiteOrigin` / `CanonicalUrl` | Combine a configured origin and validated internal URL for canonical metadata. |

## HTTP API

| Item | Purpose |
| --- | --- |
| `Router` | Register GET/HEAD pages and explicit form/JSON mutations, with method-scoped ambiguity detection. |
| `Request` | Inspect paths, captures, queries, form/JSON data and validated headers. |
| `Query` | Look up unique decoded query keys and convert values through `FromStr`. |
| `QueryMode` | Explicitly select URI or form query decoding without changing path semantics. |
| `InternalUrl` | Build validated internal paths and encode segment/query data. |
| `Response` | Return text, rendered HTML, or binary bytes with a typed `StatusCode`. |
| `MediaType` | Select a fixed MIME type for binary responses and embedded assets. |
| `Server` | Bind a loopback socket and run a bounded worker pool. |
| `ServerConfig` | Configure byte, count, concurrency, and I/O time budgets. |
| `ShutdownHandle` | Stop accepting connections and interrupt socket I/O. |
| `StaticSite` | Plan bounded pages/assets, export to a new directory, and build an exact local preview. |
| `StaticPath` | Validate a portable static URL and derive its relative output filename. |

## Security scope

The renderer escapes `&`, `<`, `>`, double quotes, and apostrophes, and replaces
null characters with the Unicode replacement character. Element and attribute
names come from enums. Links, forms, stylesheets, and images have dedicated typed URL APIs. Script and style
elements, event-handler and style attributes, arbitrary URL attributes,
foreign content, and raw HTML are outside the current API.

Ruvane does not sanitize existing HTML, encode JavaScript or CSS, or
validate HTML nesting and language tags. Applications remain responsible for
renderer allocations and handler CPU, memory, and termination. The server
limits the response size after the handler has built it; it cannot interrupt
arbitrary handler code. GET and HEAD handlers must be read-only.

Malformed framing is rejected before route dispatch. Duplicate headers,
conflicting content lengths, unsupported transfer encodings, and invalid Host
values cannot select a handler. Responses use fixed headers and generic error
bodies. Queries and dynamic captures have fixed byte/count limits and are
decoded exactly once. These controls are not application authorization or
domain validation. No CORS access is granted. Loopback binding and Host validation do not
authenticate a local user or application.

Starting with alpha.6, all documents include a responsive viewport, and all
server responses carry the fixed CSP exposed as `CONTENT_SECURITY_POLICY`.
Metadata has no arbitrary tag names, `http-equiv`, or raw markup interface.
Public origins come from explicit configuration. See
[metadata and CSP](docs/metadata-and-csp.md) for compatibility and security limits.

Static export requires a trusted filesystem parent, exclusive destination/file
creation, and an output tree protected from concurrent tampering. Preview
routes serve registered immutable bytes rather than mapping URLs to disk.
Exported HTML needs the hosting server to provide CSP and the other required
headers; a metadata manifest cannot enforce them.

Form query decoding is selected by route registration, never inferred from
request fields. Native POST actions require same-origin verification and bounded
form bodies. Session CSRF protection is opt-in; see [CSRF tokens](docs/csrf.md). Uploads and
credential verification are not provided.
Do not place secrets in GET fields. See [server forms](docs/post-forms.md).

Embedded assets are explicitly selected application data, not a filesystem
mount or an upload service. CSS is trusted code. Type declarations and
`nosniff` do not sanitize file contents or validate image encodings. See
[the asset security scope](docs/assets-and-media.md).

The alpha has not undergone an independent security audit. Reporting
information is available in [SECURITY.md](SECURITY.md).

## Project status

Ruvane is in early development. The alpha API may change between releases.
This version provides rendering and local serving; it does not yet provide a
complete full-stack runtime or a production transport.

Requires Rust 1.85 or newer and uses the Rust 2024 edition.

## License

MIT. See the LICENSE file.
