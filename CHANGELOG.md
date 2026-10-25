# Changelog

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
