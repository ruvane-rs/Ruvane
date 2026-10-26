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
                .access_output(namespace, index, &url, &policy.tags)
                .map_err(|_| ())
        });
        let lease = match access {
            Ok(Access::Hit(branch, None)) => return Self::output_response(request, &branch),
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
                    self.cached_branch_finish(index, &context, &fresh, &branch, None)
                });
                match output {
                    Ok(output) if output.status() == StatusCode::Ok && !output.is_streaming() => {
                        branch.output = Some(Arc::from(output.body()));
                        let response = Self::output_response(request, &branch);
                        if lease.publish(public.then_some(branch)).is_err() {
                            return Self::cache_busy();
                        }
                        response
                    }
                    output => self.boundary_failure(
                        index,
                        branch.packet.layouts().len(),
                        branch.packet.layouts().to_vec(),
                        request,
                        output.err().unwrap_or(PageError::InternalServerError),
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
