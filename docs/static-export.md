# Static export and local preview

Ruvane 0.1.0-alpha.7 renders an explicitly declared site into HTML and asset
files. Export and preview use only standard Rust and project code. No request
router is crawled, dynamic route is guessed, directory is scanned, or remote
resource is fetched to discover pages.

## Declare the plan

```rust
use ruvane::{Document, MediaType, StaticPath, StaticSite};

let mut site = StaticSite::new();
for slug in ["hello-rust", "typed-urls"] {
    let path = StaticPath::new(&format!("/articles/{slug}/")).unwrap();
    site.page(path, Document::new(slug, "Source content").render()).unwrap();
}
site.asset(StaticPath::new("/assets/site.css").unwrap(),
    b"body { color: #193e33; }", MediaType::Css).unwrap();
```

The application enumerates dynamic-page data and renders each page as `Html`.
Existing components, metadata, internal links, and escaping compose normally.
No raw HTML constructor is added. The builder does not parse generated HTML
to verify content models or discover links; applications declare all needed
pages/assets and check their links. The mini-blog's links are covered by tests.

Registration validates paths, kinds, MIME suffixes, conflicts, and budgets
before mutating the plan or copying bytes. Rejected entries consume no budget.
Accepted content is retained as shared immutable bytes. Assets are copied once
into that storage; they remain trusted source, without decoding or sanitization.
HTML rendering and caller allocations happen before registration's output checks.

## Portable path profile

`StaticPath::new` creates both an internal URL and its output filename:

| URL | Relative output file |
| --- | --- |
| `/` | `index.html` |
| `/articles/` | `articles/index.html` |
| `/articles/page/2/` | `articles/page/2/index.html` |
| `/assets/site.css` | `assets/site.css` |

Directory URLs end with `/` and are required by `StaticSite::page`. Asset URLs
are files and are required by `StaticSite::asset`. Paths begin with one slash.
Segments begin with `[a-z0-9]` and otherwise use `[a-z0-9_-]`. A final asset
filename may additionally contain single interior dots. Segments are at most
64 bytes, with at most 16 URL segments and 240 ASCII bytes in the whole URL.
The generated `index.html` adds a bounded filename to directory URLs.

Uppercase, Unicode filenames, percent escapes, queries, fragments, spaces,
controls, backslashes, colons, repeated slashes, dot segments, leading/trailing
dots, and repeated dots are rejected, never normalized. This avoids case,
encoding, Unicode-normalization, and path-separator aliases. HTML text and
metadata remain UTF-8. Windows device stems such as con/prn/aux/nul and com0–9
or lpt0–9 are rejected, including before extensions. This is a deliberately
narrow profile informed by [filesystem naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file),
not a complete implementation of every filesystem's naming semantics.

Asset suffixes must match their explicit MIME declaration:

| `MediaType` | Suffix |
| --- | --- |
| PlainText | `.txt` |
| Css | `.css` |
| Png | `.png` |
| Jpeg | `.jpg` or `.jpeg` |
| WebP | `.webp` |
| OctetStream | `.bin` |

This prevents an exported plain-text/opaque asset from accidentally receiving
an HTML extension at an external host. It does not inspect bytes or certify a
file's declared format. HTML files come from the typed page renderer.

## Budgets and deterministic metadata

| Budget | Limit |
| --- | --- |
| Content entries | `MAX_STATIC_ENTRIES`: 256 |
| Each content file | `MAX_STATIC_FILE_BYTES`: 1 MiB |
| Entire export, including metadata | `MAX_STATIC_TOTAL_BYTES`: 16 MiB |
| Generated metadata files | Three additional files, including an empty marker |

`len()` counts content entries. `planned_bytes()` counts all output bytes.
`manifest()` emits a sorted TSV document with a format version, kind, URL,
relative filename, fixed content type, and byte length. `required_headers()`
emits a provider-neutral list of required headers. Neither contains timestamps,
absolute output paths, or registration-order-dependent data. Given the same
rendered input bytes, exports have identical file contents across destinations
and registration orders. Filesystem timestamps and permissions are not part
of that byte-reproducibility claim.

## Write to a fresh destination

Call `site.export_to("dist")` after constructing the complete plan. The parent
must already exist, be trusted, and consist of real directories. The exporter
checks ancestor components with `symlink_metadata`, rejects observed symbolic
links and parent traversal, and creates only the destination leaf. It does
not resolve a requested symlink alias into a target; use a trusted canonical
parent path explicitly when an OS directory such as `/tmp` is an alias.

Existing destinations, including files and dangling symlinks, are refused.
Generated subdirectories are created explicitly and files use
[`OpenOptions::create_new`](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new).
There is no overwrite, recursive deletion, automatic cleanup, incremental
update, or destination reuse. Choose a new destination for each build.

The generated tree contains:

