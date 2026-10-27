# Ruvane Desk · alpha.62

A local ticket application built with public Ruvane APIs. All source, UI and docs
are in English. Cargo uses only this application's packages and the two local
framework packages. There are no third-party Cargo or npm dependencies.

Desk has isolated workspaces, persistent memberships, ticket lists/editors,
conditional updates and ticket attachments. Its pages render on the server and
hydrate the same controls in Rust/Wasm. Native HTML forms remain available.

## Run

From the repository root:

```sh
cargo build --offline --locked --bin ruvane
cd apps/desk
../../target/debug/ruvane dev --offline
```

Open the printed loopback address and select a **local test profile**. Anyone
using the local pilot can select any profile. This is an authorization test
application, not verified account authentication. There are no passwords, TLS or
public deployment. Sessions expire on server restart; business data persists.

Initial memberships, used only when initializing or importing alpha.61 data:

| Test profile | Ruvane workspace | Sandbox workspace |
| --- | --- | --- |
| Alex | Owner | Owner |
| Blair | Editor | No access |
| Casey | Viewer | No access |
| Drew | No access | Viewer |

| Role in one workspace | Allowed operations |
| --- | --- |
| Owner | Read/edit tickets and files; manage memberships |
| Editor | Read/edit tickets; add and remove files |
| Viewer | Read tickets; list and download files |

An owner of at least one visible workspace may create a workspace, becoming its
first owner. Memberships are persisted and editable for the four predefined
profiles. Keep at least one owner in every workspace. Close the current test
session explicitly before selecting another profile. The session stores a profile
key, not a cached role; every business request checks current persisted rights.

For a standalone bundle, run in this application directory:

```sh
../../target/debug/ruvane build --offline
./dist/build-000001/ruvane-app --port 3000
```

Use the actual build directory printed by the CLI. The executable embeds its
matching Wasm, browser modules and CSS and runs without application sources.
Build/asset-export commands never open business storage. ABI 15/RVN9, 22 numeric
exports and four core runtime assets are retained.

Data defaults to `desk-data/` under the server's working directory. Set
`RUVANE_DESK_DATA` to an **absolute path** with an existing trusted parent to keep
one location across development runs and bundles. Supported entropy/filesystem
profiles are macOS and Linux x86-64/aarch64. Paths must resolve through real
directories without symlinks. Directories use mode 0700; files use mode 0600.
The local OS account and directory parent must be trusted.

## Pages and limits

- `/workspaces`: visible spaces, current test profile and workspace creation.
- `/w/ID/tickets`: a paginated list for that workspace.
- `/w/ID/new-ticket`: ticket creation for editors and owners.
- `/w/ID/tickets/TICKET`: details; edit controls depend on the current role.
- `/w/ID/members`: owner-only membership editing.
- `/w/ID/tickets/TICKET/files`: attachment listing and upload controls.

Add `?native=yes` for a script-blocked document. Native forms carry CSRF claims
and retained versions where required. Successful writes use POST/303. Native
attachment preparation is an explicit link; its form retains the upload identity.
A file control cannot restore a local selection after native submission: reselect
the same file only when explicitly resuming the same attempt.

The legacy GET `/tickets` redirects authorized members to Ruvane's list. Old
mutation/API routes are retired; existing alpha.61 forms cannot perform writes.

| Resource | Bound |
| --- | --- |
| Workspaces | 4 |
| Local test profiles | 4 |
| Tickets across all workspaces | 128 |
| Tickets per list page | 3 |
| Workspace name | 1–48 UTF-8 bytes, nonblank, no controls |
| Ticket title | 1–120 UTF-8 bytes, nonblank, no controls |
| Description | 0–1024 UTF-8 bytes; LF and tabs allowed |
| Attachments per ticket | 3 / 96 KiB |
| One file | 32 KiB |
| Files across the application | 64 / 2 MiB |
| Ticket/membership snapshot | 160 KiB |

Ticket IDs are local to a workspace; `(workspace, ticket)` is the identity.
Names must differ from names already visible to the creator. Hidden workspace
names do not act as a global uniqueness oracle; separate spaces may share a name.
Wire ticket states are `open`, `in_progress` and `closed`. The state button changes
only the draft; saving persists all three ticket fields.

## Authorization and editing

Private HTML, navigation seeds, metadata and downloads are no-store. Mutations
check same origin, session, CSRF and quota guards. Workspace membership and ticket
association are checked inside the business store for every operation. A guessed
workspace/ticket/file ID grants no access. Missing and foreign resources use 404;
a member without write/administration rights receives 403 on those API mutations.
Protected page loaders conceal inaccessible management/creation pages with 404.

Ticket and membership updates use strong retained `If-Match` validators. Compare,
revision increment and publication share one mutex. Missing/wildcard conditions
receive 428; stale writers receive 412. A conflict keeps the Wasm draft and blocks
saving until explicit replacement. Native error pages retain valid submitted
fields. Invalid control characters or over-limit native fields fail before draft
rendering. Restart creates new tag namespaces per workspace, so old tabs cannot
write using a validator from a previous lifecycle.

