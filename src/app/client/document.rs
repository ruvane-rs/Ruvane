use super::*;
use crate::streaming::{BodyStream, ByteProducer};
use crate::{
    LayoutSnapshot, NavigationLayout, NavigationSnapshot, StreamContext, StreamError, StreamLimits,
};

/// Resolves a document head and request errors before any HTML is sent.
pub type ClientDocumentHead = fn(&PageContext<'_>) -> Result<ClientDocument, PageError>;

/// Head and budgets known before an initial App document starts streaming.
/// The page loader must return the same title. A leaf boundary can recover late errors.
pub struct ClientDocument {
    title: String,
    language: String,
    metadata: Metadata,
    limits: StreamLimits,
    hydrate_layouts: bool,
    hydrate_shell: bool,
}
impl ClientDocument {
    /// Creates a fixed head with English language and the default stream budgets.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            language: "en".into(),
            metadata: Metadata::new(),
            limits: StreamLimits::default(),
            hydrate_layouts: false,
            hydrate_shell: false,
        }
    }
    /// Sets the escaped document language.
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }
    /// Sets typed metadata before the headers are committed.
    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }
    /// Activates each validated layout while the initial document is still loading.
    /// Server actions stay blocked until the complete page branch commits.
    pub fn hydrate_layouts(mut self, enabled: bool) -> Self {
        self.hydrate_layouts = enabled;
        self
    }
    /// Activates marked independent shell views before the page completes.
    /// Server actions stay blocked until the complete document commits.
    pub fn hydrate_shell(mut self, enabled: bool) -> Self {
        self.hydrate_shell = enabled;
        self
    }
    /// Replaces the budgets, additionally constrained by the server configuration.
    pub fn limits(mut self, limits: StreamLimits) -> Result<Self, StreamError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }
}

const SLOT: &str = "<!--ruvane-document-slot-->";
const LAYOUT_END: &str = "</div></section>";

#[derive(Clone)]
struct LayoutInput {
    index: usize,
    props: ComponentProps,
    key: InternalUrl,
}

