# Browser runtime

Alpha.12 connects Ruvane's reactive engine to a real browser using only our code,
Rust's standard toolchain and web APIs. The `ruvane-browser` crate is a first-party
local workspace member, not a third-party dependency. The core remains a separate
safe library. This release mounts a client-side counter into an empty root;
shared-view server hydration is not implemented yet.

## Run from a checkout

```sh
rustup target add wasm32-unknown-unknown
cargo build --offline --locked -p ruvane-browser --target wasm32-unknown-unknown --release
cargo run --offline --locked -p ruvane --example browser
```

The example uses port 3001 by default. Optional positional arguments select a
trusted Wasm file and port; port 0 asks the OS for an available port. Press Enter
to request shutdown. The server remains loopback-only with the existing HTTP
limits. The module file is opened only at startup under the asset-byte cap, never
from an HTTP path. This example requires the complete source checkout; the
unpublished browser adapter is not part of the standalone core package.

The main page demonstrates increment/decrement/reset, a derived doubled value,
button bounds, native keyboard behavior and component disposal/remount. Rust owns
counter state, event dispatch and derived updates. JavaScript owns host DOM
operations and lifecycle controls, without implementing the counter arithmetic.
Each view has an independent ReactiveRuntime and Scope.

The example's `/tests` page is a local test harness, with known test-only assets.
Its report covers actual Wasm exports and DOM lifecycle, 1,000 mount/dispose
cycles, stale handles, per-view isolation, capacity, invalid versions/MIME and
DOM-ownership failure cleanup. Tests report engine resources, not guaranteed GC
timing or a shrinking Wasm memory allocation.

## Integrating the bridge

Register `BROWSER_BRIDGE_MODULE.as_bytes()` as a trusted `MediaType::JavaScript`
asset and the compiled module as `MediaType::WebAssembly`. JavaScript's type is
`text/javascript; charset=utf-8`, Wasm's is `application/wasm`. A declaration does
not validate program contents. Never register untrusted uploaded code as these
application assets.

`module_script(&InternalUrl)` emits an escaped external module reference, with no
inline body. `Response::interactive_html` explicitly selects the fixed profile:

```text
default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'
```

No general JavaScript eval or inline script allowance is added. All other
constructors and protocol errors retain `CONTENT_SECURITY_POLICY`. GET and HEAD
have the same selected headers. The App/static export interfaces still use the
blocking profile; merely including JS/Wasm assets does not activate scripts.
Static asset suffix validation and existing-export preview recognize `.js`,
`.mjs` and `.wasm`, but the static hosting header contract remains blocking.

The ES module exposes `createRuntime(url)`, `decodeCommands(bytes)` and
`validateExports(exports)`. A runtime provides `mount(emptyElement)`, `stats()`
and `dispose()`. A mounted controller provides `id`, `dispatch(event)` and
idempotent `dispose()`. Counter events are 1=increment, 2=decrement, 3=reset.
Values stay in 0..=1000. Mounting requires an empty, unclaimed HTML container
from the current document. Host code must not move or mutate bridge-owned nodes.

There are no network mutation endpoints, server actions, sessions or additional
server authority. The browser is never an authorization boundary. Keep private
data out of downloaded modules and their memory.

## Version one exports

The module has no host imports and exports memory plus these names:

| Export | Contract |
| --- | --- |
| `ruvane_abi_version()` | Returns 1 before any DOM allocation |
| `ruvane_mount(root)` | Returns a new nonzero view handle, or zero on error |
| `ruvane_dispatch(view, event)` | Returns 1 on success, zero on error |
| `ruvane_dispose(view)` | Returns 1 on success, zero for an invalid/expired view |
| `ruvane_commands_ptr()` | Borrowed output byte offset, never an accepted input pointer |
| `ruvane_commands_len()` | Output bytes including the frame header |
| `ruvane_commands_clear()` | Acknowledges the batch, returning 1 on success |
| `ruvane_last_error()` | Fixed numeric category, zero after successful mutation |
| `ruvane_live_instances()` | Live widget instances |
| `ruvane_live_signals()` | Live signal and memo cache count |
| `ruvane_live_effects()` | Live effect and memo computation count |

