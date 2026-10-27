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

/// A router for GET/HEAD pages and explicit mutation body profiles.
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
    body: MutationBody,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum MutationBody {
    Form,
    Json,
    Empty,
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
        segments_overlap(&self.segments, &other.segments)
    }
}

fn segments_overlap(a: &[Segment], b: &[Segment]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Segment::Literal(a), Segment::Literal(b)) => a == b,
            (Segment::Literal(value), _) | (_, Segment::Literal(value)) => !value.is_empty(),
            _ => true,
        })
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
    pub(crate) fn overlaps_get(&self, pattern: &str) -> Result<bool, RouteError> {
        let (segments, _) = pattern_segments(pattern)?;
        Ok(self.routes.iter().any(|route| {
            route.method == Method::Get && segments_overlap(&route.segments, &segments)
        }))
    }
    /// Registers a revision-checked POST action with cookie-capable responses.
    /// Use Response::action for the typed body; origin, MIME and form checks still apply.
    pub fn post_action_response(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.post_form(path, handler)?;
        self.routes.last_mut().unwrap().action = true;
        Ok(())
    }

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
        self.post_action_response(path, move |request| match handler(request) {
            Ok(reply) => Response::action(&reply),
            Err(_) => Response::error(StatusCode::InternalServerError),
        })
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
    /// Registers an explicit UTF-8 JSON POST with mandatory same-origin and bounded-body checks.
    /// The body is validated before guards/handlers and is available through Request::json.
    /// This shares the POST slot with forms/actions and does not opt into the RVA1 protocol.
    pub fn post_json(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register_json(path.into(), Method::Post, handler)
    }

    /// Registers a same-origin JSON PUT. The handler defines full replacement semantics.
    pub fn put_json(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register_json(path.into(), Method::Put, handler)
    }

    /// Registers a same-origin JSON PATCH. The handler defines and atomically applies its patch format.
    pub fn patch_json(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register_json(path.into(), Method::Patch, handler)
    }

    /// Registers a same-origin DELETE with no body, Content-Type or Content-Encoding.
    /// Content-Length may be absent or zero. Session and CSRF guards remain explicit.
    pub fn delete(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register(path.into(), Method::Delete, QueryMode::Uri, handler)?;
        self.routes.last_mut().unwrap().body = MutationBody::Empty;
        Ok(())
    }

    fn register_json(
        &mut self,
        path: String,
        method: Method,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        self.register(path, method, QueryMode::Uri, handler)?;
        self.routes.last_mut().unwrap().body = MutationBody::Json;
        Ok(())
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
            body: MutationBody::Form,
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

    pub(crate) fn allowed(&self, path: &str) -> String {
        [
            (Method::Get, "GET"),
            (Method::Head, "HEAD"),
            (Method::Post, "POST"),
            (Method::Put, "PUT"),
            (Method::Patch, "PATCH"),
            (Method::Delete, "DELETE"),
        ]
        .into_iter()
        .filter_map(|(method, name)| self.select(method, path).is_some().then_some(name))
        .collect::<Vec<_>>()
        .join(", ")
    }

    pub(crate) fn prepare_mutation(
        &self,
        request: &Request,
        maximum: usize,
    ) -> Result<usize, StatusCode> {
        use StatusCode::{
            ContentTooLarge, Forbidden, LengthRequired, MethodNotAllowed, NotFound,
            UnsupportedMediaType,
        };
        let selected = self.select(request.method(), request.path());
        if selected.is_none() {
            return Err(if self.has_path(request.path()) {
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
        if selected.unwrap().body == MutationBody::Empty {
            if request.content_length.is_some_and(|length| length != 0) {
                return Err(ContentTooLarge);
            }
            if request.header("content-type").is_some() {
                return Err(UnsupportedMediaType);
            }
            return Ok(0);
        }
        let mut content_type = request
            .header("content-type")
            .ok_or(UnsupportedMediaType)?
            .split(';');
        if !content_type
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case(if selected.unwrap().body == MutationBody::Json {
                crate::JSON_MEDIA_TYPE
            } else {
                "application/x-www-form-urlencoded"
            })
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
    pub(crate) fn parse_mutation(
        &self,
        request: &mut Request,
        bytes: &[u8],
    ) -> Result<(), StatusCode> {
        let route = self
            .select(request.method(), request.path())
            .ok_or(StatusCode::NotFound)?;
        match route.body {
            MutationBody::Empty => {
                if !bytes.is_empty() {
                    return Err(StatusCode::ContentTooLarge);
                }
            }
            MutationBody::Form => {
                request.form = Some(crate::FormData::parse(bytes).map_err(|error| match error {
                    crate::FormError::TooLarge | crate::FormError::TooManyFields => {
                        StatusCode::ContentTooLarge
                    }
                    _ => StatusCode::BadRequest,
                })?)
            }
            MutationBody::Json => {
                request.json = Some(crate::JsonValue::parse(bytes).map_err(|error| {
                    if error.is_limit() {
                        StatusCode::ContentTooLarge
                    } else {
                        StatusCode::BadRequest
                    }
                })?)
            }
        }
        Ok(())
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
        if request.method().is_mutation()
            && !match route.body {
                MutationBody::Empty => {
                    request.form.is_none()
                        && request.json.is_none()
                        && request.content_length.is_none_or(|length| length == 0)
                }
                MutationBody::Form => request.form.is_some() && request.json.is_none(),
                MutationBody::Json => request.json.is_some() && request.form.is_none(),
            }
        {
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
