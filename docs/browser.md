# Browser runtime and hydration

Alpha.13 uses the same Rust components for server HTML and browser interaction.
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
Enabled. Its buttons are disabled until the Wasm loads and hydration validates
the DOM. Hydration reuses elements and text nodes, preserves focus and connects
handlers once. After one increment the same output elements display 6 and 12.
The demo's lifecycle button can unmount and client-mount the counter again.
Counter arithmetic, derived state and the toggle event live in Rust.

`/ssr` returns the same content without a module script and with the blocking CSP.
`/tests` runs the browser harness, including 1,000 client mounts and 1,000
hydrations, node/text identity, focus, mismatches, claims and cleanup. Counts
measure live registry resources, not GC timing or shrinking Wasm memory.

## Shared components

See [shared views](views.md) for the API. The application supplies a component
factory to `BrowserRuntime::new`. The runtime stores `Box<dyn Component>` and
queues a component's initial view or changed text/button states. The demo's
factory and widgets are in `examples/support/widgets.rs`, compiled by both
server and Wasm. The registry and bridge contain no counter event rules.

Initial props identify a registered component and one typed number or boolean.
The server emits fixed attributes on the host div:

```html
<div data-ruvane-version="2" data-ruvane-component="1" data-ruvane-state="1:5">…</div>
```

State kind 1 is a u32; kind 2 is a boolean encoded as 0 or 1. Numeric fields use
canonical decimal spelling, with no sign, spaces or leading zeros. Factories
check the component ID, state kind and domain range. The demo registry uses
1=counter and 2=toggle. State is public page data; no arbitrary JSON, source code,
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
leaf text, event IDs and the exact button attributes must match. Server buttons
must still be disabled. Whitespace/comment nodes outside text leaves, extra
attributes and version/state mismatches are rejected. There is no automatic
HTML replacement or remount fallback.

Failed hydration preserves the supplied DOM, releases the temporary native
component and root claim, and leaves other views active. Corrected server HTML
can be retried. Claims are shared by runtimes using the same bridge module;
duplicate or nested claims are refused before allocating a component.

Client mounts require an empty HTML container from the current document. Valid
host tags are div, section, main, article and aside. Nodes belong to one view;
host code must not change them while the view is active. Updates verify the
existing DOM before changing text or disabled states.

`dispatch` accepts only events declared by that view. Unknown public events are
rejected locally. The counter uses 1/2/3 for increment/decrement/reset; the toggle
uses 7. A controller's `dispose()` removes its nodes and handlers and returns
false when already disposed. Stale view IDs never alias new instances.

On protocol or DOM failure, the bridge stops and removes its handlers/handles.
Client-created nodes are removed. Hydrated content is retained with controls
disabled; unrelated host siblings remain. Native disposal is attempted. After
an uninspectable trap, native counts are null/unknown and the instance reference
is released. Explicit runtime disposal removes both client and hydrated views.

`stats()` returns state, views, DOM handles, event listeners and native instance,
signal and effect counts. `decodeCommands`, `validateProps` and `validateExports`
are exported for integration tests.

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

## Browser ABI v2

Alpha.13 changes the mount signature from alpha.12. Rebuild the Wasm and use its
matching bridge together. The module exports memory and these eleven functions,
with no host imports:

| Export | Contract |
| --- | --- |
| `ruvane_abi_version()` | Returns 2 |
| `ruvane_mount(root, component, kind, value)` | New nonzero view ID, or zero on error |
| `ruvane_dispatch(view, event)` | 1 on success, zero on error |
| `ruvane_dispose(view)` | 1 on success, zero for a stale/invalid view |
| `ruvane_commands_ptr()` | Borrowed output offset |
| `ruvane_commands_len()` | Output bytes including the frame header |
| `ruvane_commands_clear()` | Acknowledge the batch; 1 on success |
| `ruvane_last_error()` | Fixed numeric error; zero after a successful mutation |
| `ruvane_live_instances()` | Live application components |
| `ruvane_live_signals()` | Reported signals, including memo caches |
| `ruvane_live_effects()` | Reported effects, including memos |

Errors are 1=invalid/duplicate root, 2=limit, 3=unknown view, 4=unknown event,
5=busy/unacknowledged batch, 6=reactive failure, 7=invalid props and 8=invalid
view/shape change. Mutations refuse a pending batch. Root/view IDs are numeric;
view IDs increase with checked arithmetic and never wrap or get reused.

The ABI registry belongs to one Wasm instance. Only the ABI module permits
Rust 2024 `#[unsafe(no_mangle)]` export attributes. Export names are unique to
that module. There are no unsafe blocks, foreign imports or accepted input
pointers; the core keeps `#![forbid(unsafe_code)]`.
[Rust export attributes](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html).

Output offsets are valid until the next mutation. The bridge refreshes the
memory view, checks ranges and copies the bounded batch before acknowledging
it. Signed JavaScript i32 results are interpreted as u32 bit patterns; public
JavaScript inputs are validated before crossing the ABI.

## RVN1 command frames

The command encoding is unchanged from alpha.12. Integers are little-endian.
The header is `RVN1` plus a u32 count, with no trailing bytes. Each command starts
with a u8 opcode and a nonzero u32 view ID.

| Opcode | Remaining fields | Operation |
| --- | --- | --- |
| 1 | node u32, parent u32, tag u8 | Create/claim an element; parent zero is the host root |
| 2 | node u32, byte length u32, UTF-8 bytes | Set or validate leaf text |
| 3 | node u32, nonzero event u32 | Declare one button click binding |
| 4 | node u32, boolean u8 | Set the button's disabled state |
| 5 | none | Remove a complete view |

Tags are section=1, h2=2, p=3, span=4, output=5 and button=6. A shared view assigns
sequential node IDs. Each leaf has text; each button has one binding and disabled
state. A complete initial plan is validated before mounting or hydration.
Dispatch batches can only update text/disabled state; disposal is one remove
command. Text stays inert; no arbitrary HTML, attribute, URL or selector is encoded.

## Limits and verification

| Resource | Limit |
| --- | --- |
| Decoded Wasm response | 1 MiB |
| Fetch and response reading | 10 seconds, same origin, no redirects |
| Live components | 16 |
| Elements per view | 32, plus the host root |
| Element depth | 8 |
| Click bindings per view | 16 |
| Command frame | 64 KiB including header, 128 commands |
| One text leaf | 1024 UTF-8 bytes |
| Reported signals/effects per component | 1024 each |

Factories and components are trusted Rust code. These limits do not bound all
CPU, allocations or reference cycles, and a fetch timeout cannot preempt Wasm.
There are no mutation HTTP endpoints, server actions, sessions or cryptography.
Nested islands, dynamic lists/subtrees and richer props remain future work.

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
