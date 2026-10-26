# Hydrating layouts during the initial stream

Alpha.27 can activate each complete layout while its initial document is still
loading. A slow leaf no longer delays local layout events. The final branch
adopts those live owners, their DOM and their current state, without another
page GET. This is an explicit extension of initial document streaming.

```rust,ignore
app.client_page_document_streaming(
    "project",
    Some(|_| Ok(ClientDocument::new("Project / Ruvane").hydrate_layouts(true))),
)?;
```

The generated application demonstrates this at `/tasks/live?delay=yes` through
**Interactive streamed layouts**. `/tasks/document?delay=yes` keeps alpha.26's
activation after parsing. Native-only requests and typed navigation retain
their existing profiles. Rust/Wasm ABI v11 and all 22 exports are unchanged.

## Delivery and validation

The prefix declares the `segments` document profile, expected layout count and
bounded client deadline. It includes one same-origin external asynchronous
module. This starts loading the existing Wasm while the parser continues.
There are no inline scripts, evaluated payloads or additional runtime assets.
The four versioned runtime assets and existing interactive CSP are reused.

After rendering each layout's complete view, the server places a hidden RVH1
record beside that view, inside its `data-ruvane-layout-view` wrapper. The record
contains lowercase hex followed by an empty `br` ready marker. The marker arrives
after the bytes. Incomplete text cannot be mistaken for a complete record.
The layout's child outlet follows; loaders still run root-first, once each.
A waiting stream occupies its server worker. Another worker must remain free to
serve runtime assets; a single-worker server delays early activation until the
loader finishes. The default worker pool supports the demonstrated behavior.

RVH1 contains magic, exact application revision u64, packet length u32 and one
nonempty, unselected RVP4 prefix. Integers are little-endian. The record is at
most 65,552 decoded bytes or 131,104 hex characters. The prefix retains its
64 KiB packet and six-layout limits. Its length identifies the ready layout;
its expected count identifies the complete chain. RVI1 remains complete-only.
Rust exposes the first-party `LayoutSnapshot` codec for the same envelope.

The browser checks the current URL, actual Wasm revision, expected depth,
root-first order, names, canonical keys, props and equal earlier seeds. Rust
validates route ownership and all supplied component schemas before mounting
an owner. Pending validation permits pages which explicitly declare document
streaming or progressive navigation; other routes still reject RVP4 packets.
The DOM wrapper and component must match. Consumed records are removed before
hydrating their view. The parser keeps ownership of the separate child outlet.

A bounded document observer processes only complete records. It has a single
scheduled pass, a deadline and at most six retained parent owners. Early layout
text edits use the existing input-adoption transaction. A rejected layout
preparation restores its fields and releases its owner. The document session
retires any previously activated parents after a later validation failure.

## Local events and server actions

Counters, local validation and text editing work on validated layouts before the
leaf arrives. Prepared layouts have an action gate. Any attempted server action
is completed locally as `ActionFailure::Rejected`, without a POST or a queued
retry. Its component can render ordinary rejection feedback. No mutation epoch
or uncertain-mutation latch is advanced because no request was sent.

An action attempted during initial input adoption still rejects that layout's
hydration transaction. A component which immediately queues another action
while handling local rejection fails closed; it cannot create a retry loop.
`Rejected` also retains its existing unexpected-server-status meaning.

The leaf and independent shell islands remain inert until final validation.
The complete RVI1 snapshot, completion marker and parsed document are required.
The runtime checks that already active parents belong to this runtime, match
the exact route/index/seed and retain valid live DOM. It prepares only missing
owners, including the leaf. History and native fallback guards are unchanged.
Only a successful branch commit releases the parents' action gates and transfers
their owners to the navigator. Explicit actions can then use the usual transport.

The commit does not replay initial parent state or hydrate those roots twice.
Counters, drafts, focus, selection, node identities and listeners survive the
handoff. Subsequent navigation retains matching parents and retires departing
ones through the normal navigator.

