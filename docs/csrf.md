# Session CSRF tokens

CSRF protection is opt-in. Add it to cookie-authenticated mutations in addition
to resource authorization and the existing Host/Origin, MIME, body and application
revision checks. The synchronizer token is independent of the HttpOnly session
identifier. Never expose that identifier as a CSRF token.

## Native setup

Keep one shared `SessionStore` in native application state. Register a GET endpoint
using `FileEndpoint::get` or `Router::get`:

```rust,no_run
use ruvane::{Router, SessionStore};
fn token_route(router: &mut Router, store: SessionStore) {
    router.get("/csrf-token", move |request| store.csrf_response(request)).unwrap();
}
```

The response is status 200 with `Content-Type: application/vnd.ruvane.csrf`,
`Content-Length: 0`, `Cache-Control: no-store` and, for a live session, one
`Ruvane-Csrf-Token` header. Anonymous requests receive the same empty response
without a token. Store or entropy failures return 503 without a token.
The endpoint needs no CORS permissions and must not redirect or be publicly cached.
Its path must fall within the configured session cookie's path scope.

Use `store.csrf_guard()` on protected App branches or native `guard.rs` endpoints.
GET/HEAD require a live session. POST, PUT, PATCH and DELETE additionally require its current CSRF token.
JSON mutations and bodyless DELETE use the dedicated header.
Accepted requests are Private; invalid claims return 403 and store failure returns
503. Guards run before loaders, page caches and endpoint handlers. Manually added
Router handlers are outside App guards: explicitly call `verify_csrf(request)`
before any mutation there and map errors to 503, false to 403.

Obtain a token from a live native handle with `session.csrf_token()?`. Render it
through `PostForm::csrf(token)`:

```rust
use ruvane::{CsrfToken, InternalUrl, PostForm};
fn form(token: CsrfToken) -> Result<ruvane::Html, ruvane::FormError> {
    Ok(PostForm::new(InternalUrl::new("/account/save").unwrap())?
        .csrf(token)?
        .text("name", "Name", "")?
        .submit_label("Save")?
        .render())
}
```

The reserved `_ruvane_csrf` field is separate from business fields: `get`, `iter`,
`len`, `is_empty` and `has_only` inspect only business data. Up to 32 business fields
plus one security field are allowed within the existing total body limit. A repeated
security field, including an encoded alias, returns 400 before guards. Supplying
both the header and hidden field is ambiguous and fails verification even if equal.
Malformed form encoding retains its existing 400/413 behavior.

`FormData::field` and ordinary `PostForm` controls reject the reserved name. Parsed
submissions containing it cannot be re-encoded as outgoing `ActionRequest` data;
actions use the dedicated header instead. `Request::csrf_token()` validates only
syntax and source ambiguity. Only the session store verifies ownership. Query
parameters and route captures never supply a CSRF claim.

## Wasm actions

Configure the native runtime with an internal path without a query:

```rust,no_run
use ruvane::{ClientRuntime, InternalUrl};
fn configure(runtime: ClientRuntime) -> Result<ClientRuntime, ruvane::AppError> {
    runtime.csrf_endpoint(InternalUrl::new("/csrf-token").unwrap())
}
```

Buffered and streamed documents emit only the endpoint path in `data-ruvane-csrf`.
The application bootstrap forwards it to `createRuntime`. Direct bridge users can
pass `{ applicationRevision, csrfEndpoint: "/csrf-token" }`. Existing applications
without this setting keep their previous transport and must not assume CSRF tokens
are automatically enforced.

Before each action POST, the bridge fetches that path with same-origin credentials,
same-origin mode, redirects disabled and `cache: "no-store"`. It validates status,
MIME, no-store, the zero length, actual empty body and optional canonical token
header. It then sends the token only as `Ruvane-Csrf-Token` on the action request.
One abort controller and deadline cover both requests. A failed or timed-out token
fetch sends no POST, returns a rejected action and does not set the uncertain
mutation flag. Failures after sending a POST retain the existing conservative
uncertainty behavior.

An anonymous token response permits a public action to proceed without the header.
A protected action still rejects it. The fixture's public demo-session creation is
not a login design. Real login flows need their own authorization and CSRF policy,
including an anonymous session when using synchronizer tokens before login.

## Lifetime and limits

Issuance is lazy and uses 32 OS-random bytes, rendered as 64 lowercase hexadecimal
characters. Zero and collisions with live session IDs or tokens are retried up to
four times; there is no fallback random generator. A token may be reused by forms
or tabs while its session generation remains live. Valid verification refreshes
idle lifetime; rejected verification and native-handle token reads do not. Token
endpoint lookup refreshes idle lifetime like ordinary session lookup. Other guards
or application lookups can independently refresh it.

Rotation replaces an issued token and identifier together without extending the
absolute deadline. Entropy failure preserves the old generation. Old native forms
return 403 after rotation; load a fresh document before submitting again. Wasm
actions fetch again for each submission. Concurrent rotation can still retire a
token between fetch and POST: the server denies it, and the runtime never retries
a mutation automatically. Logout, expiry and `clear` remove token state.

Token storage is bounded by the session entry limit, with one token and one keyed
lookup index per issued live session. `SessionStats::data_bytes` measures application
payload only, excluding this fixed per-entry overhead. The index uses standard
library keyed hashing; this release adds no cryptography implementation or Cargo
dependency. Session backend and entropy source remain absent from Wasm.

## Handling secrets

Tokens appear intentionally in native hidden fields and same-origin HTTP headers.
Keep token-bearing forms private and uncached. Do not place them in static exports,
pre-render artifacts, retained public shells, URLs, analytics or application logs.
The framework does not inspect arbitrary HTML for sensitive values. Native guards
do not make an exported artifact private.

`CsrfToken`, requests, parsed forms and token responses redact security values in
Debug; errors contain no submitted value. Explicit `as_str()` and wire serialization
expose the token by design. Tokens stay outside page seeds, navigation packets,
snapshots and action command bodies. Protected process memory and same-origin
JavaScript remain trusted. CSRF tokens do not prevent XSS, authenticate users or
replace production TLS. See the
[OWASP CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).
