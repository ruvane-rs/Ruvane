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
            let identity = if cache.has_persistence() {
                Some(
                    self.persistent_output_identity(index, context, request, policy)
                        .ok_or(())?,
                )
            } else {
                None
            };
            cache
                .access_output(namespace, index, &url, policy, identity)
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
        match lease.restore() {
            Ok(Some(restored)) => {
                let validated = Self::guarded(|| self.check_prerender_row(&restored.row));
                if validated.is_ok()
                    && restored
                        .expires
                        .is_none_or(|end| end > std::time::Instant::now())
                {
                    let branch = CachedBranch {
                        output: Some(Arc::from(restored.row.html.into_bytes())),
                        head: None,
                        packet: restored.row.packet,
                        fallback: Html::default(),
                        policy: NavigationCachePolicy::no_store(),
                    };
                    let response = Self::output_response(request, &branch);
                    return if lease.publish_restored(branch, restored.expires).is_ok() {
                        response
                    } else {
                        Self::cache_busy()
                    };
                }
                if lease.reject_restore().is_err() {
                    return Self::cache_busy();
                }
            }
            Ok(None) => {}
            Err(_) => return Self::cache_busy(),
        }
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

    fn persistent_output_identity(
        &self,
        index: usize,
        context: &PageContext<'_>,
        request: &Request,
        policy: &PrerenderRevalidationPolicy,
    ) -> Option<crate::server_cache::OutputIdentity> {
        let runtime = self.client_runtime.as_ref()?;
        let page = &self.pages[index];
        let row = self
            .client_prerender
            .as_ref()?
            .rows
            .iter()
            .find(|row| row.name == page.route.name() && row.url.as_str() == request.path())?;
        let mut scope = b"ruvane-output-scope\x01".to_vec();
        let mut field = |bytes: &[u8]| {
            scope.extend((bytes.len() as u32).to_le_bytes());
            scope.extend(bytes);
        };
        field(&runtime.revision.to_le_bytes());
        field(
            context
                .origin
                .map(|origin| origin.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        field(
            self.client_metadata_origin
                .as_ref()
                .map(|origin| origin.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        field(page.route.name().as_bytes());
        field(page.route.pattern().as_bytes());
        field(&row.packet.encode());
        field(&policy.max_age_seconds().unwrap_or(0).to_le_bytes());
        field(&policy.stale_seconds().to_le_bytes());
        field(&(policy.tags.len() as u32).to_le_bytes());
        for tag in &policy.tags {
            field(tag.as_bytes());
        }
        field(
            &page
                .client?
                .cache
                .max_age_seconds()
                .unwrap_or(0)
                .to_le_bytes(),
        );
        let chain = self.layout_chain(page.client?.layout);
        field(&(chain.len() as u32).to_le_bytes());
        for layout in chain {
            let layout = &self.client_layouts[layout];
            field(layout.route.name().as_bytes());
            field(layout.route.pattern().as_bytes());
            field(&layout.component.to_le_bytes());
            field(&layout.cache.max_age_seconds().unwrap_or(0).to_le_bytes());
        }
        field(&(runtime.stylesheets.len() as u32).to_le_bytes());
        for stylesheet in &runtime.stylesheets {
            field(stylesheet.as_str().as_bytes());
        }
        if scope.len() > crate::server_cache::persistence_scope_limit() {
            return None;
        }
        Some(crate::server_cache::OutputIdentity {
            scope,
            name: page.route.name().into(),
            revision: runtime.revision,
        })
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
