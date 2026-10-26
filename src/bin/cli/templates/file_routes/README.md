# Ruvane file routes

Run `ruvane dev --offline` and open the printed URL. Cargo generates one shared
Rust registry from `src/routes`; no filesystem routing runs in the browser or
in HTTP handlers. The only dependencies are the first-party Ruvane packages.

- `src/routes/page.rs` declares `/` through `pub fn page() -> ruvane::FilePage`.
- `tasks/page.rs` declares `/tasks` and inherits `tasks/layout.rs`.
- `tasks/[id]/page.rs` declares `/tasks/{id}` and inherits both task layouts.
- `tasks/settings/page.rs` is the fixed route `/tasks/settings`.
- A layout exports `pub fn layout() -> ruvane::FileLayout`.

Names, components, state validation and loaders stay explicit in each declaration.
Use `crate::` imports for shared helpers outside the route tree. Route files are
trusted Rust source; the generator does not parse or sandbox their bodies.
Only `page.rs`, `layout.rs` and route directories belong in `src/routes`.

Adding, renaming or removing a page triggers a rebuild. Invalid trees or Rust
signatures retain the previous dev server. Declared CSS still updates in place.
`?native=yes` keeps native forms and the script-free policy.

Build with `ruvane build --offline`; the versioned executable contains its route
registry and Wasm. Production does not need the route source directory.
Existing `App` APIs configure caching, boundaries and streaming after registration.
