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
