# Recovering streamed branch errors

Alpha.28 connects initial document streaming to `ClientBoundary`. A caught leaf
loader, view, schema or title failure can finish as a complete typed error page
while keeping every already validated layout. Early layout controllers keep
their DOM, counters and drafts. A user can then request a fresh GET through
**Try again**, without reloading the document or replaying a server action.

```rust,ignore
let boundary = ClientBoundary::new(LOADING, ERROR)?;
app.client_page_boundary("project", Some(boundary))?;
app.client_page_document_streaming(
    "project",
    Some(|_| Ok(ClientDocument::new("Project").hydrate_layouts(true))),
)?;
```

This also works with the default document stream, which activates after parsing.
A child boundary on the innermost layout can handle the leaf if the page has no
boundary. Through alpha.29, recovery required retaining the full emitted layout chain.
Alpha.30 also supports a boundary above emitted descendants, as described below.

## Error completion

The server records the emitted depth before attempting each layout or leaf.
A failed layout cannot be relabelled as a leaf error, even if its seed had already
been constructed. Loaders are not rerun to build the error response.

For an eligible failure, the server creates an RVP3 packet with the same URL,
precommitted title, exact layout props/seeds, failed segment index, empty error seed
and logical 400/404/500 status. The configured error component receives the status
as Number. Its view must be read-only and cannot queue an initial action. Shared
ownership, schema, packet and rendering checks still apply. The error is rendered
in the ordinary stable leaf host, followed by an RVI1 snapshot and the existing
completion suffix. No extra codec, bootstrap asset or ABI export is added.

HTTP 200 already sent remains 200. The RVP3 status describes the logical page
failure and the bootstrap displays it as `Page 400.`, `Page 404.` or `Page 500.`.
The title and metadata already sent remain unchanged. Native-only and buffered
SSR retain their real HTTP error statuses. Errors found before commitment keep
their existing response behavior.

The final error snapshot follows the same atomic activation checks as a healthy
page. The early session verifies revision, URL, order, title and identical parent
seeds. Rust/Wasm validates the complete error branch before the navigator adopts
its attested parent owners. Only a successful branch commit releases their action
gates. Parent fields are not reconciled from the initial seed again. The error
component stays read-only; the navigator owns the accessible retry control.
No automatic page GET or POST is needed to activate the error document.

An absent or invalid boundary or a root layout failure retains the existing failure path. In early-hydration mode,
caught application failures use generic abort HTML when budgets permit; other
streams remain incomplete. Invalid/eventful/panicking error components and initial
actions also use that fallback. Diagnostics are never reflected in the response.
Transport deadlines, quotas, failed writes and cleanup errors cannot be bypassed
by error rendering. A real network failure may still cause a browser error page.

## Explicit retry

`navigator.retry()` and **Try again** issue a fresh GET through the existing
navigation transport. The request bypasses the cache and uses the current module
revision. The server can rerun layout and page loaders; this is not a leaf-only
server request. The browser retains parents whose declaration, key, props and
public seed still match. It replaces the leaf and any incompatible descendants.

For retrying the displayed URL, history is replaced rather than pushed. Existing
history fields are kept. Focus, UTF-16 selection and current scroll are preserved
when focus remains inside a retained parent's own view. If the retry control has
focus instead, the new leaf receives the usual navigation focus. A history-write
failure before DOM replacement leaves the displayed branch intact and exposes
failure feedback. Failures after DOM replacement begins retain the existing
navigation fallback policy.

A retry may commit another typed error, in which case another explicit retry is
available. Errors are no-store and never enter the navigation cache. Successful
public GETs retain their existing caching rules. Duplicate clicks during a pending
retry are ignored. New navigation, disposal or action-driven invalidation retires
stale replies before they can replace the active branch. No POST is retried,
queued or replayed. Uncertain-mutation safeguards remain unchanged.

The generated application's **Recover a failed page** link opens
`/tasks/live?recover=yes&delay=yes`. That demo loader reports one failure, then
becomes ready for the process lifetime. The bounded atomic flag is test/demo
availability state; it resets on server restart. The workspace counter remains
usable on the typed error and keeps its value after retry. Ordinary
`?unavailable=yes` continues to simulate a persistent failure.

