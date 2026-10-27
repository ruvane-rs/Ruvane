# Component JSON client

Alpha.55 adds JSON requests owned by Rust/Wasm components, without Cargo or npm
dependencies. The first-party bridge performs browser I/O. Rust builds requests,
parses responses, converts application data and decides what to render next.

```rust
use ruvane::{EntityTag, InternalUrl, JsonMethod, JsonRequest, JsonValue};
let body = JsonValue::object(vec![
    ("title".into(), JsonValue::string("Updated task")?),
])?;
let retained_version = EntityTag::strong("snapshot-version")?;
let request = JsonRequest::new(
    3, JsonMethod::Patch, InternalUrl::new("/api/tasks/1")?, Some(body),
)?.with_if_match(retained_version)?;
assert_eq!(request.method(), JsonMethod::Patch);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Keep a queued `Option<JsonRequest>` in the component and take it in
`Component::take_json`. Set the component's pending state when queuing the request.
`Component::json_result(action, JsonOutcome)` receives its completion after the
runtime checks the live owner and monotonically increasing transport token.
Update pending state and the model there. A completion may queue a subsequent
read; it must never silently retry an uncertain mutation.

Form actions, JSON and [uploads](browser-uploads.md) share one pending slot per component. Queuing two, or
queuing another while one is active, rejects that component. Synchronous dispatch
of unrelated state remains available. Seeded App pages and loading/error
presentations cannot queue initial requests, so server rendering and client
preparation remain free of initial network effects. Use an explicit event.

## Requests

`JsonMethod` supports GET, POST, PUT, PATCH and DELETE. GET and DELETE require
`None` for the body. Other methods require `Some(JsonValue)`. Requests have a
nonzero application action ID, an `InternalUrl` of at most 1024 encoded bytes,
and at most 16 KiB of encoded JSON. Use `InternalUrl::query` to add query fields.
No custom headers, external destinations, redirects or automatic retries are
exposed. Request Debug omits the URL, body and validator.

All requests use same-origin mode and credentials, redirect errors and no-store.
They send the configured application revision. Mutations use the existing
header-only CSRF preflight when `csrfEndpoint` is configured; reads skip it.
Server session, CSRF, origin and quota guards still enforce access independently.
The preflight never appears in the component's JSON model.

`with_if_match` accepts one strong `EntityTag`. Store it with the loaded resource
snapshot and attach that same tag to the explicit edit or deletion. Never replace
it with a fresh version immediately before submitting an older draft. Conflicts
require an explicit decision to reload or reconcile. PUT responses omit validators
under Ruvane's native HTTP contract; a successful PUT can be followed by a GET.
No request obtains or substitutes an ETag automatically.

## Results and certainty

`JsonOutcome` is `Result<JsonReply, JsonFailure>`:

- `JsonReply::status` preserves the HTTP status. Success, validation 400,
  precondition failure 412, required precondition 428 and quota 429 remain distinct.
- `value` returns the bounded Rust `JsonValue`. Non-JSON HTTP error bodies are
  discarded; `has_json()` is false and `value()` is null in that case. JSON number text never passes
  through JavaScript number conversion. Convert it explicitly with checked methods.
- `etag` returns only this response's validator. `retry_after` returns a bounded
  delta on a no-store 429; it schedules nothing.
- `validation(&["title", "completed"])` decodes a 400 field/code report using
  trusted static field declarations. Unknown fields/codes, extra members,
  duplicates and oversized reports fail closed. Unknown input names are never
  turned into application declarations.
- `JsonFailure::kind` reports Network, Timeout, Protocol, Rejected or RateLimited.
  Both replies and failures expose `uncertain()` for submitted mutations.

A successful response and ordinary HTTP rejection statuses
400/401/403/404/405/409/412/413/415/422/428 are treated according to their rejection
semantics. A 429 is a confirmed pre-handler refusal only with Ruvane's fixed
rate-limit marker and valid Retry-After. A handler's unmarked 429, a server error,
a network failure after submission, a malformed reply or an elapsed deadline may
leave a mutation applied. Application handlers must honor their response semantics;
an HTTP rejection cannot undo writes a handler already made.

Cancellation aborts browser transport and ignores late results. It cannot roll
back a server mutation. Disposing an owner settles its `whenIdle()` as `disposed`;
no callback can reach a replacement owner. The JSON deadline settles even when a
transport ignores the abort signal. There is one timer per active request and
response readers cancel on abort.

Mutations invalidate navigation caches at start and completion. Uncertain results
also disable cache restoration/prefetch for that runtime's remaining lifetime;
a full document reload can establish a new runtime. GET requests do not change
the mutation epoch. `runtime.actionState()` retains this conservative uncertainty,
while the application may reconcile its own editor through explicit reads.

## Budgets and wire format

The same `actionTimeoutMs` setting covers CSRF preflight, request and response
body, from 1 to 10000 ms (default 10000). Synchronous Wasm execution is outside
that timer. Successful responses must have application/json, no-store and a complete
nonempty JSON body. No-store HTTP errors with another MIME type preserve their
status and quota metadata, but their bodies and validators are discarded without
reading. JSON error bodies still require the normal parser and size budgets. Empty 204 responses and redirect responses are outside this
profile. Declared and streamed body sizes are checked against 64 KiB, followed
by Rust's JSON depth, value, string, member and number limits. ETags have the
existing 128-byte profile. Transport metadata adds at most 142 bytes.

Browser ABI v15 retains the same 22 numeric exports and four core runtime assets.
Rebuild modules and use the matching bridge together; v13 modules and hydration
roots are rejected. RVN9 retains opcode 14 after view/request/action u32 identifiers:
method u8 (GET=0, POST=1, PUT=2, PATCH=3, DELETE=4), then URL, body and If-Match,
each as u32 byte length plus UTF-8 bytes. An empty validator means absent.

The existing bounded action staging exports carry RVJ1 for JSON owners: magic,
kind u8, flags u8, HTTP status u16, Retry-After u32, tag length u16, tag bytes and
remaining JSON bytes. Integers are little-endian. Kind 0 is HTTP; kinds 1..5 are
transport failures, with no body/tag/status. Failure flag 1 records submission;
HTTP flag 2 records a confirmed quota refusal; flag 4 records a discarded
non-JSON error body and forbids JSON/tag bytes. Reserved bits and invalid metadata
combinations are rejected. Form owners continue to decode RVA1 only.

## Task example and verification

```sh
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked --example json_api -- 8080
```

The second optional argument selects the matching demo Wasm file. Enter stops the
local server cleanly. The example stores shared demo tasks and sessions in memory;
it has no real account authentication. Its JavaScript bootstrap only mounts the
Rust component and disposes it on page exit. The model, validation, explicit JSON
calls, ETag retention, errors and two-item pagination live in
`examples/support/json_component.rs`. Invalid input keeps the draft. A conflict
clears the writable version until the user explicitly reloads the snapshot.

Tests cover exact numeric HTTP/Wasm values, all five methods, preflight rejection,
confirmed versus uncertain quotas, malformed metadata/JSON/UTF-8, streamed budgets,
network failures, deadlines, late replies, disposed/replaced owners and a full
create/edit/conflict/reload/delete editor flow. The shared task view also walks
all 32 stored tasks without exceeding the existing node/event budgets.
