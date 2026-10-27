use super::*;
use crate::prerender::PrerenderRow;
use crate::{
    ClientPrerender, MAX_CLIENT_PRERENDER_BYTES, MAX_CLIENT_PRERENDER_PAGES, PrerenderError,
};

impl App {
    /// Reports explicitly declared pre-render pages without executing callbacks.
    pub fn has_client_prerender(&self) -> bool {
        self.pages.iter().any(|page| page.prerender.is_some())
    }
    /// Enumerates public, query-free client pages to freeze during a production build.
    /// None disables pre-rendering. This cannot be combined with a server cache policy.
    pub fn client_page_prerender(
        &mut self,
        name: &str,
        params: Option<Vec<PageParams>>,
    ) -> Result<(), AppError> {
        let mut candidate = self.clone();
        let page = candidate
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        if page.client.is_none() {
            return Err(AppError::InvalidPage);
        }
        page.prerender = params;
        if page.prerender.is_none() {
            page.revalidation = None;
        }
        candidate.client_prerender = None;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub(in crate::app) fn prerender_entries(
        &self,
    ) -> Result<Vec<(usize, InternalUrl, PageParams)>, AppError> {
        let mut entries = Vec::new();
        for (index, page) in self.pages.iter().enumerate() {
            if page.revalidation.is_some() && page.prerender.is_none() {
                return Err(AppError::InvalidPage);
            }
            if let Some(params) = &page.prerender {
                if page.client.is_none()
                    || params.is_empty()
                    || params.len() > MAX_CLIENT_PRERENDER_PAGES
                    || page.server_cache.max_age_seconds().is_some()
                {
                    return Err(AppError::InvalidPage);
                }
                for params in params {
                    if entries.len() == MAX_CLIENT_PRERENDER_PAGES {
                        return Err(AppError::TooManyStaticPages);
                    }
                    let url = self.url(page.route.name(), &params.pairs())?;
                    if entries.iter().any(|(_, other, _)| other == &url) {
                        return Err(AppError::RouteConflict);
                    }
                    entries.push((index, url, params.clone()));
                }
            }
        }
        Ok(entries)
    }
    /// Runs only explicitly listed read-only loaders, without headers, body or query.
    /// Produces complete buffered HTML and matching typed snapshots for one revision.
    /// Build callbacks are trusted Rust code, not sandboxed or automatically crawled.
    pub fn client_prerender_plan(&self) -> Result<ClientPrerender, PrerenderError> {
        self.validate()
            .map_err(|_| PrerenderError::InvalidDeclaration)?;
        let runtime = self
            .client_runtime
            .as_ref()
            .ok_or(PrerenderError::InvalidDeclaration)?;
        if runtime.development.is_some() {
            return Err(PrerenderError::InvalidDeclaration);
        }
        let mut plan = ClientPrerender {
            revision: runtime.revision,
            rows: Vec::new(),
        };
        let mut total = 24usize;
        for (index, url, params) in self
            .prerender_entries()
            .map_err(|_| PrerenderError::InvalidDeclaration)?
        {
            let request = Request::prerender_get(&url, params.0);
            let context = PageContext {
                app: self,
                route: Some(&self.pages[index].route),
                path: &url,
                params: &request.params,
                request: Some(&request),
                origin: None,
                exporting: true,
                stream: None,
            };
            let (branch, public) = self
                .load_client_branch(index, &context, &request)
                .map_err(|_| PrerenderError::RenderFailed)?;
            if !public {
                return Err(PrerenderError::InvalidDeclaration);
            }
            let response = Self::guarded(|| {
                self.cached_branch_finish(index, &context, &request, &branch, None)
            })
            .map_err(|_| PrerenderError::RenderFailed)?;
            if response.status() != StatusCode::Ok || response.is_streaming() {
                return Err(PrerenderError::RenderFailed);
            }
            let html = response
                .body_text()
                .map_err(|_| PrerenderError::RenderFailed)?
                .to_owned();
            let name = self.pages[index].route.name().to_owned();
            total = total
                .checked_add(
                    16 + name.len()
                        + url.as_str().len()
                        + branch.packet.encode().len()
                        + html.len(),
                )
                .ok_or(PrerenderError::Limit)?;
            if total > MAX_CLIENT_PRERENDER_BYTES {
                return Err(PrerenderError::Limit);
            }
            plan.rows.push(PrerenderRow {
                name,
                url,
                packet: branch.packet,
                html,
            });
        }
        ClientPrerender::decode(&plan.encode())?;
        Ok(plan)
    }
    /// Attaches trusted bytes embedded by this same application's production build.
    /// Checks revision, exact declared URL coverage, ownership and component schemas.
    /// This is not an authenticated artifact-upload or external HTML loading API.
    pub fn client_prerendered(&mut self, bytes: &'static [u8]) -> Result<(), PrerenderError> {
        let plan = ClientPrerender::decode(bytes)?;
        self.check_prerender(&plan)?;
        self.client_prerender = Some(Arc::new(plan));
        Ok(())
    }
    fn check_prerender(&self, plan: &ClientPrerender) -> Result<(), PrerenderError> {
        let runtime = self
            .client_runtime
            .as_ref()
            .ok_or(PrerenderError::InvalidDeclaration)?;
        if plan.revision != runtime.revision || runtime.development.is_some() {
            return Err(PrerenderError::InvalidArtifact);
        }
        let expected = self
            .prerender_entries()
            .map_err(|_| PrerenderError::InvalidDeclaration)?;
        if expected.len() != plan.rows.len() {
            return Err(PrerenderError::InvalidArtifact);
        }
        Self::guarded(|| {
            for ((index, url, _), row) in expected.iter().zip(&plan.rows) {
                if row.name != self.pages[*index].route.name() || &row.url != url {
                    return Err(PageError::InternalServerError);
                }
                self.check_prerender_row(row)?;
            }
            Ok(())
        })
        .map_err(|_| PrerenderError::InvalidArtifact)
    }
    pub(super) fn check_prerender_row(&self, row: &PrerenderRow) -> Result<(), PageError> {
        let factory = self.client_factory.ok_or(PageError::InternalServerError)?;
        let runtime = self
            .client_runtime
            .as_ref()
            .ok_or(PageError::InternalServerError)?;
        if !self.validate_navigation(&row.packet)
            || !row.html.contains(&format!(
                "data-ruvane-module=\"{}\"",
                runtime.asset("app.wasm")
            ))
        {
            return Err(PageError::InternalServerError);
        }
        let _ = Self::client_view(factory, row.packet.props(), row.packet.data(), false)?;
        for layout in row.packet.layouts() {
            let _ = Self::client_view(factory, layout.props(), layout.data(), false)?;
        }
        Ok(())
    }
    pub(in crate::app) fn validate_prerender_artifact(&self) -> Result<(), AppError> {
        if self.has_client_prerender()
            && self.client_prerender.is_none()
            && self
                .client_runtime
                .as_ref()
                .is_none_or(|runtime| runtime.development.is_none())
        {
            return Err(AppError::InvalidPage);
        }
        if let Some(plan) = &self.client_prerender {
            self.check_prerender(plan)
                .map_err(|_| AppError::InvalidPage)?;
        }
        Ok(())
    }
    pub(super) fn prerender_response(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
    ) -> Option<Response> {
        if self.pages[index].prerender.is_none()
            || request.query().is_some()
            || request.cache_bypass()
        {
            return None;
        }
        let plan = self.client_prerender.as_ref()?;
        let row = plan.rows.iter().find(|row| {
            row.name == self.pages[index].route.name() && row.url.as_str() == request.path()
        })?;
        if let Some(policy) = &self.pages[index].revalidation {
            return Some(self.regenerated_response(index, context, request, policy));
        }
        Some(
            if matches!(
                request.header("accept"),
                Some(crate::NAVIGATION_MEDIA_TYPE | crate::NAVIGATION_STREAM_MEDIA_TYPE)
            ) {
                let client = self.pages[index].client.unwrap();
                let mut policy = client.cache;
                for layout in self.layout_chain(client.layout) {
                    policy = Self::combine_cache(policy, self.client_layouts[layout].cache);
                }
                Response::navigation_with_cache(&row.packet, policy)
            } else {
                Response::interactive_html(Html::rendered(row.html.clone())).navigation_variance()
            },
        )
    }
}
