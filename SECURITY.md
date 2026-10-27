# Security

Alpha.53 adds [conditional resource updates](docs/conditional-requests.md).
Typed ASCII ETag/If-Match values have fixed byte/member limits and reject malformed
conditions before callbacks. Strong comparison never accepts weak tags. Protected
task writes require a precise matching version under the mutation lock, after
session/CSRF/quota guards; failed conditions cannot modify state. Browser conflicts
retain drafts and require explicit reload without replay. Native version sources
use independent OS-random namespaces and checked shared counters, failing closed
on initialization failure, exhaustion or poisoning. Tags are public versions,
not authorization or session credentials. The transport omits PUT validators
because it cannot attest that request bytes were stored without transformation.

Alpha.52 extends explicit mutation profiles to JSON PUT/PATCH and bodyless DELETE.
All mutations share mandatory source validation, current-session CSRF verification
when guarded, and route/family quotas. DELETE refuses positive declared lengths
and media/encoding headers before guards or handlers. Method-scoped registration
and exact Allow values prevent MIME or method fallback into a different handler.
The task example validates entire replacements/patches before atomic updates;
applications remain responsible for resource authorization and stale-write policy.
No mutation is automatically retried, and the Wasm action protocol remains POST.
See [JSON APIs](docs/json.md).

Alpha.51 adds opt-in [bounded JSON APIs](docs/json.md). Parsing and immutable
construction enforce input/output bytes, depth, total values, members, strings
and number-text limits. Decoded duplicate keys, invalid UTF-8, unpaired surrogates,
extensions and trailing input fail closed. Number text is preserved without
implicit f64 conversion. Explicit registration selects the body profile; MIME
cannot switch it. Transport/JSON checks precede guards and handlers. JSON POSTs
retain mandatory source validation; session CSRF uses only the dedicated header.
Debug hides JSON request/response data and errors omit input fragments. Typed
responses use no-store, nosniff and the blocking CSP. JSON is not safe inline HTML
or JavaScript embedding or a substitute for schema validation and authorization.
The loopback transport scope is unchanged.

Alpha.50 adds opt-in [native request quotas](docs/rate-limits.md), with bounded
token buckets, monotonic integer refill, shared clones and no eviction of active
debt. Per-session keys use native store/family identities and survive identifier
rotation; anonymous or invalid identifiers share one global quota per limiter. Selected patterns
and one unmatched scope prevent raw-path churn from creating arbitrary buckets.
Entry/route-byte exhaustion fails closed with 503. Guards reject over-quota requests
with no-store 429 and bounded Retry-After before cache lookup, loaders or handlers.
Only mutation guard failures emit the fixed pre-handler refusal marker; a manual 429
does not assert non-execution. The bridge never retries POST automatically. This
is process-local admission control after request parsing, not a distributed or
network DDoS defense. Existing session, CSRF and origin checks remain separate.
ABI v12 pairs the new typed action failure and closed read-only 429 navigation shape.

Alpha.49 adds opt-in [session CSRF tokens](docs/csrf.md). Tokens use independent
256-bit OS randomness and remain bound to one live session generation. Native forms
use a reserved hidden field; configured browser actions fetch an empty same-origin,
no-store token response before each POST. A failed token fetch sends no mutation.
Missing, malformed, ambiguous, cross-session and retired claims fail closed before
protected handlers. Session rotation replaces issued tokens atomically; revoke,
expiry and clear remove them. Tokens do not enter navigation packets, snapshots or
Wasm state. Debug masks token values. Keep native token forms out of public/static
artifacts; protection requires explicit guards and does not address XSS or credential
verification. Existing Host, Origin, MIME, body and revision checks remain active.

Alpha.48 adds [native memory sessions](docs/sessions.md), with OS-generated opaque
256-bit identifiers, server-only bounded data, cookie-only lookup and monotonic
idle/absolute expiry. Rotation retires old generations without extending the
absolute deadline. Revocation through a resolved native handle also fences a
concurrently rotated family. Native guards fail invalid sessions closed before
page caches/loaders. The supported source is the macOS or Linux x86_64/aarch64
kernel random device; errors never fall back to application-generated randomness.
State is memory-only and assumes protected process memory. This supplies session
lifecycle primitives; credential verification and production TLS remain application work.


