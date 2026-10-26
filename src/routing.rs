use crate::http::valid_path;
use crate::url::{decode, valid_segment};
use crate::{
    AssetError, InternalUrl, MAX_ASSET_BYTES, MAX_ASSETS, MAX_PATH_PARAMS, MAX_TOTAL_ASSET_BYTES,
    MediaType, Method, Query, QueryMode, Request, Response, SiteOrigin, StatusCode,
};
use std::fmt;

type Handler = Box<dyn Fn(&Request) -> Response + Send + Sync>;

/// A route registration error, detected before the server starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError {
    /// The path is outside the local profile, or contains a query string.
    InvalidPath,
    /// A handler is already registered for this exact path.
    DuplicatePath,
    /// Two dynamic routes can match the same raw path.
    AmbiguousRoute,
}

impl fmt::Display for RouteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "invalid route path",
            Self::DuplicatePath => "duplicate route path",
            Self::AmbiguousRoute => "ambiguous dynamic route",
        })
    }
}

impl std::error::Error for RouteError {}

/// A router for GET/HEAD pages and explicit POST form mutations.
///
/// Exact paths take priority regardless of registration order. Overlapping
/// dynamic patterns are rejected. Matching uses raw, case-sensitive segments;
/// only captures are decoded once, after selection. Queries do not participate
/// in matching. GET handlers must be read-only. All handlers must be bounded
/// and safe to call concurrently; the framework cannot interrupt their code.
#[derive(Default)]
pub struct Router {
    routes: Vec<Route>,
    asset_count: usize,
    asset_bytes: usize,
    not_found: Option<Response>,
    client_not_found: Option<Handler>,
    application_revision: Option<String>,
    internal_error: Option<Response>,
}

#[derive(Clone, Debug)]
pub(crate) enum Segment {
    Literal(String),
    Capture(String),
}

struct Route {
    method: Method,
    pattern: String,
    segments: Vec<Segment>,
    dynamic: bool,
    handler: Handler,
    query_mode: QueryMode,
    action: bool,
}

impl Route {
    fn matches(&self, path: &str) -> bool {
        let mut parts = path[1..].split('/');
        for segment in &self.segments {
            let Some(part) = parts.next() else {
                return false;
            };
            if match segment {
                Segment::Literal(value) => value != part,
                Segment::Capture(_) => part.is_empty(),
            } {
                return false;
            }
        }
        parts.next().is_none()
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self
                .segments
                .iter()
                .zip(&other.segments)
                .all(|(a, b)| match (a, b) {
                    (Segment::Literal(a), Segment::Literal(b)) => a == b,
                    (Segment::Literal(value), _) | (_, Segment::Literal(value)) => {
                        !value.is_empty()
                    }
                    _ => true,
                })
    }
}

pub(crate) fn pattern_segments(pattern: &str) -> Result<(Vec<Segment>, bool), RouteError> {
    if !pattern.starts_with('/') || pattern.contains("//") {
        return Err(RouteError::InvalidPath);
    }
    let mut segments = Vec::new();
    let mut names = Vec::new();
    for part in pattern[1..].split('/') {
        if let Some(name) = part
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
        {
            let mut bytes = name.bytes();
            if name.len() > 64
                || !bytes
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || names.contains(&name)
                || names.len() == MAX_PATH_PARAMS
            {
                return Err(RouteError::InvalidPath);
            }
            names.push(name);
            segments.push(Segment::Capture(name.to_owned()));
        } else {
            if !valid_path(&format!("/{part}")) {
                return Err(RouteError::InvalidPath);
            }
            segments.push(Segment::Literal(part.to_owned()));
        }
    }
    Ok((segments, !names.is_empty()))
}