Errors are 1=invalid/duplicate root, 2=limit, 3=unknown view, 4=unknown event,
5=busy/unacknowledged batch and 6=reactive failure. Mutating operations refuse a
pending batch. IDs are checked monotonically increasing u32 values and never
reuse a disposed view. A failed construction releases its scope and commands.
An unexpected reactive failure disposes the affected counter.

The narrow ABI module uses `#[unsafe(no_mangle)]` because Rust 2024 requires
explicit safety acknowledgement for link-name exports. Names use one reviewed
namespace; there are no arbitrary symbols, unsafe blocks, pointer dereferences
or foreign imports. `#![forbid(unsafe_code)]` remains in the core.
[Rust export attribute requirements](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html).

The ABI registry is instance-local Wasm host storage, not a process-wide server
reactive graph. Reentry returns a fixed failure. Pending output offsets remain
valid only until the next mutating call. The bridge refreshes the memory view,
checks its range and copies the bounded bytes before acknowledging them. Signed
JavaScript i32 results are interpreted as u32 bit patterns, without masking
invalid JavaScript inputs into seemingly valid identifiers.

## Command frame

All integers are little-endian. The header is four bytes `RVN1` and a u32 command
count. No trailing data is accepted. Every command starts with a u8 opcode and
nonzero u32 view ID.

| Opcode | Remaining fields | Operation |
| --- | --- | --- |
| 1 | node u32, parent u32, tag u8 | Create a whitelisted element; parent zero is the host root |
| 2 | node u32, byte length u32, UTF-8 bytes | Set leaf text as inert textContent |
| 3 | node u32, nonzero event token u32 | Attach one native button click |
| 4 | node u32, boolean u8 | Set a native button's disabled state |
| 5 | none | Dispose one complete view |

Tags are section=1, h2=2, p=3, span=4, output=5 and button=6. Buttons always use
type=button. Parents must already exist; parent types and nesting are restricted.
There is no arbitrary HTML, attribute, URL, selector or JavaScript expression.

Before modifying DOM, the entire frame is decoded, version/size/UTF-8 checked
and prevalidated against the view graph. Nodes must be new on create, known on
update and owned by the expected view. Text targets are leaves, events belong
to buttons and each button has at most one click subscription. Mount batches
may create nodes, dispatch batches only update, and disposal is exactly one
remove command. Ownership checks verify actual DOM parents before application.

On protocol or DOM-application error, the bridge clears its handlers and handles,
removes the top-level nodes it created, releases its host-root claims and attempts
native disposal. Unrelated host-owned siblings are preserved. Native event errors
also emit a generic console diagnostic. A trap may prevent further Rust cleanup;
in that case native statistics are null/unknown, and module references are
released. Do not claim native counts are zero after an uninspectable trap.

## Budgets and validation

| Resource | Limit |
| --- | --- |
| Decoded Wasm response | 1 MiB |
| Fetch and response reading | 10 seconds, no redirects, same origin only |
| Live views | 16 |
| Registered element handles per view | 32, plus the supplied root |
| DOM creation depth | 8 |
| Click subscriptions per view | 16 |
| Command batch | 64 KiB including header, 128 commands |
| One text update | 1024 UTF-8 bytes |

These limits bound the bridge protocol and current component registry. Wasm
application code remains trusted; it can still consume CPU or allocate memory.
A fetch timeout does not preempt WebAssembly execution. No Wasm optimizer,
browser library or external package is downloaded implicitly.

Development validation uses Python 3.9+, Node 18+ built-in tools and a real
browser. No Python or npm package installation is required:

```sh
python3 tools/check_first_party.py
cargo test --workspace --offline --locked --all-targets
cargo test --workspace --offline --locked --doc
cargo clippy --workspace --offline --locked --all-targets -- -D warnings
node --test browser/tests/bridge.test.mjs
```

Build the Wasm module before Node tests. `RUVANE_WASM_FILE` can select a module
built by another supported toolchain. CI checks both stable and Rust 1.85.0,
the complete all-features Cargo metadata graph, local package paths and the ABI
attribute boundary. The source guard is a regression check, not a substitute
for manual review or a Rust parser/security audit.
