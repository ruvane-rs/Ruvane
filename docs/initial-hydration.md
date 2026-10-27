# Initial App hydration

Alpha.25 activates App pages and layouts directly from their server HTML and
public seeds. The application bootstrap sends no second page GET and runs no
page loader in the browser. Native SSR remains complete; initial document
streaming is available as an [explicit opt-in](document-streaming.md) since alpha.26.

Alpha.27 also supports [early layout hydration](layout-hydration.md) during an
opted-in initial stream. Default documents still prepare the full branch together.

## Server output

An interactive App document includes a hidden direct child of
`#ruvane-application`, named `ruvane-initial`. Its only text node contains
lowercase hexadecimal RVI1 bytes. It is ordinary inert HTML, with no inline
script, JSON parser, external codec or CSP exception.

RVI1 contains magic `RVI1`, application revision u64, packet length u32 and one
complete RVP1/RVP2/RVP3 packet. Integers are little-endian. The envelope is at
most 65,552 decoded bytes or 131,104 hexadecimal characters. The packet retains
its 64 KiB limit and maximum six layouts. Pending RVP4 prefixes and selected
layout packets cannot initialize a document. The same envelope is exposed by
the dependency-free `NavigationSnapshot` Rust codec.

The snapshot contains the exact URL, title, status, component props and public
seeds used to render that response. It adds no extra loader call. The server
also renders a stable `data-ruvane-page` host around the leaf. Layout wrappers
keep their existing shape. Snapshot bytes count toward the normal HTML and
server response limits.

HTML remains no-store even for a publicly cacheable page. The initial snapshot
does not enter the navigation cache. Seeds are public browser data: never place
secrets in them. Hex encoding prevents markup injection, not data disclosure.
`?native=yes` keeps the blocking CSP, omits the snapshot and bootstrap, and
retains ordinary native forms. Typed navigation responses keep their formats.

## Activation

The generated application uses this automatically. Custom bootstraps can read
the same metadata and initialize an existing server outlet:

```js
const snapshot = readInitialSnapshot(root);
const navigator = createNavigator(runtime, {
  scope: root, outlet, status, boundaries: readBoundaries(root), progressive: true,
});
const outcome = await navigator.start({ snapshot, accept: nativeFormsArePristine });
```

The runtime must already have the matching application revision. The reader
checks the unique inert metadata element, encoding, lengths, current target and
root revision. Startup checks the envelope again against the runtime and current
URL. Rust/Wasm then validates complete route ownership, layout contexts, props,
schemas, resources and absence of initial actions before binding any page events.

Every layout and leaf is prepared against its existing DOM. The branch commits
only after all expected component trees match and all preparations succeed.
Root props, wrapper shape and claims are checked too. No HTML from the envelope
is executed or inserted into the document. Independent shell islands retain
their own hydration lifecycle.

Successful startup reuses server elements and text nodes, enables controls and
attaches listeners. It adopts early text/textarea edits into Rust and preserves
focus and UTF-16 selection. Normal reconciliation can reflect those edits in
derived content. Interactive App text fields are editable before activation;
buttons, submits, checkboxes and explicitly disabled fields stay inert. Generic
`View::hydratable` output keeps its existing disabled-controls policy.

The native fallback guard runs before preparation and again before commit.
If a native form is focused or dirty, the default bootstrap leaves it active
and declines page activation. It never submits or resets that form. Early input
adoption cannot automatically send a server action: an action command during
this transaction rejects activation and restores the draft without a POST.

Startup uses history replacement, retains existing history fields and initial
scroll, and does not push an entry or focus the leaf. The pristine native
fallback and inert snapshot are removed after success. Subsequent navigation,
progressive delivery, back/forward, refresh, caching and explicit GET retry use
the existing navigator and adopted parent owners.

## Failure and cleanup

A bad snapshot, stale revision, wrong destination, schema/DOM mismatch, view
budget failure or history exception leaves the server page and native fallback
in place. All temporary page/layout owners and listeners are released. Changes
made while activating the branch are rolled back using bounded records of the
original nodes, attributes, text, field values and focus. Claims are released;
healthy independent islands can remain active. Native callbacks and application
code remain trusted, without an execution or side-effect sandbox.

The bootstrap does not silently retry with another GET after failed activation.
Native links and forms remain available. A typed SSR error can hydrate its
validated parents, retain its logical 400/404/500 status and expose the existing
explicit fresh-GET retry. If the document leaves while Wasm is loading, late
completion cannot activate it. Pagehide releases active owners; persisted-page
restoration retains the existing document-reload policy.

The runtime's advanced `preparePageHydration(roots, packet)` API prepares
root-first layout roots followed by the leaf. `activate()` adopts them as one
transaction and returns controllers; `commit()` releases rollback records.
`rollback()` releases the prepared owners and restores their original DOM.
Callers must finish the transaction; the standard navigator manages it. The
16-view budget includes prepared components and independent islands.

Manual navigators calling `start()` without a snapshot retain the complete GET
startup path. The standard App bootstrap requires the embedded snapshot. Browser
ABI v12 and the same 22 numeric exports are reused; RVS1/RVP4 navigation and
RVA1 action formats are unchanged. Rebuild native/Wasm and matching browser
assets together when upgrading.

## Verification

Rust tests cover the envelope, malformed lengths, byte budgets, unsigned
revisions, complete branches and SSR embedding with one loader call. Node's
built-in runner checks the JavaScript decoder independently.

`web/initial-tests.mjs` runs on a fresh generated application's explicit
interactive fixture containing `initial-report` and `initial-summary`. It checks
the real bootstrap with data fetch blocked, node identity, early edits,
native fallback, corrupt metadata, stale identities, partial preparation,
history failure, schema rejection, budgets, initial typed errors, navigation,
refresh and 100 hydration/disposal cycles. Its test-only task-model variant
requests autosave on a marked early input to verify that activation sends no
POST. Run it separately from the other application task harnesses.

For that isolated fixture, add this branch after assigning `self.title = value`
in the generated task model's event-5 handler:

```rust,ignore
if self.title.starts_with("Autosave fixture") {
    let fields = FormData::default()
        .field("title", self.title.as_str())
        .map_err(|_| ComponentError::InvalidProps)?;
    return self.request(2, fields);
}
```

The ordinary generated model does not autosave. Restore it after running the
fixture. Source and assets used by these checks remain first-party.
