use super::*;
use crate::{
    ActionData, Component, ComponentError, ComponentProps, InitialState, NavigationMatch,
    NavigationPage, NavigationRoutes,
};
mod boundaries;
mod cache;
mod document;
mod layouts;
mod metadata;
mod not_found;
mod prerender;
mod progressive;
mod revalidation;
pub use boundaries::ClientBoundary;
pub use document::{ClientDocument, ClientDocumentHead};
pub(super) use layouts::ClientLayout;
pub use layouts::ClientLayoutData;

/// Application-owned component constructor shared by native and Wasm builds.
pub type ComponentFactory = fn(ComponentProps) -> Result<Box<dyn Component>, ComponentError>;

/// Parses the CLI's decimal build revision at compile time. This is not a secret.
pub const fn application_revision(value: &str) -> u64 {
    let bytes = value.as_bytes();
    assert!(
        !bytes.is_empty() && bytes.len() <= 20,
        "invalid application revision"
    );
    let mut result = 0u64;
    let mut index = 0;
    while index < bytes.len() {
        assert!(
            bytes[index] >= b'0' && bytes[index] <= b'9',
            "invalid application revision"
        );
        result = match result.checked_mul(10) {
            Some(value) => value,
            None => panic!("invalid application revision"),
        };
        result = match result.checked_add((bytes[index] - b'0') as u64) {
            Some(value) => value,
            None => panic!("invalid application revision"),
        };
        index += 1;
    }
    result
}

/// Public page seed and optional enabled native fallback returned by a GET loader.
pub struct ClientPageData {
    title: String,
    data: ActionData,
    fallback: Html,
    no_store: bool,
    metadata: Option<Metadata>,
}
impl ClientPageData {
    /// Creates a page seed. The component checks its schema before SSR and mounting.
    pub fn new(title: impl Into<String>, data: ActionData) -> Self {
        Self {
            title: title.into(),
            data,
            fallback: Html::default(),
            no_store: false,
            metadata: None,
        }
    }
    /// Replaces the page's complete public metadata in SSR and navigation.
    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Tightens a public route to no-store for this response.
    pub fn no_store(mut self) -> Self {
        self.no_store = true;
        self
    }
    /// Adds native forms or other server-rendered fallback content.
    pub fn fallback(mut self, html: impl Into<Html>) -> Self {
        self.fallback = html.into();
        self
    }
}