## Verification

Native tests cover both document profiles, 400/404/500 logical errors, panic,
invalid seed/title, single loader execution, gated delivery, zero-layout pages,
innermost/outer boundaries, invalid error components and transport bounds.
The compiled application Wasm test validates the same owned error branch and
rejects forged failure scopes without retaining instances.

`web/recovery-tests.mjs` uses an isolated generated-app fixture with editable
parents, a gated failing leaf and controlled availability. It checks real
bootstrap activation, counters, drafts, selection, explicit buffered/progressive
GET retry, incompatible seeds, repeated errors, bad replies, history failure,
supersession, action invalidation, duplicate requests, disposal, stale identities,
native SSR and one hundred startup/cleanup cycles. The fixture model, routes and
assets are removed from the generated demo after verification. The normal browser
regressions also cover the previous default and early-hydration paths.

The workspace remains two first-party crates using `std`, with ABI v13, 22 numeric
exports, four versioned runtime assets, and no third-party Cargo/npm/build/test
dependency.


## Nested layout recovery (alpha.29)

A layout that fails before emitting its HTML can use its nearest validated
parent's child boundary. For `workspace → context → page`, a failure in `context`
leaves `workspace` and its error child visible. The failed layout and page are
never mounted, and the page loader is not called. A layout cannot catch its own
failure. Its parent must declare a child boundary.

The final RVP3 branch contains exactly the emitted prefix. Its failed index is
the prefix length, shorter than the full depth declared by the document. Both
the early session and runtime validate the shorter error branch. Attested parent
owners must agree on the original depth, URL, title and seeds; successful packets
still require the full chain. A rejected record's uncommitted seed is discarded.

An explicit retry fetches the full route and builds the missing layout and page.
Compatible ancestors keep their live state and DOM, including fields, focus and
selection. Repeated errors stay retryable and uncached. No GET or POST is retried
automatically. Alpha.30 adds removal of already emitted descendants to reach a higher boundary.
Alpha.31 adds an explicit App boundary for failures without a surviving parent.

The generated **Recover a failed layout** link opens
`/tasks/resilient?recover=yes`. Its context fails once per server process. Increment
the workspace counter, then use **Try again** to see the missing context and page
appear while the workspace counter keeps its value.

`web/layout-recovery-tests.mjs` covers a gated real document, shortened error
activation, buffered/progressive retry, ancestor node identity and field state,
invalid final packets, repeated failures and one hundred cleanup cycles. Native
tests include a cumulative prefix that exceeds the packet budget after a seed
was constructed, invalid boundaries and a root failure without a surviving parent.


## Ancestor boundary recovery (alpha.30)

For `workspace → context → page`, a page failure can use the workspace child
boundary even after context was displayed and activated. The workspace retains
its own DOM, state, drafts and focus. Context and everything below it are removed
only after validating the complete error branch. Their local drafts are no longer
part of the active page. An explicit GET retry rebuilds these descendants.

The server closes departed layout wrappers before rendering the error host beside
the departed subtree. It cannot retract HTML already sent. A hidden direct App
child `ruvane-retired`, immediately before `ruvane-initial`, contains the original
emitted RVP4 prefix inside RVH1. The existing RVP3 identifies both the failing index
and the shorter retained prefix. This uses existing codecs and adds no ABI export
or runtime asset. Without JavaScript, the received descendants and error remain
visible; native-only requests still use buffered SSR and actual HTTP statuses.

The browser checks exact revision, URL, title, prefix length, retained descriptors,
canonical keys, seeds and complete DOM shape. Rust/Wasm validates both packets.
Early owners must attest the full emitted prefix; other documents validate the
retiring roots against temporary prepared views without activating them. Missing,
duplicate, malformed, inconsistent or unnecessary retirement records refuse startup.

