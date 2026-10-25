use crate::{Html, MediaType, ParamError, Query, QueryMode};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// A read-only request method supported by the local server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// Retrieve a representation.
    Get,
    /// Retrieve the headers of a representation without sending its body.
    Head,
}

/// A validated, bodyless request in Ruvane's local HTTP profile.
///
/// Paths and queries retain their original ASCII percent encoding. Routing
/// matches raw segments without normalizing the path. Query values and matched
/// dynamic captures have separate, strictly decoded accessors.
#[derive(Clone, Debug)]
pub struct Request {
    method: Method,
    target: String,
    headers: Vec<(String, String)>,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) query_params: Query,
}

impl Request {
    /// Returns the request method. GET handlers also receive HEAD requests.
    pub fn method(&self) -> Method {
        self.method
    }

    /// Returns the path without its query string.
    pub fn path(&self) -> &str {
        self.target.split_once('?').map_or(&self.target, |(p, _)| p)
    }

    /// Returns the raw query string, excluding `?`, if present.
    pub fn query(&self) -> Option<&str> {
        self.target.split_once('?').map(|(_, query)| query)
    }

    /// Returns the query decoded once according to the selected route's mode.
    ///
    /// Ordinary routes preserve `+`; routes registered with `get_form()`
    /// interpret it as a space. Both modes reject duplicate decoded keys.
    pub fn query_params(&self) -> &Query {
        &self.query_params
    }

    /// Returns the query convention selected by route registration.
    pub fn query_mode(&self) -> QueryMode {
        self.query_params.mode()
    }

    /// Returns a decoded dynamic path capture by name, if present.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Converts a path capture using the selected type's `FromStr` implementation.
    ///
    /// The handler decides how to map missing or invalid data to a response.
    pub fn param_as<T: FromStr>(&self, name: &str) -> Result<T, ParamError> {
        crate::url::parse_value(self.param(name))
    }

    /// Returns a header value using an ASCII case-insensitive name lookup.
    ///
    /// Leading and trailing spaces and tabs have been removed. Duplicate
    /// header names are rejected by this profile rather than combined.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A status supported by the initial HTTP response serializer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StatusCode {
    /// 200: the request succeeded.
    Ok,
    /// 400: the request is malformed or ambiguous.
    BadRequest,
    /// 404: no route matches this exact path.
    NotFound,
    /// 405: a known method is outside the read-only profile.
    MethodNotAllowed,
    /// 408: the request did not arrive within its total time budget.
    RequestTimeout,
    /// 413: a nonempty request body is outside this profile.
    ContentTooLarge,
    /// 414: the request line exceeds the configured limit.
    UriTooLong,
    /// 417: request expectations are unsupported.
    ExpectationFailed,
    /// 431: request headers exceed a byte or count limit.
    RequestHeaderFieldsTooLarge,
    /// 500: a handler failed or returned a body exceeding the output limit.
    InternalServerError,
    /// 501: the method or transfer/upgrade functionality is unsupported.
    NotImplemented,
    /// 505: the HTTP version is outside this profile.
    HttpVersionNotSupported,
}

impl StatusCode {
    /// Returns the numeric HTTP status code.
    pub fn as_u16(self) -> u16 {
        match self {
            Self::Ok => 200,
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::RequestTimeout => 408,
            Self::ContentTooLarge => 413,
            Self::UriTooLong => 414,
            Self::ExpectationFailed => 417,
            Self::RequestHeaderFieldsTooLarge => 431,
            Self::InternalServerError => 500,
            Self::NotImplemented => 501,
            Self::HttpVersionNotSupported => 505,
        }
    }

    /// Returns the fixed reason phrase used on the wire.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::BadRequest => "Bad Request",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::RequestTimeout => "Request Timeout",
            Self::ContentTooLarge => "Content Too Large",
            Self::UriTooLong => "URI Too Long",
            Self::ExpectationFailed => "Expectation Failed",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
            Self::HttpVersionNotSupported => "HTTP Version Not Supported",
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.as_u16(), self.reason())
    }
}

/// A text or binary response with a fixed, injection-resistant header set.
///
/// Body lengths are serialized in bytes. HEAD sends the same content length
/// as GET, but no body. There is no arbitrary header or raw HTML constructor.
#[derive(Clone, Debug)]
#[must_use]
pub struct Response {
    status: StatusCode,
    content_type: &'static str,
    body: Cow<'static, [u8]>,
}

