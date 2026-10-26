# Ruvane file routes

Run `ruvane dev --offline` and open the printed URL. Cargo generates one shared
Rust registry from `src/routes`; no filesystem routing runs in the browser or
in HTTP handlers. The only dependencies are the first-party Ruvane packages.

- `src/routes/page.rs` declares `/` through `pub fn page() -> ruvane::FilePage`.
- `tasks/page.rs` declares `/tasks` and inherits `tasks/layout.rs`.
- `tasks/[id]/page.rs` declares `/tasks/{id}` and inherits both task layouts.
- `tasks/settings/page.rs` is the fixed route `/tasks/settings`.
- `tasks/loading.rs` and `tasks/error.rs` supply inherited read-only presentation.
- Root `not_found.rs` handles unknown URLs; `tasks/not_found.rs` handles missing tasks.
- A layout exports `pub fn layout() -> ruvane::FileLayout`.

Names, components, state validation and loaders stay explicit in each declaration.
Use `crate::` imports for shared helpers outside the route tree. Route files are
trusted Rust source; the generator does not parse or sandbox their bodies.
Only `page.rs`, `layout.rs`, `loading.rs`, `error.rs`, `not_found.rs` and route directories belong
in `src/routes`. The first screen scope needs both files. A descendant can replace
one screen while inheriting the other. A layout catches child failures, never its
own loader. Use the App fallback for root layout failures.

Screen files export `pub fn loading() -> ruvane::FileLoading` and
`pub fn error() -> ruvane::FileError`. Their component IDs are checked before
activation with empty seeds and all required states. Views must be read-only and
start no actions. The navigator owns retry: a fresh GET, with no POST replay.

Adding, renaming or removing a page triggers a rebuild. Invalid trees or Rust
signatures retain the previous dev server. Declared CSS still updates in place.
`?native=yes` keeps native forms and the script-free policy.

Build with `ruvane build --offline`; the versioned executable contains its route
registry and Wasm. Production does not need the route source directory.
Existing `App` APIs configure caching, boundaries and streaming after registration.

A not-found file exports `pub fn not_found() -> ruvane::FileNotFound` and declares
a read-only Number(404) component with an empty seed. Descendants inherit it;
a local file replaces it. Missing resources keep validated parents. Unknown
URLs use only the root presentation with no guessed layout chain. Errors retain
HTTP 404, no-store and noindex metadata. Initial documents under a custom 404
profile are buffered before headers; progressive navigation keeps RVS1 framing.
Native pages cannot inherit file presentations. For a mixed file tree, configure
the client global fallback with App APIs rather than a root not_found.rs.
