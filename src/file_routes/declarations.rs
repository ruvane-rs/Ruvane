use super::FileRouteError;
use crate::{
    App, ClientBoundary, ClientLayoutData, ClientPageData, Html, InitialState, NavigationMatch,
    Page, PageContext, PageError, PageMode, PageRoute,
};
use std::sync::Arc;

type State = fn(&NavigationMatch) -> Result<InitialState, PageError>;
type PageLoader = Arc<dyn Fn(&PageContext<'_>) -> Result<Page, PageError> + Send + Sync>;
type ClientLoader =
    Arc<dyn Fn(&PageContext<'_>) -> Result<ClientPageData, PageError> + Send + Sync>;
type LayoutRenderer = Arc<dyn Fn(&PageContext<'_>, &str, Html) -> Html + Send + Sync>;
type LayoutLoader =
    Arc<dyn Fn(&PageContext<'_>) -> Result<ClientLayoutData, PageError> + Send + Sync>;

/// A loading component declared by `pub fn loading()` in loading.rs.
pub struct FileLoading(u32);
impl FileLoading {
    /// Declares a component checked by the registry with Number(0) and an empty seed.
    pub fn new(component: u32) -> Self {
        Self(component)
    }
}

/// An error component declared by `pub fn error()` in error.rs.
pub struct FileError(u32);
impl FileError {
    /// Declares a component checked with every supported error state and an empty seed.
    pub fn new(component: u32) -> Self {
        Self(component)
    }
}

/// A read-only Number(404) component declared by `pub fn not_found()`.
pub struct FileNotFound(u32);
impl FileNotFound {
    /// Declares a component validated with an empty seed before registration commits.
    pub fn new(component: u32) -> Self {
        Self(component)
    }
}

/// A validated file 404 presentation inherited by generated client routes.
#[derive(Clone, Copy)]
pub struct FileRouteNotFound(u32);

/// A complete, validated pair of inherited file presentations.
#[derive(Clone, Copy)]
pub struct FileRouteBoundary {
    loading: u32,
    error: u32,
}
impl FileRouteBoundary {
    fn client(self) -> ClientBoundary {
        ClientBoundary::new(self.loading, self.error).unwrap()
    }
}

/// A page declaration returned by a route file's `pub fn page()`.
pub struct FilePage {
    name: String,
    kind: PageKind,
    boundary: Option<FileRouteBoundary>,
    not_found: Option<FileRouteNotFound>,
    server_cache: crate::ServerCachePolicy,
    prerender: Option<Vec<crate::PageParams>>,
}
enum PageKind {
    Server(PageMode, PageLoader),
    Client(u32, State, ClientLoader),
}
impl FilePage {
    /// Declares a native page. Static exports use directory URLs with a trailing slash.
    pub fn server(
        name: impl Into<String>,
        mode: PageMode,
        render: impl Fn(&PageContext<'_>) -> Result<Page, PageError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            kind: PageKind::Server(mode, Arc::new(render)),
            boundary: None,
            not_found: None,
            server_cache: crate::ServerCachePolicy::no_store(),
            prerender: None,
        }
    }
    /// Declares a shared SSR/Wasm page. The component factory remains application-owned.
    pub fn client(
        name: impl Into<String>,
        component: u32,
        state: State,
        load: impl Fn(&PageContext<'_>) -> Result<ClientPageData, PageError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            kind: PageKind::Client(component, state, Arc::new(load)),
            boundary: None,
            not_found: None,
            server_cache: crate::ServerCachePolicy::no_store(),
            prerender: None,
        }
    }
    /// Freezes these explicitly enumerated public client URLs during production build.
    /// Native-only declarations and enabled server cache policies are rejected.
    pub fn prerender(mut self, params: Vec<crate::PageParams>) -> Self {
        self.prerender = Some(params);
        self
    }
    /// Opts this client page's entire branch into public server data caching.
    /// Native-only FilePage::server declarations reject an enabled policy.
    pub fn server_cache(mut self, policy: crate::ServerCachePolicy) -> Self {
        self.server_cache = policy;
        self
    }
    /// Attaches the generated directory's presentations to this client page.
    pub fn with_boundary(mut self, boundary: &FileRouteBoundary) -> Self {
        self.boundary = Some(*boundary);
        self
    }
    /// Attaches the directory's inherited 404 presentation to a client declaration.
    pub fn with_not_found(mut self, presentation: &FileRouteNotFound) -> Self {
        self.not_found = Some(*presentation);
        self
    }
}

/// A layout declaration returned by a route file's `pub fn layout()`.
pub struct FileLayout {
    name: String,
    kind: LayoutKind,
    boundary: Option<FileRouteBoundary>,
    not_found: Option<FileRouteNotFound>,
}
enum LayoutKind {
    Server(LayoutRenderer),
    Client(u32, State, LayoutLoader),
}
impl FileLayout {
    /// Declares a native fragment layout inherited by native descendant pages.
    pub fn server(
        name: impl Into<String>,
        render: impl Fn(&PageContext<'_>, &str, Html) -> Html + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            kind: LayoutKind::Server(Arc::new(render)),
            boundary: None,
            not_found: None,
        }
    }
    /// Declares a persistent client layout inherited by interactive descendant pages.
    pub fn client(
        name: impl Into<String>,
        component: u32,
        state: State,
        load: impl Fn(&PageContext<'_>) -> Result<ClientLayoutData, PageError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            kind: LayoutKind::Client(component, state, Arc::new(load)),
            boundary: None,
            not_found: None,
        }
    }
    /// Attaches presentations for this client layout's children, excluding its own loader.
    pub fn with_boundary(mut self, boundary: &FileRouteBoundary) -> Self {
        self.boundary = Some(*boundary);
        self
    }
    /// Attaches the directory's inherited 404 presentation to a client declaration.
    pub fn with_not_found(mut self, presentation: &FileRouteNotFound) -> Self {
        self.not_found = Some(*presentation);
        self
    }
}

/// A validated parent layout handle used by the generated registry.
pub struct FileRouteLayout {
    name: String,
    client: bool,
}

/// Registers generated declarations on a private App clone until `finish` succeeds.
/// Dropping an unfinished registry leaves the original App unchanged.
pub struct FileRouteRegistry<'a> {
    target: &'a mut App,
    candidate: App,
    failed: bool,
}
impl<'a> FileRouteRegistry<'a> {
    /// Starts an atomic registration batch without executing page/layout loaders.
    pub fn new(app: &'a mut App) -> Self {
        Self {
            candidate: app.clone(),
            target: app,
            failed: false,
        }
    }

    /// Adds native GET/HEAD and POST handlers without executing them.
    pub fn endpoint(
        &mut self,
        source: &str,
        pattern: &str,
        declaration: crate::FileEndpoint,
    ) -> Result<(), FileRouteError> {
        let result =
            self.candidate
                .file_endpoint(pattern, declaration)
                .map_err(|error| match error {
                    crate::AppError::Route(error) => FileRouteError::at(source, error),
                    error => FileRouteError::at(source, error),
                });
        self.failed |= result.is_err();
        result
    }

    /// Checks a local 404 component and optionally sets the global unknown-URL fallback.
    pub fn not_found(
        &mut self,
        source: &str,
        declaration: FileNotFound,
        global: bool,
    ) -> Result<FileRouteNotFound, FileRouteError> {
        let result = (|| {
            self.candidate.validate_file_presentation(declaration.0, &[404])
                .map_err(|_| FileRouteError::at(source, "not-found presentation must accept Number(404), an empty seed and a read-only view"))?;
            if global {
                self.candidate
                    .client_not_found(Some(declaration.0))
                    .map_err(|error| FileRouteError::at(source, error))?;
            }
            Ok(FileRouteNotFound(declaration.0))
        })();
        self.failed |= result.is_err();
        result
    }

    /// Resolves local screens against file ancestors, without consulting the App fallback.
    /// Runs local presentation factories and read-only views, never page/layout loaders.
    pub fn boundary(
        &mut self,
        parent: Option<&FileRouteBoundary>,
        loading: Option<(&str, FileLoading)>,
        error: Option<(&str, FileError)>,
    ) -> Result<FileRouteBoundary, FileRouteError> {
        let result = (|| {
            let source = loading
                .as_ref()
                .map(|item| item.0)
                .or_else(|| error.as_ref().map(|item| item.0))
                .unwrap_or("<boundary>");
            let loading_id = loading
                .as_ref()
                .map(|item| item.1.0)
                .or(parent.map(|p| p.loading));
            let error_id = error
                .as_ref()
                .map(|item| item.1.0)
                .or(parent.map(|p| p.error));
            let (Some(loading_id), Some(error_id)) = (loading_id, error_id) else {
                return Err(FileRouteError::at(
                    source,
                    "loading.rs and error.rs must resolve a complete pair from file ancestors",
                ));
            };
            for (local, states) in [
                (
                    loading.map(|(path, component)| (path, component.0)),
                    &[0][..],
                ),
                (
                    error.map(|(path, component)| (path, component.0)),
                    &[0, 408, 400, 404, 500][..],
                ),
            ] {
                if let Some((path, component)) = local {
                    self.candidate.validate_file_presentation(component, states)
                        .map_err(|_| FileRouteError::at(path, "presentation must accept all required states, an empty seed and a read-only view"))?;
                }
            }
            Ok(FileRouteBoundary {
                loading: loading_id,
                error: error_id,
            })
        })();
        self.failed |= result.is_err();
        result
    }

    /// Adds a file layout under its nearest generated ancestor.
    pub fn layout(
        &mut self,
        source: &str,
        pattern: &str,
        parent: Option<&FileRouteLayout>,
        declaration: FileLayout,
    ) -> Result<FileRouteLayout, FileRouteError> {
        let result = self.add_layout(source, pattern, parent, declaration);
        self.failed |= result.is_err();
        result
    }
    fn add_layout(
        &mut self,
        source: &str,
        pattern: &str,
        parent: Option<&FileRouteLayout>,
        declaration: FileLayout,
    ) -> Result<FileRouteLayout, FileRouteError> {
        let client = matches!(declaration.kind, LayoutKind::Client(..));
        if !client && (declaration.boundary.is_some() || declaration.not_found.is_some()) {
            return Err(FileRouteError::at(
                source,
                "file presentations require a client layout",
            ));
        }
        if parent.is_some_and(|parent| parent.client != client) {
            return Err(FileRouteError::at(
                source,
                "native and client layout kinds cannot be mixed",
            ));
        }
        let route = PageRoute::new(&declaration.name, pattern)
            .map_err(|error| FileRouteError::at(source, error))?;
        let parent = parent.map(|parent| parent.name.as_str());
        match declaration.kind {
            LayoutKind::Server(render) => self.candidate.layout(
                &declaration.name,
                parent,
                move |context, title, children| render(context, title, children),
            ),
            LayoutKind::Client(component, state, load) => {
                self.candidate
                    .client_layout(route, parent, component, state, move |context| {
                        load(context)
                    })
            }
        }
        .map_err(|error| FileRouteError::at(source, error))?;
        if let Some(boundary) = declaration.boundary {
            self.candidate
                .client_layout_boundary(&declaration.name, Some(boundary.client()))
                .map_err(|error| FileRouteError::at(source, error))?;
        }
        if let Some(presentation) = declaration.not_found {
            self.candidate
                .client_layout_not_found(&declaration.name, Some(presentation.0))
                .map_err(|error| FileRouteError::at(source, error))?;
        }
        Ok(FileRouteLayout {
            name: declaration.name,
            client,
        })
    }

    /// Adds a page at its generated pattern and attaches its nearest file layout.
    pub fn page(
        &mut self,
        source: &str,
        pattern: &str,
        parent: Option<&FileRouteLayout>,
        declaration: FilePage,
    ) -> Result<(), FileRouteError> {
        let result = self.add_page(source, pattern, parent, declaration);
        self.failed |= result.is_err();
        result
    }
    fn add_page(
        &mut self,
        source: &str,
        pattern: &str,
        parent: Option<&FileRouteLayout>,
        declaration: FilePage,
    ) -> Result<(), FileRouteError> {
        let client = matches!(declaration.kind, PageKind::Client(..));
        if !client
            && (declaration.boundary.is_some()
                || declaration.not_found.is_some()
                || declaration.server_cache.max_age_seconds().is_some()
                || declaration.prerender.is_some())
        {
            return Err(FileRouteError::at(
                source,
                "file presentations and server cache policies require a client page",
            ));
        }
        if parent.is_some_and(|parent| parent.client != client) {
            return Err(FileRouteError::at(
                source,
                "page and inherited layout kinds do not match",
            ));
        }
        let directory_pattern;
        let pattern = if matches!(declaration.kind, PageKind::Server(PageMode::Static(_), _))
            && !pattern.ends_with('/')
        {
            directory_pattern = format!("{pattern}/");
            directory_pattern.as_str()
        } else {
            pattern
        };
        let route = PageRoute::new(&declaration.name, pattern)
            .map_err(|error| FileRouteError::at(source, error))?;
        let parent = parent.map(|parent| parent.name.as_str());
        match declaration.kind {
            PageKind::Server(mode, render) => {
                self.candidate
                    .page(route, parent, mode, move |context| render(context))
            }
            PageKind::Client(component, state, load) => {
                self.candidate
                    .client_page(route, component, state, move |context| load(context))
                    .map_err(|error| FileRouteError::at(source, error))?;
                self.candidate.client_page_layout(&declaration.name, parent)
            }
        }
        .map_err(|error| FileRouteError::at(source, error))?;
        if client {
            self.candidate
                .client_page_server_cache(&declaration.name, declaration.server_cache)
                .map_err(|error| FileRouteError::at(source, error))?;
        }
        if let Some(params) = declaration.prerender {
            self.candidate
                .client_page_prerender(&declaration.name, Some(params))
                .map_err(|error| FileRouteError::at(source, error))?;
        }
        if let Some(boundary) = declaration.boundary {
            self.candidate
                .client_page_boundary(&declaration.name, Some(boundary.client()))
                .map_err(|error| FileRouteError::at(source, error))?;
        }
        if let Some(presentation) = declaration.not_found {
            self.candidate
                .client_page_not_found(&declaration.name, Some(presentation.0))
                .map_err(|error| FileRouteError::at(source, error))?;
        }
        Ok(())
    }

    /// Validates and commits the complete registry without running loaders.
    pub fn finish(self) -> Result<(), FileRouteError> {
        if self.failed {
            return Err(FileRouteError::at(
                "<registry>",
                "registration failed; original application retained",
            ));
        }
        self.candidate
            .validate()
            .map_err(|error| FileRouteError::at("<registry>", error))?;
        *self.target = self.candidate;
        Ok(())
    }
}