impl Response {
    /// Creates a successful plain-text response.
    pub fn text(body: impl Into<String>) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: "text/plain; charset=utf-8",
            body: Cow::Owned(body.into().into_bytes()),
        }
    }

    /// Creates a successful HTML response from the renderer's output.
    pub fn html(body: Html) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: "text/html; charset=utf-8",
            body: Cow::Owned(body.into_string().into_bytes()),
        }
    }

    /// Creates a successful owned byte response with an explicit MIME type.
    ///
    /// Bytes are never converted to text or modified. The MIME declaration
    /// does not validate file contents. The server's response body budget
    /// applies after construction, just as it does for text and HTML.
    ///
    /// ```
    /// let response = ruvane::Response::bytes(vec![0, 255, 13, 10], ruvane::MediaType::OctetStream);
    /// assert_eq!(response.body(), &[0, 255, 13, 10]);
    /// assert!(response.body_text().is_err());
    /// ```
    pub fn bytes(body: impl Into<Vec<u8>>, media_type: MediaType) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: media_type.as_str(),
            body: Cow::Owned(body.into()),
        }
    }

    pub(crate) fn embedded(body: &'static [u8], media_type: MediaType) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: media_type.as_str(),
            body: Cow::Borrowed(body),
        }
    }

    /// Sets the response status without changing its body or content type.
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Returns the response status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Borrows the exact response bytes, including for a HEAD representation.
    ///
    /// This returned `&str` before alpha.4. Use `body_text()` when UTF-8 is needed.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Borrows the body as UTF-8, returning an error for invalid byte sequences.
    ///
    /// No replacement, loss, or mutation of binary data occurs.
    pub fn body_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(self.body())
    }

    /// Returns the fixed MIME type, with a UTF-8 charset for text types.
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }

    pub(crate) fn error(status: StatusCode) -> Self {
        Self::text(format!("{}\n", status.reason())).with_status(status)
    }

    pub(crate) fn wire_headers(&self) -> String {
        let allow = if self.status == StatusCode::MethodNotAllowed {
            "Allow: GET, HEAD\r\n"
        } else {
            ""
        };
        format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\n{}\r\n",
            self.status,
            self.content_type,
            self.body.len(),
            allow
        )
    }
}

pub(crate) fn token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

