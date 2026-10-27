//! Shared page declarations. Native loaders capture the application's backend.
use crate::components;
use ruvane::*;

pub fn application() -> App {
    with_loader(|_| Err(PageError::InternalServerError))
}
pub fn with_loader(
    load: impl Fn(&PageContext<'_>) -> Result<ClientPageData, PageError> + Send + Sync + 'static,
) -> App {
    let mut app = App::new();
    app.components(components::create_component, components::ERROR)
        .unwrap();
    let load = std::sync::Arc::new(load);
    for (name, path, component) in [
        ("tickets", "/tickets", components::LIST),
        ("new-ticket", "/tickets/new", components::EDITOR),
        ("ticket", "/tickets/{id}", components::EDITOR),
    ] {
        let loader = load.clone();
        app.client_page(
            PageRoute::new(name, path).unwrap(),
            component,
            if name == "ticket" { ticket_state } else { zero },
            move |context| loader(context),
        )
        .unwrap();
    }
    app
}
fn zero(_: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(0))
}
fn ticket_state(route: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(
        crate::model::id(route.param("id").ok_or(PageError::BadRequest)?)
            .ok_or(PageError::BadRequest)?,
    ))
}
pub fn url(path: &str) -> InternalUrl {
    InternalUrl::from_target(path).unwrap()
}
pub fn shell(children: Html) -> Html {
    element(Tag::Main)
        .child(
            element(Tag::Header)
                .child(element(Tag::P).child("RUVANE / LOCAL WORKSPACE"))
                .child(element(Tag::H1).child("Ruvane Desk"))
                .child(element(Tag::P).child("Project Ruvane · alpha.61"))
                .child(
                    element(Tag::Nav)
                        .child(link(&url("/tickets"), "Tickets"))
                        .child(" · ")
                        .child(link(&url("/tickets/new"), "New ticket"))
                        .child(" · ")
                        .child(link(&url("/"), "Local session")),
                ),
        )
        .child(children)
        .child(
            element(Tag::Footer)
                .child("Local pilot · data stays on this computer · no account authentication"),
        )
        .render()
}
