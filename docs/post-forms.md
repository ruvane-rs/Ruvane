# Server POST forms

Alpha.16 adds native forms and explicit Rust mutation handlers. The server stays
loopback-only and keeps its fixed script-blocking CSP. There is no browser runtime
needed for these forms. The existing [Wasm controls](client-forms.md) remain a
separate client-side API. Alpha.17 adds [Wasm actions](actions.md) with typed replies
and pending/completion callbacks; native forms still use their HTML/303 flow.

## Run the task example

```sh
cargo run --offline --locked --example server_forms
```

Open the printed `/tasks` URL on port 3002. Pass `0` to choose a free port and
press Enter to stop. The example creates, completes and removes tasks with native
POST forms. Successful submissions redirect to a GET page. Refreshing that page
keeps the task list without submitting it again.

Each server owns at most 32 tasks, with a mutex protecting concurrent changes.
Names are trimmed and limited to 80 UTF-8 bytes; notes to 160 bytes. Validation
errors return 400 with escaped field values and `aria-invalid`. A title containing
internal line breaks or tabs is cleared on error because it cannot be rendered
as a single-line input. Unknown fields and invalid checkbox values are rejected.
All visitors to that server share the list. State is lost when the process stops;
this is not a database, per-user storage or an authentication example.

## Build a native form

```rust
use ruvane::{InternalUrl, PostForm};

let form = PostForm::new(InternalUrl::new("/tasks").unwrap()).unwrap()
    .text("title", "Task name", "Write docs").unwrap()
    .textarea("notes", "Notes", "Review the API.\nAdd examples.").unwrap()
    .checkbox("done", "Completed", false).unwrap()
    .submit_label("Save task").unwrap();
let html = form.render();
```

Actions must be validated internal URLs without query parameters. Control names
are unique `[A-Za-z_][A-Za-z0-9_-]*` identifiers of at most 64 bytes. There are at
most 32 fields and 1024 UTF-8 bytes per value. `text`, `textarea` and `checkbox`
use wrapping labels. `hidden` adds plain escaped data, not trusted server state.
`invalid(name)` marks a declared visible field with `aria-invalid=true`.

The renderer fixes POST, URL-encoded enctype, UTF-8 and an unnamed submit button.
No arbitrary action override, enctype, script or event attribute is available.
`novalidate` leaves domain validation to Rust. `maxlength=1024` is browser guidance
in UTF-16 units and does not replace server byte limits. Textarea defaults preserve
leading LF through HTML parsing; values supplied to the builder use LF, not CRLF.

## Register a handler

```rust
use ruvane::{InternalUrl, Response, Router, StatusCode};

let mut router = Router::new();
router.post_form("/tasks", |request| {
    let fields = request.form_data().unwrap();
    let title = fields.get("title").unwrap_or("").trim();
    if !fields.has_only(&["title", "notes", "done"])
        || title.is_empty() || title.len() > 80
        || fields.checkbox("done").is_err()
    {
        return Response::text("Invalid task.\n").with_status(StatusCode::BadRequest);
    }
    Response::see_other(InternalUrl::new("/tasks").unwrap())
}).unwrap();
```

This snippet validates and redirects; application code supplies the state update.
The complete example in `examples/support/server_tasks.rs` includes that update
and the GET renderer. A handler runs only after transport and form parsing succeed.
It is trusted concurrent Rust code and must bound and synchronize its own work.

`FormData::get_as<T>` uses `FromStr`, distinguishing missing from invalid values.
`checkbox` accepts an absent field as false and exactly `on` as true. Other values
are invalid. `has_only` checks an application's declared names; it does not check
required fields or field types. Each handler must validate those rules.

GET/HEAD and POST registrations have independent namespaces and ambiguity checks.
The same path can render a page on GET and mutate on POST. Fixed routes win within
each method. Path captures use the existing URI rules. POST URL queries use URI
mode, and `query_params()` never merges with `form_data()`. Unknown actions return
404; a path registered only for another method returns 405. Allow lists the
registered methods, including explicit JSON PUT/PATCH and bodyless DELETE.
Unsupported known methods on an unknown path receive an empty Allow.

## Origin policy

Every `post_form` route requires a canonical `Origin` equal to the HTTP origin
of the request's server-validated Host. Host must already match the bound IP or
localhost and the actual listener port. Those aliases are distinct origins:
a localhost source cannot submit to a 127.0.0.1 target, or vice versa. Arbitrary
DNS hosts remain refused, including when Origin agrees with them.

Missing, null, malformed, multiple or different origins return 403. There is no
Referer fallback. `Sec-Fetch-Site`, when supplied, must be exactly `same-origin`;
its absence does not bypass the required Origin check. Forwarded headers cannot
change the target origin. This policy applies to the direct loopback HTTP server,
not to proxy or public HTTPS deployment. Canonical metadata configuration is
independent of this check.

The policy follows the source/target origin comparison described by
[OWASP](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html#using-standard-headers-to-verify-origin).
It constrains browser cross-origin submissions. It does not authenticate users,
authorize resources or stop a non-browser client from forging headers. Trusted
same-origin scripts can also submit. Add the opt-in [session CSRF guard](csrf.md)
to cookie-authenticated mutations; origin validation alone supplies no token.
A caller using a raw HTTP client must send the correct Origin.

## Body profile and failures

POST requires one decimal Content-Length and `application/x-www-form-urlencoded`,
optionally with a single `charset=UTF-8` parameter. MIME/charset spelling is case
insensitive; the charset value may be quoted. Other parameters, media types and
any Content-Encoding are rejected. Multipart, uploads, compression and chunked
request bodies are unsupported.

The default and hard maximum body size is 64 KiB, lowered through
`ServerConfig::max_request_body_bytes` (0..=65536). The request deadline covers
queue time, headers and the complete body together. Valid route, Host, origin,
media type and size checks happen before additional body reads. EOF before the
declared length returns 400; a deadline returns 408. Shutdown interrupts body I/O.
At most one request is processed; bytes beyond its declared body are discarded
when the connection closes, never dispatched as another request.

`FormData::parse` splits `&` and the first `=` before decoding, turns raw `+` into
space, and percent-decodes once. Raw bodies must be ASCII; non-ASCII data is
percent-encoded UTF-8. Empty names, bare fields, empty pairs, duplicate decoded
names and invalid UTF-8 fail. CRLF becomes LF in values, with a normalized limit
of 1024 bytes; LF and tabs are allowed, while lone CR and other controls fail.
The bounded decoder permits up to twice that size before CRLF normalization.
Names cannot contain controls. There is no nested-object or JSON interpretation.

| Failure | Status |
| --- | --- |
| Invalid Host, headers, encoding or truncated body | 400 |
| Untrusted or absent origin, inconsistent Fetch Metadata | 403 |
| Missing Content-Length on a registered form action | 411 |
| Body, normalized value or field count over budget | 413 |
| Unsupported MIME, charset, parameters or content encoding | 415 |
| Expect header | 417 |
| Transfer-Encoding | 501, or 400 with Content-Length |

Duplicate, comma-separated, signed and overflowing Content-Length values fail
before dispatch. The strict framing profile is described in [HTTP](http-profile.md)
and based on [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3).
Errors have fixed messages without reflected request data. Application validation
can instead return escaped HTML with 400, as the task example does.

`Response::see_other(InternalUrl)` is the only redirect constructor. It emits 303,
one validated Location and an empty body. Changing the response to another status
removes Location. Post/Redirect/Get prevents an ordinary page refresh from
repeating the POST; it does not deduplicate retries or guarantee exactly-once
application effects. Handler failures do not roll back arbitrary state changes.
