use crate::http::valid_path;
use crate::url::{decode, valid_segment};
use crate::{
    AssetError, InternalUrl, MAX_ASSET_BYTES, MAX_ASSETS, MAX_PATH_PARAMS, MAX_TOTAL_ASSET_BYTES,
    MediaType, Query, QueryMode, Request, Response, StatusCode,
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

/// A GET router with exact paths, named single-segment captures, and HEAD support.
///
/// Exact paths take priority regardless of registration order. Overlapping
/// dynamic patterns are rejected. Matching uses raw, case-sensitive segments;
/// only captures are decoded once, after selection. Queries do not participate
/// in matching. Handlers must be read-only, bounded,
/// and safe to call concurrently; the framework cannot interrupt their code.
#[derive(Default)]
pub struct Router {
    routes: Vec<Route>,
    asset_count: usize,
    asset_bytes: usize,
}

enum Segment {
    Literal(String),
    Capture(String),
}

struct Route {
    pattern: String,
    segments: Vec<Segment>,
    dynamic: bool,
    handler: Handler,
    query_mode: QueryMode,
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

fn pattern_segments(pattern: &str) -> Result<(Vec<Segment>, bool), RouteError> {
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
        if self.routes.iter().any(|route| route.pattern == path) {
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
        self.register(path.into(), QueryMode::Uri, handler)
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
        self.register(path.into(), QueryMode::Form, handler)
    }

    fn register(
        &mut self,
        path: String,
        query_mode: QueryMode,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        let (segments, dynamic) = pattern_segments(&path)?;
        if self.routes.iter().any(|existing| existing.pattern == path) {
            return Err(RouteError::DuplicatePath);
        }
        let route = Route {
            pattern: path,
            segments,
            dynamic,
            handler: Box::new(handler),
            query_mode,
        };
        if dynamic
            && self
                .routes
                .iter()
                .any(|existing| existing.dynamic && existing.overlaps(&route))
        {
            return Err(RouteError::AmbiguousRoute);
        }
        self.routes.push(route);
        Ok(())
    }

    pub(crate) fn dispatch(&self, mut request: Request) -> Response {
        let selected = self
            .routes
            .iter()
            .find(|route| !route.dynamic && route.pattern == request.path())
            .or_else(|| {
                self.routes
                    .iter()
                    .find(|route| route.dynamic && route.matches(request.path()))
            });
        let mode = selected.map_or(QueryMode::Uri, |route| route.query_mode);
        request.query_params = match Query::parse_with_mode(request.query().unwrap_or(""), mode) {
            Ok(query) => query,
            Err(_) => return Response::error(StatusCode::BadRequest),
        };
        let Some(route) = selected else {
            return Response::error(StatusCode::NotFound);
        };
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
