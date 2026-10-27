use super::*;
use crate::routing::Segment;
use crate::{GuardContext, GuardDecision, MAX_APP_GUARDS, MAX_ROUTE_GUARDS, RouteGuard};

#[derive(Clone)]
pub(super) struct Registration {
    scope: Option<Vec<Segment>>,
    guard: RouteGuard,
}
pub(super) struct Outcome {
    pub active: bool,
    pub private: bool,
}
fn segments(pattern: &str) -> Result<Vec<Segment>, AppError> {
    PageRoute::new("guard", pattern).map_err(|_| AppError::InvalidGuard)?;
    if pattern != "/" && pattern.ends_with('/') {
        return Err(AppError::InvalidGuard);
    }
    if pattern == "/" {
        return Ok(Vec::new());
    }
    let (mut parts, _) =
        crate::routing::pattern_segments(pattern).map_err(|_| AppError::InvalidGuard)?;
    for part in &mut parts {
        if let Segment::Literal(value) = part {
            *value = crate::url::decode(value)
                .and_then(|value| {
                    crate::url::valid_segment(&value)?;
                    Ok(value)
                })
                .map_err(|_| AppError::InvalidGuard)?;
        }
    }
    Ok(parts)
}
fn may_match(scope: &[Segment], pattern: &str) -> bool {
    let trimmed = pattern.trim_end_matches('/');
    let Ok(parts) = segments(if trimmed.is_empty() { "/" } else { trimmed }) else {
        return false;
    };
    compatible(scope, &parts)
}
fn compatible(scope: &[Segment], parts: &[Segment]) -> bool {
    scope.len() <= parts.len()
        && scope
            .iter()
            .zip(parts)
            .all(|(left, right)| match (left, right) {
                (Segment::Literal(a), Segment::Literal(b)) => a == b,
                _ => true,
            })
}
fn matches(scope: &[Segment], request: &Request) -> Result<bool, StatusCode> {
    if scope.is_empty() {
        return Ok(true);
    }
    let parts = request.path()[1..].split('/').collect::<Vec<_>>();
    if scope.len() > parts.len() {
        return Ok(false);
    }
    for (part, raw) in scope.iter().zip(parts) {
        let value = crate::url::decode(raw)
            .and_then(|value| {
                crate::url::valid_segment(&value)?;
                Ok(value)
            })
            .map_err(|_| StatusCode::BadRequest)?;
        if matches!(part,Segment::Literal(expected) if expected!=&value) {
            return Ok(false);
        }
    }
    Ok(true)
}
impl App {
    /// Appends a native guard for application pages, endpoints and unmatched page requests.
    /// Embedded/runtime assets and manually added Router handlers are outside this scope.
    pub fn guard(&mut self, guard: RouteGuard) -> Result<(), AppError> {
        self.add_guard(None, guard)
    }
    /// Appends an inherited segment-prefix guard. Literals compare decoded segments.
    pub fn guard_branch(&mut self, pattern: &str, guard: RouteGuard) -> Result<(), AppError> {
        self.add_guard(Some(segments(pattern)?), guard)
    }
    fn add_guard(
        &mut self,
        scope: Option<Vec<Segment>>,
        guard: RouteGuard,
    ) -> Result<(), AppError> {
        let mut candidate = self.clone();
        candidate.guards.push(Registration { scope, guard });
        candidate
            .guards
            .sort_by_key(|entry| entry.scope.as_ref().map_or(0, |scope| scope.len() + 1));
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Appends a GET/HEAD guard to a named native or client page.
    /// POST handlers at the same URL need an endpoint or inherited guard.
    pub fn page_guard(&mut self, name: &str, guard: RouteGuard) -> Result<(), AppError> {
        let mut candidate = self.clone();
        candidate
            .pages
            .iter_mut()
            .find(|page| page.route.name() == name)
            .ok_or(AppError::UnknownRoute)?
            .guards
            .push(guard);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub(super) fn guarded_pattern(&self, pattern: &str, local: &[RouteGuard]) -> bool {
        !local.is_empty()
            || self.guards.iter().any(|entry| {
                entry
                    .scope
                    .as_ref()
                    .is_none_or(|scope| may_match(scope, pattern))
            })
    }
    pub(super) fn validate_guards(&self) -> Result<(), AppError> {
        let total = self.guards.len()
            + self
                .pages
                .iter()
                .map(|page| page.guards.len())
                .sum::<usize>()
            + self
                .endpoints
                .iter()
                .map(|endpoint| endpoint.declaration.guards.len())
                .sum::<usize>();
        if total > MAX_APP_GUARDS {
            return Err(AppError::InvalidGuard);
        }
        for target in &self.guards {
            let target = target.scope.as_deref().unwrap_or(&[]);
            let count = self
                .guards
                .iter()
                .filter(|entry| {
                    entry
                        .scope
                        .as_ref()
                        .is_none_or(|scope| compatible(scope, target))
                })
                .count();
            if count > MAX_ROUTE_GUARDS {
                return Err(AppError::InvalidGuard);
            }
        }
        for (pattern, local) in self
            .pages
            .iter()
            .map(|page| (page.route.pattern(), page.guards.len()))
            .chain(
                self.endpoints
                    .iter()
                    .map(|entry| (entry.pattern.as_str(), entry.declaration.guards.len())),
            )
        {
            let count = local
                + self
                    .guards
                    .iter()
                    .filter(|entry| {
                        entry
                            .scope
                            .as_ref()
                            .is_none_or(|scope| may_match(scope, pattern))
                    })
                    .count();
            if count > MAX_ROUTE_GUARDS {
                return Err(AppError::InvalidGuard);
            }
        }
        Ok(())
    }
    pub(super) fn evaluate_guards(
        &self,
        request: &Request,
        pattern: Option<&str>,
        page: Option<&str>,
        local: &[RouteGuard],
    ) -> Result<Outcome, StatusCode> {
        let mut outcome = Outcome {
            active: false,
            private: false,
        };
        let mut count = 0;
        for guard in self
            .guards
            .iter()
            .filter_map(|entry| match &entry.scope {
                None => Some(Ok(&entry.guard)),
                Some(scope) => match matches(scope, request) {
                    Ok(true) => Some(Ok(&entry.guard)),
                    Ok(false) => None,
                    Err(status) => Some(Err(status)),
                },
            })
            .chain(local.iter().map(Ok))
        {
            let guard = guard?;
            count += 1;
            if count > MAX_ROUTE_GUARDS {
                return Err(StatusCode::InternalServerError);
            }
            outcome.active = true;
            let context = GuardContext {
                request,
                pattern,
                page,
                private: outcome.private,
            };
            let decision = catch_unwind(AssertUnwindSafe(|| (guard.0)(&context)))
                .map_err(|_| StatusCode::InternalServerError)?;
            match decision {
                GuardDecision::Allow => {}
                GuardDecision::Private => outcome.private = true,
                GuardDecision::Forbidden => return Err(StatusCode::Forbidden),
                GuardDecision::Unavailable => return Err(StatusCode::ServiceUnavailable),
            }
        }
        Ok(outcome)
    }
    pub(super) fn guard_failure(
        &self,
        request: &Request,
        status: StatusCode,
        client: bool,
    ) -> Response {
        if client {
            return self.client_guard_failure(request, status);
        }
        let title =
            crate::guards::guard_title(status.as_u16().into()).unwrap_or("Invalid page request");
        Response::html(
            Document::new(
                title,
                crate::element(crate::Tag::Main)
                    .child(crate::element(crate::Tag::H1).child(title))
                    .render(),
            )
            .metadata(Metadata::new().for_error())
            .render(),
        )
        .with_status(status)
    }
    pub(super) fn guarded_endpoint(
        &self,
        request: &Request,
        pattern: &str,
        local: &[RouteGuard],
        handler: impl FnOnce(&Request) -> Response,
    ) -> Response {
        match self.evaluate_guards(request, Some(pattern), None, local) {
            Err(status) => self.guard_failure(request, status, false),
            Ok(outcome) => {
                let mut private;
                let request = if outcome.private {
                    private = request.clone();
                    private.guard_private = true;
                    &private
                } else {
                    request
                };
                let response = handler(request);
                if outcome.active {
                    response.guard_no_store()
                } else {
                    response
                }
            }
        }
    }
}
