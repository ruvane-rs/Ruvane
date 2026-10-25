# Routing and URLs

Ruvane provides dynamic routes, typed conversions, bounded query parsing,
and internal links without third-party dependencies. Alpha.5 adds explicit
URI/form query modes and native GET forms.

## Route selection

`Router::get("/articles/{slug}", handler)` captures one nonempty segment.
Capture names are unique ASCII identifiers: `[A-Za-z_][A-Za-z0-9_]*`, up to
64 bytes each, with at most `MAX_PATH_PARAMS` (16) captures. A capture occupies
an entire segment. Wildcards, optional segments, embedded captures, and
regular expressions are unsupported. A trailing slash remains significant.

Matching uses raw, case-sensitive paths. Fixed paths take priority over
dynamic routes, independently of registration order. Two dynamic patterns
that could match the same raw path are rejected, even if one has more literal
segments. For example, `/{category}/new` and `/articles/{slug}` overlap and
cannot coexist. A fixed `/articles/new` can coexist with `/articles/{slug}`.
Duplicate patterns also fail at registration.

Once a dynamic route is selected, each captured segment is percent-decoded
exactly once as UTF-8. Empty values, Unicode control characters, decoded `/`
or `\`, and values equal to `.` or `..` return 400 before the handler runs.
Invalid UTF-8, malformed encoding, and values beyond the byte budget also
return 400. There is no fallback to another route after a capture error.

Fixed routes retain alpha.2's exact raw matching. `/articles/new` and
`/articles/%6Eew` select different raw paths: the latter can match a dynamic
route and produce the decoded capture `new`. An existing fixed route
`/articles/%FF` can still match literally; the new UTF-8 capture decoder is
not applied to fixed paths. These distinctions are not authorization rules.
Applications must check access to the identified resource, not assume that
literal route precedence protects all equivalent parameter values.

`Request::path()` and `Request::query()` retain their raw representation.
`Request::param("slug")` returns a decoded capture. `param_as::<T>` delegates
to `T: FromStr`, distinguishing `ParamError::Missing` from `Invalid`. The
handler selects its error response; failed conversions do not automatically
return HTTP 400. The selected Rust type defines numeric syntax and overflow
behavior. Applications validate domain-specific ranges separately.

## Query parsing

For accepted GET/HEAD requests, raw route selection chooses the query mode
before semantic decoding. Every query is then parsed before any handler runs,
including fixed routes and unknown paths. `Query::parse` exposes the original
URI-mode parser without a server; `Query::parse_with_mode` accepts an explicit
`QueryMode`. Both take raw query data without the structural leading `?`.

- Split on `&`, then on the first `=` in each field, before decoding.
- Percent-decode keys and values exactly once, requiring valid UTF-8.
- `QueryMode::Uri` preserves literal `+`. It is the default for `Query::parse`,
  `Router::get`, embedded assets, and unknown paths. Spaces use `%20`.
- `QueryMode::Form`, selected by `Router::get_form`, maps raw `+` to spaces
  before percent decoding. `%2B` remains a plus; `%252B` becomes literal `%2B`.
  Mode selection never depends on a query field. Path captures always use URI
  decoding, including on form routes.
- A bare key and `key=` both have an empty string value.
- Empty keys, empty fields, controls, and invalid encoding are rejected.
- Reject duplicate decoded keys, including `a=1&%61=2`, rather than choosing
  the first or last value. Keys are case-sensitive. No Unicode normalization
  or case folding is performed.
- Preserve pair order. `Query::get`, `get_as::<T>`, and `iter` expose decoded
  data; `Request::query_params()` borrows the already parsed query.
  `Query::mode()` and `Request::query_mode()` expose the selected convention.

For example, `x=a%26b%3Dc&plus=a+b&once=%2526` yields `x = "a&b=c"`,
`plus = "a+b"`, and `once = "%26"`. Decoded values never become separators
or undergo a second framework decoding pass. Do not decode them again before
using them as application values.

In form mode, `a+b=1&a%20b=2` is a duplicate-key error, while
`a+b=1&a%2Bb=2` has distinct keys `a b` and `a+b`. URI mode reverses those
outcomes. The framework never preparses a form query under URI semantics.
Existing `InternalUrl::query` output works in both modes: it encodes literal
plus signs as `%2B` and spaces as `%20`. Native forms submit spaces as `+`.

Separating structure before decoding and avoiding repeated decoding follows
[RFC 3986 section 2.4](https://www.rfc-editor.org/rfc/rfc3986.html#section-2.4).
This parser does not implement nested objects, bracket notation, JSON, or an
application-specific schema. Form mode is a strict profile of URL-encoded
queries; malformed UTF-8, duplicate keys, and empty fields are rejected rather
than processed permissively. See [GET forms](get-forms.md).

## Internal links

`InternalUrl::new` validates an already encoded path beginning with a single
`/`, preserving its encoding. Schemes, external authorities, fragments, raw
query strings, repeated slashes, decoded separators, dot segments, invalid
UTF-8, and controls are rejected. Each nonempty path segment also obeys the
decoded component budget.

`segment` appends an **unencoded** value, escaping reserved characters and
UTF-8 bytes. It rejects empty/dot/separator/control values. `query` appends
unencoded, unique keys and values, preserving insertion order. A segment
cannot be appended after query construction begins. Percent signs supplied
as data become `%25`: a literal value `%2F` becomes `%252F` on the wire.

`link(&url, label)` produces a quoted `href`, escaping HTML attribute syntax
after URL encoding. String labels are escaped text; existing `Html` labels
compose normally. There is no raw `href` setter or unchecked URL constructor.
This ordering follows the separate URL and HTML contexts described by
[OWASP](https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html#output-encoding-for-url-contexts).

An `InternalUrl` does not prove a route exists, authorize access, validate
application identifiers, or constrain subsequent application decoding. Nested
anchors and other HTML content-model rules remain the caller's responsibility.
The general element builder still has no arbitrary URL attributes.

## Fixed budgets

| Budget | Limit |
| --- | --- |
| Decoded capture, query key, or query value | `MAX_COMPONENT_BYTES`: 1024 UTF-8 bytes |
| Query pair count | `MAX_QUERY_PAIRS`: 32 |
| Dynamic route captures | `MAX_PATH_PARAMS`: 16 |
| Capture name | 64 ASCII bytes |
| Complete encoded internal URL | `MAX_INTERNAL_URL_BYTES`: 4096 bytes |
| Complete raw query | `MAX_INTERNAL_URL_BYTES`: 4096 bytes |

These fixed alpha limits apply in addition to the configurable HTTP request
line/header limits. A request line may exceed its limit first and produce
414. Otherwise query and capture validation errors produce 400 before the
handler. Route registration and standalone URL/query construction instead
return typed errors without including input in their messages.

The mini-blog stores five articles in source, shows two per page, preserves
its search filter in pagination links, and uses checked page arithmetic.
Its route builder is shared with the network integration tests. It adds no
storage, mutations, authentication, public listener, or browser runtime.

## Changes from alpha.2

Raw fixed-path behavior and raw request accessors are preserved. Query
validation is intentionally stricter for all routes: duplicates, empty
keys/fields, invalid UTF-8, and decoded controls now return 400. Callers that
previously interpreted the raw query themselves must account for this
validation and the explicit literal-plus convention.

`RouteError` has an additional `AmbiguousRoute` variant. Alpha APIs may change;
applications exhaustively matching this enum must handle the new variant.
