# File routes

Alpha.35 adds opt-in file routes with zero third-party dependencies. Cargo
generates Rust code once for each target; the native server and application
Wasm compile the same declarations. HTTP handlers never discover route files.

```sh
ruvane new my-app --interactive --file-routes --framework-path /absolute/path/to/Ruvane
cd my-app
ruvane dev --offline
```

Ordinary `--interactive` projects keep their explicit `pages.rs` declarations.
Existing projects are not rewritten. The new starter shares the existing
components, server actions, native forms, development feedback and CSS updates.

## Directory conventions

| Source | Interactive/server-only URL | Layout |
|---|---|---|
| `src/routes/page.rs` | `/` | Root layout, if present |
| `src/routes/tasks/page.rs` | `/tasks` | `tasks/layout.rs` and its ancestors |
| `src/routes/tasks/[id]/page.rs` | `/tasks/{id}` | Both task layouts, if present |
| `src/routes/tasks/settings/page.rs` | `/tasks/settings` | Task ancestors |

Each `page.rs` exports `pub fn page() -> ruvane::FilePage`. Each `layout.rs`
exports `pub fn layout() -> ruvane::FileLayout`. Rust checks visibility,
arguments and return types. The generator does not parse Rust function bodies.
Use `crate::` imports for application helpers outside the route tree.

```rust
use ruvane::{ActionData, ClientPageData, FilePage, InitialState, NavigationMatch, PageError};

fn state(route: &NavigationMatch) -> Result<InitialState, PageError> {
    let id = route.param("id").ok_or(PageError::BadRequest)?
        .parse::<u32>().map_err(|_| PageError::BadRequest)?;
    Ok(InitialState::Number(id))
}
pub fn page() -> FilePage {
    FilePage::client("task", 10, state, |context| {
        let id = context.param_as::<u32>("id").map_err(|_| PageError::BadRequest)?;
        Ok(ClientPageData::new(format!("Task {id}"), ActionData::default()))
    })
}
```

Component 10 and its data schema must exist in the application's shared factory.
The starter supplies a task component, loader and canonical numeric ID check.
Directory captures are strings; their domain validation stays explicit.

```rust
use ruvane::{ActionData, ClientLayoutData, FileLayout, InitialState, NavigationMatch, PageError};
fn state(_: &NavigationMatch) -> Result<InitialState, PageError> { Ok(InitialState::Number(0)) }
pub fn layout() -> FileLayout {
    FileLayout::client("workspace", 12, state, |_| {
        Ok(ClientLayoutData::new(ActionData::default()))
    })
}
```

The nearest directory layout is attached automatically, including the layout
beside a page. Parents register before children. Layouts without descendant
pages are rejected. The existing layout-depth and component-validation rules
still apply. Names such as `task` and `workspace` remain explicit; they are
validated in their existing App namespaces. Moving a file changes its URL while
retaining its declared name for `app.url("task", &[("id", "1")])`.

Fixed routes retain priority over captures. Ambiguous dynamic siblings such as
`tasks/[id]/page.rs` and `tasks/[slug]/page.rs`, repeated capture names and
reserved framework prefixes fail before generating code. Catch-all parameters,
route groups and additional special filenames are outside this first format.

## Native pages and existing App features

`FilePage::server(name, mode, render)` accepts the existing `PageMode` policies.
Static-capable native pages use directory URLs with a trailing slash, as
required by static export. A file at `journal/[slug]/page.rs` using
`PageMode::Static(...)` therefore owns `/journal/{slug}/`. Native server-only
pages and interactive pages use the slashless patterns shown above.

`FileLayout::server(name, render)` creates a native fragment layout. Native and
client layout kinds cannot be mixed within an inherited chain. Native-only
pages remain script-free. Static rendering still requires explicitly enumerated
parameter values; route discovery does not infer content or execute loaders.

After generated registration, ordinary App APIs configure named routes:
`client_page_cache`, `client_layout_cache`, boundaries and streaming remain
available. Component IDs, props, action authorization and loader data schemas
are application-owned. File discovery grants no additional capabilities.

## Build integration

The starter adds only Ruvane itself as a first-party build dependency. Its
`build.rs` watches `src/routes` and generates a module under Cargo's `OUT_DIR`:

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src/routes");
    println!("cargo:rerun-if-env-changed=RUVANE_CONTENT_REVISION");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    ruvane::FileRoutes::read_dir("src/routes")?.write_module(output.join("ruvane-routes.rs"))?;
    Ok(())
}
```

The shared application includes that module and calls its generated
`register(&mut app)` after configuring the component factory. This works in
existing applications too; merge the build step with their current build script
and add the `ruvane` build dependency. No new application is required.

`FileRouteRegistry` builds a private App clone. Successful completion validates
and commits the batch. An error poisons the batch, so even an ignored method
error cannot commit partial routes. Registration runs no loader or renderer;
the application's `page()` and `layout()` declaration functions do run and
must be bounded and deterministic across targets.

Adding, renaming or removing a source triggers the normal protected dev rebuild.
Invalid trees and Rust signatures keep the previous native/Wasm pair. Duplicate
declaration names and other App errors are detected while constructing the
candidate application, before readiness; startup failure attempts to restore
the previous executable. Diagnostics identify the relative source file.

Production bundles contain compiled routes and their matched Wasm. The source
directory and Cargo output modules are not required at runtime or exposed by
the HTTP router. The generated native and Wasm modules use the same source
tree; target-dependent declaration code remains the author's responsibility.

## Bounds and trust

- Only `page.rs`, `layout.rs` and route directories are accepted in the tree.
- Literal directories use the portable lowercase ASCII path profile. Dynamic
  names use lowercase ASCII letters, digits and underscores, beginning with a
  letter or underscore. Literal and parameter segments are limited to 64 bytes.
- The scan limits directories to 128, entries to 512, depth to 16 and relative
  directory paths to 512 bytes. At most 128 page/layout sources are accepted,
  including at most the existing 32 App layouts.
- A source file is at most 256 KiB of UTF-8; the total is at most 4 MiB. Generated
  Rust is at most 1 MiB. Existing App, static-export and runtime quotas also apply.
- Observed symlinks, nonregular files, parent traversal and invalid source names
  are refused. Generated module paths are escaped Rust string literals, with
  fixed internal module identifiers rather than filesystem-derived Rust names.
- Publication replaces only a regular module with the recognized format marker,
  using a new temporary file and atomic rename. Unknown output files are retained.

Route source is trusted executable Rust. This is not a sandbox, Rust parser,
filesystem mount or HTTP-to-source dispatch mechanism. Rust module bodies can
import other application code. Source/output directories must remain protected
from hostile concurrent writers; preflight filesystem checks are not an atomic
filesystem security boundary. Application callbacks retain their usual bounds
and origin/authorization responsibilities.

Native tests cover discovery, publication, inheritance and failed-batch rollback.
CLI tests compile real projects in quoted paths and exercise route lifecycle and
dev recovery. Node/Wasm tests check ownership before and after file additions
and removals. No third-party build or test dependencies are used.
