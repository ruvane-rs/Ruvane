//! Shared page declarations and the application's generic shell.
use crate::{components, model};
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
    for (name, path, component, state) in [
        (
            "workspaces",
            "/workspaces",
            components::WORKSPACES,
            zero as fn(&NavigationMatch) -> Result<InitialState, PageError>,
        ),
        (
            "tickets",
            "/w/{workspace}/tickets",
            components::LIST,
            space_state,
        ),
        (
            "new-ticket",
            "/w/{workspace}/new-ticket",
            components::EDITOR,
            new_state,
        ),
        (
            "ticket",
            "/w/{workspace}/tickets/{id}",
            components::EDITOR,
            ticket_state,
        ),
        (
            "members",
            "/w/{workspace}/members",
            components::MEMBERS,
            space_state,
        ),
        (
            "files",
            "/w/{workspace}/tickets/{id}/files",
            components::FILES,
            ticket_state,
        ),
    ] {
        let loader = load.clone();
        app.client_page(
            PageRoute::new(name, path).unwrap(),
            component,
            state,
            move |context| loader(context),
        )
        .unwrap();
    }
    app
}
fn zero(_: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(0))
}
fn workspace(route: &NavigationMatch) -> Result<u32, PageError> {
    model::workspace_id(route.param("workspace").ok_or(PageError::BadRequest)?)
        .ok_or(PageError::BadRequest)
}
fn space_state(route: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(workspace(route)?))
}
fn new_state(route: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(model::packed(workspace(route)?, 0)))
}
fn ticket_state(route: &NavigationMatch) -> Result<InitialState, PageError> {
    Ok(InitialState::Number(model::packed(
        workspace(route)?,
        model::id(route.param("id").ok_or(PageError::BadRequest)?).ok_or(PageError::BadRequest)?,
    )))
}
pub fn url(path: &str) -> InternalUrl {
    InternalUrl::from_target(path).unwrap()
}
pub fn shell(children: Html) -> Html {
    element(Tag::Main)
        .child(
            element(Tag::Header)
                .child(element(Tag::P).child("RUVANE / LOCAL WORKSPACES"))
                .child(element(Tag::H1).child("Ruvane Desk"))
                .child(element(Tag::P).child("Spaces, tickets and attachments · alpha.62"))
                .child(
                    element(Tag::Nav)
                        .child(link(&url("/workspaces"), "Workspaces"))
                        .child(" · ")
                        .child(link(&url("/"), "Local test profile")),
                ),
        )
        .child(children)
        .child(
            element(Tag::Footer)
                .child("Local pilot · selectable test profiles · no account authentication"),
        )
        .render()
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_routes_register_without_ambiguity() {
        let _ = super::application();
    }
}
