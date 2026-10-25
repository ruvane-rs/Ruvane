# Security

Alpha.20 adds only explicit public navigation caching. App pages default to
no-store; HTML, action replies, errors and requests carrying Cookie/Authorization
stay no-store. A loader can tighten a public declaration. Never opt user-specific
or authorization-dependent data into a public policy. GET loaders, component
factories and validation remain trusted read-only code, not a side-effect sandbox.

Prefetch accepts only marked internal links or explicit calls. The navigator
checks MIME, status, exact target, the public response policy and Vary, then uses
Rust to validate the full packet/schema/view before storing bytes. It retains no
DOM or component owners. Budgets are 32 entries, 2 MiB, two speculative fetches,
32 jobs, 64 KiB per packet and an absolute fetch/read deadline. There is no disk,
localStorage, sessionStorage or cross-tab page cache. Public freshness is bounded
and is not a guarantee against independent server-side changes.

Action start/end events retire cached and in-flight data. Generation checks
prevent late replies from re-entering the cache. Unknown transport/server outcomes
latch speculative loading and storage off until document reload; fresh foreground
reads remain possible. Abort does not roll back server effects. A 409 blocks cache
reuse and asks for reload. Revision headers remain build metadata, not access
control. See [navigation cache](docs/navigation-cache.md) for the exact contract.

Alpha.19 adds application-owned browser registries, App client pages and paired
CLI builds. State resolution and schema checks are shared; only server GETs run
loaders. Factories, loaders and shells remain trusted Rust callbacks. SSR checks
seeds and views before serving them, and refuses initial queued actions. Explicit
native forms retain the existing Origin, Host, framing and field checks.

The server embeds one prepared module and serves resources under its build
revision. Before hydration the bridge checks ABI v8 and the exact u64 revision.
The export macro isolates twenty-one numeric exports, accepts no input pointers and
provides no host imports; only the reviewed no_mangle attributes allow unsafe
code annotations. The core continues to forbid unsafe code.

App browser requests carry their public revision. Stale navigation requests and
stale/missing revisions on typed actions return 409 before handlers, so an old
open tab cannot apply typed mutations to a rebuilt application. This header does
not authorize access; native source-origin checks and application authorization
still apply. Native forms remain usable without the header.

The CLI bounds watched sources, module size and output files, preserves the
previous server on compilation failure and attempts restoration after startup
failure. The exact prepared module is embedded in its server. Bundles export
resources from that executable, validate the manifest and ABI/revision record,
and compare module bytes before publishing the current pointer. Watched source
changes reject a candidate. Framework/dependency source trees outside the project
are not snapshotted. Cargo and application code run as trusted local programs;
this is not a build sandbox, cryptographic signature or durability guarantee.
See [interactive applications](docs/interactive-apps.md) for limits and scope.

Alpha.18 adds explicit shared client routes and bounded RVP1 page seeds. GET
handlers still authorize every request and render direct URLs. The browser
requests a fixed same-origin MIME, refuses redirects and caps fetch/read time
at 10 seconds and decoded bytes at 64 KiB. Metadata, exact requested URL, HTTP
status, complete wire framing, shared route ownership and application seed schema
are checked before replacing the current page. No fetched HTML is injected.
Titles are plain text; link targets use the internal URL profile and cannot
select another origin or an executable scheme. Seeds contain public data only.

The next page is prepared in a detached host. Seeded mounts cannot queue initial
actions. Preparation failures preserve the current owner; a newer navigation
supersedes an older request. Committing replaces one page owner and releases its
listeners/actions while sibling layout islands remain alive. A failed history
pop or commit falls back to a normal document request. History state is routing
and scroll metadata, never authorization. The new page ingress uses bounded
exact-length numeric chunks without pointers or host imports. Runtime
MediaType::Navigation responses are refused by static export. See
[client navigation](docs/navigation.md) for API boundaries and deferred work.

Alpha.17 adds explicit typed actions called by Rust/Wasm components. All action
routes reuse the alpha.16 origin, Host, body, charset and method checks below.
Action IDs label application callbacks, not authorization or secret function
names. Requests use validated internal paths, fixed POST and URL-encoded fields.
The bridge refuses redirects, external origins and arbitrary HTTP headers.

RVA1 replies carry named text, u64 and boolean values with strict UTF-8, unique
ASCII names, field-count and decoded-byte caps. HTTP status must agree with the
frame kind, and MIME must match the fixed action type. Rust decodes the whole
reply before invoking the model. Network bytes never become HTML, selectors or
DOM commands. Applications must still check their response schema and resource
authorization. MediaType::Action declares MIME only; it is not a byte validator
and cannot be used for static export.

