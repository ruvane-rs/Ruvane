# Interactive applications

Alpha.19 connects shared pages, an application-owned Wasm registry and the CLI.
The framework and generated workspace use no third-party crates or npm packages.

From a Ruvane checkout, install the CLI and generate a project:

```sh
cargo install --path . --offline --locked
ruvane new my-app --interactive --framework-path /absolute/path/to/Ruvane
cd my-app
ruvane dev --offline
```

The Rust toolchain must have `wasm32-unknown-unknown` installed. Interactive
generation currently requires a local checkout because `ruvane-browser` is not
published. Static project generation keeps its existing registry/local options.
The interactive marker is `ruvane-project\t2\tinteractive\n`; legacy static
projects keep marker version 1. Generation never overwrites an existing directory.

The generated application has home, task list, detail and error views, a common
layout counter, typed actions and native POST forms. Tasks are shared in memory,
limited to 32, and reset when the server restarts. Native controls are available
before activation or if Wasm fails. `?native=yes` serves the same page with scripts
blocked. Native mutations redirect to `/tasks`.

Alpha.23 generated projects also attach an explicit native `/stream` route to
the router. The **Streaming SSR** link loads a script-free document progressively;
it does not use the client navigator. See [streaming](streaming.md) for the
producer API, budgets and failure examples. Alpha.24 also enables
[progressive client navigation](progressive-navigation.md) on the project page;
its workspace layout becomes usable before the slow leaf arrives.

## Shared declarations

The root package owns `src/pages.rs`, `src/components.rs`, `src/remote_tasks.rs`
and `src/store.rs`. Its library is named `ruvane_application`. The `browser/`
member is a small cdylib entry point depending on that library and on the
first-party adapter with default features disabled.

Register the component factory and an error component once:

```rust,ignore
let mut app = App::new();
app.components(components::create_component, components::ERROR)?;
app.client_page(
    PageRoute::new("task", "/tasks/{id}")?,
    components::DETAIL,
    task_state,
    |context| {
        let id = context.param_as::<u32>("id").map_err(|_| PageError::BadRequest)?;
        let task = load_task(id)?;
        Ok(ClientPageData::new(task.title, task.public_data).fallback(native_forms(id)))
    },
)?;
```

`task_state` has type `fn(&NavigationMatch) -> Result<InitialState, PageError>`.
It resolves state from decoded captures or URI query values, without loading
server data. The declared component ID and returned state form the props. The
GET loader receives the usual `PageContext`, including its validated request,
and returns a title, public `ActionData` seed and optional `Html` fallback.

The same `App` declaration supplies native GET/HEAD handlers, named URLs and
`App::validate_navigation`. The latter checks ownership and props without running
loaders. The browser runtime then checks the seed with `Component::page_data`.
SSR performs the same schema/view checks. Seeds cannot queue initial actions.
An invalid loader seed returns 500. A configured segment boundary can render
its validated parents and a typed error; otherwise the global error applies.

The error component must accept Number(400) and Number(404), with an empty seed.
Bad state resolves to 400; missing data and unknown URLs resolve to 404. Server
failures use a configured segment boundary or an ordinary 500 response. Error component IDs are public registry
values, not authorization. GET loaders and state resolvers must be bounded,
read-only trusted Rust code. Enforce resource access in loaders and actions.

Client pages share the 128-route App budget and are listed by
`server_only_routes`; they are omitted by `static_site`. Existing `App::page`,
its nested server layouts and static export retain their behavior.

## Server configuration and shell

The native executable attaches its prepared module before calling `App::router`:

```rust,ignore
app.client_runtime(
    ClientRuntime::new(include_bytes!(env!("RUVANE_WASM_FILE")), REVISION)?
        .shell(pages::shell)
        .stylesheet(InternalUrl::new("/assets/site.css")?)?
)?;
let router = app.router(None)?;
```

A client-page App without a runtime is rejected when constructing its router.
`ClientRuntime` requires a module header and a maximum size of 1 MiB. The browser
checks the actual exports, ABI and revision before hydration. Four reserved
resource routes under `/ruvane-runtime/<revision-in-hex>/` serve the module,
bridge, navigator and bootstrap with fixed MIME and no-store headers. They share
App route/asset budgets and named URL ownership checks. Up to 16 common
stylesheets are accepted; register their bytes with the ordinary `App::asset`.

The shell is `fn(Html) -> Html` and is common to all client pages. It must keep
the supplied outlet/status content and must not depend on the changing route or
page title. It may include sibling hydratable islands. The bootstrap hydrates
those islands, then hydrates the current page/layouts from an inert embedded
snapshot without another GET. The page title
and outlet change during navigation; the shell and its islands retain state.
Route-dependent nested client layouts use [persistent client branches](client-layouts.md).

