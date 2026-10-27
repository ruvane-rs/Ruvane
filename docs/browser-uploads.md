# Component file uploads

Alpha.57 adds explicit multipart POSTs from Rust/Wasm components. Rust owns the
form model, validation and response handling. The first-party bridge retains the
browser `File` and sends it with native FormData. No Cargo or npm dependency is
added. Native [multipart forms](multipart.md) remain available without JavaScript.

## Component flow

Create one labelled file control with `View::file(parent, key, event, invalid)`.
The control has no value or initial selection. Its change event supplies
`EventValue::File(FileSelection)`: Empty, Selected(BrowserFile), or a fixed local
Rejected(FileSelectionError). A selected empty file has size zero and remains
distinct from an empty picker.

`BrowserFile` exposes a display name, size and untrusted MIME hint. It contains
neither file bytes nor an operating-system path. Store the reference in the
component, validate the other business fields, and queue one request:

```rust,ignore
let request = UploadRequest::new(
    1,
    InternalUrl::new("/upload")?,
    "attachment",
    selected_file.clone(),
    FormData::default().field("caption", &caption)?,
)?;
```

Keep an `Option<UploadRequest>` and return it from `Component::take_upload`.
`Component::upload_result(action, UploadOutcome)` receives the result after the
runtime checks its live component and request token. `UploadOutcome` uses the
same [bounded JSON result](json-client.md) as component JSON requests, including
trusted field/code validation reports. The server must return JSON for successful
interactive uploads. `Response::validation_errors` supplies a typed 400 report.
The example selects this response profile with `Accept: application/json` and
keeps a 303 HTML flow for native forms.

File changes, invalid selections, subtree removal and component destruction
retire the previous reference. A request must refer to the current selection of
that exact view, control and reset epoch. Cloning a reference does not extend its
lifetime. Explicit reset uses a checked increase of `View::field_epoch` and clears
the component's stored reference. Text/error updates preserve the live File and
picker identity while the epoch is unchanged. Preserve the file after a validation
failure so the user can correct the fields and submit explicitly.

One combined pending slot covers form actions, JSON and uploads. Queuing two
requests at once or reusing a retired file rejects the component. Mark controls
pending in the Rust view and clear pending on completion. Initial seeded App
pages and read-only presentation components cannot queue uploads. SSR file
controls start disabled; hydration activates the existing empty picker and
rejects a preselected picker instead of inventing a server selection.

## Transport and limits

| Resource | Limit |
| --- | --- |
| File controls / selected files per component | 1 / 1 |
| Selected file | 32 KiB, including a valid zero-byte file |
| UTF-8 display name | 1..255 bytes; no controls, slash, backslash, colon, dot or dot-dot names |
| Browser MIME hint | 127 printable ASCII bytes |
| Metadata event | 400 bytes maximum |
| Business text fields | 16, with distinct declared identifiers |
| One text value / encoded fields | 1024 UTF-8 bytes / 16 KiB |
| Serialized multipart body / decoded JSON response | 64 KiB / 64 KiB |
| Request URL | 1024 encoded bytes, internal only |

The file field cannot collide with a business field or `_ruvane_csrf`. Business
fields are decoded once before native FormData construction. Files always use
the opaque `application/octet-stream` part type, regardless of their browser
hint. The native serializer supplies the multipart boundary; its exact Content-Type
is retained. The bridge reads the serialization under the body budget before
submitting anything. Filename quotes may be escaped by the native serializer;
returned filenames are display metadata chosen by the server, never identities
or paths. The server independently enforces its own name and body limits.

Fetch uses POST, same-origin mode and credentials, redirect errors and no-store.
Application requests carry their matching build revision. With `csrfEndpoint`
configured, each upload first fetches a fresh header-only session CSRF claim.
Selection and serialization failures send no upload. Server multipart parsing,
origin checks, session/CSRF guards and quotas remain authoritative; a browser
reference is not an authorization credential.

Fetch, serialization and response reading share the action deadline. Destruction
or navigation aborts transport, releases the runtime's file record and ignores
late completions. Abort cannot undo a submitted server mutation. Timeout, network
loss or malformed replies after submission mark the result uncertain. Inspect
server state before another explicit attempt; uploads are never retried
automatically. A confirmed 413 is a rejection. A framework 429 with its rejection
marker preserves a bounded Retry-After hint; the hint does not schedule a retry.

`runtime.fileState()` returns only aggregate selected-file and byte counts for
lifecycle checks. Counts describe live runtime references, not garbage collection
or immediate browser memory reclamation. File content is never placed in props,
seeds, navigation cache entries or Wasm ingress. This transport itself has no disk writes,
public uploaded-file routes, MIME verification, large-file streaming, resumable
transfers or byte-progress reporting.

## ABI migration

Rebuild application Wasm and its matching bridge together. ABI v15/RVN9 rejects
ABI v14 modules and hydration roots. The 22 numeric function exports, four core
runtime assets and two local crates are unchanged. Tag 16 is the file input;
opcode 15 carries its reset epoch and opcode 16 queues an upload. Results reuse
RVJ1, while file changes use payload kind 3 and the existing numeric chunk ingress:

| RUF1 field | Encoding |
| --- | --- |
| Header / selection kind | `RUF1`, u8 |
| Empty / too large / invalid metadata / unavailable | Kinds 0 / 2 / 3 / 4, no further bytes |
| Selected | Kind 1, nonzero u32 selection ID, u32 file size |
| Name / MIME hint | Each u16 byte length followed by its bytes |

All integers are little-endian. Selected metadata must be exact, bounded UTF-8
for the name and printable ASCII for the hint, with no trailing bytes. View,
node and epoch come from the validated live binding. Selection IDs increase
without wrapping or reuse within one runtime.

## Demo and verification

```sh
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked --example browser_uploads -- 8080
```

Open `/interactive`, open a demo session, choose a file and submit a caption.
Local and server validation retain the selection. Success clears it and reads the
stored metadata. `/` serves the native fallback. The demo holds at most eight
shared files in process memory; it provides no real account authentication or
per-user file isolation. Enter stops the server.

For persistent owner-scoped attachments and downloads, use the separate
[private file store](file-storage.md) and [Rust/Wasm manager](file-manager.md). The original shared
memory uploader remains a transport/lifecycle example.

`/upload-tests` runs real-browser selection, validation, quota, deadline, disposal
and repeated-reset checks. `browser/tests/uploads.test.mjs` validates command
framing, typed file ingress and source-hidden native/Wasm packaging. Its compiled
fixture also serves `/cookie-checks` for real initial App hydration and navigation
during submitted uploads and delayed CSRF preflights.
