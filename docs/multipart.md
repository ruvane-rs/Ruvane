# Native multipart forms

Alpha.56 adds an explicit native multipart POST profile using only `std`. Text
fields and one opaque binary file are parsed before guards and application
handlers. The existing 64 KiB HTTP body ceiling, worker/queue bounds, origin
checks, request deadline and connection-close framing still apply.

```rust
use ruvane::{InternalUrl, MultipartForm, Response, Router, StatusCode};
let mut router = Router::new();
router.post_multipart("/upload", |request| {
    let data = request.multipart().unwrap();
    let Some(file) = data.file().filter(|file| !file.file_name().is_empty()) else {
        return Response::text("Select a file.").with_status(StatusCode::BadRequest);
    };
    // Application validation and authorized storage belong here.
    Response::text(format!("Received {} bytes", file.len()))
}).unwrap();
let form = MultipartForm::new(InternalUrl::new("/upload").unwrap()).unwrap()
    .text("caption", "Caption", "").unwrap()
    .file("attachment", "File").unwrap()
    .submit_label("Upload").unwrap();
assert!(form.render().as_str().contains("enctype=\"multipart/form-data\""));
```

The snippet demonstrates parsing; add session/CSRF and quota guards for private
uploads, as in `examples/support/multipart_demo.rs`. `FileEndpoint::post_multipart`
supports inherited file-route guards and shares the POST registration slot with
URL-encoded forms, JSON and actions. Duplicate method declarations reject the
registration batch atomically. Request MIME cannot select another body profile.

## Data and form API

`Request::multipart()` is present only on parsed multipart routes. Its `fields()`
returns `FormData` business text accessors, with unique ASCII identifiers and
UTF-8 values. CRLF becomes LF; LF and tabs are allowed, other controls are refused.
Plus signs and percent sequences remain literal. Query parameters stay separate.
`Request::form_data()` and `Request::json()` return None on multipart requests.
Sanitized background/prerender requests carry no multipart data or claims.

`file()` returns at most one `UploadedFile`. `field_name()` identifies its form
control; `file_name()` and `content_type()` are untrusted metadata. `bytes()`
borrows unchanged binary data, including NUL, non-UTF-8 and CRLF. `len()` and
`is_empty()` describe the payload. Request clones share file bytes through an Arc.
No path is opened and no file is written by parsing or route registration.

A browser may submit an unselected control as `filename=""` with empty bytes.
This still occupies the single file slot. A selected zero-byte file has a nonempty
name. Applications decide whether a file is required and which field is expected;
the parser does not infer an application schema. Empty filenames with nonempty
payloads are rejected.

`MultipartForm` provides text, textarea, checkbox, hidden and one file control,
plus csrf, invalid and submit_label. It renders native POST HTML with fixed
multipart encoding and UTF-8, wrapping labels and escaped text. No script,
multiple-selection flag, default file value or arbitrary form override is emitted.
The server remains responsible for input validation. Browser file selections
cannot be restored following a native form navigation; the demo preserves safe
caption text and asks the user to select the file again on validation failure.

## Limits and accepted grammar

| Resource | Hard limit |
| --- | ---: |
| Entire encoded body | 65536 bytes; ServerConfig may lower this |
| Single file payload | 32768 bytes |
| Business text fields | 16 |
| Parts including file and reserved CSRF | 18 |
| Normalized text value | 1024 UTF-8 bytes; at most 2048 raw bytes before CRLF normalization |
| Field name | 64 ASCII identifier bytes |
| Supplied filename | 255 UTF-8 bytes |
| Boundary | 1–70 ASCII bytes |
| Part headers including blank terminator | 2048 bytes |
| File MIME declaration | 127 ASCII bytes |

