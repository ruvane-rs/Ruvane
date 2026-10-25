# Browser runtime and hydration

Since alpha.15, the same Rust components provide server HTML and browser interaction.
The `ruvane-browser` workspace member depends only on the local core crate. Our
JavaScript bridge uses browser APIs, with no wasm-bindgen, web-sys or npm package.

## Run from a checkout

```sh
rustup target add wasm32-unknown-unknown
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked -p ruvane --example browser
```

The example serves loopback port 3001. Optional arguments select a trusted Wasm
file and port; port 0 asks the OS for a free port. Press Enter to stop. The file
is read once at startup with the existing asset limit, never from an HTTP path.
The complete checkout is required: the unpublished adapter imports the shared
example components and is not included in the standalone core package.

The server renders a counter at 5 with a double of 10 and a toggle initially
Enabled, plus a keyed item list and a task form. The list supports adding, removing,
counting, reversing and filtering items. The task form validates a name and notes
in Rust, creates tasks and tracks completion with checkboxes. Controls are disabled until the Wasm loads and hydration validates
the DOM. Hydration reuses elements and text nodes, preserves focus and connects
handlers once. After one increment the same output elements display 6 and 12.
The demo's lifecycle button can unmount and client-mount the counter again.
Counter arithmetic, derived state and the toggle event live in Rust.

`/ssr` returns the same content without a module script and with the blocking CSP.
`/tests` runs the browser harness, including 1,000 client mounts and 1,000
hydrations, node/text identity, focus, mismatches, claims, 1,000 additions/removals
and cleanup. Field tests cover UTF-8 quotas, dirty hydration, selection, composition
events and 1,000 task additions/removals. A test-only Wasm fixture emits an invalid structural patch to check
that an earlier valid command in the batch is not partially applied. Counts
measure live registry resources, not GC timing or shrinking Wasm memory.

## Shared components

See [shared views](views.md) for the API. The application supplies a component
factory to `BrowserRuntime::new`. The runtime stores `Box<dyn Component>` and
queues initial views or structural/text/button updates through retained snapshots. The demo's
factory and widgets are in `examples/support/widgets.rs`, compiled by both
server and Wasm. The registry and bridge contain no counter event rules.

Initial props identify a registered component and one typed number or boolean.
The server emits fixed attributes on the host div:

```html
<div data-ruvane-version="11" data-ruvane-component="1" data-ruvane-state="1:5">…</div>
```

State kind 1 is a u32; kind 2 is a boolean encoded as 0 or 1. Numeric fields use
canonical decimal spelling, with no sign, spaces or leading zeros. Factories
check the component ID, state kind and domain range. The demo registry uses
1=counter, 2=toggle, 3=item list and 4=task form. Component 5 is a field lifecycle test fixture; 6 is the remote task model and
7 is an action transport fixture.
State is public page data; no arbitrary JSON, source code,
function name or input pointer is accepted.

## Bridge API

```js
import { createRuntime } from "/assets/ruvane.js";
const runtime = await createRuntime("/assets/app.wasm");
const view = runtime.hydrate(serverRoot);
// Or use an empty container:
const other = runtime.mount(emptyRoot, {
  component: 2, state: { kind: "boolean", value: true }
});
view.dispatch(1);
other.dispatch(7);
view.dispose();
runtime.dispose();
```

`hydrate(root)` reads server props and validates the full expected tree before
mutating DOM or attaching handlers. Tags, namespace, IDs, parents, child order,
leaf text, sibling keys, event IDs and exact control attributes must match. Server buttons
must still be disabled. Field defaults must match the Rust model; live values and
checked state may differ before hydration and are adopted after validation.
Fields may already be enabled by host code when the model permits it.
Whitespace/comment nodes outside text leaves, extra
attributes and version/state mismatches are rejected. There is no automatic
HTML replacement or remount fallback.

Failed hydration preserves the supplied DOM, releases the temporary native
component and root claim, and leaves other views active. Corrected server HTML
can be retried. Claims are shared by runtimes using the same bridge module;
duplicate or nested claims are refused before allocating a component.

