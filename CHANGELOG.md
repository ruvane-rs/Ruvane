# Changelog

## 0.1.0-alpha.59 — Unreleased

- Add strict shared FileInfo/FileListing metadata and bounded paginated seeds.
  Keep server-assigned owners, storage paths and payload bytes native-only.
- Add typed View::download anchors that hydrate existing nodes and bypass client
  navigation and prefetch. Move to ABI v15/RVN9 with tag 17, retaining 22 exports
  and four runtime assets. Rebuild Wasm and bridge together.
- Add a persistent Rust/Wasm file manager with cursor pagination, refresh, upload
  and deletion. Retain selections on rejection, clear private state on authorization
  loss and require explicit inspection after uncertain mutation outcomes.
- Connect the manager to FileStore with session/CSRF/quota guards, private SSR seeds,
  opaque native downloads and script-free native forms. Add a standalone example,
  source-hidden packaging/restart/isolation tests and real-browser lifecycle checks.
- Keep zero third-party Cargo/npm dependencies and the RFS1 storage format.
  FileId::parse/FromStr now return shared FileMetadataError instead of FileStoreError.

## 0.1.0-alpha.58

- Add a native bounded FileStore with stable server-assigned owners, opaque
  OS-random file IDs, global/per-owner quotas and redacted metadata.
- Persist complete private snapshots through exclusive temporary files, sync and
  atomic publication. Preserve corrupt committed data and require explicit offline
  recovery for interrupted writers, with uncertainty after failed publication sync.
- Add typed UTF-8 attachment names and opaque no-store downloads with fixed
  Content-Disposition, nosniff and script-blocking CSP; preserve GET/HEAD semantics.
- Add a native upload/list/download/delete demo, restart and isolation coverage,
  filesystem failure tests and source-hidden native/Wasm bundle checks.
- Keep zero third-party dependencies, ABI v14, 22 exports and four core assets.

## 0.1.0-alpha.57

- Add typed file controls, bounded BrowserFile metadata and explicit component
  multipart requests. Keep file bytes in the browser and scope references to a
  live view, control, reset epoch and monotonic selection identity.
- Share one pending slot across forms, JSON and uploads; preserve selections on
  validation and retire them on reset, removal, disposal and navigation.
- Bound native FormData serialization before POST, use opaque file part types,
  and retain revision, session, CSRF, quota, deadline and uncertainty semantics.
- Add a Rust/Wasm uploader, JSON responses on the native demo, real-browser
  lifecycle/navigation checks and source-hidden bundle tests without dependencies.
- Move to ABI v14/RVN8 with RUF1 metadata and existing RVJ1 results. Keep 22
  numeric exports, four core runtime assets and two local crates. Rebuild Wasm
  with the matching bridge.

## 0.1.0-alpha.56

- Add explicit Router/FileEndpoint multipart POSTs with bounded text fields and
  one opaque 32 KiB file, under the existing 64 KiB HTTP body ceiling.
- Parse binary framing without text conversion, reject duplicate namespaces and
  unsafe filenames, and redact file metadata and payloads from Debug output.
- Add MultipartForm native file controls and Request::multipart accessors, with
  separate business fields and session-bound hidden/header CSRF verification.
- Apply origin, framing, deadline, session and quota checks before upload handlers;
  preserve atomic registration and exclude uploads from sanitized background reads.
- Add a native memory-only demo capped at eight files, parser/network/CLI tests,
  and binary native FormData coverage without a third-party dependency.
- Retain ABI v13, 22 numeric exports, four core runtime assets and two local crates.

## 0.1.0-alpha.55

- Add component-owned GET/POST/PUT/PATCH/DELETE JSON requests with exact Rust
  decoding, explicit typed results, retained strong If-Match and bounded reports.
- Share the form/JSON pending slot; abort disposed requests and ignore stale
  replies. Bound deadlines and streamed responses without retrying mutations.
- Preserve submission uncertainty in Rust and navigation cache state; distinguish
  validation, conflicts, pre-handler quotas and unconfirmed failures.
- Move the task editor's model, validation, requests and errors into Rust/Wasm,
  including bounded pagination, draft retention and explicit conflict reload.
- Use ABI v13 and RVN7/RVJ1 with the same 22 exports, four core assets and two
  first-party Cargo packages. Rebuild Wasm with the matching bridge.

## 0.1.0-alpha.54

- Add strict flat-object JSON decoding, exact presence/type checks, bounded text
  rules and checked signed/unsigned integer ranges without coercion.
- Add bounded field/code validation reports, generic unknown-key errors and
  fail-closed definition handling. Keep submitted values out of reports and Debug.
- Add explicit Request::decode_json conversions and typed no-store field errors.
- Share CreateTask/ReplaceTask/PatchTask between native and Wasm; reject invalid
  input before any resource or ETag change. Display field errors and retain drafts.
- Compare sixteen shared input cases byte for byte across native and Wasm, with
  live browser form checks and preserved session/CSRF/quota/conflict protections.
- Keep zero third-party dependencies, ABI v12 and 22 function exports.

## 0.1.0-alpha.53

- Add bounded typed entity tags, strong If-Match comparisons and 412/428 statuses.
  Validate request conditions before native callbacks and omit validators on PUT.
- Add a native version source with independent OS-random namespaces, shared
  checked counters and no fallback on entropy, counter or lock failure.
- Check task versions under the mutation lock before atomic replacement, patch
  or deletion. Retired versions cannot target new store lifecycles or recreated IDs.
- Preserve browser drafts on conflicts and require explicit reload before another
  edit. Never fetch a fresh tag to automatically replay an old mutation.
- Extend native, Wasm, source-free bundle and real-browser conflict coverage.
  Keep zero third-party dependencies, ABI v12 and 22 exports.

## 0.1.0-alpha.52

- Add JSON PUT/PATCH and bodyless DELETE in Router and FileEndpoint, with separate
  method slots, atomic registration and precise 405/Allow responses.
- Share source, body, deadline, CSRF and quota checks across every mutation.
  Refuse DELETE bodies and media/encoding headers before native callbacks.
- Extend the task API and browser demo with full replacement, partial updates,
  individual reads and deletion. Validate schemas before atomic state changes.
- Cover method combinations, malformed framing, authorization, shared quotas,
  concurrent updates and deletion in native, CLI bundle and browser checks.
