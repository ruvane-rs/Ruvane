mod client;
use crate::page_route::valid_name;
use crate::{
    Document, ExportError, Html, InternalUrl, MAX_ASSET_BYTES, MAX_ASSETS, MAX_METADATA_TEXT_BYTES,
    MAX_STATIC_ENTRIES, MAX_STATIC_FILE_BYTES, MAX_TOTAL_ASSET_BYTES, MediaType, Metadata,
    MetadataError, PageParams, PageRoute, ParamError, QueryMode, Request, Response, Robots,
    RouteError, Router, SiteOrigin, StaticPath, StaticSite, StatusCode, UrlError,
};
pub use client::{
    ClientBoundary, ClientDocument, ClientDocumentHead, ClientLayoutData, ClientPageData,
    ClientRuntime, ComponentFactory, application_revision,
};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;
use std::sync::Arc;

/// Maximum combined page and asset routes in one application.
pub const MAX_APP_ROUTES: usize = 128;
/// Maximum named layouts in one application.
pub const MAX_APP_LAYOUTS: usize = 32;
/// Maximum layouts applied to one page, including its root layout.
pub const MAX_LAYOUT_DEPTH: usize = 8;
/// Maximum distinct stylesheet links on an application page.
pub const MAX_PAGE_STYLESHEETS: usize = 16;

/// An application declaration, link or render error, without reflected input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AppError {
    /// A page or layout name is invalid.
    InvalidName,
    /// A page or layout name already exists in its namespace.
    DuplicateName,
    /// A requested page name is not registered.
    UnknownRoute,
    /// A layout or its parent has not yet been registered.
    UnknownLayout,
    /// The bounded layout chain would be too deep.
    LayoutDepth,
    /// The application route budget is exhausted.
    TooManyRoutes,
    /// The named layout budget is exhausted.
    TooManyLayouts,
    /// The total number of concrete export entries exceeds the static limit.
    TooManyStaticPages,
    /// Parameters are missing, duplicated, extra, empty or oversized.
    InvalidParams,
    /// A generated URL is shadowed by another page/asset or duplicated in export.
    RouteConflict,
    /// A title, language or stylesheet list is outside the page profile.
    InvalidPage,
    /// A body, layout intermediate or final document exceeds 1 MiB.
    OutputTooLarge,
    /// A page/layout panicked or an exported page returned a page error.
    RenderFailed,
    /// A low-level route registration was rejected.
    Route(RouteError),
    /// URL validation failed.
    Url(UrlError),
    /// Static planning failed.
    Export(ExportError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidName => "invalid application name",
            Self::DuplicateName => "duplicate application name",
            Self::UnknownRoute => "unknown named page",
            Self::UnknownLayout => "unknown layout or parent",
            Self::LayoutDepth => "layout depth limit exceeded",
            Self::TooManyRoutes => "application route limit exceeded",
            Self::TooManyLayouts => "application layout limit exceeded",
            Self::TooManyStaticPages => "application static entry limit exceeded",
            Self::InvalidParams => "invalid page parameters",
            Self::RouteConflict => "conflicting concrete page URL",
            Self::InvalidPage => "invalid page properties",
            Self::OutputTooLarge => "page output limit exceeded",
            Self::RenderFailed => "page rendering failed",
            Self::Route(_) => "invalid application route",
            Self::Url(_) => "invalid application URL",
            Self::Export(_) => "application static planning failed",
        })
    }
}
impl std::error::Error for AppError {}
impl From<RouteError> for AppError {
    fn from(e: RouteError) -> Self {
        Self::Route(e)
    }
}
impl From<UrlError> for AppError {
    fn from(e: UrlError) -> Self {
        Self::Url(e)
    }
}
impl From<ExportError> for AppError {
    fn from(e: ExportError) -> Self {
        Self::Export(e)
    }
}

/// A page error mapped to an HTTP status. Keep details in local diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageError {
    /// The page's application-level input is invalid (400).
    BadRequest,
    /// The requested content does not exist (404).
    NotFound,
    /// Rendering or a trusted data operation failed (500).
    InternalServerError,
}
impl PageError {
    fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BadRequest,
            Self::NotFound => StatusCode::NotFound,
            Self::InternalServerError => StatusCode::InternalServerError,
        }
    }
}
impl fmt::Display for PageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.status().fmt(f)
    }
}
impl std::error::Error for PageError {}

