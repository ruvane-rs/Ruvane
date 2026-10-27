# Typed HTTP cookies

Ruvane validates request cookies before routing, guards or loaders and writes
response cookies through a bounded API. It uses only std. Cookies are untrusted
application data; this API does not sign, encrypt or authenticate them.

## Reading

```rust
fn preference(request: &ruvane::Request) -> &str {
    match request.cookies().get("preference") {
        Some("dark") => "dark",
        _ => "light",
    }
}
```

`Request::cookies()` returns validated `Cookies` with `get`, `iter`, `len` and
`is_empty`. Names are case-sensitive. Values are never percent-decoded or treated
as form data: `+`, `%2F` and `=` remain literal bytes. Matching outer double quotes
are removed once; escapes and internal quotes are rejected.

Malformed pairs, invalid octets, duplicate names (including cookies from different
paths) and repeated Cookie headers fail with 400 before callbacks. Request count
or header-size overflow returns 431. A present but empty Cookie header still
bypasses public page caches. `Cookies::parse` exposes the same parser independently.

## Writing and removal

```rust
use ruvane::{Cookie, Response, SameSite};

let cookie = Cookie::new("preference", "dark")?
    .same_site(SameSite::Strict)?
    .max_age(3600)?;
let response = Response::text("saved").with_cookie(cookie)?;
# Ok::<(), ruvane::CookieError>(())
```

`Cookie::new` defaults to Secure, HttpOnly, SameSite=Lax, Path=/ and no explicit
lifetime. It is always host-only: there is no Domain attribute. Supported setters
are `path`, `http_only`, `same_site` and `max_age`. Invalid attribute combinations
return `CookieError`; no raw header or extension-attribute API is provided.

Names must be nonempty HTTP tokens. Outgoing values use the cookie-octet ASCII
profile without quotes, escaping or encoding. Paths must be bounded internal
paths without queries, fragments, delimiters or controls. The API enforces
`__Secure-`, `__Host-`, `__Http-` and `__Host-Http-` prefix requirements and requires
Secure for SameSite=None. Browser support for prefixes varies; server validation
is unconditional. The prefixes do not turn a request cookie into verified identity.

The current server supports loopback HTTP, not HTTPS. For that profile, explicitly
use `Cookie::local_http(name, value)` to omit Secure. HttpOnly and SameSite=Lax
remain enabled. Secure/prefix requirements are not silently relaxed. This local
constructor does not enable public binding, TLS or reverse-proxy deployment.

```rust
let cookie = ruvane::Cookie::local_http("preference", "dark")?
    .path("/settings")?;
let removal = cookie.removal();
let response = ruvane::Response::see_other(
    ruvane::InternalUrl::new("/settings")?
).with_cookie(removal)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`removal()` preserves the name, exact path and flags, clears the value and emits
Max-Age=0. Reconstruct the same scope when removing a cookie on a later request;
request cookies do not carry their original Path or other response attributes.
There is no Expires, Domain, Partitioned or arbitrary extension support in this
profile. No explicit Max-Age means a browser session cookie, not a framework login
session or a promise that the browser deletes it on restart.

`Response::add_cookie` appends atomically; `with_cookie` is its consuming builder.
`Response::cookies()` inspects the ordered declarations. Each cookie becomes a
separate Set-Cookie field, including on redirects, errors and HEAD. The body and
its length do not change. Duplicate name/path pairs are refused within a response;
a new response can replace or remove a previously stored cookie normally.

## Native forms and typed actions

Return `Response::see_other(url).with_cookie(cookie)` from a native POST form to
preserve the existing origin, MIME and body checks and internal 303 behavior.

For typed replies with cookies, use `Router::post_action_response` or the matching
`FileEndpoint::post_action_response` builder:

```rust
fn route() -> ruvane::FileEndpoint {
    ruvane::FileEndpoint::new().post_action_response(|_| {
        let reply = ruvane::ActionReply::success(ruvane::ActionData::default());
        ruvane::Response::action(&reply)
            .with_cookie(ruvane::Cookie::local_http("preference", "dark").unwrap())
            .unwrap()
    })
}
```

These action routes retain revision enforcement when mounted in an interactive
App. Return `Response::action` for typed success/validation bodies. Callbacks are
trusted; the browser rejects incompatible MIME, status or reply bytes. Existing
`post_action` remains the simpler ActionReply API. Endpoint guards still run first.
Cookie fields are HTTP metadata, never added to RVA1/RVP/RVI payloads or Wasm props.
Applications may explicitly send a public preference in their typed page data.

## Caches and browser behavior

Every response with cookies is no-store and emits the fixed non-sensitive header
`Ruvane-Cache-Reset: cookies`. JavaScript cannot inspect Set-Cookie, so the navigator
uses this signal to clear all byte caches and fence concurrent prefetch work.
A cookie-changing prefetch is discarded and cancels other queued/running jobs.
A current navigation can still commit its validated response, rebuilding its
layouts rather than retaining the earlier branch. Invalid bodies still clear
caches when the reset signal arrives. Cookies must be attached before streaming
starts; producers cannot add response headers later.

Framework actions already invalidate caches at start and completion, including
cookie-setting actions. Cookie/Authorization request headers and Private guards
continue to bypass native page caches, frozen pre-render output and persistent
restoration. Headers/cookies are stripped from sanitized public refresh requests.

This signal covers responses processed by Ruvane's navigator. Cookies changed in
other tabs, by document.cookie, or through custom fetch code are not observable
here; explicitly invalidate or reload as needed. Protected routes must still run
server guards. Pre-render declarations remain public artifacts and must not contain
private cookie-dependent content.

## Budgets and security

| Budget | Limit |
| --- | --- |
| Request Cookie header | 4096 bytes |
| Request pairs | 32 |
| Name / value | 128 / 2048 bytes |
| Path | 1024 bytes |
| Complete Set-Cookie line | 4096 bytes |
| Response fields / combined line bytes | 8 / 8192 |
| Explicit lifetime | 365 days |

Cookie/Cookies Debug output omits names and values. Request Debug redacts Cookie
and Authorization headers. Error messages contain no input data. Getters and
`Response::cookies()` still expose values intentionally; do not log them.
Cookies are scoped by host/path, not port, including on loopback. Origin checks,
HttpOnly, SameSite and Secure do not provide application authentication or replace
a verified session/CSRF design.

References: [RFC 6265](https://httpwg.org/specs/rfc6265.html) and
[Set-Cookie attributes and prefixes](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie).
