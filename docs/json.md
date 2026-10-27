# Bounded JSON APIs

Ruvane parses and writes JSON with `std` only. The same value engine works on
native and Wasm targets; HTTP endpoints remain native. The profile follows
[RFC 8259](https://www.rfc-editor.org/rfc/rfc8259.html), with resource limits,
unique decoded keys and Unicode scalar strings. There are no derive macros or
automatic Rust struct conversions.

## Values and numbers

```rust
use ruvane::{JsonValue, JsonNumber};
let value = JsonValue::object(vec![
    ("title".into(), JsonValue::string("Rust 🦀").unwrap()),
    ("id".into(), JsonValue::from(1_u64)),
    ("active".into(), JsonValue::from(true)),
    ("optional".into(), JsonValue::null()),
]).unwrap();
assert_eq!(JsonValue::parse(value.encode().as_bytes()).unwrap(), value);
let number = JsonNumber::parse("18446744073709551615").unwrap();
assert_eq!(number.to_u64().unwrap(), u64::MAX);
```

`JsonValue` is immutable and opaque. `string`, `array` and `object` validate their
inputs. Validated numbers, i64/u64 and booleans convert with `From`; null has an
explicit constructor. Objects retain insertion order. `get`, `as_object`,
`as_array`, `as_str`, `as_bool`, `as_number` and `is_null` inspect without coercion
or mutable access.

Numbers preserve exact bounded text, including `-0`, fractions and exponent
spelling. `to_i64` and `to_u64` require integer literals and check range; unsigned
conversion rejects negative literals, including `-0`. `to_f64` is explicit and
may round or underflow according to IEEE-754; nonfinite results fail. Parsing and
output never perform that conversion implicitly. Other clients choose their own
numeric representation: use strings for IDs a client cannot represent exactly.
Equality compares preserved number text and ordered object members.

`encode()` produces compact UTF-8 JSON, escaping quotes, backslashes and JSON
controls. It preserves number text, not original whitespace or string escape
spellings. It is a JSON document API, not HTML or inline JavaScript embedding.

## Bounds and rejected inputs

| Budget | Maximum |
| --- | ---: |
| Input and encoded output | 65,536 bytes |
| Nested array/object containers | 16 |
| Total values, including root and containers | 1,024 |
| Members/elements per container | 128 |
| Decoded string/key | 4,096 UTF-8 bytes |
| Number representation | 128 ASCII bytes |

Scalars have depth zero; an empty root container has depth one. Object keys do
not count as values, but their bytes and member counts are bounded. Constructors
apply the same aggregate limits as parsing. Children cannot later be mutated to
bypass those bounds, so serialization of a constructed value is infallible and
allocates at most the output byte limit. Application-owned allocations and
arbitrary handler CPU remain application responsibilities.

Parsing accepts one complete value and JSON's four whitespace characters. It
refuses trailing data, comments, trailing commas, NaN/Infinity, non-JSON number
spellings, a leading BOM and invalid UTF-8. Unicode escapes decode once; valid
UTF-16 pairs combine into a scalar, while unpaired surrogates fail. Duplicate
keys are compared after escape decoding: `"x"` and `"\u0078"` cannot name two
members. Keys are case-sensitive and are not Unicode-normalized. Escaped controls,
including null, are valid string data; domain validation decides whether to allow them.

`JsonError` contains only a category, never source text or key/value fragments.
Value/number Debug omits contents; request Debug hides JSON keys and data, and
JSON response Debug masks its encoded body. Accessors, `encode` and response body
access intentionally expose data and must not be logged blindly.

## HTTP routes

```rust
use ruvane::{Router, Response};
let mut router = Router::new();
router.post_json("/api/echo", |request| {
    Response::json(request.json().unwrap())
}).unwrap();
```

A native `route.rs` can use `FileEndpoint::new().post_json(...)` with the same
handler signature. This shares the POST slot with forms and actions; repeated or
mixed declarations fail registration atomically. GET JSON uses an ordinary
read-only GET handler returning `Response::json`. HEAD sends that response's
headers and byte length without its body.

The route selects the body profile. MIME cannot switch a form route to JSON or
vice versa. JSON POSTs accept `application/json`, optionally one UTF-8 charset
parameter. Other media types, parameters, charsets or any Content-Encoding return
415. The existing matching-Origin, Fetch Metadata, Content-Length, byte and I/O
deadline requirements still apply. JSON syntax, UTF-8 and duplicate errors return
400; resource-budget errors return 413. These checks precede guards and handlers.

`Request::json()` is Some only for a successfully parsed JSON POST. `form_data()`
is None there. Forms, GET/HEAD and sanitized pre-render/background GET requests
have no JSON body. Query parameters and captures stay separate. Supplied stale
application revision headers still return 409. JSON endpoints do not require an
RVA1 action revision and do not change that wire protocol.

`Response::json(&value)` returns 200 with fixed `application/json`, no-store,
nosniff and the default blocking CSP. Status/cookie helpers remain available.
There is no unchecked JSON MIME in the raw byte/asset API. Transport, parser and
guard refusals keep the generic framework error formats; handlers may return
fixed JSON errors. Clients must check status and MIME before decoding.

## Session and mutation protection

Apply session, CSRF and quota guards explicitly. A session CSRF guard requires
`Ruvane-Csrf-Token` for JSON POSTs. JSON fields never supply that claim:
`_ruvane_csrf` in an object is ordinary application data. Missing, forged,
cross-session and retired header tokens fail before mutation. Native forms retain
their separate reserved hidden field. There is no query/cookie token fallback.

Quota guards see validated immutable data and run before handlers, after earlier
authorization/CSRF guards. Existing 429/503 behavior remains. Native page caches
do not wrap API handlers. Validate allowed fields, domain types and permissions
before applying changes. JSON grammar validation is not authorization. The
loopback-only transport, unsupported methods/compression/chunking and CORS scope
remain unchanged.

## Task example

```sh
cargo run --offline --locked --example json_api -- 8080
```

Open the printed address. The example creates a labeled demo session, lists
GET `/api/tasks`, creates with JSON POST at the same URL and logs out with JSON
POST `/api/logout`. It applies session, CSRF and quota guards. The store caps
entries at 32 and titles at 256 UTF-8 bytes, validates the complete schema and
constructs a response before committing a new record. Data is shared demo memory,
discarded on restart; this is not real user authentication or a database. The
first-party browser module renders titles with text nodes and never replays POST.

CLI tests run a bundle with source files hidden, including concurrency, guard
ordering and the shared parser inside an import-free Wasm component. Browser ABI
v12, the 22 exports and four runtime assets stay unchanged.
