# Conditional resource updates

Alpha.53 adds bounded entity tags and If-Match conditions using std only.
They follow the strong-comparison rules in
[RFC 9110, section 13.1.1](https://www.rfc-editor.org/rfc/rfc9110.html#name-if-match).
Applications choose which resources require a version. Conditions do not
replace resource authorization, sessions, CSRF tokens or request quotas.

## Types and transport

- `EntityTag::strong(opaque)` constructs a quoted ASCII tag.
- `EntityTag::parse(value)` accepts one strong or `W/` weak tag without trimming
  or escape processing. `as_str()` returns its complete wire value.
- `IfMatch::parse(value)` accepts `*` or a tag list. Commas and backslashes inside
  quoted tags are literal data. `matches(Some(&current))` uses strong equality;
  weak tags never match. `matches(None)` is false, including for `*`.
- `Request::if_match()` borrows the parsed condition, or None when absent.
- `Response::with_etag(tag)` attaches a typed ETag to a successful buffered
  response. `etag()` and serialization omit it on errors and lazy streams.

The ASCII HTTP profile accepts tags up to 128 wire bytes, including quotes and
an optional weak prefix. If-Match is capped at 2,048 bytes and sixteen tags.
Up to sixteen empty list members are ignored; an empty list matches nothing.
Mixed wildcard/list syntax, broken quotes, non-ASCII tags and header injection
fail. Duplicate field names remain forbidden. Syntax errors return 400; byte or
member excess returns 431, before guards or handlers. Debug omits tag contents.
Sanitized pre-render/background GET requests drop these conditions.

Adding ETag does not enable caching, 304 responses, If-None-Match, or automatic
precondition evaluation by page caches/loaders. The existing no-store API
responses remain no-store. A tag must identify the exact selected representation;
applications must not assign one strong tag to different bytes at the same URL.

The transport conservatively strips ETag from every PUT response. JSON PUT can
transform whitespace, field order and the response representation, so it cannot
attest that the received bytes were stored unchanged. Read the saved resource
to obtain its current validator. See
[RFC 9110, section 9.3.4](https://www.rfc-editor.org/rfc/rfc9110.html#name-put).
The task example also obtains fresh versions by GET after PATCH.

## Check and commit together

Parse and validate application input, then authorize through the configured
native guards. Under the resource's mutation lock, resolve the record and compare
its current version. Keep that lock held until the change is committed. A guard
that checks a tag and releases the lock before a handler writes is insufficient.

For a resource that requires precise versions:

```rust
use ruvane::{EntityTag, Request, StatusCode};
fn check(request: &Request, current: &EntityTag) -> Result<(), StatusCode> {
    let condition = request.if_match()
        .filter(|condition| !condition.is_any())
        .ok_or(StatusCode::PreconditionRequired)?;
    if condition.matches(Some(current)) {
        Ok(())
    } else {
        Err(StatusCode::PreconditionFailed)
    }
}
```

Call this while holding the resource lock. Missing conditions return 428;
`*` also returns 428 under this application policy because it only tests existence.
A well-formed weak, empty or stale tag list returns 412. A list containing the
current strong tag can succeed. The statuses follow
[RFC 6585, section 3](https://www.rfc-editor.org/rfc/rfc6585.html#section-3) and
RFC 9110. A missing resource still returns its ordinary 404 before a condition
is evaluated. Invalid domain input retains its ordinary 400.

A failed precondition makes no resource change. After acceptance, allocate the
new version and construct the response before publishing the new record. If
allocation, version generation or serialization fails, retain the old resource.
Do not automatically refresh a version and replay a mutation: that would replace
an explicit conflict with an unseen overwrite.

## Native version source

`EntityTagSource::new()` uses the existing supported macOS/Linux x86_64/aarch64
kernel random provider to create a fresh independent 256-bit namespace. It
never falls back to time, process IDs or a custom random generator. Clones share
one checked u64 counter through a mutex. `next_tag()` returns a strong tag and
fails closed on counter exhaustion or lock poisoning. Initialization fails if
entropy is unavailable or the platform is unsupported. The source is excluded
from Wasm; parsing and comparisons remain shared Rust code.

Use one source for a store lifecycle. Every accepted task write gets a new tag,
including edits that return to earlier content. Deleting records does not reset
the counter. A fresh random namespace makes accidental reuse across independent
stores or process restarts negligibly likely; it is not a mathematical guarantee
against random collisions. Tags are public versions, not secret authentication
claims or content hashes. They never reuse session or CSRF tokens.

This is memory-only version generation. Persistent or replicated storage must
persist and coordinate its own representation versions atomically with data;
a fresh source cannot recover that storage or provide cross-process transactions.

## Task example and conflicts

Run `cargo run --offline --locked --example json_api -- 8080` in the repository.
Open the printed address in two tabs, open the labeled demo session in each,
and select the same task using its Edit button. Both editors now hold a snapshot
and its tag. Save from the first tab; saving the second tab returns 412, keeps
its title and completion draft, and disables further writes until an explicit
reload. Reload server version adopts the current task; it does not replay a save.

Individual GET/HEAD responses carry ETag. Collection reads and creation do not
supply a per-task validator. PUT, PATCH and DELETE require a matching precise
If-Match. The browser stores the tag when loading an editor, never fetches a new
tag immediately before sending an old draft, and sends each mutation once.
Its separate CSRF preflight still retrieves the current session token.
Refresh list leaves an existing editor snapshot unchanged. A successful save
can perform fresh reads; if those fail, it reports the committed save and asks
for an explicit reload. Titles render through text nodes.

Tests cover strong/weak/list grammar, byte/member limits, redaction, sanitized
requests, source failures, counter overflow, concurrent writers, authorization
ordering, deletion, recreated IDs and actual process restarts. Browser ABI v13,
22 exports and four core runtime assets remain unchanged.

Alpha.54 adds [typed input validation](validation.md). A validation failure returns
400 before the resource lock without changing its data or ETag. The editor keeps
that loaded version for a corrected explicit save. It still refuses stale versions
with 412 and never refreshes a tag to replay an old draft automatically.
