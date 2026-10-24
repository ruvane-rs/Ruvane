# Ruvane

Ruvane is an experimental web framework written from scratch in Rust. Its goal
is to bring server-side and browser-side development into a single stack,
without third-party dependencies.

The current alpha provides HTML rendering primitives: escaped text, typed
elements and attributes, fragment composition, and complete UTF-8 documents.
HTTP transport, routing, server actions, and browser-side reactivity are
planned for future versions.

## Principles

- **Zero third-party dependencies**, including build and test dependencies.
- **Explicit composition** through typed elements and attributes.
- **Escaped text by default**, with no raw HTML constructor.
- **No unsafe code** in the crate.

## Example

```rust
use ruvane::{Attribute, Document, Tag, element};

let message = "Hello <world> & Rust";

let body = element(Tag::Main)
    .attr(Attribute::Class, "page")
    .child(element(Tag::H1).child("Ruvane"))
    .child(element(Tag::P).child(message));

let page = Document::new("Ruvane", body)
    .language("en")
    .render();

println!("{page}");
```

Strings passed as children are rendered as text. The message above becomes
`Hello &lt;world&gt; &amp; Rust`. Existing `Html` fragments retain their markup
when composed. Serialized output is available through `as_str()`,
`into_string()`, or `Display`.

## Rendering API

| Item | Purpose |
| --- | --- |
| `text` | Render a string as escaped HTML text. |
| `element` | Build an element selected from the `Tag` enum. |
| `Attribute` | Set `id`, `class`, or `title` with quoted, escaped values. |
| `fragment` | Join rendered fragments in order. |
| `Document` | Render a complete document with a doctype, language, UTF-8 declaration, title, and body. |

## Security scope

The renderer escapes `&`, `<`, `>`, double quotes, and apostrophes, and replaces
null characters with the Unicode replacement character. Element and attribute
names come from enums. Script and style elements, event-handler, style, and URL
attributes, foreign content, and raw HTML are outside the current API.

Ruvane does not sanitize existing HTML, encode JavaScript, CSS, or URLs, or
validate HTML nesting and language tags. Applications remain responsible for
input and output size limits. The alpha has not undergone an independent
security audit. Reporting information is available in SECURITY.md.

## Project status

Ruvane is in early development. The alpha API may change between releases.
This version is a rendering foundation; it does not yet provide a complete
full-stack runtime.

Requires Rust 1.85 or newer and uses the Rust 2024 edition.

## License

MIT. See the LICENSE file.