- `_ruvane/headers.txt`: required global HTTP headers.
- `_ruvane/manifest.tsv`: sorted content manifest.
- `_ruvane/complete`: empty marker created last, only after every content and
  metadata write succeeds.

The `_ruvane` namespace cannot be registered by application paths. A successful
`ExportReport` counts all files and bytes, including metadata. An I/O error
returns its `io::ErrorKind` inside `ExportError::Io`, with a generic message.
Partial files may remain; an interrupted export has no completion marker and
must not be deployed. The exporter does not remove partial output on retry.

The caller must prevent concurrent hostile changes to the parent, its
ancestors, and the output tree. Checks followed by pathname-based operations
are not a portable capability-based filesystem sandbox. The completion marker
is not an integrity signature, an atomic directory swap, or a power-loss
durability guarantee. Filesystem/OS failures can still occur with valid paths.

## Preview the planned bytes

Pass `site.preview_router()?` to the existing loopback `Server`. The router
captures shared immutable content. It reads no file or manifest per request,
copies no response body, and remains valid after the plan is dropped. Later
plan registrations and on-disk edits do not alter an existing preview.

Only exact declared URL paths are served. There are no encoded aliases,
automatic slash redirects, `index.html` aliases, directory listings, or
metadata endpoints. Undeclared paths return 404. Queries retain URI-mode
validation; a valid query does not change a static response. GET/HEAD, CSP,
Host/framing validation, deadlines, concurrency, and response limits follow
the normal [HTTP profile](http-profile.md). A configured lower response limit
can reject an otherwise valid exported page.

## Preview an existing export

`cargo run --example blog -- preview dist 3001` reads the actual files in an
existing completed export, without writing to it. Open `http://127.0.0.1:3001`
while the command is running. The equivalent library API is
`StaticSite::preview_export("dist")`. Port defaults to 3001 when omitted.

The loader bounds the manifest to 160 KiB and validates every row before reading
declared content: supported format, canonical sorted rows, portable URLs,
derived output filenames, MIME/suffix consistency, entry count, and byte budgets.
It requires the empty completion marker and the current header contract, rejects
symlinks/non-regular files, checks exact lengths and HTML UTF-8, then holds an
immutable snapshot for requests. File reads are bounded even if a file grows.
Undeclared extra files, including build metadata, have no preview routes.

The export and its parents must be trusted and protected from hostile concurrent
changes. Loading trusted generated HTML is not sanitization or authentication;
the unsigned manifest and marker do not prove who created the bytes. An active
preview does not reload disk edits: restart it for another validated build.

## Export the mini-blog

```sh
cargo run --example blog -- export dist https://journal.example
cargo run --example blog -- preview dist 3001
```

The export destination must be new; preview uses the existing directory.
To export and immediately preview in one command, add `--preview 3001` to the
export command. Port 0 can select an available local preview
port. The configured origin supplies canonical metadata, not the local listener
or external network access. The `.example` origin is a documentation placeholder.

The static edition shares article content, layouts, cards, metadata, CSS, and
images with the server edition. It declares the home page, About, five article
pages, three listing pages, and four assets: 14 content files plus three metadata
files. Pagination uses `/articles/`, `/articles/page/2/`, and `/articles/page/3/`,
with distinct canonical URLs and complete internal navigation. Static slugs
are authored explicitly; `ete-rust` is used for the article titled “Été en Rust”.
No transliteration or decoding is guessed by the exporter.

The server edition retains its UTF-8 slugs, GET search form, typed query
decoding, and query-based pagination. The static edition provides lists and
path-based pagination, without a form requiring server-side filtering.

## Hosting contract

Deploy completed content files at the site's URL root. Subpath deployment and
base-path rewriting are outside this version. Directory URLs must resolve to
their generated `index.html`; serve files with the manifest's MIME declarations.
Apply these global HTTP headers, including to error responses where supported:

```text
Content-Security-Policy: default-src 'none'; script-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'
X-Content-Type-Options: nosniff
Cache-Control: no-store
```

Translate the TSV header contract into the hosting platform's configuration;
it is not automatically applied by a generic static host. Keep build metadata
private or exclude `_ruvane` from public deployment, and disable directory
listings. HTML files do not carry HTTP headers. In particular, CSP's
`frame-ancestors` cannot be preserved through a meta tag; see the
[CSP specification](https://www.w3.org/TR/CSP/). Local `file://` browsing also
does not reproduce root-relative URLs or HTTP policy: use the preview or a
correctly configured HTTP host. Export does not configure hosting, publish
files, or turn the development server into a production HTTP/TLS server.

## Verification

Dependency-free tests cover portable path rejection, exact limits, failed
registration, sorted manifests, output bytes/counts, independent exports,
existing destinations, symlinks on Unix, interrupted writes, completion,
shared body behavior, preview isolation, GET/HEAD/CSP, all mini-blog links,
and correct metadata and pagination. These are regression checks, not an
independent security audit or exhaustive filesystem/browser coverage.