Alpha.47 adds [typed cookies](docs/cookies.md). Cookie pairs are validated before
routing and callbacks; repeated names and malformed inputs fail closed. Response
cookies are bounded host-only declarations with validated prefixes and attributes,
separate Set-Cookie fields and explicit local HTTP opt-in. Cookie-bearing responses
are no-store and send a fixed cache-reset signal containing no values. Navigator
caches and prefetch work are retired on receipt. Cookie values never enter Wasm
protocol fields implicitly; Debug redacts cookie data and credential headers.
This provides cookie transport, not signing, sessions, TLS or authentication.


Alpha.46 adds [native route guards](docs/route-guards.md). Inherited and local
callbacks run after request validation and before page caches, persistent restore,
loaders and endpoint mutation. Denials stop execution; Private bypasses public
storage without rewriting credentials. All guarded responses are no-store so
browser caches cannot reuse earlier decisions. Built-in read-only failures remove
the old route branch and metadata; retained SPA shells must remain public.
Guard modules are excluded from Wasm. Guards do not authenticate headers or
provide sessions; applications must verify identity. Pre-render declarations
remain public build artifacts and guards never make them private. Low-level
Router handlers and assets are outside App guard scope.


Alpha.28 recovers only leaf failures whose configured boundary retains every
validated emitted layout. Typed RVP3/RVI1 errors undergo the full revision, URL,
ownership, schema, read-only view and initial-action checks before adoption.
Logical status does not rewrite committed HTTP headers. Error records are
no-store; retries are explicit fresh GETs. Layout failures, invalid error views
and unsupported outer scopes retain generic fallback and existing bounds.
Stale and invalidated replies cannot commit, and no POST is replayed.
See [streamed leaf recovery](docs/streamed-leaf-errors.md).

Alpha.27 adds explicit layout hydration during initial document loading. Bounded
inert RVH1 records bind the revision, URL, order and layout seeds. Rust/Wasm
validates ownership and schemas before events activate. Prepared layout actions
are rejected locally without a POST or queued retry until complete branch
adoption. Only attested live parents can transfer into the final navigator.
Malformed records, late failures, departure, deadlines and rejected final commits
retire temporary listeners/owners while preserving received HTML and drafts.
Caught application failures finish with generic abort HTML when budgets permit;
they contain no final page snapshot and cannot activate the leaf. Actual network
failures may still cause the browser to replace the document. External modules
reuse the existing CSP and four runtime assets. See
[early layout hydration](docs/layout-hydration.md).

Alpha.26 adds opt-in initial App document streaming. The typed head and route
props are resolved before headers. HTML is escaped and bounded; all seeds remain
public, no-store browser data. The completion suffix is withheld until producer
cleanup succeeds. Streamed startup requires the complete snapshot and a unique
inert end marker before preparing any page events. Late failures truncate the
response without diagnostics, partial activation or automatic data retry.
Native-only SSR retains buffered errors and its blocking CSP. The end marker
proves generation completed, not independent verification of HTTP framing by
document scripts. Callbacks remain trusted and cancellation is cooperative.
See [initial document streaming](docs/document-streaming.md).

Alpha.25 embeds a bounded inert RVI1 snapshot in interactive App HTML. Hex text
cannot inject markup, but its seeds remain public browser data. HTML stays
no-store. The bootstrap checks exact target/revision and Rust/Wasm validates
complete ownership, props, schemas and resources before page events activate.
The whole branch must match its server DOM. Failed activation releases all
temporary owners and restores nodes, attributes, draft values and selection.
Native fallback guards remain active and input adoption cannot send automatic
POST actions. Corrupt startup does not trigger a hidden recovery GET. Late Wasm
completion after pagehide is retired. Application callbacks remain trusted.
ABI v11, existing CSP values and the 22 numeric exports remain unchanged. See
[initial hydration](docs/initial-hydration.md).

Alpha.24 adds ordered progressive client navigation. RVS1 binds the exact
application revision and request token; each RVP4 prefix passes shared route,
canonical key, props and component schema validation in Rust/Wasm. Pending leaves
cannot mount. Network data never supplies HTML, selectors or executable scripts.
The parser bounds packets, layout depth, aggregate bytes and absolute duration,
and requires an explicit terminal plus successful response EOF.

