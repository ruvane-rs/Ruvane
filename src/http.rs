use crate::{FormData, Html, InternalUrl, MediaType, ParamError, Query, QueryMode};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

/// The default browser policy, including errors, assets and ordinary HTML.
/// Explicitly interactive HTML uses INTERACTIVE_CONTENT_SECURITY_POLICY instead.
///
/// Scripts and unspecified resource types are blocked. CSS and images may
/// load from the same origin; forms may submit there. Base elements and frame
/// embedding are blocked explicitly. The policy has no request-derived fields,
/// unsafe-inline/eval allowances, or nonces. It supplements HTML escaping;
/// it does not provide authorization, CSRF protection, or a handler sandbox.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'";

/// Opt-in page policy for trusted same-origin modules and Wasm compilation.
/// No inline JavaScript or general JavaScript eval is enabled. This policy
/// grants no authentication, authorization, or execution sandbox.
pub const INTERACTIVE_CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'";

/// A request method supported by the local server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// Retrieve a representation.
    Get,
    /// Retrieve the headers of a representation without sending its body.
    Head,
    /// Submit a body in the explicit route's form, multipart or JSON profile.
    Post,
    /// Replace a resource using an explicit JSON body.
    Put,
    /// Partially update a resource using an explicit JSON body.
    Patch,
    /// Delete a resource without a request body.
    Delete,
}
impl Method {
    /// Whether this method can change application state and requires mutation checks.
    pub const fn is_mutation(self) -> bool {
        matches!(self, Self::Post | Self::Put | Self::Patch | Self::Delete)
    }
}

/// A validated request in Ruvane's local HTTP profile.
///
/// Paths and queries retain their original ASCII percent encoding. Routing
/// matches raw segments without normalizing the path. Query values and matched
/// dynamic captures have separate, strictly decoded accessors.
#[derive(Clone)]
pub struct Request {
    method: Method,
    target: String,
    headers: Vec<(String, String)>,
    cookies: crate::Cookies,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) query_params: Query,
    pub(crate) content_length: Option<u64>,
    pub(crate) form: Option<FormData>,
    pub(crate) json: Option<crate::JsonValue>,
    pub(crate) multipart: Option<crate::MultipartData>,
    pub(crate) guard_private: bool,
    if_match: Option<crate::IfMatch>,
}

impl Request {
    pub(crate) fn prerender_get(url: &crate::InternalUrl, params: Vec<(String, String)>) -> Self {
        Self {
            method: Method::Get,
            target: url.as_str().into(),
            headers: Vec::new(),
            cookies: crate::Cookies::default(),
            params,
            query_params: Query::default(),
            content_length: None,
            form: None,
            json: None,
            multipart: None,
            guard_private: false,
            if_match: None,
        }
    }
    pub(crate) fn background_get(&self) -> Self {
        Self {
            method: Method::Get,
            target: self.target.clone(),
            headers: Vec::new(),
            cookies: crate::Cookies::default(),
            params: self.params.clone(),
            query_params: self.query_params.clone(),
            content_length: None,
            form: None,
            json: None,
            multipart: None,
            guard_private: false,
            if_match: None,
        }
    }
    /// Returns the request method. GET handlers also receive HEAD requests.
    pub fn method(&self) -> Method {
        self.method
    }
    /// Borrows the parsed If-Match condition. It is untrusted until compared with current state.
    /// Applications must evaluate it under the same lock as their mutation, after authorization.
    pub fn if_match(&self) -> Option<&crate::IfMatch> {
        self.if_match.as_ref()
    }
    /// Requests carrying credentials or marked Private by a guard bypass public caches.
    /// This flag is not an authentication result.
    pub fn cache_bypass(&self) -> bool {
        self.guard_private
            || self.header("cookie").is_some()
            || self.header("authorization").is_some()
    }

    /// Returns cookies validated before routing or application callbacks.
    /// These are untrusted values, never authentication claims.
    pub fn cookies(&self) -> &crate::Cookies {
        &self.cookies
    }

