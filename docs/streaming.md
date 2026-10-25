# Progressive HTML responses

Alpha.23 adds explicit native HTTP streaming through `HtmlStream` and
`Response::stream_html`. The initial document reaches the socket before the
producer computes its first deferred fragment. This API uses only the standard
library and the existing worker pool.

```rust
use ruvane::{Document, Html, HtmlStream, Response, StreamContext, StreamError, Tag, element};
use std::time::Duration;

let document = Document::new("Report", element(Tag::H1).child("Report header").render());
let plan = HtmlStream::new(document, |_| {
    let mut sent = false;
    Ok(move |context: &StreamContext| -> Result<Option<Html>, StreamError> {
        if sent {
            return Ok(None);
        }
        sent = true;
        context.wait(Duration::from_millis(500))?;
        Ok(Some(element(Tag::P).child("Deferred <report> & details").render()))
    })
});
let response = Response::stream_html(plan);
assert!(response.is_streaming());
assert!(response.buffered_body().is_none());
```

Return that response from a GET handler. The route handler builds a plan, not a
complete body. Each GET calls its factory once to create a fresh `HtmlProducer`;
a closure with the shown signature implements the trait. Cloning a response
shares its immutable plan, not a partially consumed producer.
Captured application state can still be shared deliberately and must be safe
for concurrent requests.

The factory runs before headers, so an immediate setup failure can select a
500 response. Keep it cheap. Data loading and deferred rendering belong in
`next`. After headers and the initial prefix are written, the worker calls
`next`, validates the fragment and writes it completely before pulling again.
There is no producer thread, unbounded channel or background task. Socket writes
provide backpressure; acceptance by the OS socket buffer does not mean the
browser has already displayed the bytes.

`Some(Html)` appends a body sibling after the initial content. `None` closes
`body` and `html`. The outer head, title and metadata are already committed;
fragments cannot fill an arbitrary slot inside an open parent. Empty fragments
consume a producer-yield budget but emit no HTTP chunk. All fragments use the
existing typed, escaped HTML renderer. There is no raw HTML streaming API.

## Budgets and cancellation

`HtmlStream::limits(StreamLimits { ... })` validates these ranges:

| Limit | Default | Supported range |
| --- | --- | --- |
| Decoded bytes per chunk | 16 KiB | 1 byte–64 KiB |
| Total decoded body bytes | 1 MiB | 1 byte–64 MiB |
| Prefix, suffix and producer yields | 128 | 2–1024 |
| Factory, production and writes together | 5 seconds | 1 ms–60 seconds |

The server tightens total bytes with `ServerConfig::max_response_body_bytes`
and duration with `ServerConfig::write_timeout`. The prefix and closing suffix
must each fit the chunk limit and their combined size must fit the body limit.
These checks run before the factory or headers, including for HEAD. Limits
count UTF-8 bytes after HTML escaping, excluding HTTP framing. A maximum of
`chunks - 2` `Some` yields is allowed, including empty yields. The final `None`
does not consume a yield.

One absolute monotonic deadline begins after route dispatch, before factory
setup. It is never extended per fragment. `StreamContext::remaining` reports
the remaining time; `check` checks deadline and server shutdown. `wait` performs
a cooperative wait, checking both at intervals of at most 10 ms. Socket writes
retain the server's shutdown checks and remaining-time budget.

A disconnect stops further production when a socket write fails. A callback
already running must return or cooperate with the deadline; it cannot detect
every remote disconnect immediately. A client closing only its request-writing
half is allowed to continue receiving the response.

Factories, producers and destructors are trusted synchronous Rust code.
The framework cannot preempt their CPU work, blocking I/O or destructor, and
cannot prevent them from allocating an oversized fragment before checking it.
Use bounded inputs, allocations and operations, and pass the remaining time to
application I/O. Cooperative shutdown can join only after a running callback
returns. The framework retains the prefix and one returned fragment, with no
accumulated response body; memory retained inside application callbacks is the
application's responsibility.

## Wire and failure behavior

Streaming responses use `Transfer-Encoding: chunked`, omit Content-Length and
retain `Connection: close`, `Cache-Control: no-store`, nosniff and the fixed
script-blocking CSP. Chunk lengths count payload bytes. No trailers, extensions,
compression or arbitrary headers are exposed by this API. Initial App documents have their own
[opt-in streaming path](document-streaming.md) with the existing interactive CSP.
Chunked request bodies are still rejected. See
[RFC 9112 section 7.1](https://www.rfc-editor.org/rfc/rfc9112.html#section-7.1).

HEAD builds and validates the same plan but executes neither the factory nor
the producer. It returns the stream's headers, including chunked framing,
with no content or terminal chunk. The route handler itself still runs and
must be read-only. Since factory errors are deliberately not probed, HEAD can
return 200 when a later GET would fail during setup. See
[RFC 9110 section 9.3.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.3.2).

Invalid prefix limits, a stream returned by POST, and factory errors or
unwinding panics select a bounded buffered 500 before headers. The configured
error response is used only if buffered and within the server body budget;
otherwise the fixed generic error is used. If the deadline or shutdown has
already expired, the server may close without enough time to write an error.

After headers, a producer error, limit, unwinding panic, shutdown, expired
deadline or write failure closes the socket. There is no second HTTP status,
inline diagnostic or fabricated successful ending. The zero-size terminal
chunk is sent only after successful production, socket writes and producer
cleanup. Its absence makes the chunked response incomplete, even if some HTML
was displayed; Fetch rejects an incomplete body. See
[RFC 9112 section 8](https://www.rfc-editor.org/rfc/rfc9112.html#section-8).
The installed panic hook still runs locally. `panic = "abort"` cannot be caught.
Cancellation and failure do not roll back application side effects.

`Response::body`, `body_text` and `HtmlStream::prefix` inspect only the already
rendered prefix of a stream. They never execute the factory. Use
`Response::buffered_body` to require a complete buffered body, or
`Response::is_streaming` to identify the plan. Static export accepts complete
HTML/bytes and never runs stream producers.

## Demo and scope

```sh
cargo run --offline --locked --example streaming -- 3004
```

Open `/stream` for a script-free report. `/tests` has six dependency-free browser
checks. Run them, inspect the report window while only its header is visible,
then use **Release deferred report** in the checks window. The gate endpoints
are explicit POST forms with the normal origin checks, solely for this example.
The example allows 60 seconds for manual inspection; ordinary plans default
to five seconds. Native tests also exercise panic handling, quotas, disconnects,
slow readers, shutdown and worker recovery.

New CLI interactive projects include a native `/stream` page and an ordinary
link to it. `?q=...` demonstrates escaped deferred content; `?fail=early` and
`?fail=late` demonstrate failure before and after headers. The route is attached
explicitly to the router after `App::router`; it is not a client-page loader.

Alpha.24 also offers [progressive client navigation](progressive-navigation.md)
for opted-in App pages, with typed layouts and a pending leaf. That RVS1 data
transport shares the bounded socket writer but has its own ownership protocol.
Complete RVP1/RVP2/RVP3 navigation remains available.
[Initial App document streaming](document-streaming.md) has a separate opt-in.
Partial hydration, parallel outlets and out-of-order replacement remain future work. The loopback development transport still has no TLS,
HTTP/2 or public deployment profile.