- Keep zero third-party dependencies, browser ABI v12 and 22 exports.

## 0.1.0-alpha.51

- Add a std-only UTF-8 JSON parser and immutable bounded values, with arrays,
  ordered objects, strings, booleans, null and exact number representations.
- Enforce aggregate depth/value/output limits and container/string/number bounds;
  reject decoded duplicate keys, invalid Unicode, extensions and trailing data.
- Add checked explicit numeric conversions without implicit floating-point parsing.
- Add Request::json, redacted JSON Debug and typed no-store responses with fixed
  MIME, nosniff and the blocking CSP.
- Add explicit Router/FileEndpoint JSON POSTs sharing the POST namespace, source
  checks, deadlines and byte budgets; parse JSON before native guards.
- Add the bounded task API example and cover session/header-CSRF/quota guards,
  source-free bundles, concurrency and shared Rust/Wasm parser behavior.
- Keep zero dependencies, ABI v12, 22 exports and four runtime assets.

## 0.1.0-alpha.50 — Unreleased

- Add native std-only token buckets with validated burst/refill policies, monotonic
  integer credits, bounded entries/route bytes, cleanup and aggregate counters.
- Add shared per-pattern and per-session guards. Session debt survives rotation;
  invalid identifiers share a global anonymous quota and unmatched paths share one scope.
- Preserve active debt on capacity saturation and fail closed with 503; reject
  exhausted requests with no-store 429 and bounded typed Retry-After before handlers.
- Add read-only 429 errors across native HTML, initial hydration, navigation and
  Wasm validation without exposing limiter state or credentials to the browser.
- Deliver a distinct RateLimited action failure only for confirmed pre-handler
  refusals; expose retry deltas without automatic POST replay or uncertain mutation.
- Advance browser ABI to v12 with the same 22 exports and four runtime assets;
  retain zero third-party dependencies and process-local, memory-only quotas.

## 0.1.0-alpha.49 — Unreleased

- Add independent OS-generated CSRF tokens bound to native session generations,
  lazy issuance, atomic rotation and removal on revoke, expiry or clear.
- Add a CSRF guard that rejects invalid POST claims before mutation, with keyed
  token lookup and no idle refresh on rejected verification.
- Add typed native hidden fields separate from business data, duplicate/ambiguous
  claim rejection, redacted Debug and empty no-store token responses.
- Let configured Wasm actions fetch a fresh same-origin token before every POST;
  failed preflights send no mutation and do not mark the action outcome uncertain.
- Preserve the existing HTTP checks, dependency-free workspace, ABI v11, 22 exports
  and four runtime assets.

## 0.1.0-alpha.48 — Unreleased

- Add bounded native memory sessions with 256-bit OS-generated identifiers,
  cookie-only lookup and no entropy fallback or browser-side session backend.
- Add validated server-only payloads, capacity limits, monotonic idle/absolute
  expiration, lazy cleanup and aggregate counters without live-session eviction.
- Rotate identifiers/data atomically while preserving the absolute deadline;
  fence stale generations and allow resolved native handles to revoke rotated families.
- Integrate SessionStore guards with Private/403/503 behavior before page caches,
  loaders and handlers; retain typed cookie, native 303 and action protections.
- Support macOS and Linux x86_64/aarch64 OS random devices without unsafe code;
  exclude all session types/backend from Wasm and discard sessions on restart.
- Cover failures, collisions, concurrency, revocation, expiry and source-free bundles
  without third-party dependencies. ABI v11, 22 exports and four assets stay unchanged.

## 0.1.0-alpha.47 — Unreleased

- Add bounded typed request cookies, strict duplicate/syntax rejection before
  callbacks, case-sensitive names and raw values without URL decoding.
- Add host-only response cookies with Secure/HttpOnly/Lax defaults, explicit local
  HTTP mode, validated paths/prefixes/lifetimes and scoped Max-Age=0 removal.
- Emit bounded separate Set-Cookie fields across responses, HEAD and 303 redirects;
  redact cookie values in Debug and preserve atomic append failures.
- Add cookie-capable typed action response handlers while retaining origin, MIME,
  form, revision and inherited guard checks.
- Force no-store and signal browser cache reset without cookie values; cancel
  prefetch work and rebuild layouts on a cookie-changing navigation.
- Keep std-only dependencies, existing cache bypass, loopback transport, ABI v11,
  22 Wasm function exports and four runtime assets. Sessions remain out of scope.

## 0.1.0-alpha.46 — Unreleased

- Add native App, branch, page and endpoint guards with typed allow, private,
  forbidden and unavailable decisions, atomic declaration and bounded inheritance.
- Check every protected GET/HEAD/POST before caches, persistent restoration,
  loaders and handlers; keep Private sticky without altering original headers.
- Generate native-only guard.rs modules with decoded segment prefix matching,
  covering pages, endpoints and unmatched descendants.
- Return fixed read-only 403/503/500 views across HTML, hydration and SPA navigation,
  retiring the old branch and metadata without invoking application error factories.
- Keep guarded browser responses no-store and native public cache reuse explicit;
  refuse guarded native static export and retain public build-artifact semantics.
- Exercise warm/frozen/persistent caches, POST validation, native module exclusion
  and Wasm error mounts with zero dependencies, ABI v11 and the same four assets.

## 0.1.0-alpha.45 — Unreleased

- Add optional bounded persistent storage for regenerated public client output,
  with a private native Unix directory, application identity and exclusive owner marker.
- Restore complete fresh HTML/navigation/hydration generations after clean restart,
  validating revision, origins, declarations, policy and component schemas without loaders.
- Retain original expiry, reject stale/corrupt/incompatible output conservatively,
  and expose separate persistent catalog limits and aggregate restore counters.
- Synchronize atomic catalog publication and path/tag/clear invalidation, including
  unloaded records; fence late work and fail public cache access closed on I/O failure.
- Require explicit empty recovery after interrupted ownership or failed transactions;
  preserve memory-only defaults, private/query fallbacks and cooperative shutdown.
- Verify process restart, forced termination, recovery, corruption, budget and schema
  cases without dependencies. ABI v11, 22 Wasm exports and four assets remain unchanged.

## 0.1.0-alpha.44 — Unreleased