    /// Reads the reserved header or form claim, never URL parameters.
    /// A claim still requires verification against the current session.
    pub fn csrf_token(&self) -> Result<Option<crate::CsrfToken>, crate::CsrfError> {
        let header = self.header(crate::CSRF_HEADER);
        let form = self.form.as_ref().and_then(FormData::csrf_claim);
        let multipart = self
            .multipart
            .as_ref()
            .and_then(|data| data.fields().csrf_claim());
        if form.is_some() && multipart.is_some() {
            return Err(crate::CsrfError::Ambiguous);
        }
        let form = form.or(multipart);
        match (header, form) {
            (Some(_), Some(_)) => Err(crate::CsrfError::Ambiguous),
            (Some(value), None) => crate::CsrfToken::parse(value).map(Some),
            (None, Some(claim)) => claim.cloned().map(Some),
            (None, None) => Ok(None),
        }
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

    /// Validated URL-encoded business fields, separate from multipart, query and CSRF data.
    pub fn form_data(&self) -> Option<&FormData> {
        self.form.as_ref()
    }
    /// Borrows parsed native multipart data. Other route profiles and sanitized reads return None.
    pub fn multipart(&self) -> Option<&crate::MultipartData> {
        self.multipart.as_ref()
    }
    /// Borrows the validated body of an explicitly registered JSON mutation route.
    /// GET/HEAD, DELETE, form/multipart routes and sanitized background requests return None.
    pub fn json(&self) -> Option<&crate::JsonValue> {
        self.json.as_ref()
    }

    /// Converts a parsed JSON body using the application's explicit checked conversion.
    /// Missing JSON produces an object-level validation error. This does not run guards.
    pub fn decode_json<'a, T>(&'a self) -> Result<T, crate::ValidationErrors>
    where
        T: TryFrom<&'a crate::JsonValue, Error = crate::ValidationErrors>,
    {
        match self.json() {
            Some(value) => T::try_from(value),
            None => {
                let mut errors = crate::ValidationErrors::new();
                errors.add(None, crate::ValidationCode::ExpectedObject);
                Err(errors)
            }
        }
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

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers = self
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name,
                    if matches!(
                        name.as_str(),
                        "cookie" | "authorization" | "ruvane-csrf-token" | "if-match"
                    ) {
                        "[redacted]"
                    } else {
                        value.as_str()
                    },
                )
            })
            .collect::<Vec<_>>();
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("target", &self.target)
            .field("headers", &headers)
            .field("cookies", &self.cookies)
            .field("params", &self.params)
            .field("query_params", &self.query_params)
            .field("content_length", &self.content_length)
            .field("form", &self.form)
            .field("json", &self.json)
            .field("multipart", &self.multipart)
            .field("guard_private", &self.guard_private)
            .finish()
    }
}

/// Maximum supported Retry-After delta in seconds.
pub const MAX_RETRY_AFTER_SECONDS: u32 = 24 * 60 * 60;

/// A bounded, positive Retry-After delta. No raw header text is accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RetryAfter(u32);
impl RetryAfter {
    /// Accepts 1 through MAX_RETRY_AFTER_SECONDS, inclusive.
    pub fn new(seconds: u32) -> Option<Self> {
        (1..=MAX_RETRY_AFTER_SECONDS)
            .contains(&seconds)
            .then_some(Self(seconds))
    }
    /// Returns the whole-second delta.
    pub fn seconds(self) -> u32 {
        self.0
    }
}

/// A status supported by the initial HTTP response serializer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StatusCode {
    /// 200: the request succeeded.
    Ok,
    /// 303: retrieve the internal Location after a successful submission.
    SeeOther,
    /// 400: the request is malformed or ambiguous.
    BadRequest,
    /// 403: access or the mutation's source origin is not permitted.
    Forbidden,
    /// 404: no route matches this exact path.
    NotFound,
    /// 405: the method is not registered for this route.
    MethodNotAllowed,
    /// 408: the request did not arrive within its total time budget.
    RequestTimeout,
    /// 409: the browser and server application revisions differ.
    Conflict,
    /// 411: a body-bearing mutation has no declared content length.
    LengthRequired,
    /// 412: a supplied resource precondition does not match current state.
    PreconditionFailed,
    /// 413: the request body exceeds the supported budget.
    ContentTooLarge,
    /// 414: the request line exceeds the configured limit.
    UriTooLong,
    /// 415: the request does not fit the registered MIME/encoding profile.
    UnsupportedMediaType,
    /// 417: request expectations are unsupported.
    ExpectationFailed,
    /// 428: this resource requires a precise mutation precondition.
    PreconditionRequired,
    /// 429: a request quota was exhausted.
    TooManyRequests,
    /// 431: request headers exceed a byte or count limit.
    RequestHeaderFieldsTooLarge,
    /// 500: a handler failed or returned a body exceeding the output limit.
    InternalServerError,
    /// 501: the method or transfer/upgrade functionality is unsupported.
    NotImplemented,
    /// 503: the service or a bounded public cache cannot fulfill this request.
    ServiceUnavailable,
    /// 505: the HTTP version is outside this profile.
    HttpVersionNotSupported,
}