Temporary layouts use the existing view budget. The previous subtree remains
owned until completion, and interruption retires previews and restores it.
History/title commit only after final validation. Mutation notifications cancel
speculative reads, including while the synchronous ABI is busy. Typed errors
can retain only validated ancestors. Streams remain no-store; complete buffered
prefetch/cache policies and the uncertain-action latch retain their behavior.
Loaders are trusted synchronous code with cooperative deadline/shutdown checks.
Native SSR remains buffered with real error statuses. See
[progressive navigation](docs/progressive-navigation.md).

Alpha.23 adds explicit native HTML streaming with bounded chunk, total-byte,
yield and duration limits. The server tightens the body/deadline budgets and
writes each fragment before pulling another. HEAD executes no factory or
producer. Invalid setup fails before headers; errors after commitment close
without a successful terminal chunk. Errors contain no application diagnostics.
Typed fragments retain HTML escaping, script-blocking CSP and no-store.

Producers, factories and destructors are trusted synchronous code. Checks occur
around callbacks and socket writes; CPU, allocations and blocking application
I/O are not preempted or sandboxed. A disconnect is observed on a write, and
cooperative callbacks must return before shutdown can join their worker.
Chunked requests remain rejected. The transport remains loopback-only and
browser navigation still validates complete packets. See
[streaming](docs/streaming.md) for budgets and incomplete-response semantics.

Alpha.22 adds local loading and typed error scopes. Read-only presentation mounts
accept an empty seed and reject event bindings and initial actions. The public
route manifest is bounded presentation metadata; it grants no access and cannot
authorize parent reuse. Retained parents in RVP3 error responses must pass the
shared declaration/key/props checks and every component schema. Only validated
ancestors of a failed segment can be retained. Unwinding native callback panics
produce generic 500 responses; Wasm traps still require document recovery.
Errors remain no-store. Retry sends only an explicit fresh GET, never repeats
POST actions and never resets the uncertain-mutation latch. Old requests cannot
restore feedback after cancellation, mutation or disposal. See
[segment boundaries](docs/segment-boundaries.md).


Alpha.21 adds bounded client layout branches. The shared App validator checks the
complete chain, canonical prefix keys and resolved props before the runtime
checks every component seed, view and resource budget. HTTP packets select only
the leaf page; numeric staging can select a validated segment for mounting. The
64 KiB packet limit includes all layout seeds. Layout components own separate
hosts and cannot patch child outlets through their own command frames.
Matching requires equal declaration, key, initial props and seed; changed parents
recreate their child branch. Factories/loaders remain trusted read-only code.
Cache permission is the intersection of the page and all layout policies, with
the shortest freshness bound. Private/error responses remain no-store. Preparation
failure releases temporary owners and retains the old branch. See
[client layouts](docs/client-layouts.md) for the ownership and wire contract.


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
revision. Before hydration the bridge checks ABI v11 and the exact u64 revision.
The export macro isolates twenty-two numeric exports, accepts no input pointers and
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

Browser ABI v11/RVN6 isolate twenty-two reviewed numeric exports. The reply
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

Alpha.33 development diagnostics use explicit compile-time configuration,
same-origin external assets, bounded status/output reads and per-session checks.
Production builds remove inherited development configuration; native-only pages
keep scripts disabled. Compiler output is visible to the local development
browser and must not contain secrets. Session IDs are public and do not
authenticate local programs. See [development feedback](docs/development.md).

Alpha.34 CSS updates read bounded snapshots of explicitly registered assets,
with source names confined to project-relative `assets/` paths. The browser
stages external stylesheets and retains working CSS after failed, expired or
retired loads. Development-only asset handlers validate session/build/version
tokens and origin metadata. CSS remains trusted application source. Production
assets keep their embedded bytes and do not read development snapshot files.

Alpha.35 file routes are discovered by trusted Cargo build scripts. Discovery
rejects observed symlinks, unknown entries, ambiguous page patterns and bounded
directory/source overflows. Generated imports use escaped absolute paths and
fixed Rust module identifiers. Registration validates a private App clone and
cannot commit a failed batch. Production contains compiled routes and does not
serve or discover route sources. The generator does not parse or sandbox Rust;
declaration functions and imported code remain trusted executable inputs. Source
and output directories must be protected from concurrent hostile changes.
See [file routes](docs/file-routes.md) for the exact format and limits.

