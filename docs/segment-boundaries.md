# Segment loading, errors and retry

Alpha.22 adds local presentation and typed error boundaries to client pages and
layouts. Loading leaves the displayed branch and its drafts in place. A server
error can retain freshly validated parents. Retry is an explicit fresh GET.

Alpha.36 [file routes](file-routes.md#loading-and-errors) attach these same
boundaries through typed `loading.rs` and `error.rs` declarations with explicit
file inheritance and validation before registration commits.

## Declaration

```rust,ignore
let boundary = ClientBoundary::new(LOADING_COMPONENT, ERROR_COMPONENT)?;
app.client_layout_boundary("workspace", Some(boundary))?;
app.client_page_boundary("project", Some(boundary))?;
```

Use None to remove a declaration. Both component IDs must be nonzero and belong
to the application's factory. Loading receives Number(0). Error presentation
receives Number(0) for transport/protocol failures, Number(408) for a timeout,
or Number(400/404/500) for a typed server error. Both implement `page_data` for an
empty seed. They must contain no event bindings or initial actions. The navigator
owns the retry button separately. Native links in their views remain usable.

A page boundary takes precedence over its ancestors. A layout boundary catches
failures in its children; it cannot catch its own loader, factory or view failure.
The nearest configured ancestor is selected. Without a usable boundary, the
existing global error/document behavior applies. Unknown URLs have no declared
branch to inherit.

## Loading and transport errors

The initial HTML contains a hidden, bounded `ruvane-boundaries` manifest with
public route patterns, ancestor names and presentation component IDs. It contains
no request fields, seeds or credentials. The bootstrap reads it through
`readBoundaries(root)` and passes `boundaries` to `createNavigator`. Manually
created navigators may pass the same declarations; an empty list keeps global
status presentation.

The browser uses this table to choose a loading component and an existing outlet.
Matching a common prefix is only a presentation hint: it does not authorize a
resource or prove that the incoming parent seed is unchanged. The old page stays
mounted until the full response is validated and prepared. Parents remain usable.
Loading neither moves focus nor replaces old fields. The local host has a polite
status announcement and marks its outlet busy; cleanup restores the previous
busy attribute. The normal page-level status remains available.

At most one temporary loading/error owner is retained. It is removed before
preparing a new branch, so it does not reduce the transition's available view
capacity. If its component or view budget is rejected, a fixed text fallback
preserves the current page. Deferred cleanup handles action notifications while
the synchronous Wasm ABI is busy. A newer navigation, invalidation or disposal
retires old feedback and prevents late replies from recreating it.

Network, MIME, framing and timeout failures leave the displayed page and URL in
place and show local feedback when configured. A failed pop or an unsafe commit
uses the existing document fallback. ABI faults and Wasm traps are not isolated
component exceptions: a faulted runtime requires a document reload.

## Typed server failures

The server loads and validates layouts root first. It checks each component's
seed, view and absence of initial actions before considering that parent usable.
A failed layout is excluded from its retained branch. Leaf loader/schema/view
failures can retain all parents when the page has its own boundary. Otherwise,
the selected ancestor boundary retains its prefix and replaces its children.

Native unwinding panics become generic 500 responses at these boundaries. Logs
remain local; exception text and arbitrary error fields are never sent as page
data. The boundary component itself is checked with an empty seed and no event
bindings. Failure of that presentation falls back to a plain 500.

The response preserves its actual HTTP status, uses the fixed navigation MIME
and always stays no-store. SSR uses the same validated parent order and error
view; `?native=yes` still blocks scripts. The Wasm validator checks the complete
declared scope, canonical parent keys, props, component schemas and resource
limits before mounting. Parent state is retained only when its descriptor still
matches, as for successful navigation. Access checks belong in every loader and
action; a boundary is not an authorization mechanism.

## Retry API

```js
const failure = navigator.failure(); // null, or { url, kind }
await navigator.retry();
```

Kinds are server, network and timeout. `retry()` retires the target's cache entry,
issues a fresh GET and leaves unchanged ancestors mounted. It never retries POST
actions or clears the runtime's uncertain-mutation latch. A successful retry of a
failed push adds the target to history once. Retrying an already committed typed
error replaces that history entry. No automatic retries, backoff or rollback are
implied. The retry button uses the navigator's existing delegated click handler;
retired buttons have no effect.

Retry returns the normal navigation outcome, or skipped when disposed, busy,
blocked by a revision mismatch, or when no failure remains. Success means a
validated response was committed; another typed 500 can still leave the page in
an error state. New navigation, matching invalidation, mutation notifications and
disposal clear the recorded failure. Background prefetch errors create no UI or
retry state. `refresh()` remains the explicit full-branch refresh operation.

`stats()` adds feedback (loading/error/network/timeout or null) and retryable.
The five delegated DOM handlers and action subscription remain bounded. A 409
asks for reload and disables retry/cache reuse for that navigator.

## Wire and limits

ABI v11 provides `ruvane_boundary_mount(root, component, kind, value) -> u32`, retaining
numeric input, no host imports and 22 reviewed exports. `mountBoundary(host,
props)` uses an empty seed and rejects event-bearing views or initial actions.
Rebuild Wasm with the matching bridge. RVN6 commands and RVA1 seeds are unchanged.

RVP1 leaf pages and RVP2 successful branches remain supported. RVP3 error packets
carry magic RVP3, selected-segment u8, failed-segment u8, the RVP1 body after its
magic, then a root-first layout count and descriptors in the RVP2 encoding.
Segments number layouts from zero; the leaf page follows its last layout. The
selected field retains the mount convention: zero is the error page, then
one-based retained layouts. HTTP responses must select zero.

RVP3 permits statuses 400/404/500, an empty error seed, at most six failed/retained
levels, and a retained count no greater than the failed index. App validates the
actual configured boundary and exact retained prefix. The whole packet remains
within 64 KiB. Duplicate names, unknown scopes, malformed seeds, invalid status,
trailing bytes and mismatched selected indices fail before display. Presentation
manifests accept at most 160 records with bounded route/name/chain fields.

The generated demo has `/tasks/project`, `?delay=yes`, `?unavailable=yes` and
`?missing=yes` cases. `web/boundary-tests.mjs` runs on an explicit interactive
fixture document containing boundary-report and boundary-summary. It exercises
real delayed GETs, typed errors and SSR, injected transport failures, explicit
retry, late replies, mutation notifications, 100 error/success cycles and cleanup.
Run it separately from the other generated-app task harnesses.

[Progressive client navigation](progressive-navigation.md) uses these boundaries
while ordered layouts arrive and for typed final errors. Native HTML streaming
remains separate. Parallel outlets and recovery from a corrupted Wasm instance
remain separate work.

Initial document streams can also [recover leaf errors](streamed-leaf-errors.md)
when their boundary retains the full emitted chain. Recovery of a failed layout
or removal of already emitted descendants remains separate work.


## App-wide fallback

`app.client_boundary(Some(ClientBoundary::new(LOADING, GLOBAL_ERROR)?))?` explicitly
adds a fallback after page and parent layout boundaries. It can handle a root
layout failure and retains zero layouts. `None` removes it. The component must
accept an empty seed and read-only error props. Unknown routes and document head
preflight errors keep their existing paths. See [App boundary recovery](streamed-leaf-errors.md#app-boundary-recovery-alpha31).
