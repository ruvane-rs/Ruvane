use super::*;
use crate::server_cache::{Access, CachedBranch};
use crate::{ServerCache, ServerCachePolicy};
use std::time::Instant;
#[cfg(test)]
mod tests;

impl App {
    pub(super) fn queue_refresh(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
        lease: crate::server_cache::Lease,
    ) {
        let owner = self.cache_owner.clone();
        let request = request.background_get();
        let path = context.path.clone();
        let origin = context.origin.cloned();
        let output = lease.is_output();
        let job = Box::new(move || {
            if !lease.is_current() {
                return;
            }
            let Some(app) = owner.upgrade() else {
                return;
            };
            let context = PageContext {
                app: &app,
                route: Some(&app.pages[index].route),
                path: &path,
                params: &request.params,
                request: Some(&request),
                origin: origin.as_ref(),
                exporting: false,
                stream: None,
            };
            match app.load_client_branch(index, &context, &request) {
                Ok((_, false)) => {
                    let _ = lease.publish(None);
                }
                Ok((mut branch, true)) => {
                    match Self::guarded(|| {
                        if output {
                            app.prepare_regenerated_output(index, &context, &request, &mut branch)
                        } else {
                            app.cached_branch_finish(index, &context, &request, &branch, None)
                                .map(|_| ())
                        }
                    }) {
                        Ok(_) => {
                            let _ = lease.publish(Some(branch));
                        }
                        Err(error) => lease.fail(error),
                    }
                }
                Err(failure) if failure.retire => {
                    let _ = lease.publish(None);
                }
                Err(failure) => lease.fail(failure.error),
            }
        });
        if let Some(cache) = &self.server_cache {
            let _ = cache.submit(job);
        }
    }
    /// Attaches an explicit cache/invalidation handle to subsequently constructed routers.
    /// Existing routers keep their handle. Each new router has a fresh cache namespace.
    pub fn server_cache(&mut self, cache: ServerCache) {
        self.server_cache = Some(cache);
    }

    /// Opts an entire client page branch, including layout data and fallback, into
    /// public server storage. Cookie/Authorization, streaming and no_store bypass it.
    pub fn client_page_server_cache(
        &mut self,
        name: &str,
        policy: ServerCachePolicy,
    ) -> Result<(), AppError> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        if page.client.is_none() {
            return Err(AppError::InvalidPage);
        }
        if page.prerender.is_some() && policy.max_age_seconds().is_some() {
            return Err(AppError::InvalidPage);
        }
        page.server_cache = policy;
        Ok(())
    }
    pub(super) fn server_cache_access(
        &self,
        index: usize,
        request: &Request,
    ) -> Result<Access, Response> {
        if self.pages[index].server_cache.max_age_seconds().is_none()
            || request.header("cookie").is_some()
            || request.header("authorization").is_some()
        {
            return Ok(Access::Bypass);
        }
        let Some(cache) = &self.server_cache else {
            return Ok(Access::Bypass);
        };
        let url = Self::target(request).map_err(|error| self.client_failure(request, error))?;
        cache
            .access(
                self.cache_namespace.ok_or_else(Self::cache_busy)?,
                index,
                &url,
                &self.pages[index].server_cache,
            )
            .map_err(|_| Self::cache_busy())
    }
    pub(super) fn cache_busy() -> Response {
        Response::error(StatusCode::ServiceUnavailable)
    }

    pub(super) fn cached_branch_finish(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
        branch: &CachedBranch,
        expires: Option<Instant>,
    ) -> Result<Response, PageError> {
        let head = if !matches!(
            request.header("accept"),
            Some(crate::NAVIGATION_MEDIA_TYPE | crate::NAVIGATION_STREAM_MEDIA_TYPE)
        ) && request.query_params().get("native") != Some("yes")
            && (self.has_not_found(index) || branch.head.is_some())
        {
            match &branch.head {
                Some(head) => Some(head.clone()),
                None => self.pages[index]
                    .client
                    .unwrap()
                    .document
                    .map(|head| head(context))
                    .transpose()?,
            }
        } else {
            None
        };
        let response = self.client_finish(
            &branch.packet,
            branch.fallback.clone(),
            request,
            branch.policy,
            head,
        )?;
        let remaining = expires
            .map(|expiry| expiry.saturating_duration_since(Instant::now()).as_secs() as u32)
            .or(self.pages[index].server_cache.max_age_seconds());
        Ok(match remaining {
            Some(seconds) => response.limit_navigation_freshness(seconds),
            None => response,
        })
    }
}