impl StatusCode {
    /// Returns the numeric HTTP status code.
    pub fn as_u16(self) -> u16 {
        match self {
            Self::Ok => 200,
            Self::SeeOther => 303,
            Self::BadRequest => 400,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::RequestTimeout => 408,
            Self::Conflict => 409,
            Self::LengthRequired => 411,
            Self::PreconditionFailed => 412,
            Self::ContentTooLarge => 413,
            Self::UriTooLong => 414,
            Self::UnsupportedMediaType => 415,
            Self::ExpectationFailed => 417,
            Self::PreconditionRequired => 428,
            Self::TooManyRequests => 429,
            Self::RequestHeaderFieldsTooLarge => 431,
            Self::InternalServerError => 500,
            Self::NotImplemented => 501,
            Self::ServiceUnavailable => 503,
            Self::HttpVersionNotSupported => 505,
        }
    }

    /// Returns the fixed reason phrase used on the wire.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::SeeOther => "See Other",
            Self::BadRequest => "Bad Request",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::RequestTimeout => "Request Timeout",
            Self::Conflict => "Conflict",
            Self::LengthRequired => "Length Required",
            Self::PreconditionFailed => "Precondition Failed",
            Self::ContentTooLarge => "Content Too Large",
            Self::UriTooLong => "URI Too Long",
            Self::UnsupportedMediaType => "Unsupported Media Type",
            Self::ExpectationFailed => "Expectation Failed",
            Self::PreconditionRequired => "Precondition Required",
            Self::TooManyRequests => "Too Many Requests",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
            Self::ServiceUnavailable => "Service Unavailable",
            Self::HttpVersionNotSupported => "HTTP Version Not Supported",
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.as_u16(), self.reason())
    }
}

/// A text or binary response with fixed headers.
///
/// Body lengths are serialized in bytes. HEAD sends the same content length
/// as GET, but no body. There is no arbitrary header or raw HTML constructor.
#[derive(Clone)]
#[must_use]
pub struct Response {
    status: StatusCode,
    content_type: &'static str,
    body: Body,
    csp: &'static str,
    location: Option<InternalUrl>,
    navigation_cache: crate::NavigationCachePolicy,
    navigation_variance: bool,
    cookies: Vec<crate::Cookie>,
    csrf_token: Option<crate::CsrfToken>,
    retry_after: Option<RetryAfter>,
    action_rate_rejected: bool,
    etag: Option<crate::EntityTag>,
    pub(crate) download: Option<crate::DownloadName>,
}
impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body: &dyn fmt::Debug =
            if self.content_type == crate::JSON_MEDIA_TYPE || self.download.is_some() {
                &"[redacted JSON]"
            } else {
                &self.body
            };
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("content_type", &self.content_type)
            .field("body", body)
            .field("csp", &self.csp)
            .field("location", &self.location)
            .field("navigation_cache", &self.navigation_cache)
            .field("navigation_variance", &self.navigation_variance)
            .field("cookies", &self.cookies)
            .field("csrf_token", &self.csrf_token)
            .field("retry_after", &self.retry_after)
            .field("action_rate_rejected", &self.action_rate_rejected)
            .finish()
    }
}

#[derive(Clone, Debug)]
enum Body {
    Bytes(Cow<'static, [u8]>),
    Shared(Arc<[u8]>),
    Stream(Arc<crate::streaming::BodyStream>),
}

impl AsRef<[u8]> for Body {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::Shared(bytes) => bytes,
            Self::Stream(plan) => plan.prefix(),
        }
    }
}