The error view is prepared without changing the current branch. Native form guards,
history, action epoch, destination and ownership checks run before commitment.
Only then are departed controllers retired, the subtree removed and the error
activated. A rejected preparation or history write leaves the received subtree
in place. Focus inside a surviving ancestor remains there; focus inside the removed
subtree moves to the error host. Removed controls lose their handlers. Parent action
gates are released only on a successful complete commit.

Retries remain explicit, uncached GETs for errors. New navigation, departure,
disposal, deadlines, exhausted quotas and invalid data cannot commit stale replies.
No POST is automatically sent or replayed. Without an explicit App boundary, a root layout still uses the existing fallback.

The generated **Recover through a parent boundary** link opens
`/tasks/resilient?recover=page&delay=yes`. Its context becomes interactive before
the leaf fails once. The workspace catches that error, removes the context and
keeps its counter. **Try again** reconstructs the context and page.

`web/parent-recovery-tests.mjs` checks gated real delivery, two retired descendants,
nearest boundaries, early/default activation, retained drafts and selection, focus
inside removed views, explicit retries, malformed or unnecessary records, native
guards, history refusal, stale owners/destinations, cancellation, budgets and one
hundred startup/cleanup cycles. Fixture routes and controls are removed from the demo.


## App boundary recovery (alpha.31)

Register an App boundary after `components` to catch failures which have no page
or ancestor boundary. Registration executes no component or loader. The existing
`components(factory, error_component)` registration still handles ordinary unscoped
400/404 responses. The App boundary is a separate opt-in for owned client routes.

```rust,ignore
app.client_boundary(Some(ClientBoundary::new(LOADING, GLOBAL_ERROR)?))?;
// Use None to restore the default behavior.
```

A page boundary takes precedence, followed by the nearest parent layout boundary,
then the App boundary. A layout's child boundary still cannot catch its own loader
failure. Unknown routes, invalid route props and failures during document/head
preflight keep their existing response paths. Invalid selected local presentations
also retain the existing minimal fallback; this does not recursively rerun boundaries.

A root failure emits an RVP3 error with failed index zero and no retained layouts.
If other layouts were already emitted, the existing `ruvane-retired` RVH1 record
attests the full received prefix. The browser validates that prefix, error packet,
DOM, revision, URL, seeds and view owners before removing the entire outlet branch.
The root completion also checks the declared chain depth against the public route
manifest. Rust/Wasm authorizes zero retained layouts only when the selected App
boundary permits that exact error component and logical status.

The manifest adds at most one `app` declaration, in addition to the existing
160-page/layout bound. Loading and transport feedback use it only for matching
client routes without a more specific presentation. The formats, ABI and four
runtime assets are unchanged.

The App shell is outside the outlet and is retained. Independent counters, field
values and nodes survive error activation and retry. Same-URL explicit retries
preserve focus and selection in surviving shell fields; focus inside a retired
branch moves to the error host. Retired controls lose their listeners and owners.
The retry is a fresh GET and can reconstruct the full branch. Errors remain
uncached; no POST is retried or replayed.

Global error components receive Number(400/404/500) and an empty public seed. They
must be read-only and cannot queue actions. Render failures, panics, event bindings,
initial actions or schema rejection use the minimal existing fallback without
internal diagnostics. Transport failures, exhausted quotas and expired deadlines
cannot be converted into a successful error completion. Without JavaScript, HTML
already sent remains visible alongside the error; native-only SSR remains buffered
and keeps actual HTTP statuses. Alpha.32 adds an independent [early shell hydration](shell-hydration.md) opt-in.

The generated **Recover a root layout** and **Recover an application error** links
use `/tasks/global?recover=root` and `/tasks/global?recover=page&delay=yes`.
Each demo failure happens once per server process. After the error, increment the
shell counter and choose **Try again**; the branch returns without resetting it.


`web/global-recovery-tests.mjs` covers real gated root/branch failures, both
initial document profiles, local boundary precedence, full-branch retirement,
independent shell drafts, explicit buffered/progressive retries, forged metadata,
foreign owners, history refusal, cancellation, mutation invalidation, invalid
global presentations and one hundred cleanup cycles per document profile.
The early session hands off an `accept` guard, including when no layout owner was
created. Bootstrap composes this guard with the native-form guard before commit.
