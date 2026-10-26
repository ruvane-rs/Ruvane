# Interactive client pre-rendering

Alpha.42 freezes explicitly enumerated public interactive pages during a
production build. A row contains complete buffered HTML, the initial hydration
snapshot, title, metadata, layout/page seeds and native fallback. Navigation and
HTML delivery use that same immutable version without executing loaders.

This produces a hybrid native server bundle. Dynamic pages and endpoints/actions
retain their normal behavior. It is separate from StaticSite export and its
trailing-slash filesystem mapping; interactive URLs keep their declared paths.
There are no new dependencies, browser runtime assets or Wasm exports.

## Declare public URLs

After registering a client page, enumerate its complete parameter sets:

```rust,no_run
use ruvane::{App, PageParams};
fn configure(app: &mut App) -> Result<(), ruvane::AppError> {
    app.client_page_prerender("article", Some(vec![
        PageParams::new(&[("slug", "hello")])?,
        PageParams::new(&[("slug", "typed-urls")])?,
    ]))
}
```

A fixed route needs exactly one empty `PageParams::default()`. None disables
the declaration. Lists must be nonempty, complete, unique and within the global
128-URL budget. Existing route ownership, parameter and URL checks apply;
shadowed URLs and duplicates are refused atomically, without executing loaders.
Changing declarations clears any attached plan and requires rebuilding it.

File routes use `FilePage::client(...).prerender(vec![...])`. Directory discovery
does not infer parameters, crawl URLs or run loaders. Native-only FilePage
declarations cannot use this option. A page cannot combine pre-rendering with
an enabled server data cache policy, including background revalidation.
Browser navigation caching remains independently configurable.

The declaration asserts that the entire branch, metadata and native fallback
are public and safe to distribute in the executable and artifact. Keep private
or identity-dependent content dynamic. Public immutable snapshots are not an
authorization mechanism or a live view of mutable data.

## Build and runtime

New interactive starters include `Ruvane.prerender`, a text capability marker,
and the generated build/main hooks. `ruvane build --offline`:

1. Builds the shared Wasm and native bootstrap with one application revision.
2. Runs only the enumerated page/layout loaders to prepare a complete plan.
3. Validates its bounds, complete snapshots and matching revision.
4. For a nonempty plan, recompiles the native executable with embedded bytes.
5. Verifies the final executable returns exactly those embedded bytes before
   publishing its completed bundle and changing `dist/current`.

An empty plan needs no second native compilation. The bundle contains a binary
`Ruvane.prerender` audit copy and records its size in `Ruvane.bundle`; the project
root file with the same name is the text build-capability marker. Runtime serves
embedded bytes and never reads this audit copy, route sources or Cargo output.
The four runtime assets remain app.wasm, bridge.mjs, navigation.mjs and
application.mjs. The native recompilation uses the original Wasm and revision.

Failure during loading, artifact checking, recompilation or final verification
does not replace the previous completed build. No-store from a page or layout,
400/404/500 results, panics, invalid seeds, foreign canonicals, inconsistent
custom document heads and budget violations abort the plan. Partial output
must not be deployed. Framework preparation invokes no POST/action dispatcher;
trusted callbacks themselves are not sandboxed and must remain read-only.

Build context is a synthetic GET with the validated path/captures, no query,
headers or body, `PageContext::is_exporting() == true`, and the configured public
metadata origin. A request object exists for compatibility with client loaders.
Do not derive public build content from a Host header, current identity or an
unkeyed request property. Supply public local/build-time content explicitly.

## Request selection

| Request | Behavior |
|---|---|
| Anonymous query-free GET/HEAD for a listed URL | Serve frozen output, without loaders. |
| Typed navigation to a listed public URL | Return its complete matching navigation packet. |
| Progressive navigation to a listed public URL | Return the buffered packet; no loader or partial stream. |
| Cookie or Authorization, including empty headers | Bypass frozen output and use dynamic rendering. |
| Any query, including an empty query or native=yes | Use the existing dynamic route. |
| Unlisted valid dynamic parameters | Use dynamic rendering; enumeration does not imply a 404. |

Frozen HTML embeds the final RVI1 snapshot, so initial hydration makes no extra
page GET. Wasm validates the same route/component/data declarations. Later
navigation retains matching parent layouts and local state as before. HEAD
suppresses the body through the existing server serializer. HTML keeps its
interactive CSP and no-store policy. Typed navigation honors its explicitly
declared page/layout browser cache policy intersection; revision guards still
reject stale clients.

Development intentionally stays dynamic. A production router with pre-render
declarations refuses to start without a matching attached artifact. These
snapshots are frozen for their build revision: a mutation, navigator.refresh,
cache invalidation or background worker does not rewrite them. Rebuild to change
them. Revocation/private variants must use dynamic rendering and normal access
checks. There is no persistent/distributed regeneration in this version.

## Programmatic use and trust

`App::client_prerender_plan()` produces a `ClientPrerender` after runtime/assets
are attached. `encode`, `decode`, `revision`, `len`, `is_empty`, `urls` and
`write_new` support trusted build tooling. `App::client_prerendered(&'static [u8])`
attaches embedded output after declarations/runtime configuration. It checks
revision, exact ordered URL coverage, ownership, props and component seed schemas.
It never executes page/layout loaders. Router construction rechecks attachment.

RPR1 stores a revision, bounded named URLs, RVP packets, UTF-8 HTML and a checksum.
The decoder rejects truncated/trailing frames, duplicates, queries, partial or
error packets, mismatched embedded snapshots and corruption. The checksum is
not cryptographic authentication. HTML is trusted compiled application output;
these APIs must not accept uploads or untrusted external HTML. Do not turn
untrusted bytes into static storage to attach them. The CLI embeds only the
artifact produced and verified during that same trusted build.

Budgets are 128 concrete URLs, 1 MiB per HTML document, the existing 64 KiB
navigation-packet limit and 16 MiB for the whole encoded artifact. They bound
output, not callback CPU/allocations or exact allocator overhead. No additional
files are exposed over HTTP by attaching an artifact.

## Older projects

Projects without the capability marker retain their existing build pipeline.
They are not silently rewritten. To adopt pre-rendering, copy the current
starter's build/main hooks: invoke `ClientPrerender::write_build_module()` from
the native package build script, include OUT_DIR/ruvane-prerender.rs, attach
nonempty embedded bytes, and implement the two pre-render export/verify modes.
The core build dependency remains first-party. Add the recognized project
marker `ruvane-prerender`, tab, `1`, newline. Keep the bootstrap/production
artifact guard from the starter. Wasm build-module output is always empty;
native pre-rendered HTML is not embedded in the browser module.

RUVANE_PRERENDER_FILE and RUVANE_PRERENDER_BOOTSTRAP are CLI-managed build inputs;
normal/development/Wasm builds clear them. Build output paths must be trusted
real directories. Unknown output modules, symlinks and overwrites of arbitrary
files are refused. This is not a filesystem execution sandbox or an atomic
boundary against hostile concurrent writers.