Browser ABI v8/RVN6 isolate twenty-one reviewed numeric exports. The reply
ingress uses exact-length bounded chunks and accepts no input pointers. Each
request has a checked, never-reused token bound to a live component. One action
per view can await I/O without holding the synchronous ABI busy. Disposal aborts
fetch/read operations, clears timers, resolves the shared idle promise and drops
the native owner; later completions cannot address a remounted view.

Fetch and response reading have an absolute deadline and a 64 KiB decoded cap;
request bodies are limited to 16 KiB. Invalid network data becomes a local
Protocol outcome. Unexpected HTTP status or lost replies can occur after a
server mutation. Timeout and abort do not roll back effects, and the bridge
does not retry automatically or promise exactly-once execution. Trusted Rust
callbacks are not preempted or sandboxed. The example keeps native forms as its
fallback and continues to hold only bounded, shared in-memory task state.
See [actions](docs/actions.md) for the exact protocol and limits.

Alpha.16 adds explicit POST form routes to the loopback HTTP server. Host must
first match the bound IP or localhost and actual port. Origin is then mandatory,
canonical and equal to the HTTP origin of that validated Host. Null, missing,
multiple and different origins fail. Fetch Metadata, when present, must indicate
same-origin. Forwarded headers and Referer cannot bypass the check. These aliases
are separate browser origins; arbitrary DNS hosts remain refused.

This policy constrains browser cross-origin submissions, not forged requests
from local programs, authentication or resource authorization. There are no
sessions, cookies or cryptographic tokens. Public listeners, HTTPS and reverse
proxy deployment remain outside the server profile. Canonical metadata origins
stay configured separately and never authorize a mutation.

Only registered POST handlers receive bodies, after route, Host, source-origin,
media/charset and size checks. One Content-Length is required; duplicate,
comma-separated, signed or overflowing lengths fail. Transfer-Encoding remains
unsupported, and combining it with Content-Length fails. There is no interim
100 response, request decompression, multipart parser or upload path.

Bodies are capped at 64 KiB, fields at 32, names at 64 ASCII bytes and normalized
values at 1024 UTF-8 bytes. Duplicate decoded names fail. Decoding occurs once;
CRLF becomes LF in values, while lone CR and other controls except LF/tab fail.
URL queries remain separate. The same absolute deadline covers queueing, headers
and body reads; shutdown interrupts waiting body I/O. Only one request is served
per connection. Rejected, incomplete or malformed data does not call a handler.

Native PostForm controls escape text and use validated internal actions. A 303
redirect accepts only InternalUrl, emits one Location and an empty body. POST
handlers remain trusted concurrent application code: domain validation, access
checks and state synchronization belong there. Redirects do not deduplicate
retries; handler failures do not roll back state changes. The example's bounded
task state is shared in memory and disappears at server shutdown.
See [server POST forms](docs/post-forms.md) for the exact profile.

Alpha.15 uses browser ABI v4 and RVN3 batches. Text payloads enter through a
bounded numeric chunk API, without input pointers or host imports. A pending
payload belongs to one live view/node/event. Its exact length, UTF-8, field type,
disabled state and control characters are checked before application code runs.
Booleans accept only 0 or 1. Invalid payloads do not drop a healthy component.
Clear aborts staged input; pending input prevents other mutations.

Fields have fixed types and wrapping labels. Forms cannot nest; they expose no
arbitrary action, method or field name. Submit is handled locally in Wasm with
native navigation prevented. There is no new HTTP mutation endpoint. The
1024-byte transport quota is separate from application validation and the DOM
UTF-16 maxlength. Client validation is not a server authorization boundary.

Hydration validates escaped defaults and adopts valid dirty live values only
after the complete tree matches. Initial defaults remain fixed while live value
and checked properties change. Composition defers text dispatch; explicit model
resets win after composition ends. Field removal releases all input/composition
listeners. Logical bindings remain capped at 16 per view, with at most 48 actual
DOM listeners. See [client forms](docs/client-forms.md) for the supported profile.

Alpha.14 introduced structural batches. Keys are nonzero u32
values unique among siblings, with a separate namespace from unkeyed positions.
DOM handles are checked monotonic values within one live view. Deletion retires
handles permanently; restoring a key allocates new handles. Tag or event-binding
changes also allocate a new lifetime. Native event dispatch checks view, node
and event together before calling application code.

