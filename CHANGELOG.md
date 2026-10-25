# Changelog

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
