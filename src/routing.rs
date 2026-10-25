use crate::http::valid_path;
use crate::url::{decode, valid_segment};
use crate::{MAX_PATH_PARAMS, Request, Response, StatusCode};
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

    /// Registers a GET handler, also used to produce HEAD response headers.
    ///
    /// A whole segment such as `{slug}` captures one nonempty value. Names are
    /// unique ASCII identifiers of at most 64 bytes, with at most
    /// [`MAX_PATH_PARAMS`] captures. Wildcards and optional segments are
    /// unsupported. Invalid, duplicate, and overlapping dynamic patterns fail
    /// at registration. Handlers may inspect the
    /// method through [`Request::method`]. They must not depend on being called
    /// only once: browsers and other clients can retry read requests.
    pub fn get(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        let path = path.into();
        let (segments, dynamic) = pattern_segments(&path)?;
        if self.routes.iter().any(|existing| existing.pattern == path) {
            return Err(RouteError::DuplicatePath);
        }
        let route = Route {
            pattern: path,
            segments,
            dynamic,
            handler: Box::new(handler),
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