/// Explicit server/static policy for a page registration.
#[derive(Clone, Debug)]
pub enum PageMode {
    /// Export exactly these concrete parameter sets. The renderer receives no
    /// request even when serving this page; valid query strings do not affect it.
    /// At least one set is required; fixed pages require one empty set.
    Static(Vec<PageParams>),
    /// Serve only, with an explicitly selected query convention. Listed by
    /// `App::server_only_routes` and deliberately omitted from static builds.
    ServerOnly(QueryMode),
}
impl PageMode {
    /// Declares one fixed page for both server rendering and static export.
    pub fn fixed() -> Self {
        Self::Static(vec![PageParams::default()])
    }
}

/// A page body and document properties, before layouts are applied.
///
/// Layouts return HTML fragments. The application emits the outer document
/// once. The renderer does not enforce all HTML content-model rules: callers
/// must not pass already complete documents as bodies or layout fragments.
#[derive(Clone, Debug)]
#[must_use]
pub struct Page {
    title: String,
    body: Html,
    metadata: Metadata,
    language: String,
    stylesheets: Vec<InternalUrl>,
    bad_request: bool,
    client_data: Option<crate::ActionData>,
    client_no_store: bool,
}
impl Page {
    /// Creates a page. Titles are limited to 1024 control-free UTF-8 bytes at
    /// rendering. Default metadata requests no indexing and has no canonical.
    pub fn new(title: impl Into<String>, body: impl Into<Html>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            metadata: Metadata::new().robots(Robots::NoIndexFollow),
            language: "en".into(),
            stylesheets: Vec::new(),
            bad_request: false,
            client_data: None,
            client_no_store: false,
        }
    }
    /// Marks a server-only validation page as HTTP 400 while retaining its
    /// escaped body (for example, a form with field errors). Canonical metadata
    /// is removed and noindex is enforced. Static planning rejects this result.
    pub fn bad_request(mut self) -> Self {
        self.bad_request = true;
        self
    }

    /// Replaces the page metadata. `PageContext::metadata` supplies public
    /// canonical metadata from a configured origin, or noindex in development.
    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }
    /// Sets the escaped language tag (at most 64 control-free bytes).
    pub fn language(mut self, value: impl Into<String>) -> Self {
        self.language = value.into();
        self
    }
    /// Adds a deduplicated typed stylesheet URL. At most 16 are accepted when
    /// rendering; stylesheets remain trusted application CSS.
    pub fn stylesheet(mut self, value: InternalUrl) -> Self {
        if !self.stylesheets.contains(&value) {
            self.stylesheets.push(value);
        }
        self
    }
}

/// Validated inputs shared by a page and its layouts.
/// Static-capable pages never receive a request, including during server use.
/// No origin is inferred from Host or forwarding headers.
pub struct PageContext<'a> {
    app: &'a App,
    route: Option<&'a PageRoute>,
    path: &'a InternalUrl,
    params: &'a [(String, String)],
    request: Option<&'a Request>,
    origin: Option<&'a SiteOrigin>,
    exporting: bool,
    stream: Option<&'a crate::StreamContext>,
}
impl PageContext<'_> {
    /// Cooperative cancellation/deadline context during progressive GET loading.
    pub fn stream_context(&self) -> Option<&crate::StreamContext> {
        self.stream
    }
    /// Returns the selected registered page name; error layouts have none.
    pub fn route_name(&self) -> Option<&str> {
        self.route.map(PageRoute::name)
    }
    /// Returns the constructed path, with captures encoded once and no query.
    pub fn path(&self) -> &InternalUrl {
        self.path
    }
    /// Returns a decoded capture.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    /// Converts a decoded capture through `FromStr`, without exposing parse errors.
    pub fn param_as<T: FromStr>(&self, name: &str) -> Result<T, ParamError> {
        crate::url::parse_value(self.param(name))
    }
    /// Returns a request only for `ServerOnly` pages. It is absent for static
    /// pages and cached error layouts.
    pub fn request(&self) -> Option<&Request> {
        self.request
    }
    /// Reports a static planning invocation; this does not authorize side effects.
    pub fn is_exporting(&self) -> bool {
        self.exporting
    }
    /// Borrows the explicitly configured public origin, if any.
    pub fn origin(&self) -> Option<&SiteOrigin> {
        self.origin
    }
    /// Constructs a URL and checks that the named page owns its concrete path.
    pub fn url(&self, name: &str, params: &[(&str, &str)]) -> Result<InternalUrl, AppError> {
        self.app.url(name, params)
    }
    /// Builds escaped metadata for this path from the trusted public origin.
    /// Without an origin it requests no indexing and omits the canonical URL.
    pub fn metadata(&self, description: &str) -> Result<Metadata, MetadataError> {
        let metadata = Metadata::new().description(description)?;
        Ok(match self.origin {
            Some(origin) => metadata
                .canonical(origin.canonical(self.path))
                .robots(Robots::IndexFollow),
            None => metadata.robots(Robots::NoIndexFollow),
        })
    }
}

