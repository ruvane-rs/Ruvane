# Resumable private upload attempts

Alpha.60 adds owner-bound upload identities and durable operation results to
FileStore. A lost acknowledgement can be checked or explicitly resubmitted with
the same identity, without creating another file. No dependency, host import,
Wasm export or automatic retry is added. The current ABI remains v15/RVN9.

## Identity and authorization

FileUploadId is exactly 80 lowercase hexadecimal digits: sixteen for its Unix
issue second and sixty-four for 32 OS-random bytes. Debug output redacts it.
An identity expires one hour after issuance. It is not an authorization token;
every preparation, inspection and submission derives the current owner from
server session state and applies the existing guards.

FileStore::upload_id(&owner) creates a bounded owner-bound reservation in memory.
It changes no files or durable operation results. Unknown, foreign, future and
expired identities cannot start an upload. Reservations disappear on restart;
a never-committed attempt then becomes unavailable and needs an explicit new
identity. A committed result survives restart and requires reauthorization as
the same stable owner. There is no client-supplied owner field.

The demo's private GET /private-files/upload-operation issues a reservation and
returns FileUploadStatus with state unconfirmed. This transient nonce allocation
has no file effect, is no-store and is subject to session quotas. It is never a
public prefetch target. Discarding an attempt in the browser releases its File
reference; the server reservation remains bounded until expiry.

## Atomic file publication

Use FileStore::save_once(&owner, operation, &upload). The complete snapshot contains
the file and its operation result in one atomic publication. The writer lock
serializes simultaneous attempts. A live exact repeat returns the original
FileInfo without another write, entropy read or file-quota charge. Comparison
uses the decoded filename and exact bytes, ignoring multipart boundary and MIME
hints. It uses no checksum as an input fingerprint or authentication mechanism.

Reusing an identity with different input returns OperationConflict. Deleting a
file keeps a small retired result until expiry; the old attempt cannot recreate
it. The retained result contains no deleted payload or filename. Unknown/foreign
attempts have the same unavailable state and never expose another owner's result.
All session, CSRF, origin, revision and request-quota checks precede replay.

FileStore::save remains an explicit non-idempotent API for existing applications.
The demo accepts legacy multipart requests without an operation field through
that API. New native forms and the Rust/Wasm manager always include an operation.
The guarantee applies only to save_once and is bounded by the identity lifetime;
it does not cover unrelated application effects or provide a generic transaction
middleware for arbitrary handlers.

## Bounded lifetime

| Resource | Limit |
| --- | --- |
| Identity lifetime | 3600 seconds from issuance |
| Active reservations plus unexpired results | 256 per store |
| Active reservations plus unexpired results per owner | 32 |
| File / HTTP body | Existing 32 KiB / 64 KiB |
| Encoded retained operation record | At most 137 bytes |

No live result is evicted to admit another attempt. Capacity exhaustion refuses
new reservations. Existing committed results can still replay when capacity or
file quotas are full. Expired results are pruned during a successful file write;
expired reservations are pruned during preparation or publication. A snapshot may
still hold up to 256 expired records (32 per owner) until the next write. Including
those inactive records and reservations, bookkeeping stays within 512 entries
globally and 64 per owner; payload quotas are unchanged. Unavailable
or expired identities cannot silently become new attempts.

The catalog retains a Unix-second clock floor at publication. Writes, identity
issuance and status checks refuse a clock earlier than that durable floor. This
prevents a pruned identity from becoming executable again after a clock rollback.
Read-only file listing/downloads remain available. The clock is not a credential
and is never trusted from a client without checking its server reservation or
committed result. There is no background expiry worker or hidden disk write.

## Status and UI

GET /private-files/operations/ID returns exactly operation/state/file:

| State | file | Meaning |
| --- | --- | --- |
| unconfirmed | null | A live reservation has no committed result; a request may still be running |
| stored | FileInfo | File and operation committed together |
| removed | null | The committed file was subsequently deleted |
| expired | null | Expired, future, unknown, foreign or lost uncommitted reservation |

FileUploadStatus::from_json checks every member, state and metadata type before
returning anything. IDs and statuses are shared by native Rust and Wasm; owners,
reservations, disk paths, stored bytes and publication remain native-only.

The manager obtains an identity after an explicit Store file submission, then
queues the upload through the existing transport. A refused or uncertain attempt
retains that identity and its selected File. The picker stays locked to that file
until success or explicit discard. Resume upload uses the same identity, after
fresh CSRF preflight. Check upload performs only a GET: stored confirms and clears
the selection; unconfirmed keeps the explicit resume choice. Listing files alone
does not prove this attempt committed. Removed/expired attempts disable resumption
until explicit discard. Discard may leave a previously committed file on the
server, so inspect before starting another upload. No POST is repeated by a timer,
status check, navigation or restart.

Navigation/disposal releases the File and ignores late completions, including a
late preparation result that could otherwise queue a POST. Authorization loss
clears private metadata, selection and the attempt. A new page does not recover
browser File objects. To retry after a server restart, retain the live component
and reauthorize in the native demo before resuming, or retain the native form's
operation identity and reselect the exact file. An already committed receipt
can also be inspected without sending the file again.

Native multipart forms carry a hidden operation field and keep their POST/303
flow. Success redirects to /private-files?operation=ID, showing confirmation and
an explicit Start another upload link. An unconfirmed identity at that URL offers
a resume form. The identity grants no access by itself; the current session owner
is checked for both the form and its submission.

## RFS1 compatibility and recovery

Alpha.60 reads existing RFS1 catalogs without rewriting them on open, issuance,
status inspection or an unsuccessful mutation. The first successful file write
publishes RFS2 atomically, preserving existing IDs, owners, names and payloads.
RFS2 adds the clock floor and bounded operation records under the existing CRC32.
The directory identity marker remains unchanged. Older binaries cannot read RFS2;
keep an offline backup before upgrading an existing store. No downgrade or
silent reset is performed.

Interrupted publication retains the existing offline-only recovery contract.
After recovery, either the old snapshot has neither new file nor receipt, or the
new snapshot has both. A sync failure after rename remains uncertain until
recovery/inspection. If only an uncommitted reservation was lost, its old identity
is refused; recovery never invents a result. Corrupt CRCs, duplicate operation or
file references, foreign owners, invalid timestamps and excess limits fail closed.

## Checks

Core tests cover concurrent replay, exact input conflicts, deletion, owner binding,
quota retention, expiration, clock rollback, scripted entropy collisions, RFS1
migration and failure before/after publication. Component tests cover preparation,
explicit resume/check, invalid status, expiry and authorization loss.
browser/tests/file-operations.test.mjs exercises a source-hidden native/Wasm
bundle across concurrent requests, guarded replay, native forms and restarts.
The file-manager browser harness also loses real acknowledgements after server
commit, checks results without another POST and explicitly replays the same
operation without increasing the file count.