This is a deliberately limited profile of
[RFC 7578](https://www.rfc-editor.org/rfc/rfc7578.html), rather than a general MIME
implementation. Content-Type requires exactly one boundary parameter, token or
quoted. The boundary uses ASCII letters/digits and `'()+_,-./:=?`; unquoted values
must also satisfy HTTP token syntax. Boundaries containing spaces, extra parameters,
preambles and epilogues are refused. Delimiters require CRLF; the closing delimiter
may have one final CRLF or end immediately. Prefix lookalikes inside a file stay
file data unless they form a complete delimiter.

Each part requires one Content-Disposition: form-data with exactly one name and
an optional filename. Parameters may be tokens or quoted strings; quoted values
support escaped quotes/backslashes. UTF-8 filenames are supported. Other
parameters, filename*, duplicate headers, folding, transfer encodings and unknown
part headers are refused. A part may also have one Content-Type. Text accepts
text/plain with an optional UTF-8 charset. Files accept a parameter-free type/subtype,
defaulting to application/octet-stream; nested multipart is refused. No charset
field changes text decoding, and no base64, percent or MIME-word decoding occurs.

File/text name collisions and all repeated names are refused. Filenames containing
controls, slash, backslash, colon, or exactly `.`/`..` are rejected. Percent
sequences are display data and are never decoded into paths. File metadata and
payloads are omitted from MultipartData/UploadedFile Debug output.

## Security and failures

Host, same-origin Origin, Fetch Metadata, HTTP framing, request MIME and declared
size are checked before further body reads. Valid multipart framing and field
limits are checked before route guards. Session/CSRF/quota guards then run before
the handler. Bodies are bounded and buffered; a quota guard does not prevent the
bounded body read or parsing cost.

Native `.csrf(token)` adds the existing reserved hidden field. Multipart text
claims are separated from business fields and verified against the current
session by `SessionStore::csrf_guard()`. A simultaneous header claim is ambiguous,
duplicate reserved parts fail parsing, and file parts cannot impersonate the
reserved field. Query fields never supply a claim. Token-bearing pages must stay
private and uncached. Parsing a token alone does not authenticate a request.

| Failure | Status |
| --- | --- |
| Invalid MIME or boundary declaration, content encoding | 415 |
| Missing declared length | 411 |
| Invalid source origin or failed session/CSRF guard | 403 |
| Malformed/truncated framing, duplicate parts or unsafe filename | 400 |
| Body, file, text, part-count or header-byte limit | 413 |
| Request deadline | 408 |
| Exhausted guard quota | 429 with bounded Retry-After |

HTTP-level duplicate headers, Transfer-Encoding and Expect retain the existing
[HTTP profile](http-profile.md). Errors use fixed messages without filenames,
field values, file bytes or CSRF tokens. Application validation may return escaped
HTML with its own 400 response. Supplied extensions and MIME declarations are
never proof that a file is safe. Applications must choose their own authorized
storage identifiers and content policy; this release adds no executable upload
mount, public file-serving route, MIME scanner or persistence service.

## Demo and checks

```sh
cargo run --offline --locked --example multipart -- 8080
```

Open a local demo session, enter a caption and submit one file without JavaScript.
The demo stores at most eight files (at most 256 KiB of file payloads) in shared
process memory, displays escaped metadata, and requires session/CSRF/quota guards
for uploads. There is no real account authentication. Restarting loses the files
and sessions. Enter stops the server. Uploaded filenames never select disk paths.

Tests exercise binary preservation, native FormData encoding, quoted and Unicode
filenames, delimiter lookalikes, truncation, every budget, duplicate namespaces,
CSRF header/field ambiguity, origins, quotas, deadlines and capacity. CLI tests
serve a compiled bundle with source files hidden and check that native upload
handlers stay out of Wasm. Alpha.56 retained ABI v13 and its 22 numeric function
exports. Alpha.57 adds [component-owned browser uploads](browser-uploads.md) with
ABI v14; the native multipart profile and four core runtime assets stay the same.
Large-file streaming remains outside this profile.
