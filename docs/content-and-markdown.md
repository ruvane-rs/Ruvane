# Local content and limited Markdown

Alpha.10 adds immutable typed content collections, a bounded Markdown profile,
flat local directory loading for trusted build tools, and literal embedding in
generated applications. There are no third-party dependencies or unsafe code.

## Documents

A `.md` file begins with an exact Ruvane header:

````text
---ruvane
title: Your first article
slug: welcome
description: A small beginning, written in Markdown.
---
## Welcome

Write **clear words** and *new ideas*. See [the home page](/).

```rust
let message = "Hello";
```
````

The three required fields may appear in any order, exactly once. Names and
values are separated by `: `; unknown fields, padding, empty values and control
characters are errors. Title/description are limited to 1024 UTF-8 bytes.
This format is a three-field Ruvane header, not YAML or TOML. Empty bodies are
allowed. CRLF and LF line endings render equivalently; embedded bare carriage
returns and controls are refused. Tabs are allowed in bodies, not metadata.

Slugs have at most 64 lowercase ASCII bytes and follow the portable single
segment `StaticPath` profile, including Windows device-name rejection. Source
names are similarly portable basenames ending in `.md`; no directory separators,
percent escapes or Unicode filenames are accepted. Document text remains UTF-8.
Source filenames determine collection order; numeric prefixes can set an
editorial order independently of the URL slug. Duplicate names/slugs fail.

```rust
use ruvane::{ContentCollection, ContentSource};

let articles = ContentCollection::new(&[
    ContentSource::new("01-welcome.md", include_str!("../content/welcome.md")),
])?;
for article in articles.iter() {
    println!("{}: {}", article.slug().as_str(), article.title());
}
```

The snippet assumes an application source file next to its `content` folder.
`ContentEntry` exposes source name, `ContentSlug`, title, description and typed
HTML body. `get` looks up a decoded slug as a key; it never opens that key as a
filesystem path. Collections own their validated data and remain independent of
later file changes. Parsing and rendering happen when constructing a collection.
Use an existing immutable collection in request callbacks rather than parsing
again. The generator and mini-blog follow this model.

## Markdown profile

Supported constructs are deliberately limited and deterministic:

- ATX headings `# ` through `###### `, without trailing-marker stripping.
- Paragraphs separated by empty lines; soft newlines remain text newlines.
- Flat unordered lists with `- `, and flat ordered lists with 1–9 digits plus
  `. `. Ordered output starts at 1; source item numbers are not preserved.
- `**strong**`, `*emphasis*`, single-backtick inline code, and backslash escapes
  for ASCII punctuation. Delimiters use the documented simple scanner; mixed
  delimiter edge cases need not match CommonMark.
- Triple-backtick fenced code blocks. An optional language is at most 32 ASCII
  letters/digits/underscores/hyphens and becomes an escaped `language-*` class.
  Closing fences are exactly three backticks. Unclosed fences are errors.
- `[label](/internal/path/)` links. Targets must pass query-free `InternalUrl`
  validation. Labels can use inline formatting. Nested interpreted links and
  image syntax are unsupported; a recognized unsupported construct is an error.

Raw HTML is always escaped through typed HTML builders. Entities are not decoded,
so entity-like text is escaped again. JavaScript expressions/import-looking text
remain ordinary text. Code spans/blocks display their contents without executing
or recursively parsing them. Unmatched inline markers are ordinary text. This
is not a CommonMark, GitHub Markdown, YAML or MDX implementation; tables, nested
lists, reference links, autolinks, images and executable components are outside
the profile. Other unrecognized syntax remains escaped text.

Links allow internal paths only, with no fragment/query in this initial profile.
Schemes, protocol-relative URLs, dot segments, invalid UTF-8, controls and decoded
separators are refused. Percent data is decoded only by the existing URL checks;
HTML attributes are independently escaped. Link validity does not prove that a
route exists; named application links and static link checks remain separate.

`markdown(&str)` provides the same bounded body renderer without a document
header. Diagnostics expose a fixed error kind, a validated source basename when
available, and a one-based line where the failing construct begins. Error text
never includes document content, unsafe filenames, external URLs or arbitrary
paths. For multiline constructs the opening line identifies the construct.

