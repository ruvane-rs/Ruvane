#![forbid(unsafe_code)]
//! Ruvane Desk: a local ticket application built with public Ruvane APIs.
pub mod attachments;
pub mod components;
pub mod model;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod pages;
pub mod spaces;

/// Build identity shared by the server and its Wasm module.
pub const REVISION: u64 = ruvane::application_revision(env!("RUVANE_APPLICATION_REVISION"));
/// Checks an incoming navigation page against this application's declarations.
pub fn validate_page(page: &ruvane::NavigationPage) -> bool {
    pages::application().validate_navigation(page)
}