- Add optional bounded expiry and stale-while-revalidate policies to public
  pre-render output, with App and FilePage declarations shared by native and Wasm.
- Reuse the fixed server-cache workers, queue, load/wait budgets, LRU storage and
  counters to refresh complete HTML, hydration/navigation data, metadata and fallback.
- Coalesce stale readers, publish replacements atomically and block past the hard
  deadline without extending old output on failed refreshes. Apply retry backoff.
- Retire missing/no_store branches and fence running/queued output publication on
  explicit invalidation, eviction and worker shutdown. Keep private/query/unlisted
  reads dynamic and regenerated responses no-store.
- Preserve immutable and explicit-invalidation defaults, fresh reads after restart,
  the same build revision, ABI v11, 22 Wasm exports, four assets and zero dependencies.
- Cover concurrent expiry, queue/load saturation, invalidation, failures and
  source-free production refresh in native, Wasm and browser tests.

## 0.1.0-alpha.43 — Unreleased

- Add explicit on-demand regeneration policies for listed public client pre-render
  URLs, including FilePage declarations; immutable build delivery stays the default.
- Load fresh on first runtime read and retain complete validated HTML, hydration
  data, navigation, metadata and native fallback until path/tag invalidation or eviction.
- Reuse bounded ServerCache storage, LRU, coalesced loaders, absolute wait deadlines
  and invalidation fencing without TTL, background jobs or embedded-data fallback.
- Keep private/query/unlisted requests dynamic, regenerate with a sanitized GET,
  and never store errors, invalid output or page/layout no_store results.
- Make regenerated responses no-store and reload after restart; keep storage per
  process with the same build revision, RPR1 format, ABI v11 and four runtime assets.
- Verify source-free production startup, mutation invalidation, concurrency,
  metadata, eviction and failure recovery without third-party dependencies.

## 0.1.0-alpha.42 — Unreleased

- Add explicit public client-page pre-render declarations with enumerated dynamic
  parameters and FilePage builders; keep query, credentialed and unlisted URLs dynamic.
- Prepare bounded immutable HTML, metadata, native fallback and navigation snapshots
  at build time, then embed them in the final native executable with the same Wasm revision.
- Add RPR1 artifact bounds, checksum, exact URL coverage, snapshot/revision, ownership
  and component-seed validation. Runtime delivery executes no page/layout loaders.
- Add a two-stage production pipeline for capable interactive starters; validate the
  embedded bytes before publishing and retain previous completed bundles on failure.
- Keep development dynamic and reject no_store/error builds or combined mutable server
  cache policies. Pre-rendered output remains frozen for its build revision.
- Preserve legacy project builds, four runtime assets and ABI v11 without new dependencies.

## 0.1.0-alpha.41 — Unreleased

- Add opt-in bounded stale-while-revalidate server data policies, with a fixed
  native worker pool and refresh queue. Default cache behavior stays unchanged.
- Serve eligible stale branches without waiting, coalesce refreshes per key,
  and atomically replace validated data, metadata and buffered custom heads.
- Keep the original hard stale deadline after failed refreshes; apply a one-second
  retry backoff and retire entries on no_store, bad requests or missing pages.
- Fence active/queued refreshes on invalidation and eviction, skip abandoned
  routers, and expose aggregate refresh/worker counters plus cooperative stop.
- Reconstruct background GET context from URL/captures and trusted origin,
  without retaining request headers, credentials or body. Private and actual
  progressive responses continue to bypass caching and background scheduling.
- Cover expiry, overload, shutdown and mutation races in native tests and compile
  the same policies into import-free Wasm, without new dependencies or assets.

## 0.1.0-alpha.40 — Unreleased

- Add explicit public server data caching for complete client page branches,
  shared by buffered SSR, native fallbacks and typed navigation. Cache typed
  seeds, metadata and fallback data; render HTML separately for every request.
- Add bounded freshness, payload/entry budgets, LRU eviction, simultaneous
  loader limits and coalescing with absolute wait deadlines.
- Add path/tag invalidation handles that retire matching active loads and
  prevent their late publication. Keep router/origin/revision namespaces separate.
- Bypass Cookie/Authorization and progressive responses; never store or share
  errors, invalid seeds or loader no_store results. Return fixed no-store 503
  responses when cache work cannot safely proceed.
- Add FilePage server-cache declarations and clamp navigation freshness to
  the remaining server entry lifetime. Keep the Wasm ABI and four runtime
  assets unchanged, with no third-party dependencies.

## 0.1.0-alpha.39 — Unreleased

- Add complete typed client metadata replacements shared by SSR, snapshots,
  navigation, caching and history, using the existing Metadata API.
- Add bounded RVP5 envelopes with native, Wasm and JavaScript validation while
  preserving legacy packets, RVI1 snapshots and RVS1 stream framing.
- Require an explicitly configured canonical origin shared by native and Wasm;
  reject foreign origins and sanitize error canonicals/indexing hints.
- Commit managed head slots only with a fully validated branch, clear stale
  values, deduplicate slots and retain CSP, viewport, scripts and stylesheets.
- Match initial streamed metadata to the prepared head and keep loading,
  prefetching and abandoned responses from changing displayed page metadata.
- Add file-starter descriptions and protocol, server, Wasm and browser coverage
  with zero third-party dependencies. ABI v11 and four runtime assets remain.

## 0.1.0-alpha.38 — Unreleased

- Add typed native-only route.rs endpoints for GET/HEAD, form queries, native
  POST forms and typed POST actions, including API-only source trees.
- Exclude endpoint modules from Wasm compilation, keep GET ownership separate
  from pages/assets, and allow page GET plus endpoint POST on the same path.
- Reject duplicate methods, reserved prefixes, dynamic ambiguity and combined
  App route overflows before committing the candidate registry.
- Reuse origin, Fetch Metadata, MIME, body/deadline and action revision checks;
  prevent dynamic handlers from consuming reserved framework prefixes.
- Move the file starter's task actions, native forms and stream into route files,
  add a health endpoint and keep shared server helpers out of Wasm.
- Verify source lifecycle, protected dev recovery, endpoint behavior and Wasm
  exclusion without third-party dependencies. ABI v11, 22 exports, four runtime
  assets and two first-party workspace crates remain.

