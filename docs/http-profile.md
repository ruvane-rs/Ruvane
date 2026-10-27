# Local HTTP profile

Ruvane 0.1.0-alpha.52 implements a small development profile based on the
message format in [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html) and
the semantics in [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html).
It does **not** claim full HTTP/1.1 compliance. In particular, it does not
decode chunked request bodies. It is not intended for public deployment or
use behind a reverse proxy.

## Requests

- HTTP/1.1, with exactly one space between request-line components.
- GET/HEAD, explicit form/JSON POST, JSON PUT/PATCH and bodyless DELETE routes. Known unsupported
  methods return 405 when the rest of the message fits this profile; unknown
  methods return 501. Allow reflects registered methods on a matching path,
  with an empty Allow on unknown paths.
- Origin-form targets beginning with `/`. Absolute URIs, asterisk-form,
  authority-form, fragments, backslashes, repeated slashes, and literal `.` or
  `..` path segments are rejected. Unsupported forms may produce 400 even for
  an unsupported method.
- ASCII URI characters and valid `%HH` escapes. Paths and queries retain
  escapes without decoding. `/a`, `/A`, `/a/`, and `/%61` are distinct routes.
  A percent-encoded slash or dot is literal route data, never a filesystem
  operation. Embedded assets use declared exact paths; no filesystem directory
  serving is implemented.
- Query URI syntax is validated in the transport parser. After raw route
  selection, queries are decoded once under the route's explicit mode, with
  unique decoded keys, strict UTF-8, and fixed budgets. Dynamic captures are
  decoded only after selection and always retain URI semantics. See
  [routing and URLs](routing-and-urls.md) and [GET forms](get-forms.md).
- CRLF line endings only. Header names must be nonempty HTTP tokens. Values
  contain printable ASCII or horizontal tabs; outer spaces/tabs are trimmed.
  Obsolete folded lines and non-ASCII header values are rejected.
- Every header name must be unique, ignoring ASCII case. Even duplicates with
  identical values are rejected rather than combined.
- One nonempty Host is required. Its host must be `localhost` (ignoring ASCII
  case) or the bound IP. Its port must match the listener's actual port; an
  omitted port means 80. IPv6 addresses use brackets. Arbitrary DNS names,
  user information, and other loopback addresses are not aliases for the
  bound address.
- GET/HEAD remain bodyless: absent Content-Length or one decimal zero is
  accepted; positive lengths return 413. Registered POST/PUT/PATCH routes require one
  Content-Length and a bounded body in their declared form/JSON profile. DELETE
  accepts absent/zero Content-Length, with no Content-Type or Content-Encoding. Malformed, overflowing,
  comma-separated or duplicate lengths return 400.
- Transfer-Encoding returns 501, or 400 when Content-Length is also present.
  Expect returns 417 without an interim response. Upgrade returns 501.

There is no keep-alive or request pipelining. The server parses the first
complete header block, writes at most one response, and closes the socket.
For body-bearing mutations it reads the declared body after route/origin/media/size checks. Bytes
beyond the first request's headers and declared body are discarded; a second
request is never dispatched. Closing with unread data can reset the connection. Clients must
not rely on receiving a response to unsupported pipelined traffic.

The framework never forwards requests to another HTTP implementation.
Rejecting ambiguous framing and closing connections does not make this
limited profile suitable for an arbitrary proxy chain.

## Routing and responses

`StaticSite::preview_router` adds exact declared static paths with shared
immutable bodies. It has no URL-to-disk lookup, directory serving, slash or
index-file aliases, and does not serve build metadata. Normal query validation
still applies; valid query fields do not change the planned static content.
See [static export](static-export.md) for its path and hosting contracts.

`Router::get`, `Router::get_form` and `Router::post_form` register an exact, case-sensitive path or a pattern with named
single-segment captures. Exact paths win regardless of registration order.
Invalid, duplicate, or overlapping dynamic patterns fail at registration.
Queries do not affect matching. Ordinary and asset routes preserve literal
`+`; form routes decode raw `+` as spaces, while `%2B` stays a plus. Unknown
paths use the ordinary URI mode. On GET/HEAD an invalid query returns 400 before
any handler, including on an unknown path. Mutation queries are checked after header
preflight and body validation. An unmatched valid request returns 404.
Framing, mutation preflight and unsupported-method checks happen before semantic query decoding,
so a request with multiple faults may receive a framing or method error first.
Every mutation method has its own route namespace and requires the fixed origin policy and body
profile described in [server forms](post-forms.md) and [JSON APIs](json.md). URL queries stay in URI mode
and separate from body fields. No wildcard routes, middleware or dynamic module
lookup are introduced. Actions are explicit Rust handlers, not remote function names.

HEAD uses the same handler as GET, preserving its response status, content
type, and buffered body length, but sends no body. Stream plans preserve chunked
framing without running their factory or producer. A handler can inspect
`Request::method()`. Recognizable HEAD requests also suppress error bodies.
GET/HEAD handlers must be read-only. All handlers must be safe for concurrent
calls; Mutation side effects are not deduplicated or automatically rolled back.