Structural batches are planned against a copy of the live metadata. Unknown
parents, cross-view references, duplicate keys, reused handles, bad list nesting,
invalid sibling targets, incomplete nodes and resource excess reject the whole
plan before DOM commands run. Removal drops all descendant bindings. Retained
nodes keep their handlers; new handlers attach after the DOM matches the plan.
The command cap is 256 within 64 KiB; live nodes, depth and events stay capped.

Reconciliation preserves keyed nodes within the same parent. A moved surviving
element can regain focus without scrolling. Unexpected DOM-application errors
stop the bridge and release resources; there is no transactional rollback of
application state or arbitrary host side effects. Hydrated DOM is retained and
disabled on failure, while client-created nodes are removed. Validation failure
before application produces no partial DOM command effects.

Alpha.13 introduced shared views and hydration with browser ABI v2. Initial state is
one u32 or boolean, carried in fixed numeric data attributes. Component IDs
belong to the application's registry; props and downloaded Wasm are public,
not authorization or secret storage. No JSON parser or inline script is used.

Views cap node count, nesting, handlers and UTF-8 text. The shared HTML profile
rejects invalid parents and text controls that do not round-trip through HTML.
Hydration checks the complete expected tree: version, props, tags, IDs, child
order, leaf text, event IDs and control attributes before attaching any handler.
It reuses elements and text nodes, then enables the appropriate controls.

A mismatch preserves the supplied DOM, releases its root claim and disposes the
new native component. Other views continue running. A later protocol or DOM
failure stops the bridge, removes handlers and disables retained hydrated
controls; server content and unrelated host siblings remain. Client-created
nodes are removed. Explicit disposal removes either kind of view. Root claims
also prevent duplicate hydration by runtimes using the same bridge module.

Components and factories are trusted Rust code. Their reported reactive counts
are capped, but custom CPU, allocation, reference cycles and destructors are not
sandboxed. A failed native update drops the affected component. Structural updates in
alpha.14 follow the handle and patch-validation rules above.

Starting with alpha.12, the core can encode a bounded RVN1 browser command batch
and explicitly serve trusted JavaScript/Wasm. Response::interactive_html opts
one HTML response into the fixed same-origin script/Wasm/connect CSP. It enables
neither inline JavaScript nor general JavaScript eval. All existing constructors,
errors, assets and static-hosting exports keep their default blocking policy.
Request headers and query data cannot supply arbitrary policies. Module URLs
use InternalUrl and quoted attribute escaping. Assets are application-owned code,
not sanitized uploads. A CSP is not an authentication or sandbox boundary.

The first-party browser crate has a capped per-Wasm-instance registry and weak
reactive handles. Only reviewed no_mangle export attributes are allowed in its
ABI module; there are no unsafe blocks, foreign imports or input pointers. The
core still forbids unsafe code. Numeric handles never wrap or reuse identities.
The output buffer is borrowed only until the next mutation; our bridge validates
pointer/length bounds against fresh memory and copies before acknowledging it.

The bridge rejects unsupported versions, oversized/truncated/invalid UTF-8
frames, unknown tags/opcodes, cross-view references, invalid DOM ownership and
duplicate command entries before applying a complete batch. Client mounts require empty
roots; hydration validates existing server nodes. textContent never interprets
command text as HTML.
There is no arbitrary selector, attribute, URL, function name, eval or HTML sink.
Protocol, DOM-application or trap failures stop the bridge and release its
handlers and handles. Rust cleanup is attempted; after a trap that prevents cleanup, native
counts are reported unknown and the instance reference is released, not claimed
to be zero. Ordinary disposal removes listeners, handles and reactive scopes.

Only trusted same-origin modules load, with redirect refusal, a fetch deadline
and a decoded-byte cap. Wasm application code itself is trusted: byte and command
limits do not preempt an infinite computation or sandbox all its allocations.
Tests use Node/Python standard tools and a real browser, without installed npm
or Python packages. See [browser](docs/browser.md) for the exact ABI and limits.

Starting with alpha.11, each ReactiveRuntime owns independent single-threaded
metadata without global or thread-local graph state. Scopes own values and
callbacks; public signal, memo and effect handles are weak. Monotonic checked
identifiers are never reused. Cross-runtime and disposed dependencies are
rejected before callbacks run. Resource budgets constrain live owners, values,
computations, dependency entries/edges and callback runs per flush.

Dependencies are explicit and fixed; reading a source does not subscribe it.
Memo writes, registration and batching are rejected. Effects may enqueue writes
without recursively invoking the scheduler. Disposal cancels pending work and
derived dependents even when their owners differ. User values and captures are
released outside scheduler metadata borrows. Custom Clone reentry reports a
borrow error instead of unconditionally borrowing a mutable value.

