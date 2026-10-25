# Page metadata and browser policy

Ruvane 0.1.0-alpha.6 adds typed metadata, configured canonical URLs, and a
fixed Content Security Policy using only the Rust standard library.

## Metadata construction

```rust
use ruvane::{Document, InternalUrl, Metadata, Robots, SiteOrigin};

let origin = SiteOrigin::parse("https://journal.example").unwrap();
let path = InternalUrl::new("/articles").unwrap().query("page", "2").unwrap();
let metadata = Metadata::new()
    .description("Notes on Rust and the web.").unwrap()
    .robots(Robots::IndexFollow)
    .canonical(origin.canonical(&path))
    .favicon_png(InternalUrl::new("/assets/mark.png").unwrap());
let page = Document::new("Journal", "Latest articles").metadata(metadata).render();
```

Each document renders its charset and title first, followed by one fixed
`width=device-width, initial-scale=1` viewport. It does not disable user zoom.
Optional fields follow in a deterministic order: description, robots,
canonical link, PNG favicon, then the existing stylesheet list. Metadata uses
one slot per field. Repeated setters replace the previous value;
`Document::metadata` replaces the entire metadata set rather than merging it.

Descriptions must contain non-whitespace text, contain no Unicode control
characters, and fit `MAX_METADATA_TEXT_BYTES` (1024 UTF-8 bytes). Failures
return `MetadataError` without reflecting input. Values remain plain text and
are escaped in quoted HTML attributes. Existing title/language escaping and
their application-managed allocation limits remain unchanged.

`Robots` offers `IndexFollow`, `IndexNoFollow`, `NoIndexFollow`, and
`NoIndexNoFollow`. No robots hint is emitted by default. These are crawler
hints, not access controls; their interpretation depends on the crawler.
See [Google's robots metadata documentation](https://developers.google.com/search/docs/crawling-indexing/robots-meta-tag).

`favicon_png` requires an `InternalUrl` and emits a void link with the fixed
type `image/png`. This declares a type; it does not inspect image bytes or
prove that the route exists. The same internal URL/escaping contracts apply
as for images and stylesheets. No arbitrary metadata names, `http-equiv`,
event handlers, raw HTML, or unrestricted URL setters are added.

## Configured canonical origins

`SiteOrigin::parse` accepts a deliberately restricted origin profile:

| Component | Accepted profile |
| --- | --- |
| Scheme | Literal lowercase `http://` or `https://` |
| DNS | ASCII alphanumeric/hyphen labels, each 1–63 bytes, at most 253 bytes total; label edges are alphanumeric; final label begins with a letter |
| IPv4 | Exactly four canonical decimal octets, such as `127.0.0.1` |
| IPv6 | Bracketed address accepted by `std::net::Ipv6Addr`, without a zone ID |
| Port | Optional 1–5 decimal digits, value 1–65535 |
| Suffix | Optional single trailing `/`; no other path, query, or fragment |
| Input budget | `MAX_SITE_ORIGIN_BYTES`: 320 ASCII bytes |

Credentials, escapes, backslashes, whitespace/controls, trailing DNS dots,
non-ASCII hosts, and `xn--` IDNA labels are rejected. Numeric-host aliases
such as shortened IPv4, hexadecimal IPv4, and integer hosts are rejected.
The final-DNS-label restriction avoids selecting the browser's numeric-host
interpretation; this parser is not a full implementation of the
[WHATWG host parser](https://url.spec.whatwg.org/#concept-host-parser).

DNS is lowercased, IP literals are serialized by the standard library, port
digits are normalized, and default `http:80`/`https:443` ports are omitted.
The normalized origin has no trailing slash. Invalid inputs return a generic
`OriginError`. Parsing performs no DNS lookup, network request, IDNA conversion,
certificate validation, or TLS negotiation.

`origin.canonical(&path)` creates a `CanonicalUrl` by joining the normalized
origin with a validated `InternalUrl`. It preserves the encoded path/query
without another decoding pass. Combined length is bounded by the origin
budget plus `MAX_INTERNAL_URL_BYTES` (4096). HTML rendering then escapes the
quoted `href`; URL construction and HTML encoding remain distinct operations.

Origins must come from trusted application configuration. Ruvane never reads
Host, Forwarded, or X-Forwarded-* to construct one automatically. Neither type
proves a resource exists or authorizes access. Canonical links express a
preferred URL to crawlers; they are not redirects or indexing guarantees.
See [canonical URL guidance](https://developers.google.com/search/docs/crawling-indexing/consolidate-duplicate-urls).

## Mini-blog behavior

`cargo run --example blog -- 3000 https://journal.example` sets an explicit
origin for metadata while retaining the local HTTP listener. The `.example`
address is a documentation placeholder, not a deployment destination.
Without an origin argument, the example uses `noindex, follow` and emits no
canonical URL. It does not guess the public hostname or actual listener port.

With an origin configured, ordinary pages use `index, follow`. Article
descriptions come from source content, and canonical paths use the selected
source article slug. `/articles` and `?page=1` canonicalize to `/articles`;
page 2 and later retain distinct `?page=N` canonicals. This follows
[pagination guidance](https://developers.google.com/search/docs/specialty/ecommerce/pagination-and-incremental-page-loading).
Unrecognized tracking fields are not copied into canonical URLs. Nonempty
searches and application-rendered errors use `noindex, follow` and no canonical
link. Invalid protocol/query requests retain generic plain-text errors.

The favicon reuses the checked-in official PNG. Form decoding, search retention,
filter-preserving pagination links, and local assets keep their existing behavior.

## Fixed HTTP Content Security Policy

Every server response, including assets, HEAD, protocol errors, timeouts,
and handler failures, carries this `CONTENT_SECURITY_POLICY` value:

```text
default-src 'none'; script-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'
```

Scripts and unspecified resource types are blocked. CSS/images may load from
the document's origin, and forms may submit there. Base elements and embedding
in frames are explicitly blocked. The policy is sent as an HTTP header;
`frame-ancestors` does not work in a metadata-delivered policy. See the
[CSP specification](https://www.w3.org/TR/CSP/).

No request data, arbitrary policy string, nonce, or unsafe-inline/eval allowance
can change this fixed profile. Declaring a public canonical origin does not
allow that origin to supply resources to a locally served page: `'self'` is
the document's actual origin. Standalone `Document::render()` produces HTML,
not an HTTP policy. A different server must supply its own headers.

CSP supplements escaping; it is not authorization, CSRF protection, a server
code sandbox, or content sanitization. Trusted same-origin CSS/assets still
require appropriate application handling. This release retains the bounded,
loopback-only development server and does not add public serving or TLS.

## Compatibility and verification

All rendered documents now include the fixed viewport, so exact HTML snapshots
change. Optional metadata is otherwise opt-in. HTTP responses gain one fixed
header; their status, body bytes, content length, and HEAD behavior are preserved.
The policy currently has no opt-out and blocks browser scripts and external
resources. Applications adding such features will need a future typed policy
design rather than injecting metadata or arbitrary headers.

Dependency-free Rust tests cover description and origin boundaries, control
characters, escaping, numeric-host aliases, singleton replacement, canonical
encoding, pagination, request-header independence, and CSP on the network
suite's successful and failed responses. Browser checks supplement those
tests; they are not an independent security audit or a claim of exhaustive
browser compatibility.
