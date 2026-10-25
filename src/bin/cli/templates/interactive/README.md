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