Responses have fixed headers: explicitly selected Content-Type, Content-Length in bytes
for buffered bodies or Transfer-Encoding: chunked for explicit streams,
Connection: close, X-Content-Type-Options: nosniff, and Cache-Control: no-store.
Alpha.6 also sends a fixed Content-Security-Policy on every response, including
HEAD, assets, framing/Host errors, timeouts, and handler failures. It blocks
scripts, base elements, and frame embedding while allowing same-origin CSS,
images, and form actions. No request field can override this policy. See
[metadata and CSP](metadata-and-csp.md) for the full value and scope.
Since alpha.12, an application can explicitly construct an interactive HTML
response with a second fixed policy for same-origin modules/Wasm. The default
profile, errors and assets still block scripts; no arbitrary policy is accepted.
See [browser](browser.md).
405 also includes Allow. Application strings cannot become header names,
header values, or reason phrases. `Response::see_other` adds a typed internal
Location with status 303 and an empty body. Changing its status removes Location.
No arbitrary custom headers, response compression or trailers are supported.
No CORS permissions are emitted. Alpha.23 adds `Response::stream_html` for
bounded typed native document progress; streams retain no-store and the fixed
script-blocking CSP. They send a successful terminal chunk only after complete
production and cleanup. A committed error closes an incomplete response.
See [streaming](streaming.md) for the API, HEAD and failure behavior.
Alpha.24 also uses this bounded writer for opted-in App navigation data with
fixed RVS1 framing, MIME and identity checks. Those streams stay no-store and
retain the script-blocking CSP. See [progressive navigation](progressive-navigation.md).

`Response::html` accepts rendered `Html`; `Response::text` is plain text.
The `link` renderer accepts only an `InternalUrl`, without adding arbitrary
URL attributes to the element builder.
`Response::bytes` supports arbitrary body bytes with a fixed `MediaType`;
`Router::asset` borrows explicitly registered static bytes. `body()` is a byte
slice, and `body_text()` validates UTF-8. For streams they expose only the initial
prefix without invoking a factory; `buffered_body()` returns `None`.
Text types include a UTF-8 charset;
binary MIME types do not. Stylesheets and images have dedicated typed helpers.
See [assets and media](assets-and-media.md).
`Document::metadata` adds typed description/robots/canonical/favicon fields.
Canonical origins are configured explicitly, independently of request headers
and of the loopback listener; they do not enable public serving or HTTPS.
Error bodies are fixed, without reflected input or panic details. Handler
panics are converted to 500 when Rust unwinds; the installed panic hook still
runs. `panic = "abort"` terminates the process instead. A response exceeding
the configured body limit is replaced by a small 500 response before writing
any headers. Streams validate their prefix first and count deferred bytes as
they are produced; exceeding the remaining budget closes the incomplete stream.

## Resource budgets

| Setting | Default | Supported range |
| --- | --- | --- |
| Workers | 4 | 1–64 |
| Queued connections | 32 | 1–1024 |
| Complete request headers, including delimiters | 16 KiB | 32 bytes–1 MiB |
| Request line, excluding CRLF | 4096 bytes | 14 bytes–64 KiB, within the header budget |
| Header fields | 64 | 1–1024 |
| Encoded mutation body | 64 KiB | 0–64 KiB |
| Application response body | 1 MiB | 64 bytes–64 MiB |
| Receiving headers and body, including queue wait | 5 seconds | 1 ms–60 seconds |
| Writing the response, including stream setup/production | 5 seconds | 1 ms–60 seconds |

Budgets are validated before binding. The header reader scans incrementally
and reads at most the configured byte budget. Request deadlines begin when
the application accepts the socket, including subsequent queue time, and
are not renewed by receiving another byte. Write deadlines cover headers
and body together, including stream setup/production after route dispatch.
An incomplete request returns 400 after EOF or 408 after
its deadline when a response can still be written. Line limits return 414;
header byte/count limits return 431.

The accept loop never blocks on a full queue: excess connections are closed
without an HTTP response. The framework retains at most the worker count
plus queue capacity plus one transient accepted socket. Operating-system
backlogs and socket buffers are outside this count.

Shutdown stops accepting, drops queued sockets, interrupts active socket I/O,
and joins workers. Socket waits check shutdown and deadlines in intervals of
at most 50 ms; the accept loop polls at 10 ms. These are polling intervals,
not real-time scheduling guarantees.

Application handlers are trusted code. There is no CPU, allocation, or
execution-time sandbox. A response limit is checked after construction and
cannot prevent that construction from allocating too much memory. A blocked
handler can occupy its worker indefinitely and delay shutdown. Applications
must bound their own work and coordinate cancellation where needed.

## Validation

The dependency-free tests cover rendering, malformed bytes, request
truncation, duplicate and conflicting framing, Host policy, URI handling,
exact size boundaries, fragmented reads, slow clients, queued timeouts,
overload, output backpressure, handler failures, HEAD, and shutdown. A
deterministic byte-mutation corpus exercises the parser without external
test libraries. These checks are regression evidence, not a security audit
or a claim of exhaustive protocol coverage.

## Application page errors

Alpha.9's `App` layer can configure cached HTML 404/500 responses with private
metadata. Routing misses and deliberate page failures retain their real HTTP
statuses. Unwinding handler/layout panics and oversized successful responses
use the configured 500 if it fits the server body budget, otherwise the small
fixed 500. Parser, framing, Host and query-validation failures retain the fixed
protocol responses. HEAD suppresses bodies as before; CSP is unchanged. See
[pages and layouts](pages-and-layouts.md).

Alpha.47 adds [typed cookies](cookies.md). Requests reject malformed or repeated
cookie names before callbacks, with separate count/byte limits. Responses can add
bounded host-only Set-Cookie fields, preserve them on HEAD/303, and emit a fixed
cookie cache-reset signal. Arbitrary header strings remain unsupported.

Alpha.51 adds the explicit [bounded JSON profile](json.md). Body decoding is
selected by route registration before native guards; Content-Type cannot change
a route's profile. Existing source, framing, byte and deadline checks remain.
