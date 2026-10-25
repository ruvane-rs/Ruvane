# Native GET forms

Ruvane 0.1.0-alpha.5 adds typed native forms for read-only searches and filters.
It uses Rust's standard library only, with no JavaScript runtime or third-party
dependencies. The controls submit using the browser's normal GET behavior.

## Construction and submission

```rust
use ruvane::{GetForm, InternalUrl};

let form = GetForm::new(InternalUrl::new("/articles").unwrap()).unwrap()
    .search("search", "Search articles", "Rust + HTML").unwrap()
    .hidden("page", "1").unwrap()
    .submit_label("Search").unwrap();
let html = form.render();
```

`GetForm` can also be passed directly to an element or `Document` as a child.
It emits a fixed `method="get"`, internal action, and `accept-charset="UTF-8"`.
Each search input has a visible wrapping label, avoiding generated ID
collisions. Inputs are void elements. The submit button has no name, so it
adds no query entry. There are no arbitrary attributes, event handlers,
method/action overrides, raw HTML children, or file controls in this API.
HTML content-model rules, such as avoiding nested forms, remain the caller's
responsibility.

The action must be a validated `InternalUrl` **without a query**. Native GET
submission [replaces the action URL's query](https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm).
Ruvane returns `FormError::ActionHasQuery` rather than silently discarding
query parameters. Declare fields, including hidden fields, for values that
must be submitted. The mini-blog declares hidden `page=1` so a new search
resets pagination. Its page links retain the current filter using `InternalUrl`.

## Explicit query decoding

Register the receiving route with `Router::get_form`. For standalone parsing,
use `Query::parse_with_mode(raw, QueryMode::Form)`. Route selection uses raw
paths and fixes the mode before semantic query decoding; request fields cannot
change it. Framing, Host, method, and transport URI checks still happen first.

| Wire data | `QueryMode::Uri` | `QueryMode::Form` |
| --- | --- | --- |
| `q=Rust+HTML` | `Rust+HTML` | `Rust HTML` |
| `q=Rust%20HTML` | `Rust HTML` | `Rust HTML` |
| `q=Rust%2BHTML` | `Rust+HTML` | `Rust+HTML` |
| `q=%252B` | `%2B` | `%2B` |

`Query::parse`, `Router::get`, assets, and unknown paths keep URI mode, preserving
the previous literal-plus behavior. `Query::mode()` and `Request::query_mode()`
report the selected mode. Raw `Request::query()` remains available unchanged.
Path captures use URI semantics even on a form route.

Both modes split on `&` and the first `=` **before decoding**, decode percent
escapes exactly once as strict UTF-8, and check decoded key uniqueness. Form
mode replaces raw plus signs before percent decoding, following the ordering
in the [URL-encoded parser](https://url.spec.whatwg.org/#application/x-www-form-urlencoded-parsing).
Decoded `%2B` is never processed as a new form space. Encoded separators remain
component data. Do not decode the resulting application strings again.

This is a deliberately strict profile, not the complete permissive WHATWG
parser: malformed escapes/UTF-8, empty keys/fields, controls, and duplicate
decoded keys are errors. Bare keys and empty values are supported. Keys are
case-sensitive, with no trimming, case folding, or Unicode normalization.
The query parser does not interpret bracket notation or nested structures.
`a+b=1&a%20b=2` is a duplicate in form mode; `a+b=1&a%2Bb=2` is valid there.
Existing `InternalUrl::query` URLs work in both modes because all data plus
signs and spaces are percent-encoded explicitly.

## Limits and validation

| Form construction budget | Limit |
| --- | --- |
| Field name syntax | `[A-Za-z_][A-Za-z0-9_-]*` |
| Field name | `MAX_FORM_NAME_BYTES`: 64 ASCII bytes |
| Named fields, including hidden inputs | `MAX_QUERY_PAIRS`: 32 |
| Each value, visible label, and button label | `MAX_COMPONENT_BYTES`: 1024 UTF-8 bytes |

Names must be unique across all control types. Values may be empty; visible
labels and the button label must contain non-whitespace text. Control
characters are rejected. Construction returns a typed `FormError` without
including rejected field data in its message. Labels, values, and the internal
action are escaped in their HTML context; no public raw HTML API is added.

The rendered `maxlength` is browser guidance measured in UTF-16 code units.
It is not a security boundary or a replacement for the server's UTF-8 byte
checks. A value within that client limit can still exceed the server limit.
Each decoded query key/value is limited to 1024 bytes, with at most 32 pairs
and 4096 bytes of raw query. HTTP request-line/header limits also apply.
Percent encoding can expand text, and several individually valid controls
can exceed the aggregate query or request-line budget when submitted.
Forms do not guarantee every possible combination of entered values will
fit those limits; errors are rejected before any handler.

`Query::get_as::<T>` retains `ParamError::Missing` versus `Invalid` conversion
errors through `FromStr`. Handlers must check domain ranges, required fields,
and allowed keys as needed; declaring a form does not create a server schema.
The mini-blog converts its page number to `usize`, requires it to be positive,
and checks pagination arithmetic. A validly decoded search is kept in the
form after an invalid page number, escaped as text. Malformed URL/query input
receives the framework's generic error response without reflection.

## Security scope

GET and HEAD handlers must be read-only and safe to retry. Hidden inputs can
be changed by clients and do not establish authorization. GET data appears in
URLs, browser history, bookmarks, and potentially logs; do not submit secrets.
Escaping handles HTML syntax, not business validation or authorization.

This release adds no POST, request bodies, multipart parsing, uploads,
authentication, sessions, CSRF mechanism, server actions, or browser reactivity.
It retains the loopback-only development server and its bounded I/O profile.
Tests cover encoding collisions, single-pass decoding, UTF-8 and count/byte
budgets, field/attribute injection, mode selection, route conflicts, HEAD,
rejected framing, and the mini-blog form flow. These are regression checks,
not an independent security audit.
