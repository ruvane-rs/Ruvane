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
`assets/`. Static pages are enumerated explicitly in `pages::site`.

Builds are stored in `dist/build-NNNNNN`. The `dist/current` record selects
the last completed build. Preview loads its validated files once, with CSP.
Static hosting must apply the exported header contract. `.example` is a
placeholder; use the site's real public origin when building for deployment.

`--offline` is available on dev/build when dependencies are already cached.
Project code and Cargo configuration are trusted local executable inputs.
This CLI does not sandbox arbitrary Rust code or third-party build scripts.
