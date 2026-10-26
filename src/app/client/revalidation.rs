use super::*;
use crate::server_cache::{Access, CachedBranch};
use crate::{NavigationCachePolicy, PrerenderRevalidationPolicy};

impl App {
    /// Regenerates listed public URLs on their first runtime request.
    /// Requires a ServerCache handle at router construction. None restores immutable mode.
    pub fn client_page_revalidation(
        &mut self,
        name: &str,
        policy: Option<PrerenderRevalidationPolicy>,
    ) -> Result<(), AppError> {
        let page = self
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?;
        if page.client.is_none() || (policy.is_some() && page.prerender.is_none()) {
            return Err(AppError::InvalidPage);
        }
        page.revalidation = policy;
        Ok(())
    }

    pub(super) fn regenerated_response(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
        policy: &PrerenderRevalidationPolicy,
    ) -> Response {
        let (Some(cache), Some(namespace)) = (&self.server_cache, self.cache_namespace) else {
            return Self::cache_busy();
        };
        let access = Self::target(request).map_err(|_| ()).and_then(|url| {
            cache
                .access_output(namespace, index, &url, policy)
                .map_err(|_| ())
        });
        let lease = match access {
            Ok(Access::Hit(branch, _)) => return Self::output_response(request, &branch),
            Ok(Access::Stale(branch, _, refresh)) => {
                let response = Self::output_response(request, &branch);
                if response.status() == StatusCode::Ok {
                    if let Some(lease) = refresh {
                        self.queue_refresh(index, context, request, lease);
                    }
                }
                return response;
            }
            Ok(Access::Load(lease)) => lease,
            _ => return Self::cache_busy(),
        };
        let fresh = request.background_get();
        let context = PageContext {
            app: self,
            route: Some(&self.pages[index].route),
            path: context.path,
            params: &fresh.params,
            request: Some(&fresh),
            origin: context.origin,
            exporting: false,
            stream: None,
        };
        match self.load_client_branch(index, &context, &fresh) {
            Ok((mut branch, public)) => {
                let output = Self::guarded(|| {
                    self.prepare_regenerated_output(index, &context, &fresh, &mut branch)
                });
                match output {
                    Ok(()) => {
                        let response = Self::output_response(request, &branch);
                        if lease.publish(public.then_some(branch)).is_err() {
                            return Self::cache_busy();
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

    pub(super) fn prepare_regenerated_output(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
        branch: &mut CachedBranch,
    ) -> Result<(), PageError> {
        let output = self.cached_branch_finish(index, context, request, branch, None)?;
        if output.status() != StatusCode::Ok || output.is_streaming() {
            return Err(PageError::InternalServerError);
        }
        branch.output = Some(Arc::from(output.body()));
        Ok(())
    }

    fn output_response(request: &Request, branch: &CachedBranch) -> Response {
        let Some(output) = &branch.output else {
            return Self::cache_busy();
        };
        if matches!(
            request.header("accept"),
            Some(crate::NAVIGATION_MEDIA_TYPE | crate::NAVIGATION_STREAM_MEDIA_TYPE)
        ) {
            Response::navigation_with_cache(&branch.packet, NavigationCachePolicy::no_store())
        } else {
            Response::interactive_shared(Arc::clone(output)).navigation_variance()
        }
    }
}
