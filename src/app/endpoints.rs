use super::*;
use crate::{ActionError, ActionReply};

type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;
type ActionHandler = Arc<dyn Fn(&Request) -> Result<ActionReply, ActionError> + Send + Sync>;
#[derive(Clone)]
enum Post {
    Form(Handler),
    Action(ActionHandler),
}

/// Native handlers returned by a route.rs file's `pub fn route()`.
/// Duplicate method declarations and empty endpoints are rejected at registration.
#[derive(Clone, Default)]
pub struct FileEndpoint {
    get: Option<(QueryMode, Handler)>,
    post: Option<Post>,
    duplicate: bool,
}
impl FileEndpoint {
    /// Creates an empty endpoint to configure with one GET and/or POST handler.
    pub fn new() -> Self {
        Self::default()
    }
    /// Declares read-only GET/HEAD handling with URI query semantics.
    pub fn get(mut self, handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
        self.duplicate |= self.get.is_some();
        self.get = Some((QueryMode::Uri, Arc::new(handler)));
        self
    }
    /// Declares read-only GET/HEAD handling with form query semantics.
    pub fn get_form(
        mut self,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.duplicate |= self.get.is_some();
        self.get = Some((QueryMode::Form, Arc::new(handler)));
        self
    }
    /// Declares a native POST form with the Router's origin, MIME and body checks.
    pub fn post_form(
        mut self,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.duplicate |= self.post.is_some();
        self.post = Some(Post::Form(Arc::new(handler)));
        self
    }
    /// Declares a typed POST action with the same checks and revision enforcement.
    pub fn post_action(
        mut self,
        handler: impl Fn(&Request) -> Result<ActionReply, ActionError> + Send + Sync + 'static,
    ) -> Self {
        self.duplicate |= self.post.is_some();
        self.post = Some(Post::Action(Arc::new(handler)));
        self
    }
    fn count(&self) -> usize {
        usize::from(self.get.is_some()) + usize::from(self.post.is_some())
    }
    pub(super) fn mount(&self, router: &mut Router, pattern: &str) -> Result<(), RouteError> {
        if let Some((mode, handler)) = &self.get {
            let handler = Arc::clone(handler);
            let handle = move |request: &Request| {
                if reserved(request.path()) {
                    Response::error(StatusCode::NotFound)
                } else {
                    handler(request)
                }
            };
            match mode {
                QueryMode::Uri => router.get(pattern, handle)?,
                QueryMode::Form => router.get_form(pattern, handle)?,
            }
        }
        if let Some(post) = &self.post {
            match post {
                Post::Form(handler) => {
                    let handler = Arc::clone(handler);
                    router.post_form(pattern, move |request| {
                        if reserved(request.path()) {
                            Response::error(StatusCode::NotFound)
                        } else {
                            handler(request)
                        }
                    })?;
                }
                Post::Action(handler) => {
                    let handler = Arc::clone(handler);
                    router.post_action_response(pattern, move |request| {
                        if reserved(request.path()) {
                            return Response::error(StatusCode::NotFound);
                        }
                        match handler(request) {
                            Ok(reply) => Response::action(&reply),
                            Err(_) => Response::error(StatusCode::InternalServerError),
                        }
                    })?;
                }
            }
        }
        Ok(())
    }
}
fn reserved(path: &str) -> bool {
    matches!(path.split('/').nth(1), Some("_ruvane" | "ruvane-runtime"))
}
#[derive(Clone)]
pub(super) struct Endpoint {
    pub(super) pattern: String,
    pub(super) declaration: FileEndpoint,
}
impl App {
    pub(crate) fn file_endpoint(
        &mut self,
        pattern: &str,
        declaration: FileEndpoint,
    ) -> Result<(), AppError> {
        PageRoute::new("endpoint", pattern)?;
        if reserved(pattern) {
            return Err(AppError::Route(RouteError::InvalidPath));
        }
        if declaration.duplicate || declaration.count() == 0 {
            return Err(AppError::InvalidEndpoint);
        }
        let mut candidate = self.clone();
        candidate.endpoints.push(Endpoint {
            pattern: pattern.into(),
            declaration,
        });
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub(super) fn validate_endpoints(&self, router: &mut Router) -> Result<(), AppError> {
        let count: usize = self
            .endpoints
            .iter()
            .map(|entry| entry.declaration.count())
            .sum();
        if count
            + self.pages.len()
            + self.assets.len()
            + self
                .client_runtime
                .as_ref()
                .map_or(0, ClientRuntime::route_count)
            > MAX_APP_ROUTES
        {
            return Err(AppError::TooManyRoutes);
        }
        for endpoint in &self.endpoints {
            if endpoint.declaration.get.is_some() && router.overlaps_get(&endpoint.pattern)? {
                return Err(AppError::EndpointConflict);
            }
        }
        for endpoint in &self.endpoints {
            endpoint.declaration.mount(router, &endpoint.pattern)?;
        }
        Ok(())
    }
}