File operations acquire the business mutex before the FileStore mutex and keep
the permission decision valid through the file operation. A revocation waits for
an already authorized operation to finish; after it commits, an old session cannot
prepare, replay or remove a file using old rights. Downloads already authorized
before revocation may finish. Rights do not come from hidden fields or headers.

The UI hides unavailable actions, and the server independently enforces them.
Role changes take effect on the next server request. Re-enter a page to refresh
its displayed capabilities. Permission/session loss clears private editor state,
metadata and selected files. Navigating away discards drafts and releases files;
there is no durable browser draft storage.

Ticket and workspace **creation are not idempotent**. Lost replies may hide a
successful creation. Wasm blocks another creation, retains the draft and requires
inspection plus explicit discard. It never automatically replays the POST.

## Attachments and uncertain uploads

The backend derives a stable FileOwner from the authorized workspace/ticket pair.
Neither owner keys, storage paths nor file bytes enter a page seed. Files are
opaque download-only attachments with fixed MIME, Content-Disposition, nosniff
and script-blocking CSP. No preview or inline execution is provided.

An explicit upload first reserves a FileUploadId, then sends the selected file
with that identity. FileStore::save_once publishes the payload and result in one
RFS2 snapshot. Within the one-hour retained lifetime, exact repeats return the
same file; changed input, foreign/unknown attempts and deleted results cannot
create another file. Current authorization is checked before every replay.

A lost reply keeps the identity and selection. **Check upload** confirms a committed
receipt without sending bytes again. **Resume upload** explicitly reuses the same
identity and file. Expired/removed attempts require explicit discard. Listing
alone cannot prove whether one particular attempt committed. Navigating away
releases the browser File; uncommitted reservations do not survive restart.

There is no second attachment-link write to the ticket catalog: FileStore's owner
key is the association, and tickets/workspaces are not deleted in this alpha.
Attachment changes do not increment a ticket's text revision. The upload guarantee
covers file/receipt publication only, not ticket creation or unrelated mutations.

## Migration, backup and recovery

The business snapshot is `RDS2`, containing workspaces, memberships and tickets.
An alpha.61 `RDS1` catalog is read into workspace 1 (`Ruvane`) with all ticket IDs,
fields and revisions preserved. Bootstrap memberships and `Sandbox` are supplied
in memory. A read, rejected write or no-op does not rewrite that business file.
The first successful business change publishes RDS2 atomically.

Opening alpha.62 also initializes the separate `attachments/` FileStore. An older
binary does not support this complete directory layout. Stop the old server and
keep an offline backup of the **entire data directory before first upgrade**.
There is no downgrade or automatic reset.

Each catalog independently writes/syncs `next.bin`, renames it over `store.bin`,
then syncs its directory. CRC32 detects accidental corruption; it does not encrypt
or authenticate local data. There is no transaction spanning the two catalogs.
Back up and restore the entire directory with every writer stopped; never mix
catalogs from different backups or times.

Normal shutdown removes both writer locks. A crash or failed publication preserves
recovery artifacts and blocks normal startup. Stop **all** writers, then run:

```sh
./dist/build-000001/ruvane-app --recover /absolute/path/to/desk-data
```

This offline command validates and recovers both stores and starts no server.
It does not overwrite corrupt committed data. Recovery may retain the old or new
committed state after uncertainty; inspect before writing again. Recovery is not
atomic across catalogs and does not infer whether another process is still using
the directory. Ensuring all writers are stopped is the operator's responsibility.
Partial initialization, unsafe permissions and foreign files require inspection.

## Source and checks

- `src/model.rs`: shared ticket, workspace and role validation.
- `src/components.rs`: ticket list/editor and conflict handling.
- `src/spaces.rs`: workspace selection/creation and membership editor.
- `src/attachments.rs`: upload lifecycle and role-aware file controls.
- `src/native.rs`: profiles, guards, pages and business endpoints.
- `src/native/files.rs`: ticket-bound FileStore integration.
- `src/native/store/`: RDS1/RDS2 persistence, authorization and failure tests.
- `src/pages.rs`, `src/main.rs`, `browser/`: shared registry, CLI and Wasm entry.

This independent Cargo workspace has four local packages in its dependency graph.
From the repository root, `python3 tools/check_first_party.py` audits both graphs;
`tools/check_desk.sh` runs native tests, formatting, Clippy, Rustdoc and the
source-hidden bundle/HTTP/Wasm suite. The test-only browser harness is built by
`browser/tests/desk-fixture.mjs`; it is not part of the shipped application.

Alpha.62 establishes the bounded workspace/role/attachment slice. Verified accounts,
TLS, richer daily workflows, deletion and production deployment remain later work.
