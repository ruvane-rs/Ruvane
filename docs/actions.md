# Actions from Rust/Wasm

Alpha.17 connects browser components to explicit server actions. The browser
performs same-origin POST requests; Rust owns the model, validation results and
view updates. There are no third-party crates, npm packages, host imports or
input pointers. This profile uses browser ABI v13, RVN7 commands and RVA1 replies.
Rebuild the Wasm and serve its matching bridge together.

Alpha.50 adds `ActionFailure::RateLimited` for a confirmed quota-guard refusal.
The bridge exposes `"rate-limited"` from `whenIdle()` and the bounded delta from
`controller.retryAfter()`. It sends no automatic retry and keeps the mutation
outcome known. An ordinary handler's manual 429 still has conservative uncertainty.
See [native request quotas](rate-limits.md).

## Run the example

```sh
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked --example server_forms
```

Open `/interactive` at the printed address. Task creation, completion and removal
update the server without navigating. The component shows pending state, disables
its controls during a request and displays server validation in the form. It
renders three rows per page to stay within the 32-node view limit, while the
server holds up to 32 tasks. Reloading fetches the same server state. Data is
shared by that server and lasts until it stops; there is no per-user database.

The initial HTML contains ordinary native forms. They stay usable if Wasm fails
to load. Enhancement mounts a separate client root and hides the native fallback
after its first successful load. It adopts unfocused create-form drafts through
typed events. If a native control is already focused, the demo keeps native mode
and does not take over that input. The permanent link opens `/tasks`, whose CSP
continues to block scripts. This is progressive enhancement in the example,
not automatic hydration of arbitrary PostForm markup.

`server_forms [PORT] [WASM_FILE]` accepts a trusted module file at startup. The
default path is the release Wasm path above. Missing default output leaves native
forms available; an explicit missing or invalid file is an error. The file is
read once within the asset budget, not selected by an HTTP request.

## Declare the server endpoint

```rust
use ruvane::{ActionData, ActionReply, ActionValue, Router};

let mut router = Router::new();
router.post_action("/actions/greet", |request| {
    let fields = request.form_data().unwrap();
    let mut data = ActionData::default();
    if !fields.has_only(&["name"]) || fields.get("name").is_none_or(str::is_empty) {
        data.insert("message", ActionValue::Text("Enter a name.".into()))?;
        return Ok(ActionReply::validation(data));
    }
    data.insert("message", ActionValue::Text("Saved.".into()))?;
    Ok(ActionReply::success(data))
}).unwrap();
```

`post_action` uses the same POST namespace, Origin, Fetch Metadata, MIME and body
checks as `post_form`. Success is HTTP 200; application validation is 400. Both
carry `application/vnd.ruvane.action`. A handler error becomes a fixed 500. The
example's native and typed create handlers share their business validation and
task store. Paths identify registered handlers; numeric action IDs are local
component labels, not Rust function names or authorization tokens.

## Queue an action from a component

```rust
use ruvane::{ActionRequest, FormData, InternalUrl};

let fields = FormData::default().field("name", "Rust + HTML").unwrap();
let request = ActionRequest::new(
    1, InternalUrl::new("/actions/greet").unwrap(), fields,
).unwrap();
```

Store the request in the component and return it once from `take_action()`. Set
the model's pending state before returning its view. The runtime queues at most
one request per live component, assigns a checked monotonic request token and
emits it after the validated DOM batch. An action can be queued on mount, after
an event or after a completion. Producing a second action while one is pending
is an invalid component update. Application controls should also ignore duplicate
submit events while pending.

`action_result(action_id, ActionOutcome)` receives either `ActionReply` or
`ActionFailure`. Validate the expected field names, types and domain limits in
that callback before changing the model. The runtime then reconciles its new
view. Response text is data; it is never interpreted as HTML or DOM commands.
Default Component methods queue no action and reject unsolicited completions.
See `examples/support/remote_tasks.rs` for the complete model and schema checks.

The bridge uses native fetch with fixed POST, same-origin mode/credentials,
redirect refusal, no-store and URL-encoded fields. It does not follow the 303
responses used by native forms. Typed actions return RVA1 directly. No retry or
optimistic mutation is performed automatically.

## Completion and disposal

The bridge checks status, exact MIME, decoded response length and deadline before
passing bounded numeric chunks into Wasm. Rust decodes the full frame before the
callback runs. Invalid network data becomes a Protocol failure, leaving the
component alive. Application schema failures should likewise become ordinary
model errors rather than returning a fatal ComponentError.