The framework reserves `ruvane-initial`, `ruvane-boundaries`, `ruvane-application`, `ruvane-status`, `ruvane-outlet` and
`ruvane-fallback` element IDs. Application shells must not duplicate them. The
bootstrap declines initial activation when a native fallback field has focus
or a changed value, so it does not discard a native draft. It releases listeners
and owners on pagehide and reloads a restored back/forward-cache document.
See [initial hydration](initial-hydration.md) for the envelope, node reuse,
early text fields, transactional validation and failure cleanup.

## Application exports and revisions

The browser entry point is:

```rust,ignore
#![deny(unsafe_code)]
ruvane_browser::export_application!(
    ruvane_application::components::create_component,
    ruvane_application::validate_page,
    ruvane_application::REVISION
);
```

Invoke this macro once in a Wasm cdylib. It isolates the twenty-two reviewed numeric
exports in one module. It adds no input pointers, unsafe blocks or host imports.
The core continues to forbid unsafe code. The optional adapter `demo` feature is
only for repository examples; generated applications disable it.

The CLI sets `RUVANE_APPLICATION_REVISION` to one decimal u64 build identifier
for both targets. Shared code reads it with
`application_revision(env!("RUVANE_APPLICATION_REVISION"))`. Wasm exports it through
`ruvane_application_revision() -> u64`. JavaScript reads this as BigInt. The
bootstrap supplies its decimal string to
`createRuntime(url, { applicationRevision: "..." })`. A mismatch rejects the module
before any view hydrates. The returned runtime exposes that string as
`applicationRevision`; repository demos can omit the option.

Automatic navigation and action requests carry `Ruvane-Application-Revision`.
The App router rejects a supplied stale revision with 409 before the handler.
Typed actions on this router require the header. Origin, Host, form and method
checks still apply first. Unversioned native POST forms remain available.
Direct GET inspection can omit the header. An older open tab after a dev rebuild
keeps its current page and receives a reload prompt; stale actions cannot mutate
the new application. Failed history pops use a document request at the target.

The revision detects mixed builds. It is public, not a secret, signature,
authorization token or content-integrity proof. Trusted application code still
controls its factory, validators and export expression.

## Development and build output

`ruvane dev` watches browser sources in addition to the existing bounded source
set. It compiles optimized Wasm, checks its size, copies it into a private run
directory and embeds that exact file in the native executable. Native dev builds
use the debug profile. Both steps receive the same revision. Watched source
changes during compilation reject the candidate; native compilation uses the
resolved lockfile with `--locked`. Cargo's initial lockfile creation is absorbed
after the Wasm step. External dependencies/framework checkouts are not watched or
snapshotted. These checks do not sandbox trusted Cargo builds.

Only a complete candidate stops the previous server. A Wasm or native compiler
failure keeps the old server and its module. Startup failure attempts to restart
the retained executable, which still embeds its old module. New alpha.33 starters
show compiler diagnostics and reload ready versions when input and actions are
safe. See [development feedback](development.md), including the update needed for
older starters. In-memory state resets on a process restart, including restoration.

```sh
ruvane build --offline
```

Interactive builds compile both targets in release mode. They use a separate
managed container from static exports and do not accept `--origin` in this
profile. A successful `dist/build-000001/` contains:

- `ruvane-app` (or `.exe`), embedding its module and browser resources;
- `resources/`, exported by that same executable, with the static manifest,
  completion marker and actual versioned assets;
- `Ruvane.runtime`, declaring the actual ABI and revision;
- `Ruvane.bundle`, recording the executable and resource-set metadata;
- `complete`, written before publishing the `dist/current` pointer.

The CLI validates the resource manifest, ABI/revision record and exact module
bytes before selecting the new build. Failures retain the previous selection.
Unknown files are not replaced or cleaned recursively. There are 128 retained
build slots per container. The manifest is not a cryptographic signature or a
power-loss durability guarantee.

Run the printed executable with `--port 3000`. It serves its embedded resources
and does not read the bundle's external copies. Interactive bundles are not static
sites and are not opened with `ruvane preview`; that command retains its static
export contract. The executable retains the loopback-only development HTTP
profile. This profile adds no TLS, public hosting, sessions or database. Alpha.20 adds
[explicit public navigation caching](navigation-cache.md); generated shared demo
pages opt in for 30 seconds and mark their links for prefetch. Remove those
declarations for personalized data.

## Immutable public build snapshots

Alpha.42 adds opt-in [client pre-rendering](client-prerender.md). New starters
include the project capability marker and native build hooks; older projects
keep their existing pipeline until explicitly migrated. Listed public URLs
serve embedded HTML/navigation snapshots for the matching revision, without
loaders. This remains a server bundle with dynamic routes and actions, not a
StaticSite export or a new ruvane preview mode.

Alpha.43 adds [on-demand regeneration](prerender-revalidation.md) for explicitly
listed public URLs. An opt-in policy loads fresh output on the first runtime
request and retains it until path/tag invalidation or cache eviction, using the
same build revision and an explicitly attached ServerCache handle.
