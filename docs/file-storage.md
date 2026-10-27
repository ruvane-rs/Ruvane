# Private persistent files

Alpha.58 adds a native `FileStore`, typed attachment names and opaque downloads.
The backend uses only `std` and the existing native OS entropy source. File bytes,
owner keys, paths and storage I/O remain outside Wasm. The current browser ABI is v15,
with 22 numeric exports and four core runtime assets.

## Open a private store

```rust,no_run
use ruvane::{FileOwner, FileStore, FileStoreLimits};
let files = FileStore::open("/srv/my-app/private-files", "my-app", FileStoreLimits::default())?;
let owner = FileOwner::new("account-123")?;
let metadata = files.list(&owner)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Supply an absolute real path under a trusted, existing parent. The parent and
its ancestors must not be symlinks or concurrently changed by other writers.
Use a trusted local filesystem supporting file/directory sync and atomic rename.
The backend supports the existing macOS and Linux x86_64/aarch64 entropy profile;
other platforms return UnsupportedPlatform. Use one writer process per directory.

The store creates its own 0700 directory and 0600 files. A pre-existing directory
must be private and empty, or contain this application's valid store. Foreign
files, subdirectories, symbolic links, hard-linked files, unsafe permissions and
incorrect application markers are refused. Paths are startup configuration,
never request fields. Do not point a store at user documents or a public asset
directory. Keep a backup before changing an existing store's configuration.

Clones share one lock and quota state. Open the store once, then pass clones to
native handlers. A process-local registry and exclusive LOCK file reject another
open of the same directory. Dropping the last healthy handle removes the lock.

## Owner-aware operations

`FileOwner::new` accepts a stable ASCII application key of 1..64 bytes. Resolve
that key from authorized server state, such as authenticated account data in a
session. It is neither an authentication system nor a credential. Never accept
the requested owner directly from a URL, form or JSON field.

- `save(&owner, &UploadedFile)` returns `FileInfo` with a new OS-random `FileId`.
- `list(&owner)` returns only that owner's metadata, in opaque ID order.
- `read(&owner, id)` returns a `StoredFile` or None.
- `remove(&owner, id)` publishes a deletion and returns whether it removed a file.

Unknown and foreign IDs both produce None/false. File IDs have exactly 64
lowercase hexadecimal digits and convey no authorization. Original filenames
remain display metadata; they never select disk paths. The browser MIME hint is
discarded. `StoredFile::bytes` preserves the exact original binary payload,
including valid selected empty files. Debug output redacts IDs, owners, names,
paths and payloads.

Sessions still expire, rotate and disappear after a restart. Re-authenticate the
caller and resolve the same stable application owner to access persisted files.
Possessing an old cookie or another file's ID never restores access by itself.

## Bounds and storage format

| Resource | Default | Hard maximum |
| --- | --- | --- |
| One uploaded payload | 32 KiB | 32 KiB |
| Files in a store | 64 | 256 |
| Payload bytes in a store | 2 MiB | 8 MiB |
| Files per owner | 8 | Global file quota |
| Payload bytes per owner | 256 KiB | Global byte quota |
| Filename | 255 UTF-8 bytes | 255 UTF-8 bytes |

`FileStoreLimits::new(files, bytes, owner_files, owner_bytes)` requires positive
limits and per-owner quotas no larger than the global quotas. Empty files consume
a file slot. Existing data exceeding newly lowered limits is refused, never
silently evicted. HTTP bodies retain their existing 64 KiB ceiling.

The legacy RFS1 snapshot contains the application identifier, count and complete
records: opaque ID, owner key, filename and payload. Lengths and integer fields
are bounded little-endian values. A CRC32 detects accidental corruption; it does
not authenticate data supplied by an attacker. Filesystem ownership and the
trusted-directory contract remain the protection against other writers.

This first backend retains the bounded data in memory and rewrites the complete
snapshot for each save or deletion. Write serialization and temporary copies
are bounded by the hard quotas. During publication, two catalog files can coexist;
disk usage includes both copies and their bounded metadata, beyond the payload
quota alone. This is suitable for small private attachments;
it is not large-file streaming, a general database or a multi-process store.

## Publication and recovery

A write creates `next.bin` exclusively, writes the complete snapshot, syncs the
file, renames it over `store.bin` within the same directory, then syncs the
directory. Initialization also syncs the parent directory before publishing the
new store's identity. Acknowledged writes retain their contents across a clean
restart on the supported filesystem. Deletion uses the same publication path.

An I/O failure poisons that handle and retains its writer lock. An error before
publication reports Unavailable; a directory-sync failure after rename reports
Uncertain because the change may already be published. Inspect the store after
recovery before issuing another explicit mutation. No operation retries a write.

An interrupted process leaves LOCK and possibly `next.bin`. Ordinary open
returns Busy. **Stop every writer process before calling `FileStore::recover`.**
The caller must guarantee exclusive offline access; `std` under the MSRV cannot
prove that another process has stopped. A live handle in this process is refused.
Do not expose recovery over HTTP or invoke it automatically after Busy.

Recovery validates the directory identity, the complete committed snapshot and
configured quotas before removing an unfinished next snapshot or stale lock.
The committed data stays authoritative; unpublished data is discarded. Missing,
truncated or corrupt committed data is preserved and refused rather than replaced
with an empty store. Restore a valid offline backup when recovery cannot validate
the committed snapshot.

## Safe attachment responses

Consume an authorized file with `file.into_response()`, or construct
`Response::download(bytes, DownloadName::new(name)?)`. Names must have 1..255 UTF-8
bytes, without controls, path separators, colon or dot/dot-dot names. Headers use
a fixed ASCII fallback and percent-encoded UTF-8 `filename*`, with no raw input
inserted into header syntax.

Downloads use application/octet-stream, Content-Disposition attachment, no-store,
nosniff and the script-blocking CSP. GET and HEAD preserve the same Content-Length;
HEAD sends no body. These response constructors do not authenticate a caller.
Route guards and the owner-aware read must run before constructing the response.

## Native demo and checks

```sh
cargo run --offline --locked --example file_store -- /absolute/trusted-parent/private-files 8080
```

Open `/private-files`. Choose alpha or beta, upload a file, download it and delete
it. Enter stops the server; restarting with the same directory retains files.
Choose the same demo identity again after restart. Both identities are public
test identities, not real account authentication. Only the selected owner's files
are listed or served. Guarded writes require session CSRF claims and quotas.
Downloads are ordinary native links, and the complete demo works without scripts.

The example also exposes guarded JSON metadata and multipart upload responses
for `Accept: application/json`, plus bodyless DELETE for component clients.
Existing `UploadRequest`/`JsonRequest` transports can use these endpoints without
an ABI change. HTML removal forms retain their native POST/303 flow.

After an interrupted demo, stop all its processes and pass `--recover` after the
port to recover the last complete snapshot. Use that flag only for the known
offline directory you intend to recover.

Unit tests cover owners, quotas, concurrent calls, redaction, symlinks, hard
links, permissions, damaged catalogs, failed publication and offline recovery.
HTTP tests cover exact binary GET/HEAD, native forms, CSRF, owner isolation,
deletion and restarts. The CLI fixture serves a compiled source-hidden bundle,
kills its writer process, verifies normal startup refusal and performs explicit
recovery. Its Wasm module excludes the native backend.

For private SSR metadata and a Rust/Wasm interface over the same backend, see the
[persistent file manager](file-manager.md). FileId and FileInfo are now shared
metadata types. FileId::parse and FromStr return FileMetadataError; FileOwner and
storage operations retain FileStoreError. Alpha.60 reads RFS1 and upgrades on a successful write to RFS2; see
[upload operation storage and compatibility](file-operations.md#rfs1-compatibility-and-recovery).

For atomic same-identity replay, use `upload_id`, `save_once` and `upload_status`
as described in [resumable upload attempts](file-operations.md). Existing `save`
remains a non-idempotent operation.
