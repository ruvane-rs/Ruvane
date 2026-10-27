# File endpoints

Alpha.38 accepts server-only `route.rs` files beside file pages. The native
registry mounts them through the existing Router; requests never discover or
open source files. There are no third-party build, runtime or test dependencies.

```text
src/routes/api/health/route.rs
src/routes/actions/tasks/create/route.rs
src/routes/tasks/page.rs
src/routes/tasks/route.rs
```

Each endpoint exports `pub fn route() -> ruvane::FileEndpoint`:

```rust
pub fn route() -> ruvane::FileEndpoint {
    ruvane::FileEndpoint::new().get(|_| ruvane::Response::text("ok"))
}
```

The directory determines the URL: `api/health/route.rs` owns `/api/health`, and
`api/items/[id]/route.rs` owns `/api/items/{id}`. Fixed endpoint paths take
priority over dynamic endpoint paths. Ambiguous dynamic patterns are rejected.
A tree containing only endpoints is accepted. Endpoint folders do not need pages
or component factories.

## Methods

| Builder | HTTP methods | Input | Output |
|---|---|---|---|
| `get` | GET and HEAD | URI-mode query | `Response` |
| `get_form` | GET and HEAD | Form-mode query | `Response` |
| `post_form` | POST | Bounded URL-encoded form | `Response` |
| `post_action` | POST | Bounded URL-encoded form | `Result<ActionReply, ActionError>` |
| `post_action_response` | POST | Bounded URL-encoded form | Cookie-capable action `Response` |
| `post_json` | POST | Bounded UTF-8 JSON | `Response` |
| `put_json` | PUT | Bounded UTF-8 JSON | `Response` |
| `patch_json` | PATCH | Bounded UTF-8 JSON | `Response` |
| `delete` | DELETE | No body or media/encoding headers | `Response` |

One file can declare one handler per supported method. `get` and `get_form`
share a method slot; all POST declarations share the POST slot. Repeated
methods and empty declarations fail registration instead of replacing a handler.
HEAD follows the existing read-only GET behavior and suppresses response bytes.
URI queries preserve literal `+`; form queries decode it as a space.

```rust
use ruvane::{ActionData, ActionReply, ActionValue, FileEndpoint};
pub fn route() -> FileEndpoint {
    FileEndpoint::new().post_action(|request| {
        let fields = request.form_data().unwrap();
        let mut data = ActionData::default();
        if !fields.has_only(&["name"]) || fields.get("name").is_none_or(str::is_empty) {
            data.insert("message", ActionValue::Text("Enter a name.".into()))?;
            return Ok(ActionReply::validation(data));
        }
        data.insert("message", ActionValue::Text("Saved.".into()))?;
        Ok(ActionReply::success(data))
    })
}
```

Typed actions retain RVA1 replies, validation status 400, success status 200 and
fixed 500 responses on handler errors. When a client runtime is configured,
actions enforce its application revision before running the handler. Native
forms can use the existing internal 303 redirect and script-free HTML responses.

## Ownership and security

A page GET and endpoint POST may share a URL. An endpoint GET cannot overlap a
page or embedded asset GET, including a fixed/dynamic overlap. This keeps client
page ownership identical in native and Wasm registries while excluding handlers
from Wasm. Use a page declaration when an interactive page owns that URL.

Framework route prefixes remain reserved. Literal `_ruvane` and `ruvane-runtime`
prefixes are rejected. Dynamic endpoints cannot execute their handlers for those
raw prefixes; known framework routes retain their normal dispatch. Method
mismatches keep 405/Allow behavior. Alpha.51 adds an explicit
[JSON body profile](json.md); alpha.52 adds JSON PUT/PATCH and bodyless DELETE.

Mutation endpoints reuse the Router's mandatory Origin comparison, Fetch Metadata,
route-selected MIME, body budgets and absolute I/O deadlines. These checks do not authenticate
users. Each handler must validate fields and authorize its operation. Page loaders,
layouts, loading/error screens and not-found presentations are not endpoint guards
and do not wrap endpoint responses. GET/HEAD handlers must be read-only; all trusted
callbacks must be bounded and safe for concurrent calls. The framework cannot
interrupt arbitrary callback CPU or memory use. Native panics retain generic HTTP
errors rather than reflecting exception text.

Generated imports and registrations for `route.rs` use
`#[cfg(not(target_family = "wasm"))]`. Those module bodies, imports and handlers
are excluded from Wasm compilation. Shared application modules still need explicit
server/client separation; the generator does not analyze secrets or Rust bodies.
The starter also keeps its server helpers in a native-only `server` module.

## Registration, builds and limits

`FileRouteRegistry::endpoint(source, pattern, declaration)` registers against the
private App candidate. All method conflicts and the combined route budget must
pass before `finish` commits. Errors poison the batch even if ignored. Callbacks
are never executed by registration, validation, router construction or static
planning. The trusted `route()` declaration function does run natively and must
be bounded and deterministic.

All six special source kinds share the existing 128-file, 256 KiB/file, 4 MiB
combined source and 1 MiB generated-module limits. Each registered method
slot counts toward the combined 128 App routes with pages, assets and runtime
routes. Existing path, query, form, response and worker limits still apply.

Adding, changing, renaming or removing endpoints uses the normal protected dev
rebuild. Rust signature errors retain the previous native/Wasm pair. Native
registry conflicts fail before readiness and trigger executable rollback, with
relative source diagnostics. Production contains compiled handlers and does not
need the source tree. Static exports omit endpoints; `server_only_routes` lists
page names followed by endpoint patterns. An otherwise empty static export keeps
the existing empty-site error.

The file starter provides `/api/health`, `/stream`, typed task actions and native
task forms through route files. Its `main.rs` configures the server and runtime;
ordinary `--interactive` starters retain their explicit Router declarations.

## Cookies in typed actions

`FileEndpoint::post_action_response` accepts a trusted callback returning Response,
so `Response::action(&reply).with_cookie(cookie)` can emit typed action bytes and
cookie fields together. It keeps the same POST preflight, revision enforcement
and inherited/local guards as post_action. See [typed cookies](cookies.md).

Alpha.56 adds `post_multipart` in the same POST slot. Request::multipart separates
bounded text and one binary file; inherited session/CSRF/quota guards run before
the handler. See [native multipart forms](multipart.md).