## 0.1.0-alpha.37 — Unreleased

- Add typed not_found.rs declarations independent of loading/error pairs, with
  inherited client presentations and a root fallback for unmatched URLs.
- Select dedicated 404 views before generic error boundaries while preserving
  healthy validated parents and excluding a failed layout from its own scope.
- Validate read-only Number(404) views and reject forged components, statuses,
  seeds and parent scopes in native and Wasm navigation.
- Preserve real HTTP 404, no-store, noindex metadata and script-free native
  responses. Buffer initial documents under custom 404 profiles before headers;
  retain configured successful heads and the existing progressive navigation wire.
- Add global/task starter views, source diagnostics, atomic dev recovery and
  native, CLI, Wasm and browser coverage without third-party dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.36 — Unreleased

- Generate typed loading.rs/error.rs declarations with explicit file inheritance;
  descendants can replace one presentation while inheriting the other.
- Validate local presentation factories, required states, empty seeds, bounded
  read-only views and absence of initial actions before committing the registry.
- Reuse page/layout boundaries, healthy parent preservation and fresh GET retry;
  retain the existing App fallback and never replay POST actions.
- Include all four source kinds in the discovery and regeneration budgets, reject
  orphan screens and native consumers, and report relative source diagnostics.
- Add starter screens and native, CLI, Wasm and browser coverage with zero
  third-party dependencies. ABI v11, 22 exports and four runtime assets remain.

## 0.1.0-alpha.35 — Unreleased

- Add opt-in `new --interactive --file-routes` with a generated native/Wasm
  registry, fixed and dynamic page directories, and inherited file layouts.
- Use typed `FilePage`/`FileLayout` exports checked by Rust, with named routes,
  component factories and loaders remaining explicit application declarations.
- Validate bounded real-directory trees, route ambiguity and escaped module
  imports before atomically publishing recognized Cargo output files.
- Register complete batches on a private App clone; failed or abandoned batches
  retain the original App, including after ignored registration errors.
- Preserve protected dev rebuilds on page addition, rename, removal and errors;
  keep production independent of route source files and runtime filesystem scans.
- Add native, CLI and Wasm coverage without third-party dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.34 — Unreleased

- Update declared CSS in generated interactive dev projects without invoking
  Cargo, restarting the application or recreating browser components.
- Publish bounded immutable stylesheet snapshots; retain the previous styles
  until every replacement loads and keep the original URL directory for assets.
- Preserve drafts and actions during CSS updates; cancel retired, hidden or
  superseded loads and retry failed loads without replacing working styles.
- Use the normal protected rebuild for Rust, mixed and unregistered changes.
- Keep development declarations, snapshots and CSS revision handling out of
  production; preserve CSP and the four versioned runtime assets.
- Add native, CLI and browser checks with zero third-party dependencies.
  ABI v11, 22 exports and two first-party crates remain.

## 0.1.0-alpha.33 — Unreleased

- Add first-build waiting pages and bounded browser compiler diagnostics for
  generated interactive development projects.
- Publish a new revision only after server readiness; retain the previous app on
  compiler errors and attempt executable rollback after startup failure.
- Reload clean documents automatically; protect drafts, composition, pending
  navigation, native submissions and pending or uncertain actions.
- Offer explicit reload for protected documents, without replaying mutations.
- Isolate development routes, scripts and configuration from production builds;
  preserve native-only CSP, loopback origin checks and the four runtime assets.
- Add native and browser lifecycle coverage without third-party dependencies.
  ABI v11, 22 exports and two first-party crates remain.

## 0.1.0-alpha.32 — Unreleased

- Add explicit early shell hydration, independently of early layout hydration.
- Add named complete-view markers, bounded declarations and attested shell owners.
- Keep early server actions gated; release them only after the final branch commit,
  preserving local state, fields, focus and selection without double hydration.
- Retire early owners on invalid documents, history refusal, cancellation or expiry.
- Add shell draft/menu demos and native/browser lifecycle checks without dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.31 — Unreleased

- Add an explicit App client boundary for root layout and unhandled branch errors,
  with page and layout boundaries retaining precedence.
- Recover typed 400/404/500 errors with no retained layouts through buffered SSR,
  progressive navigation and both initial document profiles.
- Validate complete-branch retirement before releasing owners or removing DOM;
  preserve independent shell state and focused shell fields on same-URL retry.
- Keep minimal fallback when the global error component is invalid, panics or
  queues an action; preserve transport limits and POST replay protections.
- Add global root/branch demos and native/Wasm/browser checks without dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.30 — Unreleased

- Recover through an ancestor child boundary after descendants have been emitted,
  retaining compatible ancestor state and retiring the affected subtree at commit.
- Pair the final RVP3 error with a bounded inert RVH1 prefix of the emitted branch;
  validate both packets, DOM and owners before changing the visible branch.
- Support default and early-hydrated documents, explicit fresh-GET retry, focus
  inside surviving parents and cleanup of departed Wasm owners and listeners.
- Keep root failures on the existing fallback path, reject malformed retirement
  metadata and preserve deadlines, quotas and POST replay protections.
- Add a parent-boundary demo and native/Wasm/browser coverage with no new dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.29 — Unreleased

- Recover a nested layout failure before its HTML is emitted through the nearest
  validated parent child boundary, retaining the entire emitted prefix.
- Adopt shorter typed error branches without resetting live ancestor DOM, drafts,
  counters, focus or selection; rebuild missing descendants on explicit GET retry.
- Keep root failures and boundaries above emitted descendants on the fallback path;
  preserve cancellation, packet validation, quotas and POST replay protections.
- Add a layout recovery demo and native/Wasm/browser checks with no new dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.28 — Unreleased

- Recover eligible streamed leaf failures through existing ClientBoundary, RVP3
  and RVI1 records while retaining the full validated layout chain.
- Adopt healthy early parents on typed 400/404/500 errors without resetting DOM,
  drafts, counters, focus or selection, and expose explicit fresh-GET retry.
- Preserve compatible parent focus and current scroll on same-URL retry; retain
  the current branch when history fails before DOM replacement.
- Keep error packets no-store and reject stale, duplicate, invalidated or
  disposed retries without replaying POST actions.
- Keep generic fallback for failed layouts, invalid error views and boundaries
  which would remove emitted descendants; enforce existing transport budgets.
