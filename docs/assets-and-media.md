# Assets and media

Ruvane 0.1.0-alpha.4 adds raw byte responses, embedded assets, stylesheet links,
and image rendering using only standard Rust and project code.

## Responses

`Response::bytes(data, media_type)` owns a `Vec<u8>` (or a value convertible
into one). It preserves every byte, including NUL, non-UTF-8 data, and CRLF.
`Response::text` and `Response::html` still produce UTF-8 bodies, with the
renderer's escaping rules unchanged.

`MediaType` is a fixed enum: `PlainText`, `Css`, `Png`, `Jpeg`, `WebP`, and
`OctetStream`. Each variant supplies a static Content-Type value. Text types
declare UTF-8. HTML is available only through `Response::html`; the raw byte
enum has no HTML, SVG, or JavaScript variant. A type declaration is not file
inspection, content sanitization, or validation of the format.

Content-Length is calculated from body bytes. HEAD produces the same status,
type, and length as GET without sending the body. The serializer retains
Connection: close, Cache-Control: no-store, and X-Content-Type-Options: nosniff.
No filename or application string becomes an HTTP header value.

The same server output budget applies to text, owned bytes, and embedded
assets. A body over that limit becomes a generic 500 before any response
headers are written. Owned response allocations occur before that check;
the framework does not sandbox application memory or computation.

## Embedded asset routes

`Router::asset(path, include_bytes!("file"), media_type)` registers one fixed
GET/HEAD endpoint. The path passes `InternalUrl` validation: a single leading
slash, no query, fragment, authority, dynamic capture, decoded separator,
dot segment, invalid UTF-8, or control character.

Assets share the GET namespace. An existing route or asset at the exact path
causes an error, in either registration order. Like any fixed route, an asset
takes priority over a matching dynamic pattern. Lookup remains raw and
case-sensitive: an undeclared encoded alias does not expose a declared asset.
Queries do not select different asset bytes and still pass normal query
validation before dispatch. Known unsupported methods return 405.

The response borrows the registered static slice. No per-request body copy,
directory traversal, directory listing, runtime file open, symlink lookup,
or URL-to-filesystem mapping is implemented. `include_bytes!` selects a file
at compile time under the application's control; changing that file requires
a rebuild. Asset registration itself performs no filesystem access.

| Registration budget per router | Limit |
| --- | --- |
| Individual declared asset | `MAX_ASSET_BYTES`: 1 MiB |
| Asset registrations, including empty assets | `MAX_ASSETS`: 64 |
| Total declared asset bytes | `MAX_TOTAL_ASSET_BYTES`: 8 MiB |

The same slice at multiple paths counts its length each time. Registration
checks precede route/counter mutation. Failed registration consumes no budget
and reserves no path. These bounds concern registered metadata and exposed
bytes, not the size of all data compiled into the application. The server's
configured response limit may be lower and still takes precedence at dispatch.

## Stylesheets and images

`Document::stylesheet(InternalUrl)` emits a void
`<link rel="stylesheet" href="...">` in the document head. Identical URLs
are deduplicated, retaining insertion order. Attribute syntax is escaped.
The helper does not check route existence, stylesheet MIME type, or CSS syntax.

`image(&InternalUrl, alternative, width, height)` emits a void `<img>` with
quoted, escaped URL and alternative text. Width and height are `u32` values
from 1 to `MAX_IMAGE_DIMENSION` (8192). Empty alternative text is allowed for
decorative images. These are layout dimensions; Ruvane does not inspect
intrinsic dimensions or decode, resize, compress, or re-encode images.

The generic `Tag` builder remains for normal non-void elements. It gains no
arbitrary `src`, `href`, `style`, or event-handler attributes. There is no
inline CSS constructor. The mini-blog's stylesheet and three original PNG
illustrations are checked into `examples/assets/`, embedded, and served by
the same local server. System fonts require no external font requests.

## Security and scope

CSS is trusted application code and may contain its own URL references or
imports. Do not serve untrusted input as CSS or assume `nosniff` sanitizes it.
The browser's MIME checks require an appropriate stylesheet Content-Type;
see the [Fetch Standard](https://fetch.spec.whatwg.org/#should-response-to-request-be-blocked-due-to-nosniff).

PNG, JPEG, and WebP declarations do not prove that bytes are valid images or
safe for a particular browser decoder. There is no upload pipeline or media
validator. The internal URL type restricts the helper's URL structure, not
the subsequent actions of arbitrary application responses. Resource access
and appropriate use of MIME types remain application responsibilities.

Tests cover exact binary bodies and lengths, GET/HEAD parity, fixed MIME
values, registration and output limits, failed-registration behavior, route
conflicts, undeclared paths, and escaping of stylesheet/image attributes.
The styled mini-blog is exercised using its real handlers and embedded bytes.

## Migration from alpha.3

`Response::body()` now returns `&[u8]`, rather than `&str`. Consumers needing
UTF-8 use `body_text()`, which returns `Result<&str, std::str::Utf8Error>`.
Byte consumers can use `body()` directly. Existing HTML output is unchanged
unless the application opts into the new stylesheets or image helpers.