A callback error, unwinding panic or exhausted run budget permanently faults
the runtime and clears pending work. Subsequent reads/writes/registration fail;
inspection and disposal remain available. Committed writes and external side
effects are not rolled back. Catching unwinds does not suppress the application's
panic hook and cannot recover panic=abort. Callbacks, Clone, PartialEq and Drop
implementations are trusted Rust code: their CPU, memory, recursion, side effects
and custom reference cycles are not sandboxed by the metadata/run budgets.
See [reactivity](docs/reactivity.md). This version introduces no browser ABI,
script execution, wire protocol, mutation HTTP endpoint or CSP relaxation.

Starting with alpha.10, local content has strict typed metadata and portable
unique slugs. The limited Markdown renderer constructs typed HTML, escapes raw
HTML/entities, permits only validated internal link paths, and bounds bytes,
lines, tokens, inline depth and delimiter-scan work. It interprets no Rust,
JavaScript, MDX, imports or raw HTML. Code blocks display escaped text.

ContentBundle reads a bounded flat directory, rejecting observed symlinks,
special files, non-.md entries and invalid UTF-8. Source/output ancestors must
be protected from hostile concurrent mutation. This is not a filesystem sandbox.
Diagnostics include validated basenames and line numbers rather than content.

Generated applications use trusted build.rs code with Ruvane as a first-party
build dependency. Source strings are escaped into a bounded Cargo OUT_DIR
module; content never provides tokens outside literals. The executable owns
its fixed content snapshot. Invalid content is checked before CLI compilation,
retaining the old server/build. Cargo revision invalidation is not an integrity
signature. Build scripts, Cargo configuration, toolchain and output parents
remain trusted. No runtime content directory is exposed over HTTP.
See [content and Markdown](docs/content-and-markdown.md).

Starting with alpha.9, App declares named pages, fragment layouts and assets
for both server rendering and explicit static planning. Registration validates
names, ambiguity, concrete export paths and count limits without executing
callbacks. Failed registrations leave the declaration unchanged. Layout parents
must already exist; immutable references prevent cycles and depth is bounded.

Named links encode decoded captures once and check concrete route ownership.
Static-capable pages receive no HTTP Request even on the server; server-only
pages explicitly select query decoding and are reported as excluded from export.
Route names, export membership and URL encoding are not access controls.

Page/layout callbacks remain trusted, concurrent, read-only Rust code. Their
allocations occur before output checks; CPU, side effects, globals and arbitrary
conversion code are not sandboxed. Body, intermediate layout and final document
sizes are checked. Page errors and unwinding panics abort static planning before
export. Recovery is unavailable with panic=abort. No script engine, executable
wire format, remote function dispatch, POST or cryptography is introduced.

Custom 404/500 responses are rendered once without request data, forced to
noindex without a canonical, and served with their actual statuses and fixed
CSP. Malformed protocol/query inputs retain fixed errors. A custom error too
large for the server budget falls back to the fixed small 500. A bad error
renderer refuses router construction. Static hosting error mapping remains a
provider responsibility. See [pages and layouts](docs/pages-and-layouts.md).

Starting with alpha.8, the local CLI compiles and executes an explicitly selected
trusted Rust project through Cargo. It is not a sandbox for downloaded code,
Cargo configuration, build scripts, toolchain binaries, or subprocesses started
by application code. CLI commands are not exposed as HTTP endpoints. Arguments
are passed directly to processes without a shell command string.

Generation requires a fresh portable project name and exclusively creates files.
Managed work/build directories have explicit markers, symlink checks, bounded
source/output profiles, and a project lock. These checks assume trusted parents
without hostile concurrent mutation, as does static export. Markers are not
authentication. Versioned builds preserve older files; unknown output roots
are refused. The CLI never recursively cleans user directories.

Generated servers receive an stdin control pipe and stop on data or EOF.
Normal CLI cleanup closes that pipe, waits, then kills/reaps its direct child
if needed. Cargo/application waits have limits. Arbitrary descendants and
platform-specific signal handling are outside this portable std-only contract.
The CSP remains unchanged; dev does not inject browser reload scripts.
See [the CLI contract](docs/cli.md).

Ruvane is experimental and has not undergone an independent audit.

