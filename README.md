# Ruvane

Ruvane is an experimental web framework written from scratch in Rust. Its goal
is to bring server-side and browser-side development into a single stack,
without third-party dependencies.

The current alpha provides HTML rendering, a bounded loopback HTTP server,
fixed and dynamic GET/HEAD routes, typed parameter conversion, validated
internal links, and embedded CSS/image assets. Server actions and browser-side
reactivity are planned for future versions.

## Principles

- **Zero third-party dependencies**, including build and test dependencies.
- **Explicit composition** through typed elements and attributes.
- **Escaped text by default**, with no raw HTML constructor.
- **Bounded local transport**, with explicit limits and absolute I/O deadlines.
- **No unsafe code** in the crate.

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
value; `+` remains a literal plus. Keys are unique after decoding and are
case-sensitive. See [routing and URLs](docs/routing-and-urls.md) for the full
contract, limits, and changes from alpha.2.

## Internal links

```rust
use ruvane::{InternalUrl, link};

let url = InternalUrl::new("/articles").unwrap()
    .segment("été & Rust").unwrap()
    .query("page", "2").unwrap();
let anchor = link(&url, "Read <article>");

assert_eq!(anchor.as_str(),
    "<a href=\"/articles/%C3%A9t%C3%A9%20%26%20Rust?page=2\">Read &lt;article&gt;</a>");
```

`InternalUrl::new` validates an encoded path. `segment` and `query` accept
unencoded values and encode them as URL data. `link` then escapes the quoted
HTML attribute and its text label. Schemes, external authorities, fragments,
and browser path aliases are outside this link API. URL validity does not
guarantee that a matching route exists.

## Mini-blog example

Run `cargo run --example blog` and open `http://127.0.0.1:3000`.
Use `cargo run --example blog -- 3001` to select another port.

The example contains article lists, `/articles/{slug}` pages, UTF-8 slugs,
pagination with `?page=2`, filtering with `?search=Rust`, reusable Rust layouts,
responsive CSS, and original PNG illustrations. Styles and images are embedded
at compile time and served locally. No external fonts or scripts are required.
Article data is stored in the example's source. Its handlers and assets are
also exercised by the integration tests.

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
their existing behavior. See [assets and media](docs/assets-and-media.md).

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
| `Document` | Render a complete document with a doctype, language, UTF-8 declaration, title, and body. |

## HTTP API

| Item | Purpose |
| --- | --- |
| `Router` | Register fixed and dynamic GET paths, with automatic HEAD handling and ambiguity detection. |
| `Request` | Inspect raw paths/queries, decoded captures, parsed queries, and validated headers. |
| `Query` | Look up unique decoded query keys and convert values through `FromStr`. |
| `InternalUrl` | Build validated internal paths and encode segment/query data. |
| `Response` | Return text, rendered HTML, or binary bytes with a typed `StatusCode`. |
| `MediaType` | Select a fixed MIME type for binary responses and embedded assets. |
| `Server` | Bind a loopback socket and run a bounded worker pool. |
| `ServerConfig` | Configure byte, count, concurrency, and I/O time budgets. |
| `ShutdownHandle` | Stop accepting connections and interrupt socket I/O. |

## Security scope

The renderer escapes `&`, `<`, `>`, double quotes, and apostrophes, and replaces
null characters with the Unicode replacement character. Element and attribute
names come from enums. Links, stylesheets, and images have dedicated typed URL APIs. Script and style
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
