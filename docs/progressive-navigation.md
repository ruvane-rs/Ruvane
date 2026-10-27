# Progressive client navigation

Alpha.24 adds ordered layout delivery to App navigation, with no third-party
dependencies. New layouts can appear and accept local events while the page's
GET loader is still running. Native SSR and the existing complete page packets
keep their behavior.

## Enable a page

```rust,ignore
app.client_page_streaming("project", true)?;
```

This opts an existing client page in; unknown names and ordinary server pages
are rejected. Its layouts are loaded root first, then the leaf page. There are
no parallel outlets or out-of-order segments. The generated application enables
this on `/tasks/project`; its slow link adds `?delay=yes`.

The application bootstrap enables `createNavigator(..., { progressive: true })`.
Manually constructed navigators default to false. The App bootstrap now uses
[initial snapshot hydration](initial-hydration.md) without another GET and keeps
the native dirty-form guard. Manual startup without a snapshot still requests a
complete packet. Foreground navigation, pop, refresh and retry can request the
progressive representation. Non-opted-in pages return complete packets.
Prefetch always requests a complete packet.

Loaders retain `fn(&PageContext) -> Result<..., PageError>`. During progressive
loading, `PageContext::stream_context()` exposes the same cooperative deadline
and shutdown checks as native HTML streaming:

```rust,ignore
if let Some(stream) = context.stream_context() {
    stream.wait(std::time::Duration::from_millis(500))
        .map_err(|_| ruvane::PageError::InternalServerError)?;
}
```

The existing worker calls one loader at a time after the previous packet has
been written. No producer thread, executor or channel is created. Ordinary
blocking code must bound its own CPU, allocations and I/O; it cannot be forcibly
interrupted. A disconnect is observed on a socket write. See
[native streaming](streaming.md) for the shared transport limits.

## Ownership and history

Every prefix is validated in Rust/Wasm against the shared App declarations,
canonical layout keys, resolved props and component schemas before it mounts.
The pending leaf carries no seed and cannot mount. Matching parents retain
their DOM, owners and local state. A new layout mounts only inside an outlet
created by the navigator, with local loading feedback below it.

When a prefix changes the branch, the old subtree is detached and retained
while the temporary layouts are visible. Its owners remain live until commit
or cancellation. A final valid packet, explicit terminal and successful response
EOF are required before the leaf replaces it, the title changes or history is
committed. A successful push adds one entry. A typed server error also completes
a navigation, preserving the parents allowed by its configured boundary.
Retry of a committed error replaces that entry.

Supersession, invalidation, timeout, malformed data and truncation dispose
temporary owners and restore the previous subtree, focus when appropriate and
saved scroll position. Earlier replies cannot address the newer operation.
Transport failures leave local retry feedback; retry is a fresh GET. A failed
pop or an unsafe runtime/DOM commit falls back to a normal document request.
Independent shell islands stay live. Mutations retire speculative reads and
preserve the existing uncertain-outcome cache latch; no POST is replayed.

Memory remains subject to the runtime's 16-view budget. The old branch, temporary
layouts, independent islands and feedback all count. Failed preparation releases
temporary owners rather than evicting the displayed page to obtain a slot.

## Cache and errors

Progressive responses always use `Cache-Control: no-store`, including after
successful completion. They never enter the navigation cache. Complete public
prefetch replies still use the existing policy, schema, credential, generation
and freshness checks, and a warm complete packet can satisfy a navigation.
No partial prefix or failed stream becomes a cached page.

Before headers, invalid identity, target/state or setup produces a normal error.
After headers the HTTP status is already 200. A loader failure can finish with
a typed RVP3 error at its nearest configured boundary. Its logical status is in
the packet; native SSR still sends the real 400/404/500 HTTP status. An uncaught
failure, quota violation, deadline, shutdown or write error leaves the response
incomplete. Error diagnostics are never embedded in the stream.

## Wire contract

The browser requests `application/vnd.ruvane.page-stream`, with the exact
`Ruvane-Application-Revision` and a positive decimal `Ruvane-Stream-Token`.
The token is a monotonically increasing browser operation ID, not authentication.
The server requires both for an opted-in stream. A stale/missing revision gives
409; a missing, noncanonical or overflowing token gives 400 before loaders.
HEAD validates the plan and identity but runs no deferred loaders.

The response uses the same MIME, chunked HTTP framing, no-store, nosniff and the
fixed script-blocking CSP. These are data packets, not executable HTML. RVS1 is:

1. `RVS1`, followed by revision u64 and token u64.
2. Repeated u32 packet lengths and complete packet bytes.
3. One zero u32 terminal, followed by response EOF with no trailing bytes.

Integers are little-endian. Packet zero is an RVP4 manifest with zero loaded
layouts. Each later RVP4 adds exactly one root-first layout, preserving every
prior descriptor and seed. One final RVP1/RVP2 success or RVP3 scoped error
follows. Success must contain the entire declared chain; an error can retain
only previously received, validated ancestors. The decoder checks target,
revision, token, order, counts, props and immutable prefixes.

RVP4 starts with magic, selected-segment u8 and expected-layout-count u8. The
remaining fields match RVP2, including an explicit layout count that can be zero.
Status is 200 and the pending leaf's RVA1 seed is empty. The App validator checks
the expected chain length and explicit page opt-in. ABI v13 keeps the same 22
numeric exports: validation accepts prefixes, while mounting accepts only their
already loaded layouts. Leaf mounting requires a complete packet. Rebuild the
application Wasm and matching bridge together.

Limits are six layouts, eight nonterminal packets, 65,532 bytes per packet and
512 KiB for the entire decoded response. HTTP chunks are at most 64 KiB. The
server intersects its byte limit with the stream budget and uses the shorter
of its write timeout and five seconds. The browser retains its absolute
navigation timeout, capped at ten seconds, and rejects excessive empty reads.
Packets remain validated independently of HTTP/network read boundaries.

## Verification and remaining work

Rust socket tests gate a real leaf loader after its layout prefixes arrive.
They cover identity, HEAD, opt-in, scoped errors, panics, schema rejection,
byte limits, deadlines and shutdown. Node tests fragment RVS1 at byte boundaries
and exercise malformed ordering, identity, EOF and budgets. Generated application
Wasm validates pending prefixes without allocating owners, rejects leaf mounts
and validates layout ownership.

`web/progressive-tests.mjs` runs against a fresh generated application on an
explicit interactive fixture with `progressive-report` and `progressive-summary`.
It tests a real delayed GET, stalled delivery, history, parent state, rollback,
late bytes, final schema rejection, mutations, retry, public buffered cache reuse,
refresh, view-budget exhaustion and 100 success/error cycles. Run it separately from the other generated-app
harnesses. The stall fixture holds already received bytes to test client
ownership deterministically; the native test separately gates server computation.

[Initial App document streaming](document-streaming.md) and opt-in
[early layout hydration](layout-hydration.md) are available separately.
Parallel loading, general
asynchronous loaders, out-of-order replacement and public deployment remain
separate work. The server retains its loopback development profile.
