# Persistent client layouts

Alpha.21 adds root-first layout branches to interactive App pages. A layout owns
a shared Rust component and a child outlet. Navigating between sibling pages
keeps matching parents, including their DOM, event handlers and local state.

## Declaration

Register components first, then parents before children. Attach a page to its
leaf layout after declaring the page:

```rust,ignore
app.client_layout(
    PageRoute::new("workspace", "/teams/{team}")?,
    None,
    WORKSPACE_COMPONENT,
    workspace_state,
    |context| Ok(ClientLayoutData::new(workspace_seed(context)?)),
)?;
app.client_layout(
    PageRoute::new("project", "/teams/{team}/projects/{project}")?,
    Some("workspace"),
    PROJECT_COMPONENT,
    project_state,
    |context| Ok(ClientLayoutData::new(project_seed(context)?)),
)?;
app.client_page_layout("project_settings", Some("project"))?;
```

Patterns are segment prefixes of child layouts and pages. Prefix captures must
use the same names. `/` is the common root pattern; other layout patterns omit a
trailing slash. Layout declarations do not register extra HTTP routes. The
existing page declaration still decides URL ownership and fixed-route priority.
Unknown parents, duplicate names, incompatible prefixes and excessive depth fail
without changing the builder. Server and client layouts share the 32-layout
registration budget. Client branches are limited to six layouts.

The layout state resolver receives its prefix captures and the current query.
GET loaders receive the page's PageContext, including its captures and request.
Loaders, factories and validators are trusted, bounded, read-only code. Layout
components implement `page_data`, even for an empty seed. Initial actions are
forbidden, as for prepared pages. Layout data has no document title or fallback;
those belong to the leaf page.

## Ownership and transitions

Each layout renders as a section containing two separate divs: its component host
(`data-ruvane-layout-view`) and its child outlet (`data-ruvane-child-outlet`). The
component owns only its host. The navigator owns the section and outlet. This
edition places children after the layout's own view; it does not offer arbitrary
slots within that component's view tree. Server rendering uses the same order.

A parent is retained only while its name, canonical prefix key, component ID,
initial props and encoded seed match. Matching stops at the first difference.
The key includes prefix path parameters. Query-dependent state belongs in the
resolved props or seed. Include any relevant tenant or access context in that
public identity; reusing a component is not an authorization check. Changing an
ancestor recreates its entire child branch. Leaving the section releases it;
returning creates fresh local state. No hidden view cache restores drafts.

The complete incoming branch is validated by Rust before mounting. New segments
are prepared off-document. Only a fully prepared branch changes history and the
visible outlet. Preparation failures release temporary owners and keep the old
page and parents. Failed pop/commit transitions retain the existing document
fallback behavior. Departing owners release listeners and abort their actions;
uncertain mutations keep the existing conservative cache suspension.

Only the new leaf receives focus after navigation. Title, history and scroll
remain page-level operations. Back and forward use the same branch reconciliation.
`current()` still returns the leaf controller. `stats().layouts` reports retained
layouts. Navigator disposal releases the leaf and every layout; independent
shell islands stay alive until their runtime is disposed.

The runtime still permits 16 live views. Old and prepared branches overlap, so
applications must reserve view capacity for transitions and independent islands.
Exhaustion fails preparation without evicting the displayed branch. The six-layout
cap allows two full seven-component branches alongside one shell island.

## Cache policy

Pages and layouts each default to no-store. Every member must opt in:

```rust,ignore
app.client_layout_cache("workspace", NavigationCachePolicy::public(30)?)?;
app.client_layout_cache("project", NavigationCachePolicy::public(15)?)?;
app.client_page_cache("project_settings", NavigationCachePolicy::public(30)?)?;
```

The response uses the shortest lifetime, here 15 seconds. A no-store member or
`ClientLayoutData::no_store()` disables storage of the whole packet. Cookie and
Authorization guards remain in force. App prefetch links require an explicitly
public page and every ancestor to be public. Errors contain no inherited branch
and remain no-store. No partial public cache is built around a private layout.

The cache stores complete packets and no owners. Prefetch validates every seed
without consuming view IDs. A warm navigation reconciles the same descriptors as
a fresh response. `refresh()` fetches and recreates the entire branch, including
layouts, without adding a history entry; local drafts can be discarded.

## Wire contract

ABI v9 retains the 21 numeric exports, RVN6 commands and RVA1 seeds. RVP1 is still
used for pages with no layouts. RVP2 contains:

1. Four magic bytes `RVP2`, then a selected-segment u8.
2. The RVP1 fields after its magic: status, target, title, props and seed.
3. A layout-count u8 (1..=6), then root-first descriptors.
4. Each descriptor: name and context key (u32 UTF-8 length plus bytes), props
   (three u32 fields), and a complete successful RVA1 seed (u32 length plus bytes).

All integers are little-endian. Names are bounded ASCII identifiers. The entire
packet, including every seed, stays within 64 KiB. Duplicate names, unknown
segments, malformed seeds, trailing bytes and layout-bearing error pages fail.
App validates the complete registered chain, canonical keys and resolved props;
the runtime checks every component schema, view and resource limit.

HTTP packets must select segment zero, the page. `mountPageSegment(host, bytes,
index)` copies a validated packet and selects a root-first layout at 1..=6 before
staging it through the existing numeric ingress. `mountPage` selects the page.
Neither operation accepts arbitrary selectors or HTML from the wire. A new ABI
requires rebuilding Wasm and its matching bridge together.

## Verification

Rust tests cover registration, canonical contexts, forged branches, wire bounds,
SSR order, error responses and combined cache policies. The generated-app Node
suite checks the real application-owned Wasm validator and selected mounts,
including 1,000 validation probes without retained owners. `web/layout-tests.mjs`
is a browser harness for a fresh generated app with an empty task store; serve it
on an explicit interactive fixture document containing `layout-report` and
`layout-summary`. It omits existing browser cookies for navigation GET fixtures so public cache
reuse is exercised; the normal application bootstrap retains same-origin
credentials. Run it separately from the application task harness. It checks
DOM/state preservation, parameter changes, history, failed partial preparation,
100 branch switches, errors and cleanup.

Per-segment loading/error boundaries, parallel outlets and streaming remain
separate work. Existing server fragment layouts and static export keep their
existing APIs.