Alpha.28 adds [typed leaf recovery](streamed-leaf-errors.md) when a boundary can
retain all emitted layouts. Healthy parents stay active on the completed error
branch and can survive an explicit GET retry. Alpha.29 also accepts a failed, un-emitted nested layout when its immediate
validated parent has a child boundary. The error snapshot retains only the
emitted prefix; an explicit retry builds the missing descendants. Other failures
follow the rules below.

## Cancellation and failure

Missing, duplicate, oversized, stale, out-of-order or malformed records cannot
activate an invalid layout. A bad final snapshot, wrong DOM, native fallback
guard, exhausted view budget or history failure refuses leaf activation.
The document session retires pending parents, removes their listeners and Rust
owners, and leaves their received HTML and current field values in place.
Retired controls are disabled. No implicit GET, POST or retry is sent.

Pagehide, changed destination, root removal, document parsing ending without
completion, runtime failure and deadline expiry retire the session. Late data
cannot restart it. Timers, observers and lifecycle listeners are disconnected.
Persisted-page restoration retains the existing reload policy. Application
callbacks are trusted; there is no side-effect or execution sandbox.

For an unrecovered application failure, this mode finishes with fixed generic
HTML and a `data-ruvane-stream-error` marker while the transport budgets permit.
It closes the already emitted wrappers without an RVI1 page snapshot. The usual
completion suffix still marks the end of generation; the failure marker and
missing snapshot forbid page activation. The observer retires parent events,
and an HTTP 200 already sent remains 200. No diagnostic is reflected. This keeps
the browser from replacing a deliberately cut document with its network error
page and losing edits after a known application failure.

A network/write failure, expired transport deadline or exhausted quota can still
prevent completion. Browsers may replace such a document with their own error
page; in-document code cannot guarantee that drafts survive that replacement.
Buffered/default streaming behavior is unchanged.

Transport limits, fixed precommitted head/title, no-store, escaped rendering and
cooperative loader cancellation otherwise remain those of
[initial document streaming](document-streaming.md). The client startup deadline
also includes its Wasm load; that load retains its own ten-second transport
limit. The completion marker proves generation/cleanup, not independent access
to the HTTP chunk terminator from document scripts. Fetch clients still require
successful EOF. Partial leaf activation and out-of-order HTML are not provided.

## Advanced lifecycle

The standard `application.mjs` bootstrap manages the session automatically.
A custom bootstrap can use the same first-party APIs:

```js
const session = createDocumentHydration(runtime, root);
try {
  const initial = await session.ready;
  const outcome = await navigator.start(initial);
  if (outcome === "success") session.commit();
  else session.retire();
} catch {
  session.retire();
}
```

`ready` returns the complete snapshot and attested parent controllers. The
navigator must use both. `retire()` preserves received HTML and frees owners;
`commit()` finishes the handoff only after successful navigator startup.
Callers must settle the session. `runtime.hydrateLayout` and `retireLayout` are
advanced primitives; arbitrary controllers cannot impersonate prepared parents.
The existing `preparePageHydration` transaction accepts attested parents as its
optional third argument. Existing callers retain complete-branch behavior.

## Verification

Rust tests exercise the codec, early script, ordered records, lazy HEAD,
zero-layout pages, late failures and shared transport bounds. Node's built-in
runner checks RVH1 independently and executes the compiled application Wasm.
`web/layout-hydration-tests.mjs` uses an isolated generated-app fixture with two
layouts, an editable test model, explicit action and a gated leaf. It tests
interactive parents while parsing, action gating, live-owner adoption, early
input, malformed records, final failure, native guards, budgets, navigation,
page departure and one hundred ownership cycles. Test hooks are removed from
the generated demo after verification. No third-party Cargo/npm/test package is
used.

Alpha.30 supports [ancestor boundary recovery](streamed-leaf-errors.md#ancestor-boundary-recovery-alpha30) after descendants have already been emitted. Removal happens only after validating the complete error branch.
