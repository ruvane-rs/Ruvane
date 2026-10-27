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
        if loading == 0
            || error == 0
            || loading == crate::GUARD_COMPONENT_ID
            || error == crate::GUARD_COMPONENT_ID
        {
            return Err(AppError::InvalidPage);
        }
        Ok(Self { loading, error })
    }
}
pub(super) struct BranchFailure {
    pub error: PageError,
    pub retire: bool,
    pub(super) failed: Option<usize>,
    pub(super) seeds: Vec<NavigationLayout>,
}
impl App {
    pub(crate) fn validate_file_presentation(
        &self,
        component: u32,
        states: &[u32],
    ) -> Result<(), PageError> {
        if component == crate::GUARD_COMPONENT_ID {
            return Err(PageError::InternalServerError);
        }
        let factory = self.client_factory.ok_or(PageError::InternalServerError)?;
        Self::guarded(|| {
            for state in states {
                let props = ComponentProps::new(component, InitialState::Number(*state))
                    .map_err(|_| PageError::InternalServerError)?;
                let _ = Self::client_view(factory, props, &ActionData::default(), true)?;
            }
            Ok(())
        })
    }

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
        self.failure_presentation(index, failed, packet.status())
            .is_some_and(|(depth, component)| {
                depth == packet.layouts().len()
                    && packet.data().is_empty()
                    && matches!(
                        packet.status(),
                        StatusCode::BadRequest
                            | StatusCode::NotFound
                            | StatusCode::InternalServerError
                    )
                    && ComponentProps::new(
                        component,
                        InitialState::Number(packet.status().as_u16() as u32),
                    )
                    .is_ok_and(|props| props == packet.props())
                    && self.validate_layout_prefix(packet, Some(depth))
            })
    }
    pub(super) fn guarded<T>(f: impl FnOnce() -> Result<T, PageError>) -> Result<T, PageError> {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(PageError::InternalServerError))
    }
    pub(super) fn boundary_failure(
        &self,
        index: usize,
        failed: usize,
        seeds: Vec<NavigationLayout>,
        request: &Request,
        error: PageError,
    ) -> Response {
        let Some((depth, component)) = self.failure_presentation(index, failed, error.status())
        else {
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
                ComponentProps::new(component, InitialState::Number(status.as_u16() as u32))
                    .unwrap(),
                ActionData::default(),
                status,
            )
            .and_then(|page| page.with_failure(failed, seeds.into_iter().take(depth).collect()))
            .and_then(|page| page.with_metadata(Metadata::new()))
            .map_err(|_| PageError::InternalServerError)?;
            self.client_finish(
                &packet,
                Html::default(),
                request,
                NavigationCachePolicy::no_store(),
                None,
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
        use crate::server_cache::Access;
        let lease = match self.server_cache_access(index, request) {
            Ok(Access::Hit(branch, expires)) => {
                return Self::guarded(|| {
                    self.cached_branch_finish(index, context, request, &branch, expires)
                })
                .unwrap_or_else(|error| self.client_failure(request, error));
            }
            Ok(Access::Stale(branch, expires, refresh)) => {
                let response = Self::guarded(|| {
                    self.cached_branch_finish(index, context, request, &branch, Some(expires))
                });
                return match response {
                    Ok(response) => {
                        if let Some(lease) = refresh {
                            self.queue_refresh(index, context, request, lease);
                        }
                        response
                    }
                    Err(error) => self.client_failure(request, error),
                };
            }
            Ok(Access::Load(lease)) => Some(lease),
            Ok(Access::Bypass) => None,
            Err(response) => return response,
        };
        match self.load_client_branch(index, context, request) {
            Ok((branch, cacheable)) => {
                let response = Self::guarded(|| {
                    self.cached_branch_finish(index, context, request, &branch, None)
                });
                match response {
                    Ok(response) => {
                        if let Some(lease) = lease {
                            if lease.publish(cacheable.then_some(branch)).is_err() {
                                return Self::cache_busy();
                            }
                        }
                        response
                    }
                    Err(error) => self.boundary_failure(
                        index,
                        branch.packet.layouts().len(),
                        branch.packet.layouts().to_vec(),
                        request,
                        error,
                    ),
                }
            }
            Err(failure) => match failure.failed {
                Some(failed) => {
                    self.boundary_failure(index, failed, failure.seeds, request, failure.error)
                }
                None => self.client_failure(request, failure.error),
            },
        }
    }
    pub(super) fn load_client_branch(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
    ) -> Result<(crate::server_cache::CachedBranch, bool), BranchFailure> {
        let url = Self::target(request).map_err(|error| BranchFailure {
            error,
            retire: false,
            failed: None,
            seeds: Vec::new(),
        })?;
        let mut cacheable = true;
        let props = match Self::guarded(|| self.client_props(&url)) {
            Ok(props) => props,
            Err(error) => {
                return Err(BranchFailure {
                    error,
                    retire: false,
                    failed: None,
                    seeds: Vec::new(),
                });
            }
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
                cacheable &= !data.no_store;
                let _ = Self::client_view(factory, props, &data.data, false)?;
                let policy = if data.no_store {
                    NavigationCachePolicy::no_store()
                } else {
                    layout.cache
                };
                let seed = NavigationLayout::new(layout.route.name(), key, props, data.data)
                    .map_err(|_| PageError::InternalServerError)?;
                Ok((seed, policy, !data.no_store))
            });
            match seed {
                Ok((seed, cache, public)) => {
                    cacheable &= public;
                    seeds.push(seed);
                    policy = Self::combine_cache(policy, cache);
                }
                Err(error) => {
                    return Err(BranchFailure {
                        error,
                        retire: !cacheable,
                        failed: Some(position),
                        seeds,
                    });
                }
            }
        }
        let page = Self::guarded(|| {
            let page = (self.pages[index].render)(context)?;
            cacheable &= !page.client_no_store;
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
            Err(error) => {
                return Err(BranchFailure {
                    error,
                    retire: !cacheable,
                    failed: Some(chain.len()),
                    seeds,
                });
            }
        };
        cacheable &= !page.client_no_store;
        policy = if page.client_no_store || request.cache_bypass() {
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
            let mut packet = self.packet_metadata(packet, page.client_metadata)?;
            let head = if (self.pages[index].prerender.is_some()
                || (self.pages[index].server_cache.max_age_seconds().is_some()
                    && self.server_cache.is_some()
                    && self.has_not_found(index)))
                && !request.cache_bypass()
            {
                self.pages[index]
                    .client
                    .unwrap()
                    .document
                    .map(|head| head(context))
                    .transpose()?
            } else {
                None
            };
            if let Some(head) = &head {
                let metadata = head.cache_metadata(packet.title())?;
                if packet.metadata().is_none()
                    && (self.pages[index].server_cache.stale_seconds() > 0
                        || self.pages[index].prerender.is_some())
                {
                    packet = self.packet_metadata(packet, Some(metadata))?;
                }
            }
            let branch = crate::server_cache::CachedBranch {
                output: None,
                head,
                packet,
                fallback: page.body,
                policy,
            };
            Ok(branch)
        });
        result
            .map(|branch| (branch, cacheable))
            .map_err(|error| BranchFailure {
                error,
                retire: !cacheable,
                failed: Some(chain.len()),
                seeds,
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
