use crate::http::valid_path;
use crate::{Request, Response, StatusCode};
use std::fmt;

type Handler = Box<dyn Fn(&Request) -> Response + Send + Sync>;

/// A route registration error, detected before the server starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError {
    /// The path is outside the local profile, or contains a query string.
    InvalidPath,
    /// A handler is already registered for this exact path.
    DuplicatePath,
}

impl fmt::Display for RouteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "invalid route path",
            Self::DuplicatePath => "duplicate route path",
        })
    }
}

impl std::error::Error for RouteError {}

/// An exact-path router for GET and automatic HEAD handling.
///
/// Query strings do not participate in matching. Paths are case-sensitive and
/// never percent-decoded or normalized. Handlers must be read-only, bounded,
/// and safe to call concurrently; the framework cannot interrupt their code.
#[derive(Default)]
pub struct Router {
    routes: Vec<(String, Handler)>,
}

impl Router {
    /// Creates an empty router, which returns 404 for every path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a GET handler, also used to produce HEAD response headers.
    ///
    /// Duplicate or invalid paths are rejected. Handlers may inspect the
    /// method through [`Request::method`]. They must not depend on being called
    /// only once: browsers and other clients can retry read requests.
    pub fn get(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        let path = path.into();
        if !valid_path(&path) {
            return Err(RouteError::InvalidPath);
        }
        if self.routes.iter().any(|(existing, _)| *existing == path) {
            return Err(RouteError::DuplicatePath);
        }
        self.routes.push((path, Box::new(handler)));
        Ok(())
    }

    pub(crate) fn dispatch(&self, request: &Request) -> Response {
        match self.routes.iter().find(|(path, _)| path == request.path()) {
            Some((_, handler)) => handler(request),
            None => Response::error(StatusCode::NotFound),
        }
    }
}

impl fmt::Debug for Router {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Router")
            .field(
                "paths",
                &self.routes.iter().map(|(path, _)| path).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}
