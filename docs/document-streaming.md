# Initial App document streaming

Alpha.26 can send an App shell and root-first layout HTML before its leaf loader
finishes. This is an explicit option per page. The final public RVI1 snapshot
activates the existing DOM without another page GET. Hydration starts after the
document has been parsed by default. Alpha.27 adds an explicit
[early layout hydration](layout-hydration.md) option. The default stream profile
described below keeps activation after parsing; the early-layout profile also
has explicit abort HTML for caught application errors.

```rust,ignore
use ruvane::{ClientDocument, PageContext, PageError};

fn head(_: &PageContext<'_>) -> Result<ClientDocument, PageError> {
    Ok(ClientDocument::new("Project / Ruvane"))
}
app.client_page_document_streaming("project", Some(head))?;
```

The generated interactive application demonstrates this at
`/tasks/document?delay=yes`. Its ordinary document link opens a slow initial
response. The existing `/tasks/project` demonstration still exercises progressive
client navigation. Native `HtmlStream` responses retain their script-blocking CSP.

Custom [file 404 profiles](file-routes.md#not-found-presentations) buffer their
initial documents so HTTP 404 can be decided before headers commit. Successful
buffered documents retain the configured head. Routes without a custom 404
profile retain the streaming behavior described below. Progressive navigation
keeps its separate RVS1 transport contract.

## Before headers

The head resolver returns the title, language, typed `Metadata` and `StreamLimits`.
The leaf's `ClientPageData` must return the same title. Resolve head-dependent
validation and errors here; use buffered rendering when the title, status,
redirect or metadata depends on deferred data. Late code cannot change HTTP 200
into a 404/500 or add metadata to a head which has already been sent.

Route ownership, URL, page props and all layout props are checked before headers.
The head resolver runs once, including on HEAD, with no stream context. It must
remain short and read-only. `None` restores buffered SSR. `?native=yes` always
uses buffered native rendering, including normal error boundaries and statuses.
Typed navigation keeps its existing negotiation and optional RVS1 streaming.

The runtime shell receives a private inert slot. It must preserve its children
exactly once in normal block flow. A missing, escaped or duplicated slot fails
before loaders or headers. There is no public raw HTML constructor. Title,
metadata, shell text and component text use the existing escaped typed renderers.
The renderer does not validate the HTML content model of application shells.

## Ordered body and completion

The prefix contains the complete head, configured stylesheets, App wrapper,
common shell prefix, status and open outlet. Each layout loads once, validates
its public seed and renders its own view, then opens its child outlet. Writes
finish before the next loader is called. The leaf loads once and renders inside
the same stable page host used by buffered SSR. Native fallback, closing layout
wrappers, shell suffix, boundaries and the final snapshot follow it.

The transport drops the producer before writing the completion suffix. That
suffix contains the inert `ruvane-complete` marker, closes the App wrapper and
adds the existing deferred external module bootstrap and document ending. A
cleanup panic cannot send that suffix. HTTP chunk termination follows successful
suffix delivery. Native streams and RVS1 also withhold their suffix on cleanup
failure.

Streamed App roots declare `data-ruvane-document="stream"`. The snapshot reader
requires the unique empty hidden completion marker as the App's final direct
child, immediately after the unique snapshot. It refuses an unknown profile,
missing/duplicated/misplaced marker or a document still being parsed. The standard
module executes after parsing, validates the complete snapshot and prepares all
server roots before activation. Healthy activation removes the snapshot and
completion marker. Layout/page node identity, early text edits, focus, selection
and native fallback guards use the existing initial hydration transaction.
No initial page data is fetched again and the snapshot does not enter the cache.

The completion marker proves framework generation and cleanup finished. It is
not a cryptographic signature or an independent proof of the HTTP chunk terminator:
ordinary document scripts cannot inspect that transport framing. Fetch consumers
must still await successful EOF. A network failure after the full marker and
bootstrap arrived cannot be distinguished through that marker alone.

Alpha.28 adds [typed leaf recovery](streamed-leaf-errors.md) for an eligible
page or innermost layout boundary. It completes a validated logical error without
changing the committed HTTP status. Alpha.29 also handles an un-emitted nested
layout through its nearest validated parent child boundary, keeping the emitted
prefix. Other failures retain the behavior below.

## Bounds and failures

The default budgets remain 16 KiB per HTTP chunk, 1 MiB total decoded HTML,
128 chunks and 5 seconds. `ClientDocument::limits` replaces them within the
supported ranges, and the server can tighten the byte/write budgets. Prefix,
HTML, hex snapshot, completion suffix and document ending all count. Large
validated fragments are split into bounded byte chunks, including across UTF-8
code points; the complete body remains UTF-8. Seeds retain their separate 64 KiB
navigation packet budget and six-layout bound. Temporary fragment storage is
bounded by the configured body budget. Trusted callbacks may allocate before
the framework checks their result, as with other typed rendering APIs.

Loaders receive `PageContext::stream_context()` for cooperative deadline and
shutdown checks. There are no producer threads, unbounded output queues or
parallel loaders. Synchronous application code is trusted and cannot be forcibly
interrupted; socket failures are observed at writes.

After commitment, eligible loader errors, panics and schema/title mismatches can
finish through a typed boundary. Alpha.30 also supports ancestor boundaries which
retire emitted descendants after the complete error branch is validated. The
committed HTTP status and head stay unchanged; error packets carry logical status.

Without a valid boundary, default streams remain incomplete. Early-hydration
streams can finish with generic abort HTML when transport budgets permit. Quotas,
deadlines, shutdown, failed writes and cleanup failures can still leave an
incomplete response. No diagnostic text or automatic retry is emitted. Incomplete
metadata never activates a branch. Native-only buffered SSR keeps its actual HTTP
error statuses and scoped rendering.

ABI v12, the 22 exports, the four existing runtime assets and RVI1/RVP/RVS1/RVA1
formats are unchanged. Both crates remain first-party and need no Cargo/npm,
codec, HTTP or browser-test dependency.

## Verification

`tests/document.rs` checks gated nested layouts before the leaf, lazy HEAD,
preflight rejection, shell slot integrity, escaped text, single loader execution,
snapshot identity, matching buffered structure, bounded splitting, late errors,
quotas, cancellation and worker recovery over real loopback sockets.

`web/document-tests.mjs` adds real-browser checks on a generated application. Its
isolated fixture streams `/tasks`, gates its loader with `/fixtures/prepare` and
`/fixtures/release`, and gives that response a 60-second test budget. The harness
opens a document, observes visible inert parents, releases the leaf and holds
Wasm loading to test early input adoption. It also exercises malformed completion
metadata, the real bootstrap, cleanup, navigation, native rendering and repeated
activation. Fixture hooks and test assets are removed from the generated demo
after verification. All harness code is first-party.

Alpha.30 supports [ancestor boundary recovery](streamed-leaf-errors.md#ancestor-boundary-recovery-alpha30) after descendants have already been emitted. Removal happens only after validating the complete error branch.

Alpha.31 adds an explicit [App boundary](streamed-leaf-errors.md#app-boundary-recovery-alpha31) for root and unhandled branch errors while retaining the independent shell.

Alpha.32 adds [early shell hydration](shell-hydration.md) for explicitly marked
independent views, with or without early layouts.
