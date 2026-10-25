# Local HTTP profile

Ruvane 0.1.0-alpha.3 implements a small development profile based on the
message format in [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html) and
the semantics in [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html).
It does **not** claim full HTTP/1.1 compliance. In particular, it does not
decode chunked request bodies. It is not intended for public deployment or
use behind a reverse proxy.

## Requests

- HTTP/1.1, with exactly one space between request-line components.
- GET and HEAD. Known unsupported methods return 405 with `Allow: GET, HEAD`
  when the rest of the message fits this profile; unknown methods return 501.
- Origin-form targets beginning with `/`. Absolute URIs, asterisk-form,
  authority-form, fragments, backslashes, repeated slashes, and literal `.` or
  `..` path segments are rejected. Unsupported forms may produce 400 even for
  an unsupported method.
- ASCII URI characters and valid `%HH` escapes. Paths and queries retain
  escapes without decoding. `/a`, `/A`, `/a/`, and `/%61` are distinct routes.
  A percent-encoded slash or dot is literal route data, never a filesystem
  operation. No filesystem serving is implemented.
- Queries are validated before dispatch, with unique decoded keys, strict
  UTF-8, and explicit budgets. Dynamic captures are decoded only after raw
  route selection. See [routing and URLs](routing-and-urls.md).
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
- No request body is supported. An absent Content-Length or a single valid
  decimal value equal to zero is accepted. Positive lengths return 413;
  malformed, overflowing, comma-separated, or duplicate lengths return 400.
- Transfer-Encoding returns 501, or 400 when Content-Length is also present.
  Expect returns 417 without an interim response. Upgrade returns 501.

There is no keep-alive or request pipelining. The server parses the first
complete header block, writes at most one response, and closes the socket.
Bytes following that block are discarded; a second request is never
dispatched. Closing with unread data can reset the connection. Clients must
not rely on receiving a response to unsupported pipelined traffic.

The framework never forwards requests to another HTTP implementation.
Rejecting ambiguous framing and closing connections does not make this
limited profile suitable for an arbitrary proxy chain.

## Routing and responses

`Router::get` registers an exact, case-sensitive path or a pattern with named
single-segment captures. Exact paths win regardless of registration order.
Invalid, duplicate, or overlapping dynamic patterns fail at registration.
Queries do not affect matching. An unmatched path returns 404. There are no
wildcard routes, redirects, middleware, actions, or dynamic module lookup.

HEAD uses the same handler as GET, preserving its response status, content
type, and calculated body length, but sends no body. A handler can inspect
`Request::method()`. Recognizable HEAD requests also suppress error bodies.
Handlers must be read-only and safe for concurrent calls and retries.

Responses have fixed headers: UTF-8 Content-Type, Content-Length in bytes,
Connection: close, X-Content-Type-Options: nosniff, and Cache-Control: no-store.
405 also includes Allow. Application strings cannot become header names,
header values, or reason phrases. No custom headers, response compression,
trailers, or streaming are supported. No CORS permissions are emitted.

`Response::html` accepts rendered `Html`; `Response::text` is plain text.
The `link` renderer accepts only an `InternalUrl`, without adding arbitrary
URL attributes to the element builder.
Error bodies are fixed, without reflected input or panic details. Handler
panics are converted to 500 when Rust unwinds; the installed panic hook still
runs. `panic = "abort"` terminates the process instead. A response exceeding
the configured body limit is replaced by a small 500 response before writing
any headers.

## Resource budgets

| Setting | Default | Supported range |
| --- | --- | --- |
| Workers | 4 | 1–64 |
| Queued connections | 32 | 1–1024 |
| Complete request headers, including delimiters | 16 KiB | 32 bytes–1 MiB |
| Request line, excluding CRLF | 4096 bytes | 14 bytes–64 KiB, within the header budget |
| Header fields | 64 | 1–1024 |
| Application response body | 1 MiB | 64 bytes–64 MiB |
| Receiving headers, including queue wait | 5 seconds | 1 ms–60 seconds |
| Writing the complete response | 5 seconds | 1 ms–60 seconds |

Budgets are validated before binding. The header reader scans incrementally
and reads at most the configured byte budget. Request deadlines begin when
the application accepts the socket, including subsequent queue time, and
are not renewed by receiving another byte. Write deadlines cover headers
and body together. An incomplete request returns 400 after EOF or 408 after
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