Version 0.1.0-alpha.1 provides HTML rendering only. Starting with
0.1.0-alpha.2, Ruvane also provides a loopback-only development server with
bounded socket I/O and an intentionally limited HTTP profile. It does not
provide TLS, sessions, authentication, server actions, or a production HTTP
implementation. See [the HTTP profile](docs/http-profile.md) and the README
for its exact syntax, defaults, and limitations.

Starting with 0.1.0-alpha.3, dynamic captures and query pairs are decoded once
as UTF-8 under fixed size/count limits. Ambiguous dynamic routes and duplicate
decoded query keys are rejected. Internal links require a validated
`InternalUrl`, URL component encoding, and quoted HTML attribute escaping.
The raw path remains distinct from decoded parameter data: neither literal
route precedence nor URL decoding is an authorization boundary. Applications
must validate domain values and access to the resources they identify. See
[routing and URLs](docs/routing-and-urls.md) for the exact contract.

Starting with 0.1.0-alpha.4, embedded assets have fixed registration budgets
and explicitly declared MIME types. Requests never map a URL to a filesystem
path. Assets are trusted application source; CSS is code, and declared image
types are not file validation. Images are not decoded or resized by Ruvane.
Binary and embedded responses obey the same output and I/O limits as HTML.
Stylesheet and image attributes use validated internal URLs and HTML escaping.
See [assets and media](docs/assets-and-media.md) for exact limits and scope.

Starting with 0.1.0-alpha.5, typed GET forms escape text and require an internal,
query-free action. Field names, counts, and text lengths have fixed budgets.
The selected raw route fixes query decoding: ordinary routes preserve literal
plus signs; form routes decode raw plus signs as spaces. Queries are decoded
once using that mode, and duplicate decoded keys are rejected before a handler
runs. Request fields cannot switch the mode. Path captures keep URI semantics.
Hidden fields are untrusted client input, and GET field values appear in URLs;
they must not contain secrets or establish authorization. GET forms add no mutation,
authentication, session, or CSRF protection mechanism. GET/HEAD handlers remain
read-only. See [GET forms](docs/get-forms.md).

Starting with 0.1.0-alpha.6, page metadata uses unique typed fields and escaped
attributes. Descriptions are bounded and control-free. Canonical URLs combine
a restricted, explicitly configured SiteOrigin with an InternalUrl; no origin
is inferred from Host or forwarding headers. The parser performs no network
I/O, IDNA conversion, or TLS. Robots hints do not restrict access.

Default server responses include a fixed Content-Security-Policy, including
assets, HEAD, parse errors, and handler failures. Scripts are blocked; CSS,
images, and form submissions are limited to the same origin. Alpha.12 adds the
explicit interactive response profile described above. Base elements
and frame embedding are explicitly blocked. Standalone HTML rendering does
not emit an HTTP policy. CSP supplements escaping and does not sandbox server
code or add authorization or CSRF protection. See
[metadata and CSP](docs/metadata-and-csp.md).

Starting with 0.1.0-alpha.7, static export uses explicit portable paths and
bounded pages/assets. Output paths are derived from a restricted lowercase
ASCII profile, never from request targets. Plans reject aliases, device names,
duplicate/conflicting files, and MIME suffix mismatches. Content remains
trusted application input: assets are not sanitized or decoded.

Exports require a new destination under an existing trusted parent, reject
observed ancestor symlinks, and exclusively create files. They never overwrite
or recursively delete output. I/O failures may leave partial files without the
final completion marker. The caller must prevent concurrent hostile changes
to the parent and output tree; portable safe std APIs do not provide a complete
filesystem sandbox. The marker is not an integrity signature or crash-durability
guarantee. See [static export](docs/static-export.md) for the precise scope.

Static previews serve registered shared bytes, not arbitrary disk paths. They
retain the existing local HTTP profile and CSP. Exported files carry no HTTP
headers themselves; the hosting server must apply the generated header contract
and declared MIME types. GET search remains part of the server edition.

The framework rejects ambiguous request framing before dispatch and uses
fixed response headers. HTML strings are escaped by the renderer. Handler
code remains trusted: response limits apply after construction, socket
deadlines do not interrupt application code, and shutdown waits for running
handlers to return. The normal panic hook still executes on a caught panic;
applications must keep sensitive data out of their own logs. Builds using
`panic = "abort"` cannot recover from handler panics.

Local binding and Host checks are not an authentication boundary against
other local programs. Read handlers must not perform state-changing actions.

For reporting options, consult the repository's Security tab. If private
vulnerability reporting is unavailable, contact a maintainer privately before
sharing sensitive details. Do not include credentials or personal data.

Security fixes may change the pre-release API. Users should follow the latest
pre-release and review its changelog.