- Add a recoverable demo, native/Wasm/browser coverage and zero new dependencies.
  ABI v11, 22 exports, four runtime assets and two first-party crates remain.

## 0.1.0-alpha.27 — Unreleased

- Add opt-in layout hydration during the initial document stream, with bounded
  inert RVH1 prefixes and an early same-origin asynchronous module bootstrap.
- Validate each complete layout in Rust/Wasm before activation and adopt live
  parent owners at final commit without resetting DOM, state, drafts or focus.
- Block prepared layout actions until complete branch adoption; reject locally
  without a POST, queued retry or uncertain-mutation latch.
- Retire early owners on malformed data, late failure, native guards, departure,
  deadlines and failed final preparation. Preserve received HTML and field values.
- Finish caught application failures with generic abort HTML to preserve early
  drafts where transport permits; never embed a successful page snapshot there.
- Add an interactive streamed-layout demo, protocol tests and browser lifecycle
  coverage. Keep ABI v11, 22 exports, four assets and zero third-party dependencies.

## 0.1.0-alpha.26 — Unreleased

- Add opt-in initial App document streaming with a fixed typed head, root-first
  layouts, a final RVI1 snapshot and hydration of the existing server nodes.
- Validate route/props/head before headers; keep native and navigation profiles
  separate and retain bounded cooperative transport with single loader execution.
- Require inert completion metadata before streamed document activation. Withhold
  the final suffix until producer cleanup succeeds, including native/RVS1 streams.
- Leave late failures incomplete without diagnostics, partial activation or an
  automatic page GET. Preserve early text editing and native fallback guards.
- Add a generated application demo and native/browser coverage. Keep ABI v11,
  22 exports, four runtime assets and zero third-party dependencies.

## 0.1.0-alpha.25 — Unreleased

- Embed a bounded inert RVI1 initial snapshot with the exact server page and
  module revision. Add a first-party codec without changing navigation formats.
- Hydrate App layouts and pages from their existing DOM and seeds, with no
  second page GET. Keep shell islands and matching adopted parents alive.
- Prepare the full branch before activation and roll back controls, fields,
  listeners and owners after validation or commit failures.
- Allow early text editing on interactive App pages, preserve focus/selection
  and native fallback guards, and refuse automatic actions during input adoption.
- Retire late startup after document departure, hydrate typed SSR error scopes,
  and keep snapshots out of the cache. Native SSR and generic hydration remain.
- Add codec, SSR and browser regression coverage while retaining ABI v11,
  22 numeric exports, two first-party packages and zero third-party dependencies.

## 0.1.0-alpha.24 — Unreleased

- Add explicit App page streaming with ordered root-first layouts and a final
  leaf packet. Share bounded pull-based transport with native HTML streams.
- Add RVS1 identity/framing and RVP4 pending prefixes. ABI v11 retains 22 numeric
  exports, validates loaded schemas and refuses pending leaf mounts.
- Show interactive layout previews while retaining the previous subtree for
  rollback. Preserve matching parent state; commit history/title only at EOF.
- Retire temporary owners on supersession, mutation, invalidation, timeout and
  malformed/truncated replies. Connect typed errors and explicit GET retry.
- Keep streams no-store; retain complete public prefetch/cache behavior and
  native SSR statuses/activation guards. Expose cooperative loader context.
- Enable the generated project demo and add native, compiled Wasm, Node parser
  and real-browser checks. Keep zero third-party Cargo/npm/build/test packages.

## 0.1.0-alpha.23 — Unreleased

- Add explicit HtmlStream plans and typed HtmlProducer fragments, with one pull
  after each completed socket write and no producer thread or response queue.
- Add bounded HTTP chunked responses, escaped document prefixes/fragments,
  no-store and the fixed script-blocking CSP. Keep buffered responses unchanged.
- Intersect chunk, total-byte, yield and duration budgets with server limits;
  expose cooperative deadline/shutdown checks and stop on write failures.
- Keep HEAD lazy. Convert setup failures to buffered 500 responses and leave
  interrupted committed streams incomplete, including producer cleanup panics.
- Add a native report, real-browser progressive checks and a generated /stream
  demo. Cover backpressure, quotas, failures, shutdown and worker recovery.
- Keep client packets atomic, browser ABI v10 and the two first-party packages;
  add no third-party Cargo, build, test or npm dependency.

## 0.1.0-alpha.22 — Unreleased

- Add page/layout loading and error declarations with bounded public manifests,
  read-only transient components and accessible feedback outside live view roots.
- Keep the displayed branch during loading/transport failures and prepare typed
  400/404/500 errors with only validated ancestors at the nearest configured scope.
- Add explicit fresh-GET retry, safe history replacement and delegated retry
  controls. Retire feedback on navigation, invalidation, mutations and disposal.
- Add RVP3 error branches, ABI v10 and a numeric read-only boundary mount export.
  Keep existing command/seed formats, cache safeguards and uncertain-action latch.
- Generate delayed/unavailable project examples and cover failures, SSR, retry,
  late replies, schema checks and resource cleanup without third-party packages.

## 0.1.0-alpha.21 — Unreleased

- Add shared client layout declarations, prefix contexts and bounded root-first
  branches for SSR and client navigation, with explicit leaf-page attachment.
- Preserve matching parent components and DOM; prepare changed branches before
  commit and release temporary owners on failure and departing owners on success.
- Combine page/layout cache policies conservatively and validate every seed
  before prefetch storage or mounting. Refresh recreates the complete branch.
- Add RVP2 branch packets under the existing 64 KiB budget and ABI v9 segment
  selection through the same 21 numeric exports. Keep RVN6, RVA1 and leaf RVP1.
- Generate a task workspace with nested context layouts and a sibling settings
  page. Add native, compiled application Wasm and browser lifetime coverage.
- Keep the two first-party crates and zero third-party dependencies.

## 0.1.0-alpha.20 — Unreleased

- Add explicit public client-page caching with bounded freshness, no-store
  defaults, credential guards, loader-level tightening and representation Vary.
- Add marked hover/focus prefetch, a bounded memory LRU and foreground priority.
  Validate seeds in Rust before storage without retaining views or consuming IDs.
- Add prefetch, invalidate and refresh APIs, bounded queues, deduplication,
  expiration and cleanup. Cache only successful public RVP1 responses.
