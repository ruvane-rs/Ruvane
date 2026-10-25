# Pages and layouts

Alpha.21 adds [persistent client layout branches](client-layouts.md), with shared
SSR declarations and parent component reuse during client navigation.

Alpha.19 adds `App::components`, `App::client_page` and `App::client_runtime` for
shared SSR/Wasm pages and one persistent shell. These pages are explicitly
server-only; ordinary `App::page` and its static/layout behavior remain available.
See [interactive applications](interactive-apps.md).

Alpha.9 adds `App`, a common declaration for GET/HEAD pages, named routes,
fragment layouts and embedded assets. `App::router` creates the server router;
`App::static_site` renders the explicitly enumerated export. The lower-level
`Router`, `Document` and `StaticSite` APIs remain available.

```rust
use ruvane::{App, Page, PageMode, PageParams, PageRoute, SiteOrigin, Tag, element};

let mut app = App::new();
app.layout("site", None, |_, title, children| {
    element(Tag::Main)
        .child(element(Tag::H1).child(title))
        .child(children)
        .render()
}).unwrap();
app.layout("journal", Some("site"), |_, _, children| {
    element(Tag::Section).child(children).render()
}).unwrap();
app.page(
    PageRoute::new("article", "/articles/{slug}/").unwrap(),
    Some("journal"),
    PageMode::Static(vec![PageParams::new(&[("slug", "hello-rust")]).unwrap()]),
    |context| {
        Ok(Page::new("Article", context.param("slug").unwrap())
            .metadata(context.metadata("Notes from the journal.").unwrap()))
    },
).unwrap();
let url = app.url("article", &[("slug", "hello-rust")]).unwrap();
assert_eq!(url.as_str(), "/articles/hello-rust/");
let origin = SiteOrigin::parse("https://journal.example").unwrap();
let router = app.router(Some(origin.clone())).unwrap();
let site = app.static_site(&origin).unwrap();
assert_eq!(site.len(), 1);
```

## Registration and named links

Names begin with an ASCII letter and contain ASCII letters, digits, `_` and
`-`, within 48 bytes. They are case-sensitive. Page and layout namespaces are
separate. Route patterns use the existing complete-segment `{capture}` syntax,
with the additional `InternalUrl` validation of literal segments and a depth
limit. Capture names retain the router's 64-byte ASCII identifier profile.

`PageRoute::url` accepts exactly one decoded value per capture, in any order.
Missing, duplicate and extra keys fail. Values are checked and encoded once;
`%2F` as data becomes `%252F`. Dot segments, separators and controls fail.
Append queries explicitly to the returned `InternalUrl`.

`PageRoute::url` validates the pattern and parameters independently of an app.
`App::url` and `PageContext::url` additionally require the name to exist and
own the resulting path. Fixed routes retain priority. A dynamic URL shadowed
by a fixed page or asset is refused. During rendering, a decoded alias whose
constructed canonical path belongs to another page returns 404. These are
runtime checks, not compile-time route proofs or authorization checks.

All page and asset registrations share route-conflict validation. Registration
executes no page/layout callback and is atomic on failure. A router takes a
snapshot; later builder changes do not mutate a running router.

## Layout composition and pages

Register parents before children. Immutable references to earlier parents
prevent cycles. A page selects its leaf layout; the leaf wraps the body first,
then its parents. Each callback receives the same context, plain title text,
and typed `Html` children. The title argument is plain text; HTML builders
escape it when used as a child or attribute.

`Page` contains the title, HTML fragment, metadata, language and stylesheet
links. `App` emits one outer `Document`. Layouts must return fragments rather
than complete documents; the type system does not enforce every HTML content
model and cannot recognize a `Document::render()` passed as `Html`.

`Page::new` defaults to `noindex, follow` without a canonical. The optional
`PageContext::metadata` helper uses an explicitly supplied `SiteOrigin` and
the constructed page path. Without an origin it retains noindex and omits the
canonical. Explicit page metadata remains caller-controlled. Host and
forwarding headers never configure the origin. Errors force noindex and remove
canonicals. Existing metadata escaping, singleton fields and CSP remain in force.

## Static and server-only policies

- `PageMode::fixed()` declares one fixed static-capable page.
- `PageMode::Static(Vec<PageParams>)` declares at least one concrete export
  parameter set. Fixed routes require exactly one empty set. All resulting
  paths must be unique portable directory URLs accepted by `StaticPath`.
