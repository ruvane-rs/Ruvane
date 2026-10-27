# Typed JSON validation

Alpha.54 adds explicit JSON decoding and bounded field errors using std only.
The same decoder, text rules and checked conversions compile in native Rust
and Wasm. Applications implement `TryFrom<&JsonValue>` for their input types.
There are no derive macros, implicit coercions or automatic business rules.

## Decode a record

```rust
use ruvane::{JsonObject, JsonValue, TextRules, ValidationErrors};
struct CreateTask { title: String }
impl TryFrom<&JsonValue> for CreateTask {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut input = JsonObject::new(value, &["title"]);
        let title = input.required_string(
            "title", TextRules::new(1, 256).unwrap().non_blank().no_controls(),
        );
        input.finish()?;
        Ok(Self { title: title.unwrap().to_owned() })
    }
}
```

`JsonObject::new` requires an object and declares its allowed field names.
Unknown keys add one object-level `unknown_field` issue; their names and values
are omitted. Getters record issues and return Option. Always call `finish()`
and handle its result before constructing or applying application data. Only
then may required values be unwrapped. Declaring a field does not make it
required; the selected getter determines that rule.

`Request::decode_json::<CreateTask>()` invokes the application's conversion on
an already parsed JSON body. A missing JSON body produces `expected_object`.
The helper does not authenticate, run guards, evaluate ETag conditions or commit
state. Existing route-selected MIME, body and JSON grammar checks stay active.
The original JsonValue parser and builder budgets are unchanged.

## Presence, types and domain rules

| Accessor | Accepted input |
| --- | --- |
| `required_string` / `optional_string` | JSON string satisfying explicit TextRules |
| `required_bool` / `optional_bool` | JSON true or false |
| `required_u64` / `optional_u64` | Exact unsigned integer literal within inclusive bounds |
| `required_i64` / `optional_i64` | Exact signed integer literal within inclusive bounds |
| `field` | `JsonField::Missing`, `Null`, or `Value(&JsonValue)` |

Optional getters allow absence; explicit null adds `null_not_allowed`. They never
turn missing values into false or empty text. `JsonField` exposes presence for
applications that deliberately support nullable fields or nested/custom values.
The decoder itself describes flat objects; nested paths and collection schemas
remain application code.

TextRules use UTF-8 bytes, without trimming or normalization. Check order is
minimum bytes, maximum bytes, nonblank, then control characters. `new` returns
None for inverted bounds or a maximum exceeding the JSON string budget.
`no_controls` rejects Unicode control characters, including tabs/newlines.
Other domain rules can add a fixed `ValidationCode::InvalidValue` or
`InvalidCharacters` through `input.error(Some("title"), code)`.

Numbers never convert through floating point. Fractions/exponents and wrong
JSON types add `expected_integer`; integer overflow, an unsigned negative
literal (including -0), or application range failure adds `out_of_range`.
Booleans, strings and numbers are never silently converted into each other.
For PATCH, use optional getters and `require_nonempty()`: omitted fields stay
unchanged and an empty object fails. The task example rejects null explicitly.

## Error reports and budgets

`ValidationErrors` contains only declared static field names and fixed codes.
`ValidationIssue::field()` is None for an object-level issue. `iter`, `len`,
`truncated`, `is_definition_error` and `finish` expose report state. Repeated
identical field/code pairs are deduplicated. Overflow still fails validation;
no partial report may be interpreted as success.

| Budget | Maximum |
| --- | ---: |
| Declared fields | 32 |
| Declared field name | 64 ASCII bytes |
| Distinct retained errors | 16 |
| Encoded validation response | 4,096 bytes |

Names are nonempty ASCII identifiers starting with a letter or underscore.
They must be application declarations, never keys copied from untrusted objects.
Invalid/duplicate declarations, undeclared getter/error fields or inverted integer
bounds mark a definition failure. They produce a generic 500 when serialized;
user input cannot select or repair these definitions. Even an empty error report
passed to the error response constructor produces a generic 500.

`Response::validation_errors(&errors)` returns fixed application/json, no-store,
nosniff and the blocking CSP. Normal domain failures use status 400:

```json
{"error":"Bad Request","errors":[{"field":"title","code":"required"},{"field":"completed","code":"expected_boolean"}],"truncated":false}
```

Object-level errors omit `field`. Codes and default messages are fixed; reports
contain no submitted values, arbitrary error text or unknown input names. Debug
redacts decoded fields/issues and input DTOs in the example. Raw accessors and
application-owned data still need deliberate logging rules.

JSON syntax/transport errors and guard denials retain their existing response
formats. Clients must check status, MIME and the bounded report shape before
using field errors. Schema validation never substitutes for authorization.

## Shared task inputs and the browser example

`examples/support/task_input.rs` defines CreateTask, ReplaceTask and PatchTask.
Both the native API and the Wasm fixture compile this same module. Native
handlers decode after session/CSRF/quota guards, then check resource versions
under the mutation lock. An invalid field leaves the record and ETag unchanged.
No partially decoded replacement or patch is committed.

The JSON API demo displays declared-field errors next to the title/completion
controls, with aria-invalid and aria-describedby. It accepts only bounded reports
with known fields/codes and renders fixed messages through textContent. A 400
keeps the draft and loaded tag, so a corrected explicit save can succeed. A
concurrent modification still yields 412 and requires explicit reload. There
are no automatic mutation retries or fresh-tag substitution for an old draft.

The CLI/browser fixture additionally exposes a Rust/Wasm validation form. Invalid
submissions keep its draft and show the same rule messages as the server; the
server independently revalidates submitted JSON. Sixteen shared cases compare
native and Wasm reports byte for byte, including Unicode, null/absence, unknown
keys, wrong types and exact integer extrema. All business code remains application
code; no new Wasm export or action wire format is introduced. ABI v13, 22 exports
and four core runtime assets remain unchanged.

Alpha.55 adds `ValidationErrors::from_json` and `JsonReply::validation` for bounded
remote reports. Supply trusted static fields; unknown fields/codes and extra data
are rejected. See [component JSON requests](json-client.md).