type Render = Arc<dyn Fn(&PageContext<'_>) -> Result<Page, PageError> + Send + Sync>;
type Wrap = Arc<dyn Fn(&PageContext<'_>, &str, Html) -> Html + Send + Sync>;
type ErrorRender = Arc<dyn Fn() -> Page + Send + Sync>;
#[derive(Clone)]
struct PageEntry {
    route: PageRoute,
    layout: Option<usize>,
    mode: PageMode,
    render: Render,
    client: Option<client::ClientRoute>,
}
#[derive(Clone)]
struct Layout {
    name: String,
    parent: Option<usize>,
    depth: usize,
    wrap: Wrap,
}
#[derive(Clone)]
struct Asset {
    path: StaticPath,
    bytes: &'static [u8],
    kind: MediaType,
}
#[derive(Clone)]
struct ErrorPage {
    layout: Option<usize>,
    render: ErrorRender,
}

/// One bounded declaration of pages, layouts and embedded assets for both
/// the GET/HEAD server and static builds. Registration executes no callbacks.
///
/// Names are explicit; no filesystem route discovery, macros, script engine,
/// wire deserialization or remote function dispatch is added. Handlers and
/// layouts are trusted Rust code and must be bounded and read-only. Output
/// checks run after caller allocations and cannot sandbox CPU or memory use.
#[derive(Clone, Default)]
pub struct App {
    pages: Vec<PageEntry>,
    layouts: Vec<Layout>,
    assets: Vec<Asset>,
    not_found: Option<ErrorPage>,
    internal_error: Option<ErrorPage>,
    client_factory: Option<ComponentFactory>,
    client_error: u32,
    client_boundary: Option<ClientBoundary>,
    client_runtime: Option<ClientRuntime>,
    client_layouts: Vec<client::ClientLayout>,
}
impl App {
    /// Creates an empty application.
    pub fn new() -> Self {
        Self::default()
    }

    fn layout_index(&self, name: Option<&str>) -> Result<Option<usize>, AppError> {
        name.map(|name| {
            self.layouts
                .iter()
                .position(|l| l.name == name)
                .ok_or(AppError::UnknownLayout)
        })
        .transpose()
    }

    /// Registers a fragment wrapper and an optional already registered parent.
    /// Parents are immutable backward references, preventing cycles. Wrapping
    /// proceeds leaf first, then parents, for at most eight levels.
    pub fn layout(
        &mut self,
        name: &str,
        parent: Option<&str>,
        wrap: impl Fn(&PageContext<'_>, &str, Html) -> Html + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        if !valid_name(name) {
            return Err(AppError::InvalidName);
        }
        if self.layouts.iter().any(|l| l.name == name) {
            return Err(AppError::DuplicateName);
        }
        if self.layouts.len() + self.client_layouts.len() == MAX_APP_LAYOUTS {
            return Err(AppError::TooManyLayouts);
        }
        let parent = self.layout_index(parent)?;
        let depth = parent.map_or(1, |i| self.layouts[i].depth + 1);
        if depth > MAX_LAYOUT_DEPTH {
            return Err(AppError::LayoutDepth);
        }
        self.layouts.push(Layout {
            name: name.into(),
            parent,
            depth,
            wrap: Arc::new(wrap),
        });
        Ok(())
    }

    /// Registers a named page, its optional leaf layout and explicit export
    /// policy. Static parameters and all concrete URLs are checked immediately.
    /// Invalid registration leaves the application unchanged.
    pub fn page(
        &mut self,
        route: PageRoute,
        layout: Option<&str>,
        mode: PageMode,
        render: impl Fn(&PageContext<'_>) -> Result<Page, PageError> + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        if self.pages.iter().any(|p| p.route.name() == route.name()) {
            return Err(AppError::DuplicateName);
        }
        self.check_capacity()?;
        let layout = self.layout_index(layout)?;
        if let PageMode::Static(params) = &mode {
            if params.is_empty() || params.len() > MAX_STATIC_ENTRIES {
                return Err(AppError::InvalidParams);
            }
        }
        let entry = PageEntry {
            route,
            layout,
            mode,
            render: Arc::new(render),
            client: None,
        };
        let mut candidate = self.clone();
        candidate.pages.push(entry);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Registers a content collection on a page pattern with exactly one
    /// `{slug}` capture. Its portable slugs enumerate the static pages; the
    /// same immutable entries render on the server. Unknown slugs return 404.
    pub fn content_pages(
        &mut self,
        route: PageRoute,
        layout: Option<&str>,
        collection: crate::ContentCollection,
        render: impl Fn(&PageContext<'_>, &crate::ContentEntry) -> Result<Page, PageError>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), AppError> {
        let mode = collection.page_mode();
        self.page(route, layout, mode, move |context| {
            let entry = collection
                .get(context.param("slug").ok_or(PageError::NotFound)?)
                .ok_or(PageError::NotFound)?;
            render(context, entry)
        })
    }

    /// Registers one trusted embedded asset in the common route namespace.
    /// The portable static path, MIME suffix and existing asset byte/count
    /// budgets are enforced for both server and export usage.
    pub fn asset(
        &mut self,
        path: &str,
        bytes: &'static [u8],
        kind: MediaType,
    ) -> Result<(), AppError> {
        self.check_capacity()?;
        let path = StaticPath::new(path)?;
        if path.is_page() {
            return Err(ExportError::WrongPathKind.into());
        }
        if !crate::static_site::valid_suffix(path.output_path(), kind) {
            return Err(ExportError::UnsupportedAsset.into());
        }
        if self.assets.len() == MAX_ASSETS {
            return Err(AppError::TooManyRoutes);
        }
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(ExportError::FileTooLarge.into());
        }
        if self.assets.iter().map(|a| a.bytes.len()).sum::<usize>() + bytes.len()
            > MAX_TOTAL_ASSET_BYTES
        {
            return Err(ExportError::TotalTooLarge.into());
        }
        let mut candidate = self.clone();
        candidate.assets.push(Asset { path, bytes, kind });
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    fn check_capacity(&self) -> Result<(), AppError> {
        if self.pages.len() + self.assets.len() + usize::from(self.client_runtime.is_some()) * 4
            >= MAX_APP_ROUTES
        {
            Err(AppError::TooManyRoutes)
        } else {
            Ok(())
        }
    }

    /// Sets or replaces the cached 404 page and its layout. Rendered once when
    /// constructing a router, with no request or public canonical metadata.
    pub fn not_found(
        &mut self,
        layout: Option<&str>,
        render: impl Fn() -> Page + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        self.not_found = Some(ErrorPage {
            layout: self.layout_index(layout)?,
            render: Arc::new(render),
        });
        Ok(())
    }
    /// Sets or replaces the cached 500 page. Panics and output overflows use
    /// this response when it fits the server budget; an invalid error renderer
    /// refuses router construction. Parser errors retain their fixed responses.
    pub fn internal_error(
        &mut self,
        layout: Option<&str>,
        render: impl Fn() -> Page + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        self.internal_error = Some(ErrorPage {
            layout: self.layout_index(layout)?,
            render: Arc::new(render),
        });
        Ok(())
    }

    /// Constructs a named URL and rejects paths shadowed by another registered
    /// page or asset. This is a runtime name/parameter check, not a macro-based
    /// compile-time proof. Server-only pages may also be linked explicitly.
    pub fn url(&self, name: &str, params: &[(&str, &str)]) -> Result<InternalUrl, AppError> {
        let page = self
            .pages
            .iter()
            .find(|p| p.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        let url = page.route.url(params)?;
        if self.owner(url.as_str()) != Some(name) {
            return Err(AppError::RouteConflict);
        }
        Ok(url)
    }

    fn owner(&self, path: &str) -> Option<&str> {
        if self.client_asset_path(path) {
            return None;
        }
        if self.assets.iter().any(|a| a.path.url().as_str() == path) {
            return None;
        }
        self.pages
            .iter()
            .find(|p| !p.route.dynamic && p.route.pattern() == path)
            .or_else(|| {
                self.pages
                    .iter()
                    .find(|p| p.route.dynamic && p.route.matches(path))
            })
            .map(|p| p.route.name())
    }

    fn validate(&self) -> Result<(), AppError> {
        let mut router = Router::new();
        for page in &self.pages {
            router.get(page.route.pattern(), |_| Response::text(""))?;
        }
        for asset in &self.assets {
            router.get(asset.path.url().as_str(), |_| Response::text(""))?;
        }
        self.validate_client_routes(&mut router)?;
        let mut paths = Vec::new();
        let mut outputs: Vec<String> = Vec::new();
        for page in &self.pages {
            if let PageMode::Static(params) = &page.mode {
                for values in params {
                    if paths.len() + self.assets.len() == MAX_STATIC_ENTRIES {
                        return Err(AppError::TooManyStaticPages);
                    }
                    let url = self.url(page.route.name(), &values.pairs())?;
                    let path = StaticPath::new(url.as_str())?;
                    if !path.is_page() {
                        return Err(ExportError::WrongPathKind.into());
                    }
                    if paths.contains(&url) {
                        return Err(AppError::RouteConflict);
                    }
                    outputs.push(path.output_path().into());
                    paths.push(url);
                }
            }
        }
        for asset in &self.assets {
            outputs.push(asset.path.output_path().into());
        }
        for (i, a) in outputs.iter().enumerate() {
            for b in &outputs[..i] {
                if a == b
                    || a.strip_prefix(b).is_some_and(|s| s.starts_with('/'))
                    || b.strip_prefix(a).is_some_and(|s| s.starts_with('/'))
                {
                    return Err(ExportError::PathConflict.into());
                }
            }
        }
        Ok(())
    }

    /// Lists deliberately unexported page names in declaration order. Generated
    /// applications print this list during a static build.
    pub fn server_only_routes(&self) -> impl Iterator<Item = &str> {
        self.pages
            .iter()
            .filter(|p| matches!(p.mode, PageMode::ServerOnly(_)))
            .map(|p| p.route.name())
    }

    /// Builds a server router from a snapshot of this declaration. Later changes
    /// to the builder do not alter an already constructed router. A trusted
    /// public origin is optional; never derive it from request headers.
    pub fn router(&self, origin: Option<SiteOrigin>) -> Result<Router, AppError> {
        self.validate()?;
        if self.pages.iter().any(|p| p.client.is_some()) && self.client_runtime.is_none() {
            return Err(AppError::InvalidPage);
        }
        let not_found = self.error_response(self.not_found.as_ref(), StatusCode::NotFound)?;
        let internal = self.error_response(
            self.internal_error.as_ref(),
            StatusCode::InternalServerError,
        )?;
        let app = Arc::new(self.clone());
        let mut router = Router::new();
        router.page_errors(not_found.clone(), internal.clone());
        for (index, page) in self.pages.iter().enumerate() {
            let app = Arc::clone(&app);
            let origin = origin.clone();
            let missing = not_found.clone();
            let failed = internal.clone();
            let handler = move |request: &Request| {
                let page = &app.pages[index];
                let path = match app.url(
                    page.route.name(),
                    &request
                        .params
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.as_str()))
                        .collect::<Vec<_>>(),
                ) {
                    Ok(path) => path,
                    Err(_) => {
                        return missing
                            .clone()
                            .unwrap_or_else(|| Response::error(StatusCode::NotFound));
                    }
                };
                let context = PageContext {
                    app: &app,
                    route: Some(&page.route),
                    path: &path,
                    params: &request.params,
                    request: matches!(page.mode, PageMode::ServerOnly(_)).then_some(request),
                    origin: origin.as_ref(),
                    exporting: false,
                    stream: None,
                };
                if page.client.is_some() {
                    return app.client_response(index, &context, request);
                }
                match (page.render)(&context).and_then(|rendered| {
                    let status = if rendered.bad_request {
                        StatusCode::BadRequest
                    } else {
                        StatusCode::Ok
                    };
                    app.finish(rendered, page.layout, &context)
                        .map(|html| Response::html(html).with_status(status))
                        .map_err(|_| PageError::InternalServerError)
                }) {
                    Ok(response) => response,
                    Err(PageError::NotFound) => missing
                        .clone()
                        .unwrap_or_else(|| Response::error(StatusCode::NotFound)),
                    Err(PageError::InternalServerError) => failed
                        .clone()
                        .unwrap_or_else(|| Response::error(StatusCode::InternalServerError)),
                    Err(error) => Response::error(error.status()),
                }
            };
            match page.mode {
                PageMode::ServerOnly(QueryMode::Form) => {
                    router.get_form(page.route.pattern(), handler)?
                }
                _ => router.get(page.route.pattern(), handler)?,
            }
        }
        for asset in &self.assets {
            router
                .asset(asset.path.url().as_str(), asset.bytes, asset.kind)
                .map_err(|_| AppError::RouteConflict)?;
        }
        self.mount_client_assets(&mut router)?;
        if self.client_runtime.is_some() {
            let app = Arc::clone(&app);
            router.client_not_found(move |request| app.client_missing(request));
        }
        Ok(router)
    }

    fn error_response(
        &self,
        error: Option<&ErrorPage>,
        status: StatusCode,
    ) -> Result<Option<Response>, AppError> {
        error
            .map(|error| {
                catch_unwind(AssertUnwindSafe(|| {
                    let path = InternalUrl::new("/").expect("fixed root");
                    let context = PageContext {
                        app: self,
                        route: None,
                        path: &path,
                        params: &[],
                        request: None,
                        origin: None,
                        exporting: false,
                        stream: None,
                    };
                    let mut page = (error.render)();
                    page.metadata = page.metadata.for_error();
                    let html = self.finish(page, error.layout, &context)?;
                    Ok(Response::shared(
                        Arc::from(html.as_str().as_bytes()),
                        "text/html; charset=utf-8",
                    )
                    .with_status(status))
                }))
                .map_err(|_| AppError::RenderFailed)?
            })
            .transpose()
    }

    fn finish(
        &self,
        mut page: Page,
        mut layout: Option<usize>,
        context: &PageContext<'_>,
    ) -> Result<Html, AppError> {
        if page.bad_request {
            page.metadata = page.metadata.for_error();
        }
        if page.title.len() > MAX_METADATA_TEXT_BYTES
            || page.title.chars().any(char::is_control)
            || page.language.is_empty()
            || page.language.len() > 64
            || page.language.chars().any(char::is_control)
            || page.stylesheets.len() > MAX_PAGE_STYLESHEETS
        {
            return Err(AppError::InvalidPage);
        }
        if page.body.as_str().len() > MAX_STATIC_FILE_BYTES {
            return Err(AppError::OutputTooLarge);
        }
        while let Some(index) = layout {
            let wrapper = &self.layouts[index];
            page.body = (wrapper.wrap)(context, &page.title, page.body);
            if page.body.as_str().len() > MAX_STATIC_FILE_BYTES {
                return Err(AppError::OutputTooLarge);
            }
            layout = wrapper.parent;
        }
        let mut document = Document::new(page.title, page.body)
            .language(page.language)
            .metadata(page.metadata);
        for url in page.stylesheets {
            document = document.stylesheet(url);
        }
        let html = document.render();
        if html.as_str().len() > MAX_STATIC_FILE_BYTES {
            return Err(AppError::OutputTooLarge);
        }
        Ok(html)
    }

    /// Renders the explicitly enumerated static pages and common assets using
    /// the same callbacks and layouts as the server. Server-only pages are
    /// omitted. Any page error, panic, conflict or byte-budget violation fails
    /// the complete plan before any export destination is written.
    pub fn static_site(&self, origin: &SiteOrigin) -> Result<StaticSite, AppError> {
        self.validate()?;
        catch_unwind(AssertUnwindSafe(|| {
            let mut site = StaticSite::new();
            for page in &self.pages {
                if let PageMode::Static(params) = &page.mode {
                    for values in params {
                        let path = self.url(page.route.name(), &values.pairs())?;
                        let context = PageContext {
                            app: self,
                            route: Some(&page.route),
                            path: &path,
                            params: &values.0,
                            request: None,
                            origin: Some(origin),
                            exporting: true,
                            stream: None,
                        };
                        let rendered =
                            (page.render)(&context).map_err(|_| AppError::RenderFailed)?;
                        if rendered.bad_request {
                            return Err(AppError::RenderFailed);
                        }
                        let html = self.finish(rendered, page.layout, &context)?;
                        site.page(StaticPath::new(path.as_str())?, html)?;
                    }
                }
            }
            for asset in &self.assets {
                site.asset(asset.path.clone(), asset.bytes, asset.kind)?;
            }
            if site.is_empty() {
                return Err(ExportError::EmptySite.into());
            }
            Ok(site)
        }))
        .map_err(|_| AppError::RenderFailed)?
    }
}