pub(crate) fn uri_component(value: &[u8], query: bool) -> bool {
    let mut index = 0;
    while index < value.len() {
        let byte = value[index];
        if byte == b'%' {
            if index + 2 >= value.len()
                || !value[index + 1].is_ascii_hexdigit()
                || !value[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else if byte.is_ascii_alphanumeric()
            || b"-._~!$&'()*+,;=:@/".contains(&byte)
            || (query && byte == b'?')
        {
            index += 1;
        } else {
            return false;
        }
    }
    true
}

pub(crate) fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains("//")
        && !path.split('/').any(|part| matches!(part, "." | ".."))
        && uri_component(path.as_bytes(), false)
}

/// Parses exactly one complete header block, without any body or trailing data.
pub(crate) fn parse(
    bytes: &[u8],
    max_line_bytes: usize,
    max_headers: usize,
) -> Result<Request, StatusCode> {
    use StatusCode::{
        BadRequest, ContentTooLarge, ExpectationFailed, HttpVersionNotSupported, MethodNotAllowed,
        NotImplemented, RequestHeaderFieldsTooLarge, UriTooLong,
    };
    if !bytes.ends_with(b"\r\n\r\n") {
        return Err(BadRequest);
    }
    // Reject non-ASCII bytes and controls before constructing any strings.
    if bytes.iter().any(|&byte| {
        !(byte == b'\r' || byte == b'\n' || byte == b'\t' || (32..=126).contains(&byte))
    }) {
        return Err(BadRequest);
    }
    let input = std::str::from_utf8(bytes).map_err(|_| BadRequest)?;
    let mut lines = input[..input.len() - 4].split("\r\n");
    let first = lines.next().ok_or(BadRequest)?;
    if first.len() > max_line_bytes {
        return Err(UriTooLong);
    }
    let mut words = first.split(' ');
    let method = words.next().ok_or(BadRequest)?;
    let target = words.next().ok_or(BadRequest)?;
    let version = words.next().ok_or(BadRequest)?;
    if words.next().is_some() || method.is_empty() || !method.bytes().all(token) {
        return Err(BadRequest);
    }
    if version != "HTTP/1.1" {
        let version_bytes = version.as_bytes();
        return Err(
            if version_bytes.len() == 8
                && &version_bytes[..5] == b"HTTP/"
                && version_bytes[5].is_ascii_digit()
                && version_bytes[6] == b'.'
                && version_bytes[7].is_ascii_digit()
            {
                HttpVersionNotSupported
            } else {
                BadRequest
            },
        );
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if !valid_path(path) || !uri_component(query.as_bytes(), true) {
        return Err(BadRequest);
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if headers.len() == max_headers {
            return Err(RequestHeaderFieldsTooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(BadRequest)?;
        if name.is_empty()
            || !name.bytes().all(token)
            || !value
                .bytes()
                .all(|byte| byte == b'\t' || (32..=126).contains(&byte))
            || headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            return Err(BadRequest);
        }
        headers.push((
            name.to_ascii_lowercase(),
            value.trim_matches([' ', '\t']).to_owned(),
        ));
    }
    let header = |name| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if header("host").is_none_or(str::is_empty) {
        return Err(BadRequest);
    }
    if header("transfer-encoding").is_some() {
        return Err(if header("content-length").is_some() {
            BadRequest
        } else {
            NotImplemented
        });
    }
    if let Some(value) = header("content-length") {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BadRequest);
        }
        let length = value.parse::<u64>().map_err(|_| BadRequest)?;
        if length != 0 {
            return Err(ContentTooLarge);
        }
    }
    if header("expect").is_some() {
        return Err(ExpectationFailed);
    }
    if header("upgrade").is_some() {
        return Err(NotImplemented);
    }
    let method = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS" | "CONNECT" | "TRACE" => {
            return Err(MethodNotAllowed);
        }
        _ => return Err(NotImplemented),
    };
    Ok(Request {
        method,
        target: target.to_owned(),
        headers,
        params: Vec::new(),
        // Semantic query decoding happens after route selection. The transport
        // validates raw URI syntax and framing without choosing a convention.
        query_params: Query::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_path_query_and_trimmed_case_insensitive_headers() {
        let request = parse(
            b"HEAD /a%20b?x=%26&y=? HTTP/1.1\r\nHost:\tlocalhost \t\r\nX-Test: value\r\n\r\n",
            4096,
            64,
        )
        .unwrap();
        assert_eq!(request.method(), Method::Head);
        assert_eq!(request.path(), "/a%20b");
        assert_eq!(request.query(), Some("x=%26&y=?"));
        assert_eq!(request.header("HOST"), Some("localhost"));
        assert_eq!(request.header("x-test"), Some("value"));
    }

    #[test]
    fn rejects_syntax_and_framing_corpus() {
        let cases: &[&[u8]] = &[
            b"GET / HTTP/1.1\nHost: localhost\n\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\nextra",
            b"GET  / HTTP/1.1\r\nHost: localhost\r\n\r\n",
            b"GET\t/ HTTP/1.1\r\nHost: localhost\r\n\r\n",
            b"GET / HTTP/1.1\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost:\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: a\r\nhOsT: a\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost : localhost\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\n folded: x\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nX: a\nb\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nX: a\rb\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nX: \0\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nX: \xff\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\ncontent-length: 0\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0, 0\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nContent-Length: +0\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 18446744073709551616\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n",
        ];
        for input in cases {
            assert_eq!(
                parse(input, 4096, 64).unwrap_err(),
                StatusCode::BadRequest,
                "{input:?}"
            );
        }
    }

    #[test]
    fn rejects_ambiguous_or_invalid_targets() {
        for target in [
            "http://localhost/",
            "*",
            "//other/",
            "/a/../b",
            "/./",
            "/a\\b",
            "/a#b",
            "/%",
            "/%gg",
            "/?a=%z1",
            "/é",
        ] {
            let input = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            assert!(parse(input.as_bytes(), 4096, 64).is_err(), "{target}");
        }
    }

    #[test]
    fn enforces_exact_request_line_and_header_count_boundaries() {
        let input = b"GET / HTTP/1.1\r\nHost: localhost\r\nX: y\r\n\r\n";
        assert!(parse(input, 14, 2).is_ok());
        assert_eq!(parse(input, 13, 2).unwrap_err(), StatusCode::UriTooLong);
        assert_eq!(
            parse(input, 14, 1).unwrap_err(),
            StatusCode::RequestHeaderFieldsTooLarge
        );
    }

    #[test]
    fn deterministic_byte_mutations_never_panic() {
        let seed = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        for position in 0..seed.len() {
            for value in 0..=255 {
                let mut input = seed.to_vec();
                input[position] = value;
                let _ = parse(&input, 4096, 64);
            }
        }
        for length in 0..seed.len() {
            assert!(parse(&seed[..length], 4096, 64).is_err());
        }
    }
}
