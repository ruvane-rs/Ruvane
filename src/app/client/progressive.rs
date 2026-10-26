use super::*;
use crate::streaming::{BodyStream, ByteProducer};
use crate::{
    MAX_NAVIGATION_BYTES, MAX_NAVIGATION_STREAM_BYTES, NavigationLayout, StreamContext,
    StreamError, StreamLimits,
};

fn frame(packet: &NavigationPage) -> Result<Vec<u8>, StreamError> {
    let bytes = packet.encode();
    if bytes.len() > MAX_NAVIGATION_BYTES - 4 {
        return Err(StreamError::Limit);
    }
    let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
    frame.extend(bytes);
    Ok(frame)
}

impl App {
    /// Opts a client page into ordered progressive GET navigation. Native SSR is unchanged.
    pub fn client_page_streaming(&mut self, name: &str, enabled: bool) -> Result<(), AppError> {
        let page = self
            .pages
            .iter_mut()
            .find(|p| p.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        page.client
            .as_mut()
            .ok_or(AppError::InvalidPage)?
            .progressive = enabled;
        Ok(())
    }
    pub(super) fn client_stream_response(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
    ) -> Response {
        let Some(runtime) = &self.client_runtime else {
            return Response::error(StatusCode::InternalServerError);
        };
        if request.header("ruvane-application-revision")
            != Some(runtime.revision.to_string().as_str())
        {
            return Response::error(StatusCode::Conflict);
        }
        let token = request.header("ruvane-stream-token").and_then(|raw| {
            if raw.is_empty() || raw.starts_with('0') || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            raw.parse::<u64>().ok()
        });
        let Some(token) = token else {
            return Response::error(StatusCode::BadRequest);
        };
        let built = Self::guarded(|| {
            let url = Self::target(request)?;
            let props = self.client_props(&url)?;
            let chain = self.layout_chain(self.pages[index].client.unwrap().layout);
            let initial = NavigationPage::new(
                url.clone(),
                "Loading",
                props,
                ActionData::default(),
                StatusCode::Ok,
            )
            .and_then(|p| p.with_pending_layouts(chain.len(), Vec::new()))
            .map_err(|_| PageError::InternalServerError)?;
            let mut prefix = b"RVS1".to_vec();
            prefix.extend(runtime.revision.to_le_bytes());
            prefix.extend(token.to_le_bytes());
            prefix.extend(frame(&initial).map_err(|_| PageError::InternalServerError)?);
            let app = Arc::new(self.clone());
            let request = request.clone();
            let path = context.path.clone();
            let origin = context.origin.cloned();
            let plan = BodyStream::new(
                prefix,
                [0, 0, 0, 0],
                StreamLimits {
                    chunk_bytes: MAX_NAVIGATION_BYTES,
                    body_bytes: MAX_NAVIGATION_STREAM_BYTES,
                    chunks: crate::MAX_CLIENT_LAYOUT_DEPTH + 3,
                    ..StreamLimits::default()
                },
                move |_| {
                    Ok(Box::new(BranchProducer {
                        app: Arc::clone(&app),
                        index,
                        request: request.clone(),
                        path: path.clone(),
                        origin: origin.clone(),
                        url: url.clone(),
                        props,
                        chain: chain.clone(),
                        seeds: Vec::new(),
                        done: false,
                    }) as Box<dyn ByteProducer>)
                },
            );
            Ok(Response::stream_navigation(plan))
        });
        built.unwrap_or_else(|error| self.client_failure(request, error))
    }
}

struct BranchProducer {
    app: Arc<App>,
    index: usize,
    request: Request,
    path: InternalUrl,
    origin: Option<SiteOrigin>,
    url: InternalUrl,
    props: ComponentProps,
    chain: Vec<usize>,
    seeds: Vec<NavigationLayout>,
    done: bool,
}
impl BranchProducer {
    fn failure(&self, failed: usize, error: PageError) -> Result<NavigationPage, PageError> {
        let (depth, component) = self
            .app
            .failure_presentation(self.index, failed, error.status())
            .ok_or(PageError::InternalServerError)?;
        if depth > self.seeds.len() {
            return Err(PageError::InternalServerError);
        }
        let status = error.status();
        let title = match status {
            StatusCode::BadRequest => "Invalid page request",
            StatusCode::NotFound => "Page not found",
            _ => "Page unavailable",
        };
        NavigationPage::new(
            self.url.clone(),
            title,
            ComponentProps::new(component, InitialState::Number(status.as_u16() as u32)).unwrap(),
            ActionData::default(),
            status,
        )
        .and_then(|p| p.with_failure(failed, self.seeds[..depth].to_vec()))
        .and_then(|page| page.with_metadata(Metadata::new()))
        .map_err(|_| PageError::InternalServerError)
    }
    fn pull(&mut self, context: &StreamContext) -> Result<NavigationPage, PageError> {
        let factory = self.app.client_factory.unwrap();
        let page_context = PageContext {
            app: &self.app,
            route: Some(&self.app.pages[self.index].route),
            path: &self.path,
            params: &self.request.params,
            request: Some(&self.request),
            origin: self.origin.as_ref(),
            exporting: false,
            stream: Some(context),
        };
        if let Some(index) = self.chain.get(self.seeds.len()) {
            let layout = &self.app.client_layouts[*index];
            let (matched, key) = self.app.layout_match(layout, &self.url)?;
            let props = ComponentProps::new(layout.component, (layout.state)(&matched)?)
                .map_err(|_| PageError::BadRequest)?;
            let data = (layout.load)(&page_context)?;
            let _ = App::client_view(factory, props, &data.data, false)?;
            let seed = NavigationLayout::new(layout.route.name(), key, props, data.data)
                .map_err(|_| PageError::InternalServerError)?;
            self.seeds.push(seed);
            NavigationPage::new(
                self.url.clone(),
                "Loading",
                self.props,
                ActionData::default(),
                StatusCode::Ok,
            )
            .and_then(|p| p.with_pending_layouts(self.chain.len(), self.seeds.clone()))
            .map_err(|_| PageError::InternalServerError)
        } else {
            let page = (self.app.pages[self.index].render)(&page_context)?;
            let data = page.client_data.ok_or(PageError::InternalServerError)?;
            let _ = App::client_view(factory, self.props, &data, false)?;
            self.done = true;
            let metadata = page.client_metadata;
            let packet = NavigationPage::new(
                self.url.clone(),
                &page.title,
                self.props,
                data,
                StatusCode::Ok,
            )
            .and_then(|p| p.with_layouts(self.seeds.clone()))
            .map_err(|_| PageError::InternalServerError)?;
            self.app.packet_metadata(packet, metadata)
        }
    }
}
impl ByteProducer for BranchProducer {
    fn next(&mut self, context: &StreamContext) -> Result<Option<Vec<u8>>, StreamError> {
        context.check()?;
        if self.done {
            return Ok(None);
        }
        let position = self.seeds.len();
        let packet = match App::guarded(|| self.pull(context)) {
            Ok(packet) => packet,
            Err(error) => {
                self.done = true;
                self.failure(position, error)
                    .map_err(|_| StreamError::Producer)?
            }
        };
        context.check()?;
        // Check the complete packet again before sending any of its bytes.
        if !self.app.validate_navigation(&packet) {
            return Err(StreamError::Producer);
        }
        if packet.failed_segment().is_some() {
            let _ = App::client_view(
                self.app.client_factory.unwrap(),
                packet.props(),
                packet.data(),
                true,
            )
            .map_err(|_| StreamError::Producer)?;
        }
        frame(&packet).map(Some)
    }
}