impl Router {
    pub(crate) fn has_path(&self, path: &str) -> bool {
        self.routes.iter().any(|route| route.matches(path))
    }
    /// Registers a typed POST action with the same origin/body policy as native forms.
    pub fn post_action(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Result<crate::ActionReply, crate::ActionError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), RouteError> {
        self.post_form(path, move |request| match handler(request) {
            Ok(reply) => Response::action(&reply),
            Err(_) => Response::error(StatusCode::InternalServerError),
        })?;
        self.routes.last_mut().unwrap().action = true;
        Ok(())
    }
    /// Creates an empty router, which returns 404 for every path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers trusted embedded bytes on one fixed GET/HEAD path.
    ///
    /// The path must pass `InternalUrl` validation, without a query or dynamic
    /// captures. It shares the route namespace with `get()`. Asset responses
    /// borrow the static slice, with no filesystem access or per-request body
    /// copy. Query validation and server I/O/body limits still apply.
    /// Registration is bounded by `MAX_ASSETS`, `MAX_ASSET_BYTES`, and
    /// `MAX_TOTAL_ASSET_BYTES`; a failure does not consume those budgets.
    ///
    /// ```
    /// let mut router = ruvane::Router::new();
    /// router.asset("/assets/site.css", b"body { color: #123; }", ruvane::MediaType::Css).unwrap();
    /// ```
    pub fn asset(
        &mut self,
        path: impl Into<String>,
        bytes: &'static [u8],
        media_type: MediaType,
    ) -> Result<(), AssetError> {
        let path = path.into();
        let _ = InternalUrl::new(&path).map_err(|_| AssetError::InvalidPath)?;
        if self
            .routes
            .iter()
            .any(|route| route.method == Method::Get && route.pattern == path)
        {
            return Err(AssetError::DuplicatePath);
        }
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(AssetError::TooLarge);
        }
        if self.asset_count == MAX_ASSETS {
            return Err(AssetError::TooManyAssets);
        }
        let total = self
            .asset_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_TOTAL_ASSET_BYTES)
            .ok_or(AssetError::TotalTooLarge)?;
        self.get(path, move |_| Response::embedded(bytes, media_type))
            .map_err(|error| match error {
                RouteError::DuplicatePath => AssetError::DuplicatePath,
                _ => AssetError::InvalidPath,
            })?;
        self.asset_count += 1;
        self.asset_bytes = total;
        Ok(())
    }

    /// Registers a GET handler, also used to produce HEAD response headers.
    ///
    /// A whole segment such as `{slug}` captures one nonempty value. Names are
    /// unique ASCII identifiers of at most 64 bytes, with at most
    /// [`MAX_PATH_PARAMS`] captures. Wildcards and optional segments are
    /// unsupported. Invalid, duplicate, and overlapping dynamic patterns fail
    /// at registration. Handlers may inspect the
    /// method through [`Request::method`]. They must not depend on being called
    /// only once: browsers and other clients can retry read requests.
    /// Queries use `QueryMode::Uri`, preserving literal plus signs. Use
    /// [`Self::get_form`] to select native form query decoding explicitly.
    pub fn get(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register(path.into(), Method::Get, QueryMode::Uri, handler)
    }

    /// Registers a read-only GET/HEAD route with native form query decoding.
    ///
    /// Raw `+` becomes a space, while `%2B` stays a plus. Route precedence,
    /// ambiguity checks, and all query budgets are shared with `get()`.
    /// Invalid queries are rejected before calling the handler. Request data
    /// cannot switch this mode. Path captures always retain URI semantics.
    pub fn get_form(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register(path.into(), Method::Get, QueryMode::Form, handler)
    }

    /// Registers a URL-encoded POST action with mandatory same-origin checks.
    ///
    /// Origin must match the HTTP origin of the server-validated Host. Missing
    /// or null origins are refused. Fetch Metadata, when present, must also
    /// indicate same-origin. Forwarded headers never configure this policy.
    /// The handler receives validated fields through `Request::form_data`.
    /// GET/HEAD and POST registrations have separate route namespaces.
    pub fn post_form(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register(path.into(), Method::Post, QueryMode::Uri, handler)
    }

    fn register(
        &mut self,
        path: String,
        method: Method,
        query_mode: QueryMode,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        let (segments, dynamic) = pattern_segments(&path)?;
        if self
            .routes
            .iter()
            .any(|existing| existing.method == method && existing.pattern == path)
        {
            return Err(RouteError::DuplicatePath);
        }
        let route = Route {
            method,
            pattern: path,
            segments,
            dynamic,
            handler: Box::new(handler),
            action: false,
            query_mode,
        };
        if dynamic
            && self.routes.iter().any(|existing| {
                existing.method == method && existing.dynamic && existing.overlaps(&route)
            })
        {
            return Err(RouteError::AmbiguousRoute);
        }
        self.routes.push(route);
        Ok(())
    }

    pub(crate) fn application_revision(&mut self, revision: u64) {
        self.application_revision = Some(revision.to_string());
    }
    pub(crate) fn client_not_found(
        &mut self,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) {
        self.client_not_found = Some(Box::new(handler));
    }

    pub(crate) fn page_errors(&mut self, not_found: Option<Response>, internal: Option<Response>) {
        self.not_found = not_found;
        self.internal_error = internal;
    }

    pub(crate) fn error_response(&self, status: StatusCode) -> Response {
        let custom = match status {
            StatusCode::NotFound => self.not_found.as_ref(),
            StatusCode::InternalServerError => self.internal_error.as_ref(),
            _ => None,
        };
        custom.cloned().unwrap_or_else(|| Response::error(status))
    }

    fn select(&self, method: Method, path: &str) -> Option<&Route> {
        let method = if method == Method::Head {
            Method::Get
        } else {
            method
        };
        self.routes
            .iter()
            .find(|route| route.method == method && !route.dynamic && route.pattern == path)
            .or_else(|| {
                self.routes
                    .iter()
                    .find(|route| route.method == method && route.dynamic && route.matches(path))
            })
    }

    pub(crate) fn allowed(&self, path: &str) -> &'static str {
        let get = self.select(Method::Get, path).is_some();
        let post = self.select(Method::Post, path).is_some();
        match (get, post) {
            (true, true) => "GET, HEAD, POST",
            (false, true) => "POST",
            _ => "GET, HEAD",
        }
    }

    pub(crate) fn prepare_post(
        &self,
        request: &Request,
        maximum: usize,
    ) -> Result<usize, StatusCode> {
        use StatusCode::{
            ContentTooLarge, Forbidden, LengthRequired, MethodNotAllowed, NotFound,
            UnsupportedMediaType,
        };
        if self.select(Method::Post, request.path()).is_none() {
            return Err(if self.select(Method::Get, request.path()).is_some() {
                MethodNotAllowed
            } else {
                NotFound
            });
        }
        let origin = request.header("origin").ok_or(Forbidden)?;
        let source = SiteOrigin::parse(origin).map_err(|_| Forbidden)?;
        let target = SiteOrigin::parse(&format!(
            "http://{}",
            request.header("host").ok_or(Forbidden)?
        ))
        .map_err(|_| Forbidden)?;
        if source.as_str() != origin
            || source != target
            || request
                .header("sec-fetch-site")
                .is_some_and(|site| site != "same-origin")
        {
            return Err(Forbidden);
        }
        if request.header("content-encoding").is_some() {
            return Err(UnsupportedMediaType);
        }
        let mut content_type = request
            .header("content-type")
            .ok_or(UnsupportedMediaType)?
            .split(';');
        if !content_type
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        {
            return Err(UnsupportedMediaType);
        }
        if let Some(parameter) = content_type.next() {
            let (key, value) = parameter
                .trim()
                .split_once('=')
                .ok_or(UnsupportedMediaType)?;
            if !key.trim().eq_ignore_ascii_case("charset")
                || !["utf-8", "\"utf-8\""]
                    .iter()
                    .any(|expected| value.trim().eq_ignore_ascii_case(expected))
                || content_type.next().is_some()
            {
                return Err(UnsupportedMediaType);
            }
        }
        let length = request.content_length.ok_or(LengthRequired)?;
        if length > maximum as u64 {
            return Err(ContentTooLarge);
        }
        Ok(length as usize)
    }

    pub(crate) fn dispatch(&self, mut request: Request) -> Response {
        let selected = self.select(request.method(), request.path());
        if let Some(revision) = &self.application_revision {
            let supplied = request.header("ruvane-application-revision");
            let typed_action = selected.is_some_and(|route| route.action);
            if supplied.is_some_and(|value| value != revision)
                || typed_action && supplied != Some(revision.as_str())
            {
                return Response::error(StatusCode::Conflict);
            }
        }
        let mode = selected.map_or(QueryMode::Uri, |route| route.query_mode);
        request.query_params = match Query::parse_with_mode(request.query().unwrap_or(""), mode) {
            Ok(query) => query,
            Err(_) => return Response::error(StatusCode::BadRequest),
        };
        let Some(route) = selected else {
            if self
                .routes
                .iter()
                .any(|route| route.matches(request.path()))
            {
                return Response::error(StatusCode::MethodNotAllowed);
            }
            if matches!(request.method(), Method::Get | Method::Head) {
                if let Some(handler) = &self.client_not_found {
                    return handler(&request);
                }
            }
            return self.error_response(StatusCode::NotFound);
        };
        if request.method() == Method::Post && request.form.is_none() {
            return Response::error(StatusCode::Forbidden);
        }
        if route.dynamic {
            let mut captures = Vec::new();
            for (segment, raw) in route.segments.iter().zip(request.path()[1..].split('/')) {
                if let Segment::Capture(name) = segment {
                    let value = match decode(raw).and_then(|value| {
                        valid_segment(&value)?;
                        Ok(value)
                    }) {
                        Ok(value) => value,
                        Err(_) => return Response::error(StatusCode::BadRequest),
                    };
                    captures.push((name.clone(), value));
                }
            }
            request.params = captures;
        }
        (route.handler)(&request)
    }
}

impl fmt::Debug for Router {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Router")
            .field("asset_count", &self.asset_count)
            .field("asset_bytes", &self.asset_bytes)
            .field(
                "paths",
                &self
                    .routes
                    .iter()
                    .map(|route| &route.pattern)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}
