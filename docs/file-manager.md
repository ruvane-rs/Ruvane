# Persistent Rust/Wasm file manager

Alpha.59 connects Rust components to the private persistent FileStore. The server
renders a bounded metadata page; Wasm hydrates its existing picker, rows and
native download links. Pagination, inspection, upload and deletion update the
component without a document reload. Native HTML forms remain available.

## Run the example

```sh
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked --example file_manager -- /absolute/trusted-parent/private-files 8080
```

Open `/private-files`, choose alpha or beta, then open the Rust/Wasm manager.
The two accounts are public demo identities, not credential verification. Each
account sees only its own files. Close the current native demo session before
choosing the other identity. `/file-manager` requires a current authorized session;
`/file-home` is a simple navigation destination. Enter stops the process.

The example uses the normal demo Wasm at
`target/wasm32-unknown-unknown/release/ruvane_browser.wasm`; an optional third
argument selects another matching Wasm file. Restart with the same absolute
storage directory, then sign in again to see persisted files. Sessions are
memory-only. Interrupted writer recovery remains an explicit offline operation
through the [native file store](file-storage.md#publication-and-recovery).

## Shared metadata

`FileId`, `FileInfo`, `FileListing` and `FileMetadataError` work in native and Wasm
builds. They contain no owner, filesystem path, MIME authority or file content.

| Type | Validation |
| --- | --- |
| FileId | Nonzero 32-byte opaque ID encoded as exactly 64 lowercase hexadecimal digits |
| FileInfo | Exactly id/name/bytes; typed safe display name, 0..32768 payload bytes |
| FileListing | Exactly files/next; at most three records in strict ascending ID order |
| Continuation | Null, or the last ID of a full three-record page |
| Initial seed | Exact typed flat ActionData fields, at most eleven members |

`FileInfo::new`, `from_json` and `to_json` share the metadata contract.
`FileListing::new`, `from_json`, `to_json`, `from_seed` and `to_seed` enforce the
same limits in both builds. Unknown fields and type coercions are refused. Names
are rendered as escaped text. Debug output redacts IDs and names.

The native loader resolves the current owner from server session data, obtains
at most three records and creates the seed. The component does not perform an
initial metadata GET. Invalid seeds cannot mount. Protected HTML and navigation
are no-store and are never public cache/prerender entries. FileId parsing now
returns FileMetadataError instead of native FileStoreError; RFS1 is unchanged.

## Routes and cursors

The demo service exposes the following guarded routes. Its native forms keep
their existing URLs and 303 behavior.

| Route | Operation |
| --- | --- |
| GET /file-manager | Private SSR or navigation seed |
| GET /private-files/list | First metadata page |
| GET /private-files/list?after=ID | Next page strictly after an opaque ID |
| POST /private-files/files | One multipart attachment; JSON metadata for Accept: application/json |
| GET/HEAD /private-files/files/ID | Native opaque attachment download |
| DELETE /private-files/files/ID | Explicit deletion, returning JSON true |

Every request checks the current session and derives its owner again. Cursors
are positions in opaque ID order and grant no access. Unknown queries and invalid
or duplicate cursors are refused. Previous-page cursors live only in the component,
with bounded history. Pagination is a live view: concurrent changes can alter
later pages. It is not a snapshot or an ordering by upload date. Inspect the first
page again to discover newly inserted records before the current cursor.

## Selection and request lifetime

Rust owns the visible state, stable row keys, cursor history and request intent.
The first-party bridge retains one native browser File, scoped to its component,
control, reset epoch and selection ID. Refresh and pagination retain that same
File. A successful upload resets it and reads the first metadata page. A successful
delete reads the current page. Neither mutation is repeated automatically.

Validation failures, confirmed quota refusals and storage limits retain the
selection for an explicit new submission. Transport failures and malformed success
metadata after submission preserve uncertainty and disable mutations until an
explicit valid inspection succeeds. The runtime's conservative global uncertainty
flag still prevents automatic development reload after a possible mutation.
Inspecting metadata cannot prove whether two otherwise identical files came from
the same attempted upload; the demo provides no idempotency key protocol.

Authorization loss clears private rows, cursors and selection. Leaving a component
aborts pending GETs/uploads and retires its picker and event handles. Late replies
cannot mutate the destination component. Aborting cannot undo a submitted write;
returning mounts fresh server metadata and an empty picker.

Downloads use `View::download`, a typed native anchor with no router/prefetch
markers, no event binding and no fetch through Wasm. The native route checks its
owner and returns application/octet-stream, attachment, no-store, nosniff and the
blocking CSP. File bytes never enter Wasm, navigation packets or cache entries.

## Bounds and validation

The 32 KiB file, 64 KiB HTTP body, per-owner/global storage quotas and trusted
single-writer directory contract remain unchanged. The maximum manager view fits
within 32 nodes and 16 event bindings. This example adds no account authentication,
public uploaded-file mount, large-file streaming or resumable transfer protocol.

ABI v15/RVN9 adds download tag 17, retaining 22 numeric exports, four core runtime
assets and the two first-party crates. Rebuild matching Wasm and bridge together.
There are no third-party Cargo/npm dependencies or new host imports.

`tests/file_manager.rs` checks bounded rendering, invalid continuation, retained
selection, uncertainty and authorization loss. `browser/tests/file-manager.test.mjs`
checks an actual native/Wasm bundle with its sources hidden, pagination, guarded
SSR, restart and owner isolation. Its `/cookie-checks` browser harness also checks
hydration identity, native download bypass, explicit mutations, malformed replies,
quota rejection, late completions and disposal with no live resources.