Client mounts require an empty HTML container from the current document. Valid
host tags are div, section, main, article and aside. Nodes belong to one view;
host code must not change them while the view is active. Updates verify the existing DOM before applying a complete structural plan.
Surviving keys keep nodes and handlers; removed subtrees lose all handlers.
Reordering preserves the active field's subtree and restores selection where needed.
Insertion uses native element creation and sibling placement, without HTML strings.

`dispatch(event, value?)` accepts only events declared by that view. Text fields take
strings, checkboxes take booleans, and clicks/submissions take no value. The bridge
validates values before encoding them; Rust checks their type, length and live binding
again. Invalid payloads are local errors and leave the component active.
See [client forms](client-forms.md) for validation, resets and composition behavior.
Unknown public events are
rejected locally. The counter uses 1/2/3 for increment/decrement/reset; the toggle
uses 7. List controls use 1/2/3; each item has its own actions. A controller's
`dispose()` removes its nodes and handlers and returns
false when already disposed. Stale view IDs never alias new instances.

On protocol or DOM failure, the bridge stops and removes its handlers/handles.
Client-created nodes are removed. Hydrated content is retained with controls
disabled; unrelated host siblings remain. Native disposal is attempted. After
an uninspectable trap, native counts are null/unknown and the instance reference
is released. Explicit runtime disposal removes both client and hydrated views.

`stats()` returns state, views, DOM handles, event listeners and native instance,
signal and effect counts, plus pendingActions. Controllers expose whenIdle()
for transport completion. `decodeCommands`, `planCommands`, `validateProps` and
`validateExports` are exported for integration tests. The pure planner validates
decoded commands against copied metadata; framing and byte caps are checked by
the decoder before the planner runs.

## Assets and CSP

Register `BROWSER_BRIDGE_MODULE.as_bytes()` as `MediaType::JavaScript` and the
compiled module as `MediaType::WebAssembly`. MIME types are
`text/javascript; charset=utf-8` and `application/wasm`. These are trusted
application assets; registration does not sanitize their contents.

`module_script(&InternalUrl)` emits an escaped external module reference.
`Response::interactive_html` opts one HTML response into this fixed profile:

```text
default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'
```

Inline scripts and general JavaScript eval remain blocked. Other constructors,
protocol errors, App routes and static export keep `CONTENT_SECURITY_POLICY`.
GET and HEAD use the same selected headers. Including JS/Wasm assets alone does
not make a page interactive. Static suffix/manifest support for .js, .mjs and
.wasm retains the blocking header contract.

## Browser ABI v11

Alpha.25 reuses the page ingress for transactional initial App hydration from
an inert RVI1 snapshot. No export or ABI change is required. See
[initial hydration](initial-hydration.md).

Alpha.24 adds RVP4 pending prefixes for ordered client navigation. Validation
checks their loaded layouts without a leaf seed; pending leaves cannot mount.
The same 22 numeric exports remain. See
[progressive navigation](progressive-navigation.md).

Alpha.22 adds read-only presentation mounts and RVP3 typed error scopes. See
[segment boundaries](segment-boundaries.md).

Alpha.21 adds RVP2 layout branches and segment selection through the existing
page ingress. See [client layouts](client-layouts.md).

Alpha.20 adds read-only page validation and explicit prefetch eligibility in RVN6 frames. Rebuild the Wasm and use its
matching bridge together. The module exports memory and these twenty-two functions,
with no host imports:

