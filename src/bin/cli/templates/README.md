# Ruvane site

A small Rust site with shared components, server routes, and static pages.
The only framework dependency is Ruvane itself.

```sh
ruvane dev
ruvane build --origin https://site.example
ruvane preview dist
```

Open the HTTP address printed by dev/preview. Dev recompiles changed sources
and restarts the server; refresh the browser manually. Type `q` then Enter
to stop dev. Compilation errors keep the previous working server available.

Edit `src/pages.rs`, share layout in `src/components.rs`, and place assets in
`assets/`. Static parameters are enumerated in the `pages::application` declaration.

Builds are stored in `dist/build-NNNNNN`. The `dist/current` record selects
the last completed build. Preview loads its validated files once, with CSP.
Static hosting must apply the exported header contract. `.example` is a
placeholder; use the site's real public origin when building for deployment.

`--offline` is available on dev/build when dependencies are already cached.
Project code and Cargo configuration are trusted local executable inputs.
This CLI does not sandbox arbitrary Rust code or third-party build scripts.

Pages and assets are declared once in `src/pages.rs` through `App`. The same
callbacks serve development routes and produce static output. Shared and
nested layout fragments are in `src/components.rs`. Static pages receive
parameters without an HTTP request; use `PageMode::ServerOnly` explicitly for
request-dependent pages. Named links are constructed with `PageContext::url`.
The configured 404/500 pages are server responses; exporting a site does not
configure error-page routing on a hosting provider.

Articles live in `content/*.md`. Each starts with `---ruvane`, exactly one
`title: ...`, `slug: ...`, and `description: ...`, then `---` and Markdown.
The directory must contain only portable regular .md files and remain nonempty.
Adding/removing a file changes the next compiled site's article pages.

`build.rs` uses Ruvane as a first-party build dependency to validate and embed
the files as escaped string data in Cargo's output directory. The running
application owns its text snapshot; it does not read content from disk on
requests. Dev watches the directory and retains the old server if validation
fails. Rust-looking text and HTML are displayed as escaped text; code is never
executed. The supported Markdown subset has explicit input/work/output limits.
