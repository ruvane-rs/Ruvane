# Security

Ruvane is experimental and has not undergone an independent audit.

Version 0.1.0-alpha.1 provides HTML rendering only. Starting with
0.1.0-alpha.2, Ruvane also provides a loopback-only development server with
bounded socket I/O and an intentionally limited HTTP profile. It does not
provide TLS, sessions, authentication, server actions, or a production HTTP
implementation. See [the HTTP profile](docs/http-profile.md) and the README
for its exact syntax, defaults, and limitations.

Starting with 0.1.0-alpha.3, dynamic captures and query pairs are decoded once
as UTF-8 under fixed size/count limits. Ambiguous dynamic routes and duplicate
decoded query keys are rejected. Internal links require a validated
`InternalUrl`, URL component encoding, and quoted HTML attribute escaping.
The raw path remains distinct from decoded parameter data: neither literal
route precedence nor URL decoding is an authorization boundary. Applications
must validate domain values and access to the resources they identify. See
[routing and URLs](docs/routing-and-urls.md) for the exact contract.

Starting with 0.1.0-alpha.4, embedded assets have fixed registration budgets
and explicitly declared MIME types. Requests never map a URL to a filesystem
path. Assets are trusted application source; CSS is code, and declared image
types are not file validation. Images are not decoded or resized by Ruvane.
Binary and embedded responses obey the same output and I/O limits as HTML.
Stylesheet and image attributes use validated internal URLs and HTML escaping.
See [assets and media](docs/assets-and-media.md) for exact limits and scope.

Starting with 0.1.0-alpha.5, typed GET forms escape text and require an internal,
query-free action. Field names, counts, and text lengths have fixed budgets.
The selected raw route fixes query decoding: ordinary routes preserve literal
plus signs; form routes decode raw plus signs as spaces. Queries are decoded
once using that mode, and duplicate decoded keys are rejected before a handler
runs. Request fields cannot switch the mode. Path captures keep URI semantics.
Hidden fields are untrusted client input, and GET field values appear in URLs;
they must not contain secrets or establish authorization. Forms add no mutation,
authentication, session, or CSRF protection mechanism. All handlers remain
read-only. See [GET forms](docs/get-forms.md).

Starting with 0.1.0-alpha.6, page metadata uses unique typed fields and escaped
attributes. Descriptions are bounded and control-free. Canonical URLs combine
a restricted, explicitly configured SiteOrigin with an InternalUrl; no origin
is inferred from Host or forwarding headers. The parser performs no network
I/O, IDNA conversion, or TLS. Robots hints do not restrict access.

Every server response includes a fixed Content-Security-Policy, including
assets, HEAD, parse errors, and handler failures. Scripts are blocked; CSS,
images, and form submissions are limited to the same origin. Base elements
and frame embedding are explicitly blocked. Standalone HTML rendering does
not emit an HTTP policy. CSP supplements escaping and does not sandbox server
code or add authorization or CSRF protection. See
[metadata and CSP](docs/metadata-and-csp.md).

Starting with 0.1.0-alpha.7, static export uses explicit portable paths and
bounded pages/assets. Output paths are derived from a restricted lowercase
ASCII profile, never from request targets. Plans reject aliases, device names,
duplicate/conflicting files, and MIME suffix mismatches. Content remains
trusted application input: assets are not sanitized or decoded.

Exports require a new destination under an existing trusted parent, reject
observed ancestor symlinks, and exclusively create files. They never overwrite
or recursively delete output. I/O failures may leave partial files without the
final completion marker. The caller must prevent concurrent hostile changes
to the parent and output tree; portable safe std APIs do not provide a complete
filesystem sandbox. The marker is not an integrity signature or crash-durability
guarantee. See [static export](docs/static-export.md) for the precise scope.

Static previews serve registered shared bytes, not arbitrary disk paths. They
retain the existing local HTTP profile and CSP. Exported files carry no HTTP
headers themselves; the hosting server must apply the generated header contract
and declared MIME types. GET search remains part of the server edition.

The framework rejects ambiguous request framing before dispatch and uses
fixed response headers. HTML strings are escaped by the renderer. Handler
code remains trusted: response limits apply after construction, socket
deadlines do not interrupt application code, and shutdown waits for running
handlers to return. The normal panic hook still executes on a caught panic;
applications must keep sensitive data out of their own logs. Builds using
`panic = "abort"` cannot recover from handler panics.

Local binding and Host checks are not an authentication boundary against
other local programs. Read handlers must not perform state-changing actions.

For reporting options, consult the repository's Security tab. If private
vulnerability reporting is unavailable, contact a maintainer privately before
sharing sensitive details. Do not include credentials or personal data.

Security fixes may change the pre-release API. Users should follow the latest
pre-release and review its changelog.