| Export | Contract |
| --- | --- |
| `ruvane_abi_version()` | Returns 11 |
| `ruvane_application_revision()` | Returns the application build revision as u64/BigInt |
| `ruvane_boundary_mount(root, component, kind, value)` | Empty-seed, event-free presentation owner, or zero on rejection |
| `ruvane_mount(root, component, kind, value)` | New nonzero view ID, or zero on error |
| `ruvane_dispatch(view, node, event, kind, value)` | Commit an event; 1 on success, zero on error |
| `ruvane_event_begin(view, node, event, length)` | Start one text payload of 0..=1024 bytes |
| `ruvane_event_chunk(word, count)` | Append 1..=4 little-endian bytes |
| `ruvane_action_begin(view, request, length)` | Stage at most 64 KiB for a live action |
| `ruvane_action_chunk(word, count)` | Append 1..=4 little-endian reply bytes |
| `ruvane_action_complete(view, request, kind)` | Commit RVA1 (0) or failure (1..=4); 1 handled, 2 handled malformed RVA1, 0 ABI error |
| `ruvane_page_begin(length)` | Stage one RVP1/RVP2/RVP3 packet of at most 64 KiB |
| `ruvane_page_chunk(word, count)` | Append 1..=4 little-endian page bytes |
| `ruvane_page_validate()` | Check the staged packet without IDs, commands or retained instances |
| `ruvane_page_mount(root)` | Validate route/seed and create a prepared page owner |
| `ruvane_dispose(view)` | 1 on success, zero for a stale/invalid view |
| `ruvane_commands_ptr()` | Borrowed output offset |
| `ruvane_commands_len()` | Output bytes including the frame header |
| `ruvane_commands_clear()` | Acknowledge the batch or abort staged input; 1 on success |
| `ruvane_last_error()` | Fixed numeric error; zero after a successful mutation |
| `ruvane_live_instances()` | Live application components |
| `ruvane_live_signals()` | Reported signals, including memo caches |
| `ruvane_live_effects()` | Reported effects, including memos |

Errors are 1=invalid/duplicate root, 2=limit, 3=unknown view, 4=unknown event,
5=busy/unacknowledged batch, 6=reactive failure, 7=invalid props, 8=invalid
view/patch, 9=invalid payload, 10=expired action request and 11=invalid page/route/seed. Mutations refuse a
pending batch or staged input. An action awaiting network I/O does not hold the ABI busy.
Root/view IDs are numeric;
view IDs increase with checked arithmetic and never wrap or get reused.

Payload kind 0 requires value 0 and carries no data. Kind 2 carries a boolean,
value 0 or 1. Kind 1 requires value 0 and commits the staged text for the exact
view/node/event supplied to `event_begin`. Text must have the exact declared
byte length and valid UTF-8. Empty text needs begin and commit with no chunks.
For short chunks, unused high bytes must be zero. Invalid chunks, malformed
commits and `commands_clear` discard staged input. A pending input prevents other
mutations until commit or clear; reads remain available. No input pointer crosses
the ABI. The bridge performs the complete sequence synchronously.

The ABI registry belongs to one Wasm instance. Only the ABI module permits
Rust 2024 `#[unsafe(no_mangle)]` export attributes. Export names are unique to
that module. There are no unsafe blocks, foreign imports or accepted input
pointers; the core keeps `#![forbid(unsafe_code)]`.
[Rust export attributes](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html).

Output offsets are valid until the next mutation. The bridge refreshes the
memory view, checks ranges and copies the bounded batch before acknowledging
it. Signed JavaScript i32 results are interpreted as u32 bit patterns; public
JavaScript inputs are validated before crossing the ABI.

## RVN6 command frames

Integers are little-endian. The header is `RVN6` plus a u32 count, with no trailing
bytes. Each command starts with a u8 opcode and a nonzero u32 view ID.

| Opcode | Remaining fields | Operation |
| --- | --- | --- |
| 1 | node u32, parent u32, tag u8, key u32 | Create/claim an element; key zero means unkeyed |
| 2 | node u32, byte length u32, UTF-8 bytes | Set or validate leaf text |
| 3 | node u32, nonzero event u32 | Declare a click, submit, input or change binding |
| 4 | node u32, boolean u8 | Set a control's disabled state |
| 5 | none | Remove a complete view |
| 6 | node u32 | Remove an existing subtree and its handlers |
| 7 | node u32, before u32 | Place before a sibling; before zero appends within the same parent |
| 8 | node u32, epoch u32, byte length u32, UTF-8 bytes | Set a text field's live value |
| 9 | node u32, epoch u32, boolean u8 | Set a checkbox's live checked state |
| 10 | node u32, boolean u8 | Set a field's aria-invalid state |
| 11 | request u32, action u32, URL length u32 + UTF-8, body length u32 + ASCII | Queue an internal POST action |