Failure codes are Network=1, Timeout=2, Protocol=3 and Rejected=4. Rejected covers
unexpected HTTP status; the server may already have changed state before an
error occurred. A redirect refused by fetch reports a network failure. Neither
timeout nor abort rolls back a server mutation. Refresh or otherwise establish
server state before retrying an uncertain operation; no exactly-once guarantee
is made.

Each completion belongs to a live view and its exact request token. Repeated or
expired completions are rejected. Disposal aborts fetch/response reads, clears
timers, resolves the idle promise and drops the native owner. A later response
cannot update a remounted component. The ABI is busy only during synchronous
calls and byte staging; other views remain interactive while a request awaits I/O.

```js
const runtime = await createRuntime("/assets/app.wasm", { actionTimeoutMs: 5000 });
const view = runtime.mount(root, { component: 6, state: { kind: "number", value: 0 } });
const outcome = await view.whenIdle();
```

`whenIdle()` shares one promise while that view has pending work, including an
action queued by a completion. It resolves with success, validation, network,
timeout, protocol, rejected or disposed. These are transport outcomes; the Rust
model can still reject a valid wire frame's application schema. Views without
actions resolve immediately. `stats().pendingActions` counts live fetch operations.
`actionTimeoutMs` accepts 1..=10000 and defaults to 10000; it covers fetch and
response reading, not arbitrary Rust callback execution.

## RVA1 wire format

Integers are little-endian. The header is `RVA1`, a u8 kind (0 success, 1 validation)
and a u16 field count. Each field has a u8 name length, ASCII name bytes, a u8
type and its value. Names use the form identifier profile and must be unique.

| Type | Value |
| --- | --- |
| 1 | u32 UTF-8 byte length, then text bytes |
| 2 | u64 unsigned integer |
| 3 | u8 boolean, exactly 0 or 1 |

There are no trailing bytes, implicit type conversions, objects or executable
values. Text permits LF and tabs; other control characters fail. u64 values never
pass through JavaScript Number. ActionData insertion checks limits atomically;
the owned reply encoder only accepts already-validated data. The decoder repeats
all checks before exposing a result. MediaType::Action declares this runtime MIME
without validating bytes; these responses cannot be exported as static assets.

| Resource | Limit |
| --- | --- |
| Pending actions | 1 per view, at most 16 per runtime |
| Internal action URL | 1024 bytes, no query, fragment or external origin |
| Encoded request body | 16 KiB, within the server's separate body budget |
| Request fields | 32, with the existing FormData name/value limits |
| Decoded response frame | 64 KiB |
| Response fields / name | 160 / 64 ASCII bytes |
| One response text value | 1024 UTF-8 bytes |

The fixed Origin policy remains mandatory. This is not authentication, access
control, a Wasm sandbox, a public HTTPS server or a database layer. Handlers must
validate resource access and synchronize their own state. Component factories,
callbacks and same-origin module assets remain trusted application code.

## Verification

The native and Node suites cover malformed frames, exact integer/text transport,
caps, byte staging, stale tokens, checked exhaustion and 1,000 owner lifecycles.
The example's `/action-tests` harness exercises real fetch, local failure recovery,
timeouts, late responses, disposal, independent views, pagination and 100 server
create/remove cycles. Run it against a fresh example server; it mutates that
demo's in-memory tasks. Test fixtures are local example routes, not framework
endpoints. The existing browser example's `/tests` retains hydration/IME coverage.

Alpha.19 App runtimes attach the fixed `Ruvane-Application-Revision` header.
Typed actions on those routers require its current value; stale or missing
values return 409 before the action. Native forms retain their normal origin
policy. The revision is not authorization. See [interactive applications](interactive-apps.md).

Alpha.20 runtime action epochs invalidate navigation data at start and completion.
Uncertain outcomes pause storage and prefetch until document reload. Ordinary
foreground reads remain available. See [navigation cache](navigation-cache.md).

Alpha.49 adds opt-in [session CSRF protection](csrf.md). Configure a token endpoint
on `ClientRuntime` and protect mutation routes with `SessionStore::csrf_guard()`.
The bridge retrieves a fresh same-origin token before each action. Failed token
retrieval sends no POST and leaves no uncertain mutation; failures after sending
retain the existing conservative behavior.

JSON requests share the same live-owner pending slot. Their bounded RVJ1 results
are delivered through separate component hooks; see [component JSON](json-client.md).