/// One immutable module revision and a persistent shell for an interactive App.
#[derive(Clone)]
pub struct ClientRuntime {
    wasm: &'static [u8],
    revision: u64,
    shell: fn(Html) -> Html,
    stylesheets: Vec<InternalUrl>,
    development: Option<crate::Development>,
}
impl ClientRuntime {
    /// Attaches a trusted, bounded Wasm module with the same exported revision.
    pub fn new(wasm: &'static [u8], revision: u64) -> Result<Self, AppError> {
        if wasm.len() > MAX_ASSET_BYTES || !wasm.starts_with(b"\0asm\x01\0\0\0") {
            return Err(AppError::InvalidPage);
        }
        Ok(Self {
            wasm,
            revision,
            shell: |children| children,
            stylesheets: Vec::new(),
            development: None,
        })
    }
    /// Sets a common shell. It must not depend on the current route or page title.
    /// Streamed documents require children exactly once in normal block flow.
    pub fn shell(mut self, shell: fn(Html) -> Html) -> Self {
        self.shell = shell;
        self
    }
    /// Enables local CLI diagnostics and guarded full-document reloads.
    /// Production builds must leave this unset.
    pub fn development(mut self, development: crate::Development) -> Self {
        self.development = Some(development);
        self
    }
    fn development_script(&self) -> String {
        self.development
            .as_ref()
            .map_or_else(String::new, |value| value.script(self.revision))
    }
    pub(super) fn route_count(&self) -> usize {
        4 + usize::from(self.development.is_some()) * 3
    }
    /// Adds a stylesheet shared by all interactive pages.
    pub fn stylesheet(mut self, url: InternalUrl) -> Result<Self, AppError> {
        if !self.stylesheets.contains(&url) {
            if self.stylesheets.len() == MAX_PAGE_STYLESHEETS {
                return Err(AppError::InvalidPage);
            }
            self.stylesheets.push(url);
        }
        Ok(self)
    }
    fn asset(&self, name: &str) -> String {
        format!("/ruvane-runtime/{:016x}/{name}", self.revision)
    }
    fn assets(&self) -> [(&str, &'static [u8], MediaType); 4] {
        [
            ("app.wasm", self.wasm, MediaType::WebAssembly),
            (
                "bridge.mjs",
                crate::BROWSER_BRIDGE_MODULE.as_bytes(),
                MediaType::JavaScript,
            ),
            (
                "navigation.mjs",
                crate::BROWSER_NAVIGATION_MODULE.as_bytes(),
                MediaType::JavaScript,
            ),
            (
                "application.mjs",
                include_bytes!("../../web/application.mjs"),
                MediaType::JavaScript,
            ),
        ]
    }
}

#[derive(Clone, Copy)]
pub(super) struct ClientRoute {
    component: u32,
    cache: crate::NavigationCachePolicy,
    state: fn(&NavigationMatch) -> Result<InitialState, PageError>,
    layout: Option<usize>,
    boundary: Option<ClientBoundary>,
    not_found: Option<u32>,
    progressive: bool,
    document: Option<ClientDocumentHead>,
}

impl App {
    /// Registers an application's factory and error component. No factory runs here.
    /// Error components accept Number(400/404) and an empty page seed.
    pub fn components(
        &mut self,
        factory: ComponentFactory,
        error_component: u32,
    ) -> Result<(), AppError> {
        if self.client_factory.is_some() || error_component == 0 {
            return Err(AppError::InvalidPage);
        }
        self.client_factory = Some(factory);
        self.client_error = error_component;
        Ok(())
    }
    /// Attaches versioned browser assets to the server. Client pages remain server-only.
    pub fn client_runtime(&mut self, runtime: ClientRuntime) -> Result<(), AppError> {
        if self.client_runtime.is_some() || self.client_factory.is_none() {
            return Err(AppError::InvalidPage);
        }
        let mut candidate = self.clone();
        candidate.client_runtime = Some(runtime);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Declares a typed page once for SSR and client navigation. The state resolver
    /// is shared read-only code; only GET requests execute the loader.
    pub fn client_page(
        &mut self,
        route: PageRoute,
        component: u32,
        state: fn(&NavigationMatch) -> Result<InitialState, PageError>,
        load: impl Fn(&PageContext<'_>) -> Result<ClientPageData, PageError> + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        if component == 0 || self.client_factory.is_none() {
            return Err(AppError::InvalidPage);
        }
        let mut candidate = self.clone();
        candidate.page(
            route,
            None,
            PageMode::ServerOnly(QueryMode::Uri),
            move |context| {
                let data = load(context)?;
                let mut page = Page::new(data.title, data.fallback);
                page.client_data = Some(data.data);
                page.client_no_store = data.no_store;
                page.client_metadata = data.metadata;
                Ok(page)
            },
        )?;
        candidate.pages.last_mut().unwrap().client = Some(ClientRoute {
            component,
            state,
            cache: crate::NavigationCachePolicy::no_store(),
            layout: None,
            boundary: None,
            not_found: None,
            progressive: false,
            document: None,
        });
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Opts an existing client page into public caching, or restores no-store.
    pub fn client_page_cache(
        &mut self,
        name: &str,
        policy: crate::NavigationCachePolicy,
    ) -> Result<(), AppError> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        let client = page.client.as_mut().ok_or(AppError::InvalidPage)?;
        client.cache = policy;
        Ok(())
    }
    /// Builds a marked link only for a client page explicitly allowed to cache.
    pub fn prefetch_link(
        &self,
        name: &str,
        params: &[(&str, &str)],
        label: &str,
    ) -> Result<Html, AppError> {
        let page = self
            .pages
            .iter()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        if page
            .client
            .is_none_or(|client| client.cache.max_age_seconds().is_none())
        {
            return Err(AppError::InvalidPage);
        }
        if self
            .layout_chain(page.client.unwrap().layout)
            .iter()
            .any(|index| {
                self.client_layouts[*index]
                    .cache
                    .max_age_seconds()
                    .is_none()
            })
        {
            return Err(AppError::InvalidPage);
        }
        Ok(crate::prefetch_link(&self.url(name, params)?, label))
    }
    /// Checks route ownership and props without executing loaders. The runtime then
    /// validates the complete seed with Component::page_data before preparing a view.
    pub fn validate_navigation(&self, page: &NavigationPage) -> bool {
        if self.client_factory.is_none()
            || page.metadata().is_some_and(|metadata| {
                !metadata.accepts_origin(self.client_metadata_origin.as_ref())
            })
        {
            return false;
        }
        if let Some(expected) = page.pending_layouts() {
            let path = page.url().as_str().split('?').next().unwrap();
            return page.status() == StatusCode::Ok
                && page.failed_segment().is_none()
                && page.data().is_empty()
                && self
                    .pages
                    .iter()
                    .find(|p| Some(p.route.name()) == self.owner(path))
                    .and_then(|p| p.client)
                    .is_some_and(|client| {
                        (client.progressive || client.document.is_some())
                            && self.layout_chain(client.layout).len() == expected
                    })
                && self
                    .client_props(page.url())
                    .is_ok_and(|props| props == page.props())
                && self.validate_layout_prefix(page, Some(page.layouts().len()));
        }
        if page.status() != StatusCode::Ok {
            if page.failed_segment().is_some() {
                return self.validate_boundary_page(page);
            }
            if page.status() == StatusCode::NotFound
                && self.scoped_not_found_packet_required(page.url())
            {
                return false;
            }
            return matches!(page.status(), StatusCode::BadRequest | StatusCode::NotFound)
                && page.layouts().is_empty()
                && page.data().is_empty()
                && page.props()
                    == ComponentProps::new(
                        self.failure_component(page.status()),
                        InitialState::Number(page.status().as_u16() as u32),
                    )
                    .unwrap();
        }
        if page.failed_segment().is_some() {
            return false;
        }
        self.client_props(page.url())
            .is_ok_and(|props| props == page.props())
            && self.validate_layouts(page)
    }
    fn routes(&self) -> Result<NavigationRoutes, AppError> {
        let mut routes = NavigationRoutes::new();
        for page in &self.pages {
            if let Some(client) = page.client {
                routes
                    .register(page.route.clone(), client.component)
                    .map_err(|_| AppError::RouteConflict)?;
            }
        }
        Ok(routes)
    }
    fn client_props(&self, url: &InternalUrl) -> Result<ComponentProps, PageError> {
        let path = url.as_str().split('?').next().unwrap();
        let matched = self
            .routes()
            .map_err(|_| PageError::InternalServerError)?
            .resolve(url)
            .map_err(|_| PageError::BadRequest)?
            .ok_or(PageError::NotFound)?;
        if self.owner(path) != Some(matched.name()) {
            return Err(PageError::NotFound);
        }
        let page = self
            .pages
            .iter()
            .find(|page| page.route.name() == matched.name())
            .unwrap();
        let client = page.client.unwrap();
        ComponentProps::new(client.component, (client.state)(&matched)?)
            .map_err(|_| PageError::BadRequest)
    }
    pub(super) fn client_asset_path(&self, path: &str) -> bool {
        self.client_runtime.as_ref().is_some_and(|runtime| {
            runtime
                .assets()
                .iter()
                .any(|(name, _, _)| runtime.asset(name) == path)
        })
    }
    pub(super) fn validate_client_routes(&self, router: &mut Router) -> Result<(), AppError> {
        if self.pages.len()
            + self.assets.len()
            + self
                .client_runtime
                .as_ref()
                .map_or(0, ClientRuntime::route_count)
            > MAX_APP_ROUTES
        {
            return Err(AppError::TooManyRoutes);
        }
        self.routes()?;
        if let Some(runtime) = &self.client_runtime {
            if let Some(development) = &runtime.development {
                for (url, _) in &development.styles {
                    if !self.assets.iter().any(|asset| {
                        asset.path.url().as_str() == url && asset.kind == MediaType::Css
                    }) {
                        return Err(AppError::InvalidPage);
                    }
                }
            }
            let total: usize = runtime
                .assets()
                .iter()
                .map(|(_, bytes, _)| bytes.len())
                .sum();
            if self.assets.len() + 4 > MAX_ASSETS
                || self
                    .assets
                    .iter()
                    .map(|asset| asset.bytes.len())
                    .sum::<usize>()
                    + total
                    > MAX_TOTAL_ASSET_BYTES
            {
                return Err(AppError::TooManyRoutes);
            }
            for (name, bytes, _) in runtime.assets() {
                if bytes.len() > MAX_ASSET_BYTES {
                    return Err(AppError::OutputTooLarge);
                }
                router.get(runtime.asset(name), |_| Response::text(""))?;
            }
            if let Some(development) = &runtime.development {
                development.mount(router)?;
            }
        }
        Ok(())
    }
    /// Plans only this application's embedded resources, without running page loaders.
    /// The CLI uses the resulting static manifest to verify a server bundle.
    pub fn client_assets(&self) -> Result<StaticSite, AppError> {
        self.validate()?;
        let runtime = self.client_runtime.as_ref().ok_or(AppError::InvalidPage)?;
        let mut site = StaticSite::new();
        for (name, bytes, kind) in runtime.assets() {
            site.asset(StaticPath::new(&runtime.asset(name))?, bytes, kind)?;
        }
        for asset in &self.assets {
            site.asset(asset.path.clone(), asset.bytes, asset.kind)?;
        }
        Ok(site)
    }
    /// Returns the bounded stylesheet declaration record used by the local CLI.
    /// It contains only public URLs and relative assets/ source names.
    pub fn development_styles(&self) -> Result<String, AppError> {
        self.validate()?;
        let runtime = self.client_runtime.as_ref().ok_or(AppError::InvalidPage)?;
        let mut record = format!("RVDS1\n{}\n", runtime.revision);
        if let Some(development) = &runtime.development {
            for (url, source) in &development.styles {
                record.push_str(&format!("{url}\t{source}\n"));
            }
        }
        Ok(record)
    }
    pub(super) fn mount_development_asset(
        &self,
        router: &mut Router,
        asset: &super::Asset,
    ) -> Result<bool, AppError> {
        let Some(runtime) = &self.client_runtime else {
            return Ok(false);
        };
        let Some(development) = &runtime.development else {
            return Ok(false);
        };
        let Some(index) = development
            .styles
            .iter()
            .position(|(url, _)| url == asset.path.url().as_str())
        else {
            return Ok(false);
        };
        let (development, revision, bytes) = (development.clone(), runtime.revision, asset.bytes);
        router.get(asset.path.url().as_str(), move |request| {
            development.css(request, revision, index, bytes)
        })?;
        Ok(true)
    }
    pub(super) fn mount_client_assets(&self, router: &mut Router) -> Result<(), AppError> {
        if let Some(runtime) = &self.client_runtime {
            router.application_revision(runtime.revision);
            for (name, bytes, kind) in runtime.assets() {
                router
                    .asset(runtime.asset(name), bytes, kind)
                    .map_err(|_| AppError::RouteConflict)?;
            }
            if let Some(development) = &runtime.development {
                development.mount(router)?;
            }
        }
        Ok(())
    }
    fn target(request: &Request) -> Result<InternalUrl, PageError> {
        let mut target = request.path().to_owned();
        if let Some(query) = request.query() {
            target.push('?');
            target.push_str(query);
        }
        InternalUrl::from_target(&target).map_err(|_| PageError::BadRequest)
    }
    pub(super) fn client_response(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
    ) -> Response {
        if let Some(response) = self.prerender_response(index, context, request) {
            return response;
        }
        if request.header("accept") == Some(crate::NAVIGATION_STREAM_MEDIA_TYPE)
            && self.pages[index].client.unwrap().progressive
        {
            return self.client_stream_response(index, context, request);
        }
        if !matches!(
            request.header("accept"),
            Some(crate::NAVIGATION_MEDIA_TYPE | crate::NAVIGATION_STREAM_MEDIA_TYPE)
        ) && request.query_params().get("native") != Some("yes")
            && self.pages[index].client.unwrap().document.is_some()
            && !self.has_not_found(index)
        {
            return self.client_document_response(index, context, request);
        }
        self.client_branch_response(index, context, request)
    }

    pub(super) fn client_missing(&self, request: &Request) -> Response {
        self.client_failure(request, PageError::NotFound)
    }
    fn client_failure(&self, request: &Request, error: PageError) -> Response {
        if error == PageError::InternalServerError {
            return Response::error(StatusCode::InternalServerError);
        }
        let Ok(url) = Self::target(request) else {
            return Response::error(StatusCode::BadRequest);
        };
        let status = error.status();
        let props = ComponentProps::new(
            self.failure_component(status),
            InitialState::Number(status.as_u16() as u32),
        )
        .unwrap();
        let packet = NavigationPage::new(
            url,
            if status == StatusCode::NotFound {
                "Page not found"
            } else {
                "Invalid page request"
            },
            props,
            ActionData::default(),
            status,
        )
        .and_then(|page| page.with_metadata(Metadata::new()))
        .unwrap();
        self.client_finish(
            &packet,
            Html::default(),
            request,
            crate::NavigationCachePolicy::no_store(),
            None,
        )
        .unwrap_or_else(|_| Response::error(StatusCode::InternalServerError))
    }
    fn client_view(
        factory: ComponentFactory,
        props: ComponentProps,
        data: &ActionData,
        read_only: bool,
    ) -> Result<Html, PageError> {
        Self::client_view_inputs(factory, props, data, read_only, false)
    }
    fn client_view_inputs(
        factory: ComponentFactory,
        props: ComponentProps,
        data: &ActionData,
        read_only: bool,
        editable: bool,
    ) -> Result<Html, PageError> {
        let mut component = factory(props).map_err(|_| PageError::InternalServerError)?;
        component
            .page_data(data.clone())
            .map_err(|_| PageError::InternalServerError)?;
        let view = component
            .view()
            .map_err(|_| PageError::InternalServerError)?;
        if read_only && view.event_count() != 0 {
            return Err(PageError::InternalServerError);
        }
        view.snapshot()
            .map_err(|_| PageError::InternalServerError)?;
        if component.take_action().is_some() {
            return Err(PageError::InternalServerError);
        }
        (if editable {
            view.hydratable_page(props)
        } else {
            view.hydratable(props)
        })
        .map_err(|_| PageError::InternalServerError)
    }
    fn client_finish(
        &self,
        packet: &NavigationPage,
        fallback: Html,
        request: &Request,
        policy: crate::NavigationCachePolicy,
        head: Option<ClientDocument>,
    ) -> Result<Response, PageError> {
        let factory = self.client_factory.ok_or(PageError::InternalServerError)?;
        let native = request.query_params().get("native") == Some("yes");
        let render = |props, data: &ActionData| {
            Self::client_view_inputs(factory, props, data, false, !native)
        };
        let page_view = Self::client_view_inputs(
            factory,
            packet.props(),
            packet.data(),
            packet.status() != StatusCode::Ok,
            !native,
        )?;
        let mut page_html = Html::rendered(format!(
            "<div data-ruvane-page=\"\" tabindex=\"-1\">{}</div>",
            page_view.as_str()
        ));
        for layout in packet.layouts().iter().rev() {
            let own = render(layout.props(), layout.data())?;
            page_html = Self::layout_html(layout.name(), own, page_html);
            if page_html.as_str().len() > MAX_STATIC_FILE_BYTES {
                return Err(PageError::InternalServerError);
            }
        }
        if matches!(
            request.header("accept"),
            Some(crate::NAVIGATION_MEDIA_TYPE | crate::NAVIGATION_STREAM_MEDIA_TYPE)
        ) {
            return Ok(Response::navigation_with_cache(packet, policy));
        }
        let runtime = self
            .client_runtime
            .as_ref()
            .ok_or(PageError::InternalServerError)?;
        let mut body = String::from("<div id=\"ruvane-application\" data-ruvane-module=\"");
        body.push_str(&runtime.asset("app.wasm"));
        body.push_str(&format!(
            "\" data-ruvane-revision=\"{}\">",
            runtime.revision
        ));
        let content = crate::fragment([
            crate::element(crate::Tag::P)
                .attr(crate::Attribute::Id, "ruvane-status")
                .child("Native links and forms are ready.")
                .render(),
            crate::element(crate::Tag::Div)
                .attr(crate::Attribute::Id, "ruvane-outlet")
                .child(page_html)
                .child(
                    crate::element(crate::Tag::Div)
                        .attr(crate::Attribute::Id, "ruvane-fallback")
                        .child(fallback),
                )
                .render(),
        ]);
        let wrapped = (runtime.shell)(content);
        if wrapped.as_str().len() > MAX_STATIC_FILE_BYTES {
            return Err(PageError::InternalServerError);
        }
        body.push_str(wrapped.as_str());
        body.push_str(self.boundary_manifest().as_str());
        if !native {
            let snapshot = crate::NavigationSnapshot::new(runtime.revision, packet.clone())
                .map_err(|_| PageError::InternalServerError)?;
            body.push_str(snapshot.html().as_str());
        }
        body.push_str("</div>");
        if !native {
            body.push_str(
                crate::module_script(&InternalUrl::new(&runtime.asset("application.mjs")).unwrap())
                    .as_str(),
            );
            body.push_str(&runtime.development_script());
        }
        let mut document = match head {
            Some(head) => head.buffered(packet.title(), Html::rendered(body))?,
            None => Document::new(packet.title(), Html::rendered(body)),
        };
        if let Some(metadata) = packet.metadata() {
            document = document.metadata(metadata.clone());
        }
        if packet.status() != StatusCode::Ok {
            document =
                document.metadata(packet.metadata().cloned().unwrap_or_default().for_error());
        }
        for stylesheet in &runtime.stylesheets {
            document = document.stylesheet(stylesheet.clone());
        }
        let html = document.render();
        if html.as_str().len() > MAX_STATIC_FILE_BYTES {
            return Err(PageError::InternalServerError);
        }
        Ok(if native {
            Response::html(html)
        } else {
            Response::interactive_html(html)
        }
        .with_status(packet.status())
        .navigation_variance())
    }
}
