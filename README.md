# Ruvane

Ruvane is an experimental web framework written from scratch in Rust. Its goal
is to bring server-side and browser-side development into a single stack,
without third-party dependencies.

The current alpha provides HTML rendering, a bounded loopback HTTP server,
and exact-path routing for GET and HEAD. Server actions and browser-side
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

The server accepts loopback IP addresses only. Host must identify `localhost`
or the bound IP and port. The default configuration uses four worker threads,
32 queued connections, 16 KiB of request headers, and five-second total read
and write budgets. Configure budgets with `ServerConfig` and
`Server::bind_with_config`. A `ShutdownHandle` allows another thread to stop
socket I/O and join the workers.

This is a development server with a limited HTTP/1.1 profile. It closes each
connection after one response and does not support request bodies, TLS,
keep-alive, chunked transfer, upgrades, or static files. See the
[HTTP profile](docs/http-profile.md) for supported syntax, errors, and limits.

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
| `Document` | Render a complete document with a doctype, language, UTF-8 declaration, title, and body. |

## HTTP API

| Item | Purpose |
| --- | --- |
| `Router` | Register exact GET paths, with automatic HEAD handling and duplicate detection. |
| `Request` | Inspect the method, raw path, raw query, and validated header values. |
| `Response` | Return UTF-8 text or rendered HTML with a typed `StatusCode`. |
| `Server` | Bind a loopback socket and run a bounded worker pool. |
| `ServerConfig` | Configure byte, count, concurrency, and I/O time budgets. |
| `ShutdownHandle` | Stop accepting connections and interrupt socket I/O. |

## Security scope

The renderer escapes `&`, `<`, `>`, double quotes, and apostrophes, and replaces
null characters with the Unicode replacement character. Element and attribute
names come from enums. Script and style elements, event-handler, style, and URL
attributes, foreign content, and raw HTML are outside the current API.

Ruvane does not sanitize existing HTML, encode JavaScript, CSS, or URLs, or
validate HTML nesting and language tags. Applications remain responsible for
renderer allocations and handler CPU, memory, and termination. The server
limits the response size after the handler has built it; it cannot interrupt
arbitrary handler code. GET and HEAD handlers must be read-only.

Malformed framing is rejected before route dispatch. Duplicate headers,
conflicting content lengths, unsupported transfer encodings, and invalid Host
values cannot select a handler. Responses use fixed headers and generic error
bodies. No CORS access is granted. Loopback binding and Host validation do not
authenticate a local user or application.

The alpha has not undergone an independent security audit. Reporting
information is available in [SECURITY.md](SECURITY.md).

## Project status

Ruvane is in early development. The alpha API may change between releases.
This version provides rendering and local serving; it does not yet provide a
complete full-stack runtime or a production transport.

Requires Rust 1.85 or newer and uses the Rust 2024 edition.

## License

MIT. See the LICENSE file.