- Connect cache generations to action start/end, retire late replies and suspend
  storage/speculation after uncertain mutation outcomes until document reload.
- Introduce browser ABI v8 with page validation and RVN6 prefetch flags. Keep
  RVP1/RVA1 formats and the two first-party Cargo packages unchanged.
- Opt the generated public in-memory demo pages in explicitly and add native,
  compiled Wasm and browser coverage for caching, ordering and resource bounds.

## 0.1.0-alpha.19 — Unreleased

- Connect App client pages to shared component/state declarations, GET loaders,
  typed public seeds, SSR, native fallbacks and a persistent application shell.
- Add application-owned Wasm exports through export_application!, with optional
  repository demo code. Browser ABI v7 adds an exact u64 application revision;
  RVN5 commands and RVP1 packets retain their formats.
- Check module revisions before hydration and carry them on navigation/actions.
  Reject stale App requests and unversioned typed actions with 409 before their
  handlers, while retaining native form source-origin validation.
- Generate interactive workspaces with their own shared library and browser
  entry point. Build optimized Wasm and native code with one revision, embed the
  prepared module, watch browser sources and preserve previous builds on errors.
- Produce versioned release server bundles with resources exported by their own
  executable. Verify manifests, ABI/revision records and exact module bytes
  before publishing the new selection.
- Keep the two first-party framework packages and zero third-party dependencies.

## 0.1.0-alpha.18 — Unreleased

- Add shared NavigationRoutes based on PageRoute, named link generation, typed
  navigation pages and RVP1 framing with URL/title/status/schema validation.
- Add a first-party navigation controller with native links, push/replace/pop
  history, latest-request ordering, fetch/read deadlines, loading/errors,
  title/focus updates and scroll restoration. Preserve independent layout islands.
- Prepare seeded pages before committing and retire old page actions/listeners.
  Refuse initial actions during preparation; preserve SSR or the current page on
  preparation failure, and use a document request after failed pop/commit.
- Introduce browser ABI v6 with three numeric page ingress exports, RVN5 link
  tag 15 and opcode 12. Rebuild Wasm with the matching bridge.
- Add home/list/detail/error pages sharing the server task state and native POST
  fallback. Cover direct SSR URLs, history, failures, seeds and page lifetimes.
- Keep the two first-party Cargo packages and zero third-party dependencies.

## 0.1.0-alpha.17 — Unreleased

- Connect Rust/Wasm components to explicit `Router::post_action` routes. Queue
  one request per component, use fixed same-origin POST through native fetch,
  and deliver success, validation or transport failures to the Rust model.
- Add bounded ActionRequest, ActionData, ActionValue and RVA1 reply encoding,
  with strict field names, UTF-8, booleans and exact u64 transport. Reuse native
  form origin/body validation without accepting HTML as an action result.
- Introduce browser ABI v5 and RVN4 opcode 11. Add numeric reply staging and
  completion exports with no input pointers or host imports. Reject stale
  request tokens, abort on disposal and ignore late replies after remount.
- Add fetch/read deadlines, response caps, MIME/status checks, per-view shared
  idle promises and pending-action statistics. Local transport failures keep
  the component usable; no automatic retries or mutation rollback are implied.
- Add interactive server tasks with validation, pending controls, pagination
  and shared native/Wasm state. Keep native form fallback when enhancement fails
  or a native field is already focused. Test frames, lifetimes and real requests.
- Keep only the two first-party Cargo packages, forbid unsafe core code and
  add no third-party build, test or browser dependencies.

## 0.1.0-alpha.16 — Unreleased

- Add explicit `Router::post_form` mutations, separate from GET/HEAD routes,
  with mandatory same-origin verification against the validated local Host.
  Reject absent/null origins and inconsistent Fetch Metadata; ignore forwarded
  headers when determining the target origin.
- Read bounded URL-encoded bodies with strict Content-Length, MIME/charset and
  encoding checks. Share the absolute request deadline across queueing, headers
  and body reads; retain close-after-one-request and shutdown behavior.
- Add `PostForm`, typed `FormData` access and validation markers. Decode once,
  reject duplicate names and malformed UTF-8, normalize CRLF to LF, and enforce
  body, field-count, name and normalized-value limits independently of queries.
- Add internal 303 redirects through `Response::see_other` and method-aware
  Allow headers. Update Method, StatusCode and ServerConfig for this profile.
- Add a script-free server task example with validation, completion/removal,
  concurrent bounded state and Post/Redirect/Get. State lasts until server stop.
- Keep browser ABI v4 and RVN3 unchanged. No third-party dependencies, Wasm
  action bridge, sessions, uploads or persistent database are added.

## 0.1.0-alpha.15 — Unreleased

- Add typed text inputs, textareas, checkboxes, wrapping labels, client forms
  and submit buttons to shared views, with Rust validation and aria-invalid state.
- Add bounded UTF-8 event staging and typed dispatch through numeric browser
  ABI v4. RVN3 adds live value, checked and invalid-state commands. Rebuild Wasm
  with its matching bridge; older ABI/frames are rejected.
- Preserve dirty fields during hydration, cursor/selection during updates, and
  active field subtrees during keyed reorder. Defer composition input and apply
  explicit reset epochs when composition ends.
- Add a task form demo with name/notes validation, completion filtering and
  per-task scope cleanup. Add native, compiled Wasm and real-browser coverage
  for payload bounds, field lifetimes, composition and 1,000 task lifecycles.
- Keep Cargo limited to the two first-party workspace packages. No third-party
  crates, npm packages, input pointers or HTTP mutation routes are added.

## 0.1.0-alpha.14 — Unreleased

- Add explicit sibling keys and ViewSnapshot reconciliation with checked monotonic
  DOM handles. Surviving keyed subtrees keep their nodes across reorder; removed
  handles are never reused. Tag or event changes replace the node lifetime.
- Add list/list-item elements, conditional branches and structural updates for
  inserting, deleting and ordering subtrees. Keep per-view node/depth/event caps;
  allow 256 commands per 64 KiB frame for complete structural transitions.
- Introduce RVN2 and browser ABI v3. Creation carries keys; new commands remove a
  subtree or place a node before a sibling. Dispatch identifies view, node and
  application event so expired bindings cannot alias a new element.