Alpha.36 file loading/error screens use the same bounded discovery and atomic
registry. The first file scope must resolve both components; descendants can
replace one through file inheritance. Registration checks local factories and
views against empty seeds and every required state, rejects event bindings and
initial actions, and catches native unwinding panics as generic source errors.
These checks execute trusted presentation code; they cannot sandbox callbacks or
recover Wasm traps. Native consumers and orphan screens are rejected. Existing
boundary scope validation, no-store errors, script-free native responses and
fresh GET retry apply unchanged; POST actions are never replayed by retry.

Alpha.37 not_found.rs declarations share the bounded source budgets and atomic
registry. Local 404 views accept Number(404) and empty seeds without event
bindings or initial actions. Selection is shared with Wasm validation; a file
404 cannot authorize another scope or substitute for a 500. An unmatched URL
uses only the explicit root fallback, with no inferred captures or layouts.
Buffered errors keep HTTP 404, no-store, noindex and native script blocking.
Initial documents with custom 404 profiles are buffered before headers commit;
RVS1 navigation retains its HTTP 200 framing and final typed 404 packet.
Native consumers of file presentations are rejected. The source remains trusted
executable Rust, and presentation checks do not sandbox arbitrary callbacks.

Alpha.38 route.rs endpoint modules are imported and registered only for native
builds. They share source budgets and the atomic candidate registry; each method
slot counts toward the total App route budget. GET endpoints cannot overlap page
or embedded asset ownership. POST endpoints retain the existing origin, Fetch
Metadata, MIME, body and revision checks. Reserved literal framework prefixes
are rejected, and dynamic handlers cannot execute for those raw prefixes. Handler
callbacks remain trusted bounded code and must authorize each operation; layouts
and page loaders do not guard endpoints. Excluding route modules from Wasm does
not automatically exclude arbitrary shared application modules. Production
contains compiled handlers and never discovers route source from request paths.

Alpha.39 metadata uses a closed typed schema inside bounded RVP5 packets.
Unknown flags, malformed UTF-8/URLs, nested envelopes, oversized metadata and
unsafe error canonicals/indexing hints are rejected. Canonicals must belong to
the App's explicitly configured public origin, shared with Wasm; request headers
never configure it. The browser mutates only supported head slots after complete
packet/schema validation, without HTML parsing or changes to CSP, scripts,
stylesheets, charset or viewport. Metadata is public content and grants no
resource authorization. Cache privacy restrictions remain in force. Initial
streamed heads are trusted declarations; late errors cannot rewrite committed
HTTP bytes, and their final validated snapshots sanitize the browser head.

For reporting options, consult the repository's Security tab. If private
vulnerability reporting is unavailable, contact a maintainer privately before
sharing sensitive details. Do not include credentials or personal data.

Security fixes may change the pre-release API. Users should follow the latest
pre-release and review its changelog.

Public server data caching is opt-in for a complete client page branch. Cookie
and Authorization requests bypass both lookup and storage. The cache key includes
the full validated URL and a separate immutable router namespace; a public
declaration asserts that all layout/page seeds, metadata and native fallback
are safe to share and do not vary with other headers, identity or request method.
No-store loader results and errors are never stored or shared with waiters.
Invalidation retires matching active loads; expired or retired results are not
published. Entry/payload, active-load, waiter and absolute-wait limits apply.
These payload budgets do not sandbox loader allocations or execution, and a
running synchronous loader cannot be forcibly cancelled. A busy, timed-out or
retired cache request returns a fixed no-store 503. Public cache invalidation
does not replace authorization, transaction durability or remote-browser cache
invalidation. No cross-process or persistent cache is provided.

Background server revalidation is separately opt-in, for public URL-only data.
Its stale window, queue, worker and shared active-load budgets are bounded.
Private requests and actual streams never schedule a refresh. Jobs retain only
a reconstructed GET URL/query/capture context and trusted public origin, not
request headers or bodies. Existing public-data and loader purity obligations
continue to apply. An old entry is adopted only inside its hard stale window;
failed refreshes never extend that deadline, and stale navigation is no-store.
No-store or missing-page results retire old entries. Invalidation and LRU eviction
fence late refresh publication; queued invalid jobs are skipped before loading.
Workers catch unwinding callbacks and stop cooperatively. Running synchronous
code cannot be forcibly interrupted. Queue jobs hold weak router ownership;
explicit stop retires running refreshes and discards queued work. Cache policy
is not authorization or a mechanism for revoking data already adopted by a
request/browser. Call invalidation when committing changes that must not use
the optional stale window.

