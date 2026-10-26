# Ruvane application

This workspace contains the application and its Wasm entry point. Cargo dependencies
are limited to this project's packages and the two first-party Ruvane packages.

Run `ruvane dev --offline` to compile both targets and start the loopback server.
The Rust toolchain needs the `wasm32-unknown-unknown` target. Refresh the browser
manually after changes. Type `q` then Enter to stop development.

Edit `src/pages.rs` to declare pages and their GET loaders once. Edit
`src/components.rs` and `src/remote_tasks.rs` for shared models. Native forms and
typed actions use `src/store.rs`; the bounded in-memory state resets on restart.
The layout counter persists during client navigation. `?native=yes` blocks scripts.

Startup reuses the server's page/layout nodes and embedded typed snapshot without
another page GET. Early text edits and focus survive activation. Dirty native
fallback forms decline activation; invalid metadata or DOM keeps native content
available. Snapshots are public, inert and no-store. Rebuild both targets and
browser assets together when changing shared components.

The tasks section has a persistent workspace layout and a sibling settings page.
Task details add a nested context layout. Moving between task IDs retains the
workspace and recreates the context. Leaving tasks releases both layouts.
Declare these branches with `client_layout` and `client_page_layout` in pages.rs.
Every layout and page must opt in before the complete branch can be cached.

The project page demonstrates local loading and errors. Open `/tasks/project`,
then its slow/unavailable variants. Parent counters stay usable while a GET is
pending. Configure presentation with `ClientBoundary`, `client_page_boundary`
and `client_layout_boundary`; the navigator owns the explicit GET retry button.
`?native=yes` keeps real error statuses and renders the validated parents with
scripts blocked. Error packets and transport feedback are never cached.

The project page also enables `client_page_streaming`. Open its slow variant
from Home: the workspace layout appears before the page and its counter remains
usable. The page/title/history commit after complete delivery. Interrupted
streams restore the previous page and expose a fresh GET retry. Streams are
no-store; complete public prefetches still use the existing bounded cache.
`PageContext::stream_context()` lets progressive loaders check their deadline
and shutdown cooperatively.

Open **Streaming App document** for `/tasks/document?delay=yes`. Its shell and
workspace layout arrive before the leaf. A fixed `ClientDocument` head is known
before headers, then the final snapshot hydrates the same server nodes without
a second page GET. Eligible failures use the configured typed boundary; other
late failures leave the response incomplete. Use
`?native=yes` for buffered SSR with normal HTTP error statuses.

Open **Interactive streamed layouts** for `/tasks/live?delay=yes`. Its workspace
counter becomes usable while the leaf is loading. `ClientDocument::hydrate_layouts(true)`
enables this explicitly. The final branch keeps that live layout and its state;
prepared layout actions are blocked until commitment. No additional page GET is
sent. Invalid or incomplete documents retire early owners and preserve their HTML.

Open **Recover a failed page** for `/tasks/live?recover=yes&delay=yes`. The demo
reports one failure, then becomes ready until server restart. Its typed error
keeps the workspace interactive. Change the counter and choose **Try again**:
a fresh GET replaces the leaf and retains compatible layout state. No POST is
replayed. The committed HTTP status stays 200; the initial logical error is 500.

Open **Recover a failed layout** for `/tasks/resilient?recover=yes`. Its context
layout fails once before emitting its HTML. The workspace child boundary shows
the error while keeping the workspace counter active. Increment it, then choose
**Try again**: the context and page appear without resetting the workspace.
The failed layout cannot catch its own error.

Open **Recover through a parent boundary** for
`/tasks/resilient?recover=page&delay=yes`. The context is displayed before the leaf
fails once. The workspace catches the error and the context is retired. Its counter
survives, and **Try again** reconstructs the context and page. Descendants are
removed only after the complete error branch is validated. Without JavaScript,
the received descendants and the error remain visible.

Open **Recover a root layout** for `/tasks/global?recover=root`, or
**Recover an application error** for `/tasks/global?recover=page&delay=yes`.
The App boundary catches a root failure or an error with no local boundary.
It retains the shell and retires the entire outlet branch after validation.
Increment the shell counter, then choose **Try again** to reconstruct the branch.
The counter keeps its value. Each failure resets on server restart.
`app.client_boundary(None)` disables this fallback. An invalid global error
component keeps the minimal fallback; no POST is replayed.

Open **Streaming SSR** for `/stream`, a native progressive report. Its header
arrives before the deferred content. Edit `progressive_report` in `src/main.rs`;
typed fragments use `HtmlStream` with the default byte, yield and time budgets.
`?q=...` demonstrates escaping; `?fail=early` returns 500 before headers and
`?fail=late` closes an incomplete response after the initial HTML. This page uses
an ordinary document link and the script-blocking CSP. Other App pages keep complete-packet navigation.

`ruvane build --offline` creates a versioned server bundle under `dist/`.
Run the printed executable with `--port 3000`. The executable embeds its matching
Wasm, browser modules and stylesheet. The copied resources and manifest are useful
for inspecting the bundle; the server does not read those external copies.
This server retains Ruvane's loopback development HTTP profile.

The CLI assigns the same build revision to both targets and embeds the prepared
module in the server. The browser refuses revision mismatches before hydration.
The revision detects mixed builds; it is not authentication or a signature.

The shared demo pages explicitly opt into public caching for 30 seconds. Marked
links prefetch on hover/focus. App pages otherwise default to no-store; remove
the demo cache declarations for personalized data. Runtime actions invalidate
navigation data, and uncertain replies pause caching until document reload.

## Interactive shell

Open `/tasks/shell?delay=long` and use the shell counter, **Shell draft**,
**Shell notes** and **Show tools** before the page completes. Their DOM and
state survive completion. Add `&layouts=yes` to activate layouts early too.
The shell uses `View::shell_island` and an explicit `hydrate_shell(true)`
document option. Server actions remain gated until final validation.

Use App::client_page_prerender with explicit parameter sets for public build
snapshots. The Ruvane.prerender capability marker and build/main hooks prepare
and embed these during production builds. Development stays dynamic. Frozen
pages cannot use a mutable server cache policy; rebuild to update them.