- Validate complete patch plans against copied metadata before DOM changes.
  Remove deleted handlers, retain surviving handlers and restore focus for a
  moved element that remains enabled. Preserve SSR on failed initial hydration.
- Add a shared item-list component with scoped item state, add/remove, counters,
  reverse order, filtering and an empty branch. Filtering retains hidden model
  state; logical deletion releases it. The demo caps live logical items at four.
- Test sibling-key collisions, handle exhaustion/retirement, SSR key mismatches,
  real DOM identity/focus and malformed patches without partial text updates.
  Exercise 1,000 add/remove cycles in native, Wasm and browser tests.
- Keep zero third-party dependencies and the core unsafe-code prohibition.

## 0.1.0-alpha.13 — Unreleased

- Add shared View trees, Component, typed initial props and numeric/boolean state.
  Render HTML and RVN1 command batches from the same view, with fixed IDs and
  limits on nodes, depth, handlers and UTF-8 text.
- Make the browser registry use an application factory. Move counter logic to
  shared example components and add a boolean toggle with a separate event ID.
- Add browser ABI v2: mount accepts root, component, state kind and initial value.
  Keep the RVN1 command encoding and the numeric-only, import-free Wasm boundary.
- Hydrate existing server nodes only after full structure, text, attribute and
  props validation. Preserve text nodes and focus; enable controls after validation.
- Roll back failed hydration without replacing HTML or disturbing other views.
  Share root claims across runtime instances and reject duplicate or nested claims.
- Serve initial counter state 5/10 and a toggle before Wasm loads. Add a script-free
  SSR route and tests for identity, mismatch recovery and 1,000 mount/hydration
  cycles each. Default CSP and static hosting policy remain unchanged.
- Keep zero third-party dependencies, English project text and short comments.

## 0.1.0-alpha.12 — Unreleased

- Add a first-party ruvane-browser workspace crate using the existing reactor,
  with bounded counter instances, numeric events and checked monotonic handles.
- Define the RVN1 typed little-endian DOM protocol: whitelisted elements, inert
  text, button click tokens, disabled state and view removal, with byte/count caps.
- Add our dependency-free ES module bridge: same-origin bounded Wasm fetch,
  version checks, copied memory batches, full prevalidation, scoped DOM handles,
  event cleanup and fail-stop behavior on protocol/trap/ownership failures.
- Isolate reviewed Rust 2024 export attributes in the browser ABI module; keep
  forbid(unsafe_code) in the core, with no unsafe blocks or input pointers.
- Add trusted JS/Wasm media types, an external module helper and explicitly
  interactive HTML responses. Default responses and static hosts retain the
  script-blocking CSP; interactive policy permits same-origin modules/Wasm only.
- Add native protocol/CSP tests, compiled-Wasm/decoder tests using Node built-ins,
  and a real-browser counter and lifecycle harness, including 1,000 mount cycles.
- Extend CI to both first-party packages, the complete dependency graph and Wasm
  compilation/execution. Shared-view hydration remains the next separate step.

## 0.1.0-alpha.11 — Unreleased

- Add a single-threaded, explicitly owned ReactiveRuntime without global state,
  with Signal, Memo, Effect, Source and nested Scope ownership.
- Declare static dependencies explicitly, deduplicate edges and queue each
  computation once; settle memos before effects in registration order.
- Add nested batching, deferred reentrant effect writes, equality-filtered memo
  outputs and configurable live-resource and propagation budgets.
- Use weak handles and monotonic checked identifiers; disposing an owner/source
  cancels queued computations and downstream derived values across scopes.
- Stop a runtime after callback errors, unwinding panics or propagation-budget
  exhaustion; clear scheduling state without claiming transactional rollback.
- Add native counter example and regression tests for diamond graphs, batches,
  cycles, panic recovery, reentrant destruction/Clone, cross-runtime inputs,
  disposed handles and 1,000 mount/unmount cycles.
- Preserve the server/static APIs and script-blocking CSP; browser bindings and
  hydration remain future work. No third-party dependencies or unsafe code.

## 0.1.0-alpha.10 — Unreleased

- Add immutable typed content collections with strict Ruvane headers,
  portable source names/slugs, deterministic order and bounded diagnostics.
- Add a limited Markdown renderer using typed HTML: headings, paragraphs,
  flat lists, emphasis, code and internal links. Escape raw HTML and bound
  input, output, lines, nesting, tokens and delimiter-search work.
- Add bounded flat-directory snapshots and escaped Rust string embedding for
  trusted build scripts, refusing observed symlinks and special/extra files.
- Add App::content_pages to share collection entries and static slug enumeration
  across the server and export, with normal layouts, metadata and CSP.
- Generate content/, a sample article, a content module and a first-party
  Ruvane build dependency. Watch content/build.rs, validate before compilation
  and force Cargo content invalidation for timestamp-preserving edits.
- Migrate mini-blog articles to embedded Markdown while preserving original
  search, Unicode compatibility URLs, common routes and export behavior.
- Test parser injection/complexity, paths, filesystem limits, generated literal
  embedding, content edits/additions/removals, old-content startup restoration,
  failed-build preservation and source-independent export preview.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.9 — Unreleased

- Add App, Page, PageContext and explicit static/server-only page policies,
  sharing one bounded declaration across server routing and static planning.
- Add named PageRoute URLs with exact parameter sets, single encoding,
  registered-name checks and concrete-path ownership validation.
- Add immutable parent layouts with bounded nesting and per-stage HTML limits;
  render one outer document around shared typed fragments.
- Validate concrete static parameters, portable paths, route/file conflicts and
  entry counts before callbacks; fail planning on page errors, unwinding
  panics or output-budget violations.
- Add cached private 404/500 pages, typed page errors and escaped validation
  pages with status 400. Keep protocol errors and server output budgets intact.
- Migrate generated projects and the mini-blog to the common application;
  retain the blog's GET search and original Unicode URLs as server-only pages.
- Add regression/security tests for named links, shadows, nested layouts,
  server/export parity, error privacy, atomic registration and bounded output.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.8 — Unreleased

- Add a dependency-free ruvane binary with new/dev/build/preview commands,
  bounded argument parsing, and explicit generated-project recognition.
