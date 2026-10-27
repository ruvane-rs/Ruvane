use crate::{
    ActionData, BrowserTag, Component, ComponentError, ComponentProps, InitialState, Request, View,
};
use std::sync::Arc;

/// Maximum registered application, branch and local guards in an App.
pub const MAX_APP_GUARDS: usize = 64;
/// Maximum guards evaluated for one route, including inherited guards.
pub const MAX_ROUTE_GUARDS: usize = 16;
/// Reserved read-only component used for root guard failures.
pub const GUARD_COMPONENT_ID: u32 = u32::MAX;

/// A server decision. Private is sticky across the remaining guards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuardDecision {
    /// Continue. This does not override an earlier Private decision.
    Allow,
    /// Continue with cache lookup, storage and refreshing disabled for this request.
    Private,
    /// Stop before handlers/loaders and return 403 without caching.
    Forbidden,
    /// Stop before handlers/loaders and return 503 without caching.
    Unavailable,
    /// Stop before handlers/loaders and return 429 with an explicit retry delay.
    RateLimited(crate::RetryAfter),
}
/// Read-only validated request and selected application route.
/// Headers are inputs to application authentication, not verified identity claims.
pub struct GuardContext<'a> {
    pub(crate) request: &'a Request,
    pub(crate) pattern: Option<&'a str>,
    pub(crate) page: Option<&'a str>,
    pub(crate) private: bool,
}
impl GuardContext<'_> {
    /// Includes decoded captures/query/form fields after routing and POST checks.
    pub fn request(&self) -> &Request {
        self.request
    }
    /// Returns the selected pattern, or None for an unmatched page request.
    pub fn pattern(&self) -> Option<&str> {
        self.pattern
    }
    /// Returns the page name. Endpoints and unmatched requests have no page name.
    pub fn page_name(&self) -> Option<&str> {
        self.page
    }
    /// Reports credentials or an earlier Private decision; this is not authentication.
    pub fn cache_bypass(&self) -> bool {
        self.private || self.request.cache_bypass()
    }
}
/// A bounded, read-only server callback. Registration never executes it.
#[derive(Clone)]
pub struct RouteGuard(pub(crate) Arc<dyn Fn(&GuardContext<'_>) -> GuardDecision + Send + Sync>);
impl RouteGuard {
    /// Creates a native guard. Use native cfg or guard.rs to exclude authentication code from Wasm.
    #[cfg(not(target_family = "wasm"))]
    pub fn new(check: impl Fn(&GuardContext<'_>) -> GuardDecision + Send + Sync + 'static) -> Self {
        Self(Arc::new(check))
    }
}
pub(crate) fn guard_title(code: u32) -> Option<&'static str> {
    match code {
        403 => Some("Access denied"),
        429 => Some("Too many requests"),
        503 => Some("Service unavailable"),
        500 => Some("Page unavailable"),
        _ => None,
    }
}
struct GuardMessage(u32);
impl Component for GuardMessage {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::InvalidProps)
        }
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let root = view.element(0, BrowserTag::Section)?;
        view.leaf(
            root,
            BrowserTag::Heading,
            guard_title(self.0).ok_or(ComponentError::InvalidProps)?,
        )?;
        let paragraph = view.element(root, BrowserTag::Paragraph)?;
        view.leaf(
            paragraph,
            BrowserTag::Span,
            match self.0 {
                403 => "You do not have access to this page.",
                429 => "Please wait before trying again.",
                503 => "This page is temporarily unavailable.",
                _ => "The request could not be completed.",
            },
        )?;
        Ok(view)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
}
/// First-party native/Wasm factory hook for the reserved guard component.
#[doc(hidden)]
pub fn guard_component(
    props: ComponentProps,
) -> Option<Result<Box<dyn Component>, ComponentError>> {
    if props.component() != GUARD_COMPONENT_ID {
        return None;
    }
    Some(match props.state() {
        InitialState::Number(code) if guard_title(code).is_some() => {
            Ok(Box::new(GuardMessage(code)))
        }
        _ => Err(ComponentError::InvalidProps),
    })
}