Interactive pre-rendering is an explicit assertion that the complete enumerated
page/layout data, metadata, native fallback and buffered HTML are public and
safe to embed in an executable and build artifact. Build GET context contains
only validated URL/captures and configured origin; there are no request headers,
query or body. Callbacks remain trusted read-only code and are not sandboxed.
No-store/error/oversized builds fail rather than freeze an error or private page.
RPR1 is trusted build output with bounded structure and an accidental-corruption
checksum, not authentication, signing or a safe HTML-upload format. Embed only
artifacts produced by the same trusted application build; do not accept uploaded
bytes, leak them to static lifetime or load arbitrary HTML artifacts at runtime.
The CLI verifies final embedded bytes, revision and source snapshot before
publishing. Runtime validates exact declarations, ownership, seeds and snapshots.
Cookie/Authorization and every query variant bypass frozen output; unlisted
parameters retain dynamic SSR. Frozen output is immutable for its build revision
and is separate from mutable server caches/invalidation. Existing origin, method,
body, revision and CSP guards remain in force for dynamic routes and actions.

On-demand pre-render regeneration is separately opt-in for those listed public
URLs. It ignores embedded build data at runtime and loads fresh on first read,
including after restart or eviction. It reconstructs a headerless, bodyless GET
with validated captures and trusted origin. Complete HTML, hydration/navigation
data, metadata and fallback publish together, within the existing cache budgets.
Path/tag invalidation fences active publication; errors and no_store never revive
an old snapshot. Regenerated responses are no-store. Storage remains per process;
applications must invalidate every replica after their committed mutation. There
is no persistent invalidation journal or push revocation of adopted responses.
The build artifact still contains public data and retains its existing trust
requirements. Regeneration does not compile Rust/Wasm at runtime.

Timed pre-render refreshing is explicitly opt-in for that same public output.
Freshness and the additional stale window are bounded. Deadlines begin after
validated publication and are never extended by reads or failed refreshes. A
fixed worker pool coalesces refreshes; its queue/load budgets are shared with
server data caching. Only complete validated HTML/snapshots/metadata replace an
entry. Transient failures apply retry backoff within the original hard deadline;
missing/no_store branches retire old output. Explicit invalidation and eviction
fence both running and queued publication. Adopted responses cannot be revoked,
so mutations needing immediate retirement must explicitly invalidate each replica.
Hard expiry never adopts an old output, and exhausted wait/load budgets return
no-store 503. Private/query/unlisted reads schedule no output refreshes. Existing
public-data, build-artifact trust and cooperative shutdown constraints still apply.

Persistent regenerated output is explicitly opt-in on native Unix. Its dedicated
private directory and parent must be trusted and protected from concurrent
tampering. Fixed filenames, real paths, private permissions, exact application
identity and refusal of foreign/symlink files limit accidental misuse. The owner
marker coordinates cooperative exclusive use; it is not a sandbox against a
malicious filesystem owner. Never use request-derived paths or uploaded catalogs.
RDC1 and embedded RPR1 have bounded parsing and corruption checksums, not signatures.
Restore validates exact application scope, revision, origins, declarations, policy,
snapshots and component seeds before adoption. Immutable build revisions must
cover code/assets; opaque callbacks cannot be fingerprinted automatically.

Only fresh disk entries are adopted and their original expiry is retained. Obvious
backwards/future clocks discard entries, but cross-restart expiry assumes a trusted
wall clock. External filesystem rollback and hostile writers are outside this
backend's guarantees. Successful invalidation synchronizes catalog removal before
acknowledgment, including records not loaded into memory. I/O failures leave public
cache use unavailable and preserve ownership for explicit empty recovery. A crash
or killed owner cannot be silently reopened: after all owners have stopped,
recover_empty discards old output and partial transactions. Static handles must
explicitly close after work settles. This is a single-process cache, not durable
application data, authentication, a transaction manager or distributed storage.