## Application integration

```rust
app.content_pages(
    PageRoute::new("article", "/articles/{slug}/")?,
    Some("site"),
    articles,
    |context, article| {
        Ok(Page::new(article.title(), article.body().clone())
            .metadata(context.metadata(article.description()).unwrap()))
    },
)?;
```

The pattern must have exactly one capture named `slug`. This method uses the
collection's `page_mode()` to enumerate static pages, and its immutable entries
for server rendering. Unknown slugs return 404. Existing layout, route ownership,
canonical, CSP, HEAD, page error and output-budget rules apply. `page_mode()` is
also available for applications needing a custom registration.

## Generated applications and CLI

`ruvane new` now supplies `content/welcome.md`, `src/content.rs` and `build.rs`.
The application lists its articles on the home page and serves their named
`/articles/{slug}/` routes. Add a valid `.md` file to add a page; remove one to
remove it from the next executable and export. A collection must remain nonempty.

Ruvane is the generated project's first-party dependency for both runtime and
build time. Its trusted build script calls `ContentBundle::read_dir("content")`
and `rust_module()` to produce an index in Cargo's `OUT_DIR`. Filename and text
values are serialized as escaped Rust string literals; document text never
supplies tokens outside those literals. The generated module is included through
`OUT_DIR`. The build script refuses symlinks and unknown existing module files
before replacing its known generated output. Cargo configuration, build scripts,
toolchain and output-directory parents remain trusted executable inputs.

`dev` watches `content/` and `build.rs` in addition to the existing source list.
Both are optional for older generated projects. Before Cargo, the CLI validates
any present content directory. A content error keeps the previous server running.
A changing build revision environment value makes Cargo rerun the generated build
script even for equal-size edits whose timestamp was restored. It is change
invalidation, not a cryptographic integrity token. Direct Cargo builds use normal
Cargo invalidation; the explicit revision behavior belongs to the CLI.

Each compiled executable contains a fixed content snapshot. The old executable
can therefore restore its original text if a new server fails to start. No
request-time disk lookup or parsing is needed. A failed build retains the
previous static `current` selection. Source files are not needed by preview of
a completed export. Static starters require manual browser refresh; interactive
starters support [development feedback](development.md).

The loader accepts a flat directory of regular `.md` files, with protected real
ancestors. Subdirectories, symlinks, special files, extra files, invalid UTF-8,
malformed content and budget overflows fail closed. It reads bounded bytes before
validation. Directory/file checks are observations; hostile concurrent mutation
cannot be sandboxed with this portable standard-library API. Keep source and
Cargo output parents protected. Content files and their metadata are public site
inputs and must not contain secrets.

## Limits and verification

| Resource | Limit |
| --- | --- |
| Documents per collection | 128 |
| Bytes per complete document / standalone Markdown | 64 KiB |
| Aggregate collection source bytes | 4 MiB |
| Lines per input / bytes per line | 2048 / 8192 |
| Inline nesting | 8 levels |
| Emitted block/inline tokens | 8192 |
| Parser byte-scan work | 1,048,576 steps per document |
| HTML per body / aggregate bodies | 1 MiB / 8 MiB |
| Generated escaped Rust module | 16 MiB |

The work counter bounds delimiter-search and scan work and is not a wall-clock
deadline. Input/output and nesting limits additionally bound rendering and
allocation. Builders check bodies after their local allocation; caller-owned
input allocation and arbitrary application callbacks remain outside the parser.
A page's final document must also fit the App/static/server budgets.

Tests cover strict metadata, duplicate keys/slugs, filename/path/device aliases,
UTF-8/controls, raw HTML/entity/code payloads, links, fence errors, deterministic
mutations, adversarial delimiter scans, filesystem symlinks/special files and
limits. Integration tests compile a generated project with Rust-looking content,
check server/export parity, force equal-mtime content edits, add/remove pages,
preserve the last server on invalid content, restore an old executable with its
old content after startup failure, retain static builds on validation failure,
and preview exported pages after their source has been removed. These are
regression checks, not an independent audit or a general-purpose content sandbox.