impl Response {
    /// Attaches a validated representation tag. This does not enable caching or evaluate preconditions.
    /// The transport conservatively omits validators on PUT responses; read the saved representation.
    pub fn with_etag(mut self, tag: crate::EntityTag) -> Self {
        self.etag = Some(tag);
        self
    }
    /// Returns the tag only for a successful buffered response. Errors and streams omit validators.
    pub fn etag(&self) -> Option<&crate::EntityTag> {
        (self.status == StatusCode::Ok && !self.is_streaming())
            .then_some(self.etag.as_ref())
            .flatten()
    }
    pub(crate) fn omit_etag(&mut self) {
        self.etag = None;
    }
    /// Returns fixed no-store JSON field errors with status 400. Empty reports and
    /// invalid application definitions fail closed with a generic 500 response.
    pub fn validation_errors(errors: &crate::ValidationErrors) -> Self {
        Self::json(&errors.to_json()).with_status(errors.status())
    }
    /// Returns validated UTF-8 JSON with fixed MIME, no-store and the blocking CSP.
    /// JSON is data; this constructor does not permit HTML or JavaScript embedding.
    pub fn json(value: &crate::JsonValue) -> Self {
        let mut response = Self::text(value.encode());
        response.content_type = crate::JSON_MEDIA_TYPE;
        response
    }
    /// Returns a generic no-store 429 response with a bounded Retry-After delta.
    /// This alone does not assert that an action handler has not run.
    pub fn too_many_requests(retry_after: RetryAfter) -> Self {
        Self::text("Too many requests. Please try again later.").with_retry_after(retry_after)
    }
    /// Sets status 429 and its Retry-After delta without accepting raw header text.
    pub fn with_retry_after(mut self, retry_after: RetryAfter) -> Self {
        self.status = StatusCode::TooManyRequests;
        self.retry_after = Some(retry_after);
        self
    }
    /// Returns the delta only when this response has status 429.
    pub fn retry_after(&self) -> Option<RetryAfter> {
        (self.status == StatusCode::TooManyRequests)
            .then_some(self.retry_after)
            .flatten()
    }
    pub(crate) fn guard_rate_limited(mut self, retry_after: RetryAfter, action: bool) -> Self {
        self = self.with_retry_after(retry_after);
        self.action_rate_rejected = action;
        self
    }
    /// Creates a script-blocking, no-store progressive HTML response.
    /// Only GET/HEAD transport supports this plan; HEAD executes no factory or producer.
    pub fn stream_html(plan: crate::HtmlStream) -> Self {
        let mut response = Self::html(Html::default());
        response.body = Body::Stream(Arc::new(plan.into_body()));
        response
    }
    pub(crate) fn stream_navigation(plan: crate::streaming::BodyStream) -> Self {
        let mut response = Self::html(Html::default());
        response.body = Body::Stream(Arc::new(plan));
        response.content_type = crate::NAVIGATION_STREAM_MEDIA_TYPE;
        response.navigation_variance = true;
        response
    }
    pub(crate) fn stream_application(plan: crate::streaming::BodyStream) -> Self {
        let mut response = Self::interactive_html(Html::default()).navigation_variance();
        response.body = Body::Stream(Arc::new(plan));
        response
    }
    /// Whether this response contains a lazy stream rather than a complete body.
    pub fn is_streaming(&self) -> bool {
        matches!(self.body, Body::Stream(_))
    }
    /// The complete buffered body, or None for a lazy stream.
    pub fn buffered_body(&self) -> Option<&[u8]> {
        if self.is_streaming() {
            None
        } else {
            Some(self.body())
        }
    }
    pub(crate) fn stream_plan(&self) -> Option<Arc<crate::streaming::BodyStream>> {
        match &self.body {
            Body::Stream(plan) => Some(Arc::clone(plan)),
            _ => None,
        }
    }
    /// Serves a complete typed page with its HTTP status and fixed navigation MIME.
    pub fn navigation(page: &crate::NavigationPage) -> Self {
        Self::navigation_with_cache(page, crate::NavigationCachePolicy::no_store())
    }
    /// Allows explicit caching of successful public pages. Errors stay no-store.
    pub fn navigation_with_cache(
        page: &crate::NavigationPage,
        policy: crate::NavigationCachePolicy,
    ) -> Self {
        if page.pending_layouts().is_some() {
            return Self::error(StatusCode::InternalServerError);
        }
        let mut response =
            Self::bytes(page.encode(), MediaType::Navigation).with_status(page.status());
        response.navigation_cache = policy;
        response.navigation_variance = true;
        response
    }
    pub(crate) fn navigation_variance(mut self) -> Self {
        self.navigation_variance = true;
        self
    }
    /// The effective response cache directive. Only successful typed pages can opt in.
    pub fn cache_control(&self) -> String {
        if self.cookies.is_empty()
            && self.csrf_token.is_none()
            && self.status == StatusCode::Ok
            && self.content_type == crate::NAVIGATION_MEDIA_TYPE
        {
            self.navigation_cache.header()
        } else {
            "no-store".into()
        }
    }
    pub(crate) fn limit_navigation_freshness(mut self, seconds: u32) -> Self {
        if let Some(current) = self.navigation_cache.max_age_seconds() {
            self.navigation_cache = crate::NavigationCachePolicy::public(current.min(seconds))
                .unwrap_or_else(|_| crate::NavigationCachePolicy::no_store());
        }
        self
    }
    /// Serves a typed RVA1 action reply with fixed MIME and success/validation status.
    pub fn action(reply: &crate::ActionReply) -> Self {
        let mut response = Self::bytes(reply.encode(), MediaType::Action);
        response.status = if reply.is_validation() {
            StatusCode::BadRequest
        } else {
            StatusCode::Ok
        };
        response
    }
    /// Returns an empty, no-store token response for a same-origin runtime.
    /// `None` represents an anonymous session; protected actions must still reject it.
    pub fn csrf_token(token: Option<crate::CsrfToken>) -> Self {
        let mut response = Self::text("");
        response.content_type = crate::CSRF_MEDIA_TYPE;
        response.csrf_token = token;
        response
    }
    /// Creates a successful plain-text response.
    pub fn text(body: impl Into<String>) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: "text/plain; charset=utf-8",
            body: Body::Bytes(Cow::Owned(body.into().into_bytes())),
            csp: CONTENT_SECURITY_POLICY,
            location: None,
            navigation_cache: crate::NavigationCachePolicy::no_store(),
            navigation_variance: false,
            cookies: Vec::new(),
            csrf_token: None,
            retry_after: None,
            action_rate_rejected: false,
            etag: None,
            download: None,
        }
    }

    /// Creates a successful HTML response from the renderer's output.
    pub fn html(body: Html) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: "text/html; charset=utf-8",
            body: Body::Bytes(Cow::Owned(body.into_string().into_bytes())),
            csp: CONTENT_SECURITY_POLICY,
            location: None,
            navigation_cache: crate::NavigationCachePolicy::no_store(),
            navigation_variance: false,
            cookies: Vec::new(),
            csrf_token: None,
            retry_after: None,
            action_rate_rejected: false,
            etag: None,
            download: None,
        }
    }

    /// Enables same-origin modules and Wasm for this page. Other constructors block scripts.
    pub fn interactive_html(body: Html) -> Self {
        let mut response = Self::html(body);
        response.csp = INTERACTIVE_CONTENT_SECURITY_POLICY;
        response
    }

    pub(crate) fn interactive_shared(body: Arc<[u8]>) -> Self {
        let mut response = Self::shared(body, "text/html; charset=utf-8");
        response.csp = INTERACTIVE_CONTENT_SECURITY_POLICY;
        response
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
            body: Body::Bytes(Cow::Owned(body.into())),
            csp: CONTENT_SECURITY_POLICY,
            location: None,
            navigation_cache: crate::NavigationCachePolicy::no_store(),
            navigation_variance: false,
            cookies: Vec::new(),
            csrf_token: None,
            retry_after: None,
            action_rate_rejected: false,
            etag: None,
            download: None,
        }
    }

    pub(crate) fn embedded(body: &'static [u8], media_type: MediaType) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type: media_type.as_str(),
            body: Body::Bytes(Cow::Borrowed(body)),
            csp: CONTENT_SECURITY_POLICY,
            location: None,
            navigation_cache: crate::NavigationCachePolicy::no_store(),
            navigation_variance: false,
            cookies: Vec::new(),
            csrf_token: None,
            retry_after: None,
            action_rate_rejected: false,
            etag: None,
            download: None,
        }
    }

    /// Sets the response status without changing its body or content type.
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        if status != StatusCode::SeeOther {
            self.location = None;
        }
        self
    }

    /// Redirects to an internal GET URL after a submission, with an empty body.
    pub fn see_other(location: InternalUrl) -> Self {
        let mut response = Self::text("").with_status(StatusCode::SeeOther);
        response.location = Some(location);
        response
    }

    /// The typed redirect destination, when present.
    pub fn location(&self) -> Option<&InternalUrl> {
        self.location.as_ref()
    }

    pub(crate) fn shared(body: Arc<[u8]>, content_type: &'static str) -> Self {
        Self {
            status: StatusCode::Ok,
            content_type,
            body: Body::Shared(body),
            csp: CONTENT_SECURITY_POLICY,
            location: None,
            navigation_cache: crate::NavigationCachePolicy::no_store(),
            navigation_variance: false,
            cookies: Vec::new(),
            csrf_token: None,
            retry_after: None,
            action_rate_rejected: false,
            etag: None,
            download: None,
        }
    }

    /// Returns the response status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Borrows the complete buffered body, or only the initial prefix of a lazy stream.
    /// Use buffered_body() to distinguish those cases without running application code.
    ///
    /// This returned `&str` before alpha.4. Use `body_text()` when UTF-8 is needed.
    pub fn body(&self) -> &[u8] {
        self.body.as_ref()
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

    /// Returns the fixed policy selected by the response constructor.
    pub fn content_security_policy(&self) -> &'static str {
        self.csp
    }

    pub(crate) fn error(status: StatusCode) -> Self {
        Self::text(format!("{}\n", status.reason())).with_status(status)
    }
    pub(crate) fn guard_no_store(mut self) -> Self {
        self.navigation_cache = crate::NavigationCachePolicy::no_store();
        self
    }

    /// Appends one validated cookie. Repeated name/path pairs and exceeded budgets fail atomically.
    /// Cookie changes force no-store and signal navigation cache invalidation without exposing values.
    pub fn add_cookie(&mut self, cookie: crate::Cookie) -> Result<(), crate::CookieError> {
        use crate::{CookieError, MAX_RESPONSE_COOKIE_BYTES, MAX_RESPONSE_COOKIES};
        if self.cookies.len() == MAX_RESPONSE_COOKIES {
            return Err(CookieError::TooManyCookies);
        }
        if self.cookies.iter().any(|existing| {
            existing.name() == cookie.name() && existing.cookie_path() == cookie.cookie_path()
        }) {
            return Err(CookieError::DuplicateCookie);
        }
        let bytes = self
            .cookies
            .iter()
            .map(|entry| entry.wire_line().len())
            .sum::<usize>()
            + cookie.wire_line().len();
        if bytes > MAX_RESPONSE_COOKIE_BYTES {
            return Err(CookieError::TooLarge);
        }
        self.cookies.push(cookie);
        self.navigation_cache = crate::NavigationCachePolicy::no_store();
        Ok(())
    }
    /// Adds one cookie using the same checks as add_cookie.
    pub fn with_cookie(mut self, cookie: crate::Cookie) -> Result<Self, crate::CookieError> {
        self.add_cookie(cookie)?;
        Ok(self)
    }
    /// Validated response cookies, in header emission order.
    pub fn cookies(&self) -> &[crate::Cookie] {
        &self.cookies
    }

    pub(crate) fn wire_headers(&self, allowed: &str) -> String {
        let allow = if self.status == StatusCode::MethodNotAllowed {
            format!("Allow: {allowed}\r\n")
        } else {
            String::new()
        };
        let location = self
            .location
            .as_ref()
            .map_or_else(String::new, |url| format!("Location: {}\r\n", url.as_str()));
        let framing = if self.is_streaming() {
            "Transfer-Encoding: chunked\r\n".to_owned()
        } else {
            format!("Content-Length: {}\r\n", self.body().len())
        };
        let etag = self
            .etag()
            .map_or_else(String::new, |tag| format!("ETag: {}\r\n", tag.as_str()));
        let csrf = self.csrf_token.as_ref().map_or_else(String::new, |token| {
            format!("{}: {}\r\n", crate::CSRF_HEADER, token.as_str())
        });
        let retry = self.retry_after().map_or_else(String::new, |retry| {
            let marker = if self.action_rate_rejected {
                "Ruvane-Action-Rejected: rate-limit\r\n"
            } else {
                ""
            };
            format!("Retry-After: {}\r\n{marker}", retry.seconds())
        });
        let cookies = if self.cookies.is_empty() {
            String::new()
        } else {
            let mut fields = String::from("Ruvane-Cache-Reset: cookies\r\n");
            for cookie in &self.cookies {
                fields.push_str(&cookie.wire_line());
            }
            fields
        };
        let download = self
            .download
            .as_ref()
            .map_or_else(String::new, crate::DownloadName::header);
        format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\n{}Connection: close\r\nX-Content-Type-Options: nosniff\r\nCache-Control: {}\r\nContent-Security-Policy: {}\r\n{}{}{}{}{}{}{}{}\r\n",
            self.status,
            self.content_type,
            framing,
            self.cache_control(),
            self.csp,
            cookies,
            etag,
            csrf,
            retry,
            allow,
            location,
            download,
            if self.navigation_variance {
                "Vary: Accept, Ruvane-Application-Revision\r\n"
            } else {
                ""
            }
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
    // Check bytes before decoding them.
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
        if name.eq_ignore_ascii_case("cookie") && value.len() > crate::MAX_COOKIE_HEADER_BYTES + 1 {
            return Err(RequestHeaderFieldsTooLarge);
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
    let content_length = if let Some(value) = header("content-length") {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BadRequest);
        }
        let length = value.parse::<u64>().map_err(|_| BadRequest)?;
        if length != 0 && !matches!(method, "POST" | "PUT" | "PATCH" | "DELETE") {
            return Err(ContentTooLarge);
        }
        Some(length)
    } else {
        None
    };
    if header("expect").is_some() {
        return Err(ExpectationFailed);
    }
    if header("upgrade").is_some() {
        return Err(NotImplemented);
    }
    let method = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        "OPTIONS" | "CONNECT" | "TRACE" => {
            return Err(MethodNotAllowed);
        }
        _ => return Err(NotImplemented),
    };
    let cookies =
        crate::Cookies::parse(header("cookie").unwrap_or("")).map_err(|error| match error {
            crate::CookieError::TooLarge | crate::CookieError::TooManyCookies => {
                RequestHeaderFieldsTooLarge
            }
            _ => BadRequest,
        })?;
    let if_match = header("if-match")
        .map(crate::IfMatch::parse)
        .transpose()
        .map_err(|e| match e {
            crate::EntityTagError::InvalidSyntax => BadRequest,
            _ => RequestHeaderFieldsTooLarge,
        })?;
    Ok(Request {
        method,
        target: target.to_owned(),
        cookies,
        headers,
        params: Vec::new(),
        // The route selects query decoding later; check raw URI syntax here.
        query_params: Query::default(),
        content_length,
        form: None,
        json: None,
        multipart: None,
        guard_private: false,
        if_match,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_preview_bodies_clone_handles_and_embedded_assets_still_borrow() {
        let body: Arc<[u8]> = Arc::from(&b"shared"[..]);
        let response = Response::shared(Arc::clone(&body), "text/plain; charset=utf-8");
        let clone = response.clone();
        assert_eq!(response.body().as_ptr(), body.as_ptr());
        assert_eq!(clone.body().as_ptr(), body.as_ptr());
        static ASSET: &[u8] = b"embedded";
        assert_eq!(
            Response::embedded(ASSET, MediaType::PlainText)
                .body()
                .as_ptr(),
            ASSET.as_ptr()
        );
    }

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
            "/λ",
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
