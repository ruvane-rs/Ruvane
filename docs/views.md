# Shared views and components

`View` is a small tree used by both the server and browser. It leaves the existing
`Html`, `Element` and `Document` APIs unchanged. A view supports sections,
paragraphs, h2 headings, spans, outputs and buttons. Its structure and event IDs
stay fixed; updates can change text and disabled states.

```rust
use ruvane::{BrowserTag, ComponentProps, InitialState, View};

let props = ComponentProps::new(7, InitialState::Boolean(true)).unwrap();
let mut view = View::new();
let section = view.element(0, BrowserTag::Section).unwrap();
view.leaf(section, BrowserTag::Output, "Enabled").unwrap();
view.button(section, "Toggle", 42, false).unwrap();
let server_html = view.hydratable(props).unwrap();
```

The application assigns component IDs and event IDs. IDs are not function names
or selectors. Parent zero is the host root; new nodes receive IDs in insertion
order. Sections and paragraphs contain elements. Paragraphs accept only spans,
outputs and buttons. Leaves contain plain text, with HTML escaping on the server.
Controls other than tabs and LF are rejected so text can round-trip through HTML.

`render()` returns marked HTML with the view's current button states.
`hydratable(props)` wraps it in a div with versioned numeric props and disables
all buttons until hydration succeeds. No inline script or executable state is
emitted. Keep private values out of props and browser code.

## Implementing a component

Implement `Component::view` and `Component::dispatch`. A component owns its state
and must release its resources when dropped. It may use an independent
`ReactiveRuntime` and `Scope`, or ordinary local Rust state. Components with a
reactive graph report signal/effect counts through `stats()`; the default is zero.

`view()` describes the current state without changing it. `dispatch(event)` runs
one declared event, after which the browser registry requests a new view and
encodes the changed text and disabled properties. Changing node count, tags,
parents or event IDs rejects the update and drops the native component.

The application's factory maps `ComponentProps` to `Box<dyn Component>`. Construct
`BrowserRuntime::new(factory)` with it. The registry handles ownership, batches,
limits, dispatch and disposal; it does not select a specific widget.

The demo factory and both components are in
[`examples/support/widgets.rs`](../examples/support/widgets.rs). The example
server and the Wasm adapter compile that same file. It includes a reactive
counter with a derived double, and a boolean toggle using event 7. Applications
can replace the factory with their own components and export the same ABI.

Initial state currently supports one `Number(u32)` or `Boolean(bool)`. Factories
validate domain ranges and state kinds. The demo counter accepts 0..=1000; the
toggle accepts a boolean. Rich object/string props, conditional subtrees, lists,
nested hydration roots and compiler macros are future work.

## Limits

A view allows 32 elements, depth 8, 16 click handlers and 1024 UTF-8 bytes per
text leaf. Failed additions leave the builder unchanged. Initial command frames
and updates also obey the existing 64 KiB/128-command limits.

A browser instance allows 16 live components and at most 1024 reported signals
and effects per component. Component factories, callbacks, rendering and Drop
code remain trusted application code; these counts do not sandbox all allocations
or execution time.

See [browser](browser.md) for hydration validation, lifecycle and the ABI.
