use super::FileRouteError;
use crate::{
    App, ClientLayoutData, ClientPageData, Html, InitialState, NavigationMatch, Page, PageContext,
    PageError, PageMode, PageRoute,
};
use std::sync::Arc;

type State = fn(&NavigationMatch) -> Result<InitialState, PageError>;
type PageLoader = Arc<dyn Fn(&PageContext<'_>) -> Result<Page, PageError> + Send + Sync>;
type ClientLoader =
    Arc<dyn Fn(&PageContext<'_>) -> Result<ClientPageData, PageError> + Send + Sync>;
type LayoutRenderer = Arc<dyn Fn(&PageContext<'_>, &str, Html) -> Html + Send + Sync>;
type LayoutLoader =
    Arc<dyn Fn(&PageContext<'_>) -> Result<ClientLayoutData, PageError> + Send + Sync>;

/// A page declaration returned by a route file's `pub fn page()`.
pub struct FilePage {
    name: String,
    kind: PageKind,
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
        }
    }
}

/// A layout declaration returned by a route file's `pub fn layout()`.
pub struct FileLayout {
    name: String,
    kind: LayoutKind,
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
        }
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
        .map_err(|error| FileRouteError::at(source, error))
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