- `PageMode::ServerOnly(QueryMode)` deliberately excludes a page from export
  and fixes its URI or native form query convention.

Static-capable pages receive no `Request`, even when served over HTTP. Their
validated captures, constructed path and configured origin are available.
Valid queries do not change their context; malformed queries still return 400
before callbacks. Dynamic parameters outside the enumerated export sets may
be rendered by the server, but are not present in a static preview. The
renderer decides whether that content exists with `PageError::NotFound`.

Server-only pages obtain `context.request()` and can use typed query access.
`App::server_only_routes` lists omitted names; the generated application and
mini-blog print them during build/export. This policy is explicit: the
framework does not analyze Rust functions to infer their dependencies.

The same page/layout callbacks run for server and export. `is_exporting` reports
the invocation type; callbacks, globals and external data are trusted Rust code.
Identical origin/data and deterministic callbacks produce identical common
page bytes, as exercised by integration tests. This is not a purity guarantee.

Static parameters, route ownership, output collisions and entry counts are
validated before rendering. Page errors, unwinding panics and oversized output
abort planning. Only a successful plan is passed to `StaticSite::export_to`.
The existing CLI updates `current` only after a completed export validates;
a planning failure preserves the last successful build.

## Error pages

Page callbacks return `Result<Page, PageError>`. `NotFound`, `BadRequest` and
`InternalServerError` produce 404, 400 and 500 respectively, with no arbitrary
error string reflected into HTML. Application diagnostics belong in local logs.

`App::not_found` and `App::internal_error` accept an optional registered layout
and a zero-argument page callback. Both responses are rendered and validated
once when constructing a router, without a request, route name or public origin.
Error layouts receive `/` as their neutral path. Metadata is forced to noindex
without a canonical. The cached shared bytes retain the correct status and
HEAD behavior. Invalid error callbacks refuse router construction.

Page/layout unwinding panics and output failures use the configured 500 page.
The server's response byte budget still applies: if the custom error cannot
fit, the fixed small 500 is used. Malformed HTTP, Host validation and query
errors retain their fixed protocol responses. Low-level handlers added to the
resulting router share its configured panic/output 500 fallback.

`Page::bad_request()` retains a validation page's escaped fields with status
400, removes its canonical and forces noindex. Such a result cannot be exported.
Static-capable render callbacks returning any `PageError` also fail the build;
they never silently export an error as a successful page.

Custom 404/500 pages are server behavior. Static export/preview retains its
existing manifest format and fixed unknown-path response. Hosting-provider
error-page mapping is not automatically configured by this API.

## Limits and trust

| Resource | Limit |
| --- | --- |
| Combined page/asset routes | 128 |
| Named layouts / applied layout depth | 32 / 8 |
| Page/layout name | 48 ASCII bytes |
| Page pattern segments / encoded path | 16 / 4096 bytes |
| Captures / capture name | 16 / 64 ASCII bytes |
| Decoded capture value | 1024 UTF-8 bytes |
| Combined static page instances/assets | 256 |
| Title / language | 1024 / 64 control-free UTF-8 bytes |
| Distinct stylesheet links | 16 |
| Page body, each layout result, final HTML | 1 MiB each |

The existing 64-asset, 1 MiB individual asset, 8 MiB aggregate asset and 16 MiB
static-total limits also apply. Static paths retain their narrower portable
profile. Limits are checked after caller allocations. Callbacks cannot be
interrupted or sandboxed; CPU, side effects, global state and custom `FromStr`
implementations remain application responsibilities. Panic recovery requires
unwinding, not `panic=abort`. No compiler, remote function dispatcher, script
execution, request bodies, POST, authentication or cryptography is introduced.

The generator demonstrates a global layout, nested greeting layout, named
links, one dynamic static page and private error pages. The mini-blog declares
common portable routes once, including path-based pagination. Search at
`/articles` and the original no-trailing-slash/Unicode article URLs are explicit
server-only compatibility pages; `/health` remains a low-level text handler.

## Content collections

Alpha.10 adds `App::content_pages` for a named route with one `{slug}` capture.
A validated `ContentCollection` owns metadata and typed Markdown HTML, enumerates
its static parameters, and supplies the same entries at request time. Unknown
slugs return 404. Existing layout/error/origin/output contracts still apply.
See [content and Markdown](content-and-markdown.md).