- Generate a complete Rust project with shared components, dynamic server
  routes, static page enumeration, CSS, metadata, and a documented app protocol.
- Add bounded source polling, content fingerprints, change consolidation,
  single compilation, same-port restarts, and previous-server preservation or
  restoration after compilation/startup failure. Browser refresh stays manual.
- Add project locking, private run executables, EOF-based server shutdown,
  direct-child wait/kill fallback, bounded process waits, and limited cleanup.
- Add versioned build directories and a validated current pointer, retaining
  old exports and refusing unknown output directories. Preview supports both
  managed versioned builds and existing flat exports.
- Add generated-project integration tests covering actual compilation, dynamic
  serving, automatic rebuilding, failure recovery, preview, process shutdown,
  locks, path/argument injection, watcher budgets, and symlinks.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.7 — Unreleased

- Add a separate preview command for existing completed exports, with bounded
  manifest/file loading and no writes. Document HTTP preview versus file://
  browsing, and ignore generated dist output and macOS folder metadata in Git.

- Add StaticPath and StaticSite for explicitly enumerated static HTML pages
  and trusted assets, with a portable lowercase ASCII path profile, fixed MIME
  suffix checks, collision rejection, and file/count/total byte budgets.
- Add deterministic export to a fresh directory, ancestor symlink checks,
  exclusive file creation, provider-neutral manifest/headers, and a completion
  marker created only after all writes succeed. Partial I/O failures leave
  incomplete output without deleting or replacing existing data.
- Add exact GET/HEAD preview routes backed by shared immutable bytes, retaining
  loopback-only serving, CSP, query validation, and response budgets without
  request-time filesystem access or per-response body copies.
- Add a static mini-blog edition with shared layouts/content, five explicitly
  enumerated articles, path-based pagination, canonical metadata, and local
  assets. Add the blog export subcommand with optional local preview.
- Add tests for path aliases, Unicode/case/device-name rejection, byte/count
  boundaries, reproducibility, symlinks, existing destinations, interrupted
  writes, export completeness, internal links, CSP, and shared response bodies.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.6 — Unreleased

- Add typed page metadata: bounded escaped descriptions, consistent robots
  hints, an internal PNG favicon, and one responsive viewport permitting zoom.
  Metadata setters replace singleton fields, with deterministic output order.
- Add a restricted SiteOrigin parser and CanonicalUrl construction from an
  explicit origin plus InternalUrl, without deriving origins from request headers.
- Send a fixed Content-Security-Policy on every HTTP response, including assets,
  HEAD, parser errors, timeouts, and handler failures. Allow same-origin CSS,
  images, and form actions; block scripts, base elements, and frame embedding.
- Add article/list metadata to the mini-blog, an optional configured public
  origin, distinct pagination canonicals, and noindex hints for searches/errors.
- Add security tests for origin aliases and injection, metadata escaping and
  budgets, singleton replacement, request-header independence, and CSP headers
  throughout the network regression suite.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.5 — Unreleased

- Add typed native GET forms with internal actions, labeled search controls,
  hidden fields, bounded unique names, and escaped labels and values.
- Add explicit QueryMode and Query::parse_with_mode, preserving literal-plus
  behavior in Query::parse and Router::get. Router::get_form selects native
  form plus-to-space decoding while keeping percent-encoded plus signs literal.
- Select raw routes before semantic query parsing, with one selected decoding
  pass and duplicate-key checks before any handler. Path captures retain URI
  semantics; framing and method checks still run before route selection.
- Add a styled mini-blog search form, page reset on new searches, preserved
  pagination filters, and escaped search retention on invalid page numbers.
- Add regression tests for cross-mode key collisions, encoding, UTF-8, byte
  and field budgets, HTML injection, route precedence, HEAD, and rejected framing.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.4 — Unreleased

- Add byte-preserving binary responses with fixed MIME types and fallible
  UTF-8 access. Response::body now returns &[u8]; use body_text for text.
- Add embedded GET/HEAD assets with exact paths, shared route conflict
  detection, static body borrowing, and byte/count budgets checked at registration.
- Add internal stylesheet links and typed void image elements with escaped
  attributes and bounded numeric layout dimensions.
- Style the mini-blog with responsive local CSS, system fonts, and original
  embedded PNG illustrations. Preserve its pagination, filtering, and UTF-8 routes.
- Add security and regression tests for binary framing, HEAD, MIME declarations,
  asset limits, route collisions, undeclared paths, and attribute injection.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.3 — Unreleased

- Add named single-segment GET/HEAD routes, exact-path precedence, and
  registration-time rejection of overlapping dynamic patterns.
- Add bounded, single-pass UTF-8 decoding for dynamic path captures and query
  pairs, with duplicate decoded query keys rejected before dispatch.
- Add typed parameter conversion through FromStr with distinct missing and
  invalid errors, plus validated internal URL and HTML link builders.
- Add a paginated, searchable mini-blog with UTF-8 article slugs and shared
  layouts, exercised by integration tests.
- Extend security tests for ambiguous routes, encoding, Unicode, separators,
  query pollution, byte/count boundaries, and HTML link injection.
- Preserve raw fixed-path matching and raw request accessors. Query validation
  is stricter: malformed UTF-8, controls, empty keys/fields, and duplicate keys
  now return 400 even on fixed routes. A plus remains literal, not a form space.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.2

- Add a loopback-only HTTP development server using a bounded thread pool
  and connection queue, absolute I/O deadlines, and explicit shutdown.
- Add a strict, bodyless HTTP profile with byte and header-count limits,
  Host validation, and rejection of ambiguous framing before dispatch.
- Add exact-path GET routing, automatic HEAD responses, and typed response
  statuses with fixed headers and byte-correct content lengths.
- Add generic panic and oversized-output handling, preserving worker capacity
  when application panics unwind.
- Add a runnable server example, protocol documentation, network security
  regression tests, and CI for stable Rust and the Rust 1.85 minimum version.
- Continue to use no third-party dependencies and forbid unsafe code.

## 0.1.0-alpha.1

- Add HTML text encoding and null-character replacement.
- Add typed element and attribute builders with nested composition.
- Add fragment composition and complete UTF-8 HTML documents.
- Include a runnable example, API documentation, and regression tests.
- Use no third-party dependencies and forbid unsafe code.
