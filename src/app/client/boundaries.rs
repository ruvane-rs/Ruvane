use super::*;
use crate::{NavigationCachePolicy, NavigationLayout};

/// Read-only loading and error components for an App, page or layout's children.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientBoundary {
    pub(super) loading: u32,
    pub(super) error: u32,
}
impl ClientBoundary {
    /// Loading accepts Number(0); errors accept Number(0/408/400/404/500).
    /// Both components accept an empty page seed and must not start actions.
    pub fn new(loading: u32, error: u32) -> Result<Self, AppError> {
        if loading == 0 || error == 0 {
            return Err(AppError::InvalidPage);
        }
        Ok(Self { loading, error })
    }
}
impl App {
    /// Configures the App boundary used when no page or parent boundary applies.
    /// It can catch a root layout failure. Registration runs no components.
    pub fn client_boundary(&mut self, boundary: Option<ClientBoundary>) -> Result<(), AppError> {
        if self.client_factory.is_none() {
            return Err(AppError::InvalidPage);
        }
        self.client_boundary = boundary;
        Ok(())
    }
    /// Configures presentation for a page, or removes its boundary.
    pub fn client_page_boundary(
        &mut self,
        name: &str,
        boundary: Option<ClientBoundary>,
    ) -> Result<(), AppError> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        page.client.as_mut().ok_or(AppError::InvalidPage)?.boundary = boundary;
        Ok(())
    }
    /// Configures a layout's child boundary. It cannot catch its own loader failure.
    pub fn client_layout_boundary(
        &mut self,
        name: &str,
        boundary: Option<ClientBoundary>,
    ) -> Result<(), AppError> {
        let index = self.client_layout_index(Some(name))?.unwrap();
        self.client_layouts[index].boundary = boundary;
        Ok(())
    }
    pub(super) fn error_boundary(
        &self,
        index: usize,
        failed: usize,
    ) -> Option<(usize, ClientBoundary)> {
        let client = self.pages[index].client?;
        let chain = self.layout_chain(client.layout);
        if failed > chain.len() {
            return None;
        }
        if failed == chain.len() {
            if let Some(boundary) = client.boundary {
                return Some((failed, boundary));
            }
        }
        chain
            .iter()
            .take(failed)
            .enumerate()
            .rev()
            .find_map(|(depth, index)| {
                self.client_layouts[*index]
                    .boundary
                    .map(|boundary| (depth + 1, boundary))
            })
            .or_else(|| self.client_boundary.map(|boundary| (0, boundary)))
    }
    pub(super) fn validate_boundary_page(&self, packet: &NavigationPage) -> bool {
        let Some(failed) = packet.failed_segment() else {
            return false;
        };
        let path = packet.url().as_str().split('?').next().unwrap();
        let Some(name) = self.owner(path) else {
            return false;
        };
        let Some(index) = self
            .pages
            .iter()
            .position(|page| page.route.name() == name && page.client.is_some())
        else {
            return false;
        };
        if self.client_props(packet.url()).is_err() {
            return false;
        }
        self.error_boundary(index, failed)
            .is_some_and(|(depth, boundary)| {
                depth == packet.layouts().len()
                    && packet.data().is_empty()
                    && matches!(
                        packet.status(),
                        StatusCode::BadRequest
                            | StatusCode::NotFound
                            | StatusCode::InternalServerError
                    )
                    && ComponentProps::new(
                        boundary.error,
                        InitialState::Number(packet.status().as_u16() as u32),
                    )
                    .is_ok_and(|props| props == packet.props())
                    && self.validate_layout_prefix(packet, Some(depth))
            })
    }
    pub(super) fn guarded<T>(f: impl FnOnce() -> Result<T, PageError>) -> Result<T, PageError> {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(PageError::InternalServerError))
    }
    fn boundary_failure(
        &self,
        index: usize,
        failed: usize,
        seeds: Vec<NavigationLayout>,
        request: &Request,
        error: PageError,
    ) -> Response {
        let Some((depth, boundary)) = self.error_boundary(index, failed) else {
            return self.client_failure(request, error);
        };
        let result = Self::guarded(|| {
            if seeds.len() < depth {
                return Err(PageError::InternalServerError);
            }
            let status = error.status();
            let title = match status {
                StatusCode::NotFound => "Page not found",
                StatusCode::BadRequest => "Invalid page request",
                _ => "Page unavailable",
            };
            let packet = NavigationPage::new(
                Self::target(request)?,
                title,
                ComponentProps::new(boundary.error, InitialState::Number(status.as_u16() as u32))
                    .unwrap(),
                ActionData::default(),
                status,
            )
            .and_then(|page| page.with_failure(failed, seeds.into_iter().take(depth).collect()))
            .map_err(|_| PageError::InternalServerError)?;
            self.client_finish(
                &packet,
                Html::default(),
                request,
                NavigationCachePolicy::no_store(),
            )
        });
        result.unwrap_or_else(|_| Response::error(StatusCode::InternalServerError))
    }
    pub(super) fn client_branch_response(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
    ) -> Response {
        let Ok(url) = Self::target(request) else {
            return self.client_failure(request, PageError::BadRequest);
        };
        let props = match Self::guarded(|| self.client_props(&url)) {
            Ok(props) => props,
            Err(error) => return self.client_failure(request, error),
        };
        let factory = self.client_factory.unwrap();
        let chain = self.layout_chain(self.pages[index].client.unwrap().layout);
        let mut seeds = Vec::new();
        let mut policy = NavigationCachePolicy::public(300).unwrap();
        for (position, layout_index) in chain.iter().enumerate() {
            let layout = &self.client_layouts[*layout_index];
            let seed = Self::guarded(|| {
                let (matched, key) = self.layout_match(layout, &url)?;
                let props = ComponentProps::new(layout.component, (layout.state)(&matched)?)
                    .map_err(|_| PageError::BadRequest)?;
                let data = (layout.load)(context)?;
                let _ = Self::client_view(factory, props, &data.data, false)?;
                let policy = if data.no_store {
                    NavigationCachePolicy::no_store()
                } else {
                    layout.cache
                };
                let seed = NavigationLayout::new(layout.route.name(), key, props, data.data)
                    .map_err(|_| PageError::InternalServerError)?;
                Ok((seed, policy))
            });
            match seed {
                Ok((seed, cache)) => {
                    seeds.push(seed);
                    policy = Self::combine_cache(policy, cache);
                }
                Err(error) => return self.boundary_failure(index, position, seeds, request, error),
            }
        }
        let page = Self::guarded(|| {
            let page = (self.pages[index].render)(context)?;
            let _ = Self::client_view(
                factory,
                props,
                page.client_data
                    .as_ref()
                    .ok_or(PageError::InternalServerError)?,
                false,
            )?;
            Ok(page)
        });
        let page = match page {
            Ok(page) => page,
            Err(error) => return self.boundary_failure(index, chain.len(), seeds, request, error),
        };
        policy = if page.client_no_store
            || request.header("cookie").is_some()
            || request.header("authorization").is_some()
        {
            NavigationCachePolicy::no_store()
        } else {
            Self::combine_cache(policy, self.pages[index].client.unwrap().cache)
        };
        let result = Self::guarded(|| {
            let packet = NavigationPage::new(
                url,
                &page.title,
                props,
                page.client_data.unwrap(),
                StatusCode::Ok,
            )
            .and_then(|page| page.with_layouts(seeds.clone()))
            .map_err(|_| PageError::InternalServerError)?;
            self.client_finish(&packet, page.body, request, policy)
        });
        result.unwrap_or_else(|error| {
            self.boundary_failure(index, chain.len(), seeds, request, error)
        })
    }
    pub(super) fn boundary_manifest(&self) -> Html {
        let mut output = String::from("<div hidden id=\"ruvane-boundaries\">");
        let mut record =
            |kind: &str, route: &PageRoute, chain: Vec<usize>, boundary: Option<ClientBoundary>| {
                output.push_str("<span");
                let names = chain
                    .iter()
                    .map(|index| self.client_layouts[*index].route.name())
                    .collect::<Vec<_>>()
                    .join(",");
                for (key, value) in [
                    ("kind", kind.to_owned()),
                    ("name", route.name().into()),
                    ("pattern", route.pattern().into()),
                    ("chain", names),
                    ("loading", boundary.map_or(0, |b| b.loading).to_string()),
                    ("error", boundary.map_or(0, |b| b.error).to_string()),
                ] {
                    output.push_str(" data-rvn-");
                    output.push_str(key);
                    output.push_str("=\"");
                    crate::html::escape_into(&mut output, &value);
                    output.push('"');
                }
                output.push_str("></span>");
            };
        if let Some(boundary) = self.client_boundary {
            record(
                "app",
                &PageRoute::new("app", "/").unwrap(),
                Vec::new(),
                Some(boundary),
            );
        }
        for (index, layout) in self.client_layouts.iter().enumerate() {
            record(
                "layout",
                &layout.route,
                self.layout_chain(Some(index)),
                layout.boundary,
            );
        }
        for page in &self.pages {
            if let Some(client) = page.client {
                record(
                    "page",
                    &page.route,
                    self.layout_chain(client.layout),
                    client.boundary,
                );
            }
        }
        output.push_str("</div>");
        Html::rendered(output)
    }
}