impl App {
    /// Streams a page's initial HTML, or restores buffered SSR with None.
    /// Head resolution runs before headers; loaders run root-first after them.
    /// Native-only and typed navigation requests retain their existing behavior.
    pub fn client_page_document_streaming(
        &mut self,
        name: &str,
        head: Option<ClientDocumentHead>,
    ) -> Result<(), AppError> {
        let page = self
            .pages
            .iter_mut()
            .find(|p| p.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        page.client.as_mut().ok_or(AppError::InvalidPage)?.document = head;
        Ok(())
    }
    pub(super) fn client_document_response(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
    ) -> Response {
        let result = Self::guarded(|| {
            let runtime = self
                .client_runtime
                .as_ref()
                .ok_or(PageError::InternalServerError)?;
            let url = Self::target(request)?;
            let props = self.client_props(&url)?;
            let client = self.pages[index].client.unwrap();
            let head = client.document.unwrap()(context)?;
            NavigationPage::new(
                url.clone(),
                &head.title,
                props,
                ActionData::default(),
                StatusCode::Ok,
            )
            .map_err(|_| PageError::InternalServerError)?;
            let mut inputs = Vec::new();
            for index in self.layout_chain(client.layout) {
                let layout = &self.client_layouts[index];
                let (matched, key) = self.layout_match(layout, &url)?;
                let props = ComponentProps::new(layout.component, (layout.state)(&matched)?)
                    .map_err(|_| PageError::BadRequest)?;
                inputs.push(LayoutInput { index, props, key });
            }
            // Only this internal comment can act as a slot; text is always escaped.
            let shell = (runtime.shell)(Html::rendered(SLOT.into())).into_string();
            if shell.len() > head.limits.body_bytes || shell.matches(SLOT).count() != 1 {
                return Err(PageError::InternalServerError);
            }
            let (before, after) = shell.split_once(SLOT).unwrap();
            let mut islands = Vec::new();
            if head.hydrate_shell {
                for value in shell.split("<div data-ruvane-shell=\"").skip(1) {
                    let name = value.split('"').next().unwrap();
                    if islands.len() == 8 || islands.contains(&name) {
                        return Err(PageError::InternalServerError);
                    }
                    islands.push(name);
                }
            }
            let script =
                crate::module_script(&InternalUrl::new(&runtime.asset("application.mjs")).unwrap());
            let (profile, early, bootstrap) = if head.hydrate_layouts || head.hydrate_shell {
                (
                    "segments",
                    format!(
                        "<script type=\"module\" async src=\"{}\"></script>",
                        runtime.asset("application.mjs"),
                    ),
                    String::new(),
                )
            } else {
                ("stream", String::new(), script.into_string())
            };
            let mut attributes = if head.hydrate_layouts || head.hydrate_shell {
                format!(
                    " data-ruvane-layouts=\"{}\" data-ruvane-timeout=\"{}\"",
                    inputs.len(),
                    head.limits.duration.as_millis()
                )
            } else {
                String::new()
            };
            if head.hydrate_shell {
                attributes.push_str(&format!(" data-ruvane-shells=\"{}\"", islands.join(",")));
                if !head.hydrate_layouts {
                    attributes.push_str(" data-ruvane-layout-hydration=\"off\"");
                }
            }
            let body = format!(
                "{}<div id=\"ruvane-application\" data-ruvane-module=\"{}\" data-ruvane-revision=\"{}\" data-ruvane-document=\"{profile}\"{attributes}>{early}{}<p id=\"ruvane-status\">Native links and forms are ready.</p><div id=\"ruvane-outlet\">",
                runtime.development_script(),
                runtime.asset("app.wasm"),
                runtime.revision,
                before,
            );
            let mut document = Document::new(&head.title, Html::rendered(body))
                .language(head.language)
                .metadata(head.metadata);
            for stylesheet in &runtime.stylesheets {
                document = document.stylesheet(stylesheet.clone());
            }
            let prefix = document.stream_prefix().into_string().into_bytes();
            let suffix = format!(
                "<div hidden id=\"ruvane-complete\"></div></div>{}</body></html>",
                bootstrap,
            )
            .into_bytes();
            let tail = format!("{after}{}", self.boundary_manifest().as_str());
            let app = Arc::new(self.clone());
            let request = request.clone();
            let path = context.path.clone();
            let origin = context.origin.cloned();
            let plan = BodyStream::new(prefix, suffix, head.limits, move |_| {
                Ok(Box::new(DocumentProducer {
                    app: Arc::clone(&app),
                    index,
                    request: request.clone(),
                    path: path.clone(),
                    origin: origin.clone(),
                    url: url.clone(),
                    props,
                    inputs: inputs.clone(),
                    seeds: Vec::new(),
                    title: head.title.clone(),
                    tail: tail.clone(),
                    limits: head.limits,
                    pending: Vec::new(),
                    offset: 0,
                    done: false,
                    hydrate_layouts: head.hydrate_layouts,
                    opened: 0,
                }) as Box<dyn ByteProducer>)
            });
            // Reject a head which cannot fit before executing any deferred loader.
            plan.validate(head.limits.body_bytes)
                .map_err(|_| PageError::InternalServerError)?;
            Ok(Response::stream_application(plan))
        });
        result.unwrap_or_else(|error| self.client_failure(request, error))
    }
}

struct DocumentProducer {
    app: Arc<App>,
    index: usize,
    request: Request,
    path: InternalUrl,
    origin: Option<SiteOrigin>,
    url: InternalUrl,
    props: ComponentProps,
    inputs: Vec<LayoutInput>,
    seeds: Vec<NavigationLayout>,
    title: String,
    tail: String,
    limits: StreamLimits,
    pending: Vec<u8>,
    offset: usize,
    done: bool,
    hydrate_layouts: bool,
    opened: usize,
}
impl DocumentProducer {
    fn pull(&mut self, stream: &StreamContext) -> Result<Vec<u8>, PageError> {
        let factory = self.app.client_factory.unwrap();
        let context = PageContext {
            app: &self.app,
            route: Some(&self.app.pages[self.index].route),
            path: &self.path,
            params: &self.request.params,
            request: Some(&self.request),
            origin: self.origin.as_ref(),
            exporting: false,
            stream: Some(stream),
        };
        if let Some(input) = self.inputs.get(self.seeds.len()) {
            let layout = &self.app.client_layouts[input.index];
            let data = (layout.load)(&context)?.data;
            let own = App::client_view_inputs(factory, input.props, &data, false, true)?;
            let seed =
                NavigationLayout::new(layout.route.name(), input.key.clone(), input.props, data)
                    .map_err(|_| PageError::InternalServerError)?;
            self.seeds.push(seed);
            let prefix = NavigationPage::new(
                self.url.clone(),
                &self.title,
                self.props,
                ActionData::default(),
                StatusCode::Ok,
            )
            .and_then(|p| p.with_pending_layouts(self.inputs.len(), self.seeds.clone()))
            .map_err(|_| PageError::InternalServerError)?;
            let record = if self.hydrate_layouts {
                if !self.app.validate_navigation(&prefix) {
                    return Err(PageError::InternalServerError);
                }
                LayoutSnapshot::new(self.app.client_runtime.as_ref().unwrap().revision, prefix)
                    .map_err(|_| PageError::InternalServerError)?
                    .html()
            } else {
                Html::default()
            };
            self.opened += 1;
            Ok(format!(
                "<section data-ruvane-layout=\"{}\"><div data-ruvane-layout-view=\"\">{}{}</div><div data-ruvane-child-outlet=\"\">",
                layout.route.name(), own.as_str(), record.as_str(),
            ).into_bytes())
        } else {
            let page = (self.app.pages[self.index].render)(&context)?;
            if page.title != self.title {
                return Err(PageError::InternalServerError);
            }
            let data = page.client_data.ok_or(PageError::InternalServerError)?;
            let own = App::client_view_inputs(factory, self.props, &data, false, true)?;
            let packet = NavigationPage::new(
                self.url.clone(),
                &self.title,
                self.props,
                data,
                StatusCode::Ok,
            )
            .and_then(|p| p.with_layouts(self.seeds.clone()))
            .map_err(|_| PageError::InternalServerError)?;
            self.finish(packet, own, page.body)
        }
    }
    fn failure(&mut self, failed: usize, error: PageError) -> Result<Vec<u8>, PageError> {
        if self.opened != failed || self.seeds.len() < failed {
            return Err(PageError::InternalServerError);
        }
        // A rejected record may have added a seed without emitting its layout.
        self.seeds.truncate(failed);
        let (depth, boundary) = self
            .app
            .error_boundary(self.index, self.opened)
            .ok_or(PageError::InternalServerError)?;
        let status = error.status();
        let props =
            ComponentProps::new(boundary.error, InitialState::Number(status.as_u16() as u32))
                .map_err(|_| PageError::InternalServerError)?;
        let packet = NavigationPage::new(
            self.url.clone(),
            &self.title,
            props,
            ActionData::default(),
            status,
        )
        .and_then(|page| page.with_failure(failed, self.seeds[..depth].to_vec()))
        .map_err(|_| PageError::InternalServerError)?;
        let own = App::client_view(self.app.client_factory.unwrap(), props, packet.data(), true)?;
        self.finish(packet, own, Html::default())
    }
    fn finish(
        &mut self,
        packet: NavigationPage,
        own: Html,
        fallback: Html,
    ) -> Result<Vec<u8>, PageError> {
        if !self.app.validate_navigation(&packet) {
            return Err(PageError::InternalServerError);
        }
        let retained = packet.layouts().len();
        let retired = if retained < self.opened {
            if packet.failed_segment() != Some(self.opened) {
                return Err(PageError::InternalServerError);
            }
            let prefix = NavigationPage::new(
                self.url.clone(),
                &self.title,
                self.props,
                ActionData::default(),
                StatusCode::Ok,
            )
            .and_then(|page| page.with_pending_layouts(self.inputs.len(), self.seeds.clone()))
            .map_err(|_| PageError::InternalServerError)?;
            if !self.app.validate_navigation(&prefix) {
                return Err(PageError::InternalServerError);
            }
            let prefix =
                LayoutSnapshot::new(self.app.client_runtime.as_ref().unwrap().revision, prefix)
                    .map_err(|_| PageError::InternalServerError)?;
            format!(
                "<div hidden id=\"ruvane-retired\">{}</div>",
                crate::navigation_snapshot::hex(&prefix.encode()),
            )
        } else {
            String::new()
        };
        let snapshot =
            NavigationSnapshot::new(self.app.client_runtime.as_ref().unwrap().revision, packet)
                .map_err(|_| PageError::InternalServerError)?;
        let output = format!(
            "{}<div data-ruvane-page=\"\" tabindex=\"-1\">{}</div>{}<div id=\"ruvane-fallback\">{}</div></div>{}{}{}",
            LAYOUT_END.repeat(self.opened - retained),
            own.as_str(),
            LAYOUT_END.repeat(retained),
            fallback.as_str(),
            self.tail,
            retired,
            snapshot.html().as_str(),
        );
        self.done = true;
        Ok(output.into_bytes())
    }
}
impl ByteProducer for DocumentProducer {
    fn next(&mut self, context: &StreamContext) -> Result<Option<Vec<u8>>, StreamError> {
        context.check()?;
        if self.offset == self.pending.len() {
            if self.done {
                return Ok(None);
            }
            self.pending.clear();
            self.offset = 0;
            let failed = self.opened;
            self.pending = match App::guarded(|| self.pull(context)) {
                Ok(bytes) => bytes,
                Err(error) => {
                    context.check()?;
                    let recovered = App::guarded(|| self.failure(failed, error)).ok();
                    match recovered {
                        Some(bytes) => bytes,
                        None if self.hydrate_layouts => {
                            self.done = true;
                            format!(
                                "<p data-ruvane-stream-error=\"\" role=\"alert\">This page could not be loaded. Native links remain available.</p>{}<div id=\"ruvane-fallback\"></div></div>{}",
                                LAYOUT_END.repeat(self.opened), self.tail,
                            ).into_bytes()
                        }
                        None => return Err(StreamError::Producer),
                    }
                }
            };
            if self.pending.len() > self.limits.body_bytes {
                return Err(StreamError::Limit);
            }
        }
        context.check()?;
        let end = (self.offset + self.limits.chunk_bytes).min(self.pending.len());
        let bytes = self.pending[self.offset..end].to_vec();
        self.offset = end;
        Ok(Some(bytes))
    }
}