| 12 | node u32, target length u32, encoded ASCII target | Set an internal link href |

| 13 | node u32, boolean u8 | Set explicit link prefetch eligibility |

Tags are section=1, h2=2, p=3, span=4, output=5, button=6, ul=7, li=8,
form=9, label=10, text input=11, checkbox=12, textarea=13, submit button=14 and
internal link=15. New
handles must increase beyond every handle previously allocated in that view.
Keys are nonzero u32 values unique among siblings. They are separate from node
handles and unkeyed positions. A tag/event change creates a new node lifetime.

Initial batches can create nodes, set text/disabled/field state and bind events.
Patch batches can also remove and place nodes. Bindings are created only on new
event targets; removal retires a binding. Each text leaf has text. Buttons,
forms and fields have one binding, except submit buttons which use their form's
binding. Each field has disabled, invalid, epoch and typed value state. Epochs
cannot decrease during a field's lifetime. Labels wrap exactly one field and a
nonblank span label. Forms cannot nest; submit buttons require a form ancestor.
Disposal remains exactly one opcode-5 command.

The planner rejects cross-view references, unknown or reused handles, duplicate
keys, invalid parents, list nesting, sibling targets and incomplete nodes before
DOM changes. Removal is recursive; placing a node cannot reparent it or form a
cycle. Native patches remove obsolete subtrees first, create replacements, then
order siblings. Text remains inert. Action URLs must pass the internal path profile;
no arbitrary HTML or DOM attributes are encoded. At most one opcode-11 command
is allowed per batch and per pending owner. Action metadata is checked before
DOM changes, then extracted before the structural planner runs. Fetch starts
after that batch has been applied. See [actions](actions.md) for RVA1 and lifecycle rules.
DOM-application failures stop the runtime; validated batches do not promise a
transactional rollback of arbitrary application state or host side effects.

## Limits and verification

| Resource | Limit |
| --- | --- |
| Decoded Wasm response | 1 MiB |
| Fetch and response reading | 10 seconds, same origin, no redirects |
| Live components | 16 |
| Elements per view | 32, plus the host root |
| Element depth | 8 |
| Logical event bindings per view | 16 |
| DOM listeners per view | At most 48; text fields use 3 per binding |
| Command frame | 64 KiB including header, 256 commands |
| One text leaf, field value or event payload | 1024 UTF-8 bytes |
| Pending actions | 1 per view |
| Action URL / request body / reply | 1024 bytes / 16 KiB / 64 KiB |
| Reported signals/effects per component | 1024 each |

Factories and components are trusted Rust code. These limits do not bound all
CPU, allocations or reference cycles, and a fetch timeout cannot preempt Wasm.
The adapter calls typed [actions](actions.md) through the same native POST
origin/body policy. Ordinary [POST forms](post-forms.md) retain their native flow.
Sessions and cryptography remain outside this profile.
Nested islands and richer initial props remain future work.

Python 3.9+, Node 18+ built-ins and a real browser are test tools; no npm or Python
packages are installed. Build Wasm before running Node tests:

```sh
python3 tools/check_first_party.py
cargo test --workspace --offline --locked --all-targets
cargo test --workspace --offline --locked --doc
cargo clippy --workspace --offline --locked --all-targets -- -D warnings
node --test browser/tests/bridge.test.mjs
```

`RUVANE_WASM_FILE` selects an alternate compiled module. CI checks stable and
Rust 1.85.0, the Cargo graph and ABI attributes. The source guard is a regression
check, not a Rust parser or independent security audit. Browser checks run at
`/tests`; their report includes the browser version.

See [client navigation](navigation.md) for shared routes, RVP1 pages, persistent
layouts and the navigation controller.

Application projects disable the adapter demo feature and use `export_application!`
with their own factory and validator. `createRuntime` accepts an optional decimal
`applicationRevision` string and rejects mismatches before hydration. That runtime
carries its revision on automatic navigation and action requests; stale App
requests receive 409. See [interactive applications](interactive-apps.md).

See [navigation cache](navigation-cache.md) for policy, memory/network bounds,
validation, mutation epochs and uncertainty handling.
