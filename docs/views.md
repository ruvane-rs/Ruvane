# Shared views and components

`View` is a bounded tree shared by server rendering and Wasm. It leaves the
existing `Html`, `Element` and `Document` APIs unchanged. Supported elements are
sections, paragraphs, h2 headings, spans, outputs, buttons, lists and list items.

```rust
use ruvane::{BrowserTag, View};

fn rows(keys: &[u32]) -> View {
    let mut view = View::new();
    let list = view.keyed_element(0, 1, BrowserTag::List).unwrap();
    for key in keys {
        let row = view.keyed_element(list, *key, BrowserTag::ListItem).unwrap();
        view.keyed_button(row, 1, "Run", *key, false).unwrap();
    }
    view
}

let original = rows(&[1, 2, 3]).snapshot().unwrap();
let a = original.node_for_event(1).unwrap();
let (next, commands) = original.update(&rows(&[3, 1]), 1).unwrap();
assert_eq!(next.node_for_event(1), Some(a));
assert!(next.node_for_event(2).is_none());
```

## Keys and handles

Builder IDs returned by `element`, `leaf` and `button` refer to parents while
building one view. Parent zero is the host root. They are not persistent DOM IDs.

`keyed_element`, `keyed_leaf` and `keyed_button` take a nonzero u32 sibling key.
Two explicit keys under the same parent cannot be equal. A key can be used under
another parent. Failed additions leave the builder unchanged. Plain methods
create unkeyed nodes, whose identity follows their position among siblings.
Use explicit keys for list items and for static siblings around conditional
branches. Keys and positions have separate identity namespaces.

A `ViewSnapshot` resolves the tree to live DOM handles. Retain it between updates;
commit the returned snapshot together with its batch. The browser registry does
this automatically. A key retains its handle when its parent, tag and event
binding remain the same. Text and disabled state can change without replacement.
A moved keyed subtree retains its descendant handles too.

Removal retires every descendant handle. If a hidden key returns, it receives
fresh handles. Tag or event changes replace the affected node and descendants.
Keys are scoped to their parent; moving a key to another parent creates a new
lifetime. Checked handle counters never wrap, and retired IDs are not stored in
an ever-growing tombstone table.

`View::updates(previous, view_id)` is a convenience for one transition from a
fresh previous view. Use a retained snapshot for a sequence of updates so
removed handles stay retired. Snapshot updates can insert, remove, reorder and
update subtrees. The complete bounded batch is built before the snapshot is
committed. The reconciliation algorithm is bounded by the 32-element view cap;
it does not claim a minimal or unbounded-list diff.

## Conditions and HTML

Use ordinary Rust `if` and loops to describe the current tree. A conditional
branch is added when present and removed when absent, including its DOM handlers.
Keep a container when an entire branch can be empty; an entirely empty `View`
is still rejected. List containers accept list items only; list items require a
list parent. Paragraphs accept spans, outputs and buttons. Leaves contain plain
text. Controls other than tabs and LF are rejected so text round-trips through HTML.

`render()` emits HTML with node markers and optional numeric key markers.
`hydratable(props)` adds a host div with versioned numeric/boolean state and
initially disables every button. Hydration validates the complete expected
structure and keys before attaching handlers or enabling buttons. No inline
script or executable state is emitted. Keep private data out of initial props.

## Components and resource lifetime

Implement `Component::view` and `Component::dispatch`. A component owns its model
and releases its resources when dropped. It may use a `ReactiveRuntime`/`Scope`
or ordinary Rust state. Components with a reactive graph report live signal and
effect counts through `stats()`; the default reports zero.

`view()` reads the current state without changing it. `dispatch(event)` handles
a declared application event. The registry then requests a new view and reconciles
it with the previous snapshot. Application IDs are not names of Rust functions
or selectors. Browser events also carry a live node handle; an old removed
button cannot trigger a new binding, even when an application reuses an event ID.
Public controller `dispatch(event)` resolves an explicitly requested action to
one of its current live bindings.

The factory maps `ComponentProps` to `Box<dyn Component>`. Construct
`BrowserRuntime::new(factory)` with it. The registry handles ownership, limits,
patches and disposal without knowing a specific widget.

The shared examples in `examples/support/widgets.rs` include a counter, toggle
and item list. The list has at most four logical items, with a child scope for
each item's counter and done flag. Reordering preserves the scopes. Filtering
removes DOM and event bindings while retaining hidden item model state; logical
removal drops the item scope. The empty message is a conditional subtree.

Initial props support one `Number(u32)` or `Boolean(bool)`. Factories validate
state kinds and ranges. The list's number selects its initial item count, 0..=4.
Rich object/string props, nested hydration roots, inputs and compiler macros
remain future work.

## Limits

Views allow 32 live elements, depth 8, 16 click handlers and 1024 UTF-8 bytes per
text leaf. Structural transitions allow at most 256 commands in a 64 KiB batch.
This leaves room to replace a full bounded tree and place new nodes in order.

A browser instance allows 16 live components and up to 1024 reported signals
and effects per component. Factories, callbacks, rendering and Drop remain
trusted application code; these counts do not sandbox all CPU or allocations.

See [browser](browser.md) for the versioned protocol and hydration lifecycle.
