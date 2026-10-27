# Early shell hydration

Alpha.32 lets marked independent shell views activate while an initial App page
is still streaming. It is opt-in. Page and layout ownership remain separate.

```rust,ignore
let props = ComponentProps::new(1, InitialState::Number(5))?;
let toolbar = create_component(props)?.view()?.shell_island("toolbar", props)?;
// Place toolbar in the common shell, outside the page outlet.

app.client_page_document_streaming(
    "project",
    Some(|_| Ok(ClientDocument::new("Project").hydrate_shell(true))),
)?;
```

`hydrate_shell` and `hydrate_layouts` are independent. Enable both to activate
layouts as well. With shell hydration alone, layouts and the leaf stay inert
until the complete branch validates. Existing documents keep their old timing.
An unmarked `hydratable` shell view also waits for final activation.

## Markers and validation

`View::shell_island` wraps ordinary versioned component props and disabled server
HTML. It emits a named wrapper followed by an empty hidden `br` marker. The
marker follows the closed view and wrapper, so a partially received view cannot
activate. Names use ASCII letters, digits and underscores, start with a letter
or underscore, and have at most 32 bytes.

The server declares the ordered island names on the document root. It rejects
duplicate names or more than eight islands before deferred loading. Islands
can be in the shell prefix or suffix. A footer island waits for its own complete
HTML. Prefix, chunk, body and deadline budgets still apply.

The browser checks the revision, destination, declared names, marker shape,
wrapper position, component props and exact view schema before enabling controls.
Islands must be outside the outlet and cannot nest. Each root has one runtime
owner. A malformed later island retires previously activated shell and layout
owners; it cannot leave a partially accepted document interactive.

The existing first-party registry reconstructs each independent component from
its number/boolean props. Arbitrary page seeds are not supported by this API.
No loader is rerun for shell hydration. No extra page GET is made.

## Local events and actions

Counters, menus, validation and text editing work after the island validates.
Input values, focus and UTF-16 selection survive final adoption. The final branch
keeps the existing DOM and controllers instead of hydrating the islands again.

Early shell owners have the same action gate as early layouts. An attempted
server action is rejected locally, with no POST, mutation epoch or queued retry.
An initial action or an action caused by input adoption rejects activation.
A component cannot loop by immediately requesting another action on rejection.

Only a validated final page, completion marker, parsed document and successful
navigation commit release the shell gates. A typed error handled by a local or
App boundary can commit too. The independent shell survives branch retirement
and explicit GET retry. No POST is replayed at completion or recovery.

History refusal, a dirty native fallback, invalid metadata, a changed destination,
removed or moved roots, lost owners, cancellation and deadline expiry refuse the
handoff. Retirement removes listeners and owners while retaining received HTML
and field values. Controls become inert; native links and forms remain available.
Page departure disposes the runtime. Shell state is local to this document.

## Custom bootstrap

`createDocumentHydration` owns the early islands until `commit` or `retire`.
Its `ready` result includes `snapshot`, attested `parents`, `shells` and `accept`.
Each frozen shell entry has `root` and `controller` fields. Preserve the `accept`
guard when composing a native-form guard. A final guard also runs after history
before the branch changes. Shell owners, action state and branch DOM are checked
again after that guard returns.

Pass the ready result to `navigator.start`, then commit the session only on
success. Retire on every other outcome. After commit, independent shell controllers
belong to the caller/runtime; navigation owns only the page and layout branch.
The standard bootstrap skips those shell roots during ordinary hydration and
uses runtime disposal on page departure. Custom callers must dispose their shell
controllers or runtime too. The bridge validates controller identity before retirement and rechecks DOM
ownership before releasing a shell gate.

The profile still uses ABI v14, 22 Wasm exports and four runtime assets. The shell
adds bounded HTML metadata and first-party JavaScript lifecycle methods, without
a new binary packet format or external dependency.

## Demo and verification

The generated **Interactive shell** link opens `/tasks/shell?delay=long`.
The page takes three seconds. Increment the shell counter, edit **Shell draft**
and **Shell notes**, or open **Show tools** while waiting. Their state survives
completion. Add `&layouts=yes` to activate layouts early as well. The global-error
demos also enable early shell hydration.

Native tests cover markers, name bounds, prefix/suffix delivery, independent
options, lazy HEAD/native SSR, duplicate declarations and transport budgets.
`web/shell-hydration-tests.mjs` checks gated real bootstrap delivery, action gates,
field state, typed errors, retry/navigation, malformed metadata, foreign owners,
history rollback, cancellation, expiry, budgets and 100 cleanup cycles per profile.
Isolated fixture routes and assets are removed from generated sources afterward.
