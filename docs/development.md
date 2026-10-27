# Browser development feedback

Alpha.33 adds compiler diagnostics and guarded full-document reloads to new
interactive starters. No Cargo or JavaScript dependencies are added. The native
and Wasm code still share ABI v14 and 22 exports. Production bundles keep their
four versioned runtime assets.

Alpha.34 adds CSS updates without compilation or server restart. New interactive
starters declare `/assets/site.css` and its source `assets/site.css`. The CLI
learns these declarations from a bounded `RVDS1` record written before server
readiness. Projects without this record retain the normal rebuild path.

## CSS updates

When every changed file is a declared stylesheet and all other watched sources
match the active executable, the CLI copies the declared CSS into immutable
session files and publishes an `RVD2` status. Cargo does not run. The application
revision, server memory, document and Wasm components stay in place, including
drafts and pending actions. CSS edits alongside uncompiled Rust changes take
the normal build path; a failed build keeps the previous styles and application.

The browser stages matching stylesheet links beside their originals, with
nonmatching media while loading. It replaces the originals only after every
replacement loads. Cascade order, media and IDs are retained. Relative image,
font and import URLs keep their original CSS directory because the stylesheet
URL only receives a version query. Stylesheet source text is trusted CSS, with
the same CSP restrictions as before; this feature does not parse CSS syntax.

A failed load or three-second deadline removes the staged links and keeps the
working styles. The panel reports the failure and retries with bounded backoff.
Newer generations retire pending replacements. Hidden or departed documents
cancel staged loads. Links removed or changed while loading are not committed.
Only same-origin, plain declared URLs and previous development version URLs are
eligible. Disabled, integrity-protected and custom load-handler links retain
their originals. A stylesheet URL without an active link needs no DOM change.

Declarations require embedded CSS assets and project-relative files under
`assets/`. They reject duplicates, nonportable paths and more than 16 entries.
Each CSS file is limited to 1 MiB and a complete generation to 8 MiB. At most
eight generations are retained in the session directory, for at most 64 MiB of
snapshot files. Older version requests fail after retirement rather than return
different bytes. Only known owned files are removed at shutdown.

The existing asset URL serves these snapshots only in development, using
`?ruvane-css=SESSION-APPLICATION-GENERATION`; public URLs never reveal source or
filesystem paths. Reads recheck real directories, regular files and byte limits.
Origin checks, no-store and nosniff remain in force. Snapshot paths are built
from validated integers and the registered asset index. Project directories must
remain protected from hostile concurrent writers, as for other CLI operations.
Native-only documents remain script-free and pick up current CSS on refresh.

To add another stylesheet, register its embedded asset and document link, then
add `.stylesheet("/assets/theme.css", "assets/theme.css")?` to the development
configuration. Changing declarations or adding/removing source files rebuilds
the application. Source files used as other compile-time inputs must use the
normal rebuild path rather than be declared for CSS updates.

Run `ruvane dev --offline` in a generated interactive project and open the printed
HTTP address. A temporary page is available before the first compile finishes,
including when that compile fails. The previous application continues serving
while a replacement compiles. A successful build only becomes ready after its
server has bound the expected port and written its readiness record. A failed
startup attempts to restore the previous executable. A crashed server shows the
waiting page and requires a source change before rebuilding.

## Reload behavior

The browser polls a same-origin development endpoint. A new ready revision
triggers a full page reload after activation, with no pending navigation or
mutation, uncertain action outcome, native form submission, modified field,
contenteditable input or IME composition. Hidden documents pause polling.
Checks run again immediately before reloading. Ordinary counters and other
in-memory state reset on reload. Server memory resets on a process restart.
This release does not preserve component state across code changes.

When reload is blocked, the panel shows **New version available** with an explicit
**Reload** button. Unsaved changes are discarded by that choice. The button is
disabled during a pending action or native submission. An uncertain action is
never replayed. A native submission remains conservatively blocked until the
document leaves; a cancelled submission can require manual browser navigation.
Field values stay in the document and are never sent to the development server.

Compilation errors appear as bounded plain text in a nonmodal panel. It does not
take focus or replace application content. Saving a fix starts another build.
Temporary restart gaps use bounded fetch deadlines and backoff. Malformed status,
regressing sequence numbers and another session cannot trigger automatic reload.

## Boundaries

The CLI owns a unique `.ruvane/dev-PID-SESSION/` directory. Atomic file replacement
publishes a status record of at most 32 KiB, including at most 24 KiB of compiler
output. The stderr tail is bounded in memory and mirrored to the terminal;
compiler colors are disabled and diagnostic controls are removed. A descendant
that retains stderr can temporarily prevent another compile rather than create
unlimited output-reader threads. Cargo projects and their build scripts remain
trusted code; the CLI only manages direct children.

The development routes are `/_ruvane/dev/status`, `/_ruvane/dev/client.mjs` and
`/_ruvane/dev/style.css`. Conflicting routes are rejected. Status reads recheck
absolute paths, real directories, regular files, UTF-8, size, session and schema.
Responses use no-store and nosniff. Diagnostics require a session header and
`Accept: text/plain`; cross-origin Origin/Fetch Metadata values are rejected.
There is no CORS allowance. The session is a public stale-tab check, not a secret
or authentication against local programs. Do not put secrets in compiler output.

The existing loopback Host checks, request limits and CSP remain in force. All
scripts and styles are external same-origin assets. `?native=yes` keeps its
script-free response policy, including the waiting page. Standalone native/static
routes retain their existing behavior and require manual refresh.

Generated native code uses `option_env!("RUVANE_DEV_STATUS")` and
`option_env!("RUVANE_DEV_SESSION")`, set only by interactive `dev`. Cargo `build`
explicitly removes both inherited variables for native and Wasm compilation.
Production startup cannot enable development support through environment values.
`ClientRuntime::development(Development::new(path, session)?)` is available for
trusted custom local tooling; production applications must leave it unset.

Projects generated before alpha.33 need the compile-time configuration block
from the current interactive [src/main.rs template](../src/bin/cli/templates/interactive/main.rs.txt)
to use browser feedback. For CSS updates, existing projects also need the
stylesheet declarations and the managed-startup `.styles` readiness record
shown in that template. An alpha.33 starter without that record still rebuilds
normally after CSS edits.
Updating the framework alone does not rewrite an existing application's source.

## Validation

`cargo test --workspace --offline --locked --all-targets` covers first-build
errors, old-server retention, readiness, startup rollback, production isolation,
path/file/symlink validation, origin and session checks, CSP and native opt-out.
It also checks consecutive CSS generations, retired snapshots, unchanged
application revisions, retained server data and mixed-edit rebuild failures.
`node --test browser/tests/development.test.mjs` checks parsing, bounded reads,
reload safety, escaped diagnostics, CSS load/rollback, backoff, visibility and disposal using only
Node's built-in test runner. Browser validation also exercises a generated dev
server across real source edits and a complete production build.
