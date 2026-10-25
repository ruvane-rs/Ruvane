//! SSR and typed navigation representations of the same URLs.
use super::{State, snapshot};
use ruvane::{
    ActionData, Attribute, ComponentProps, Document, Html, InitialState, InternalUrl, MediaType,
    NavigationPage, PostForm, Request, Response, Router, StatusCode, Tag, element, module_script,
    navigation_link,
};
use std::sync::{Arc, Mutex};
#[path = "widgets.rs"]
#[allow(dead_code)]
mod widgets;
use widgets::navigation_pages::{self as pages, DETAIL, ERROR};

fn descriptor(state: &State, request: &Request) -> Result<NavigationPage, ()> {
    let mut target = request.path().to_owned();
    if let Some(query) = request.query() {
        target.push('?');
        target.push_str(query);
    }
    let url = InternalUrl::from_target(&target).map_err(|_| ())?;
    let mut props = pages::props(&url);
    if props.component() == DETAIL
        && matches!(props.state(), InitialState::Number(id) if !state.tasks.iter().any(|t| t.id == id as u64))
    {
        props = ComponentProps::new(ERROR, InitialState::Number(404)).unwrap();
    }
    let (status, title, data) = match (props.component(), props.state()) {
        (ERROR, InitialState::Number(400)) => (
            StatusCode::BadRequest,
            "Invalid page request",
            ActionData::default(),
        ),
        (ERROR, _) => (
            StatusCode::NotFound,
            "Page not found",
            ActionData::default(),
        ),
        (DETAIL, InitialState::Number(id)) => (
            StatusCode::Ok,
            state
                .tasks
                .iter()
                .find(|t| t.id == id as u64)
                .unwrap()
                .title
                .as_str(),
            snapshot(state).unwrap().data().clone(),
        ),
        (pages::TASKS, _) => (
            StatusCode::Ok,
            "Tasks",
            snapshot(state).unwrap().data().clone(),
        ),
        _ => (
            StatusCode::Ok,
            "Home",
            snapshot(state).unwrap().data().clone(),
        ),
    };
    NavigationPage::new(url, &format!("{title} / Ruvane"), props, data, status).map_err(|_| ())
}
fn native_forms(state: &State, props: ComponentProps) -> Html {
    let mut body = element(Tag::Section)
        .attr(Attribute::Id, "native-forms")
        .child(element(Tag::H2).child("Native forms"));
    if props.component() == pages::TASKS {
        body = body.child(
            PostForm::new(InternalUrl::new("/tasks").unwrap())
                .unwrap()
                .text("title", "Task name", "")
                .unwrap()
                .textarea("notes", "Notes", "")
                .unwrap()
                .submit_label("Add task")
                .unwrap(),
        );
    } else if props.component() != DETAIL {
        return Html::default();
    }
    for task in &state.tasks {
        if matches!(props.state(), InitialState::Number(id) if props.component() == DETAIL && id as u64 != task.id)
        {
            continue;
        }
        let path = InternalUrl::new("/tasks")
            .unwrap()
            .segment(&task.id.to_string())
            .unwrap();
        body = body
            .child(element(Tag::H3).child(task.title.as_str()))
            .child(
                PostForm::new(path.clone().segment("status").unwrap())
                    .unwrap()
                    .checkbox("done", "Completed", task.done)
                    .unwrap()
                    .submit_label("Save status")
                    .unwrap(),
            )
            .child(
                PostForm::new(path.segment("remove").unwrap())
                    .unwrap()
                    .submit_label("Remove task")
                    .unwrap(),
            );
    }
    body.render()
}
fn render(state: &State, page: &NavigationPage, native: bool) -> Response {
    let mut component = widgets::create(page.props()).unwrap();
    component.page_data(page.data().clone()).unwrap();
    let props = ComponentProps::new(widgets::COUNTER, InitialState::Number(5)).unwrap();
    let counter = widgets::create(props)
        .unwrap()
        .view()
        .unwrap()
        .hydratable(props)
        .unwrap();
    let body = element(Tag::Main)
        .child(
            element(Tag::Header)
                .child(element(Tag::H1).child("Ruvane / Shared navigation"))
                .child(element(Tag::P).child("Rust pages, a persistent layout, and native links."))
                .child(navigation_link(
                    &pages::routes().url("home", &[]).unwrap(),
                    "Home",
                ))
                .child(" · ")
                .child(navigation_link(
                    &pages::routes().url("tasks", &[]).unwrap(),
                    "Tasks",
                )),
        )
        .child(
            element(Tag::Div)
                .attr(Attribute::Id, "layout-counter")
                .child(counter),
        )
        .child(
            element(Tag::P)
                .attr(Attribute::Id, "navigation-status")
                .child("Native links and forms are ready."),
        )
        .child(
            element(Tag::Div).attr(Attribute::Id, "page-outlet").child(
                element(Tag::Div)
                    .attr(Attribute::Id, "ssr-page")
                    .child(component.view().unwrap().hydratable(page.props()).unwrap())
                    .child(native_forms(state, page.props())),
            ),
        )
        .child(if native {
            Html::default()
        } else {
            module_script(&InternalUrl::new("/assets/navigation-demo.js").unwrap())
        });
    let html = Document::new(page.title(), body)
        .stylesheet(InternalUrl::new("/assets/forms.css").unwrap())
        .render();
    if native {
        Response::html(html).with_status(page.status())
    } else {
        Response::interactive_html(html).with_status(page.status())
    }
}
pub(super) fn register(
    router: &mut Router,
    state: Arc<Mutex<State>>,
) -> Result<(), Box<dyn std::error::Error>> {
    for path in ["/app", "/app/tasks", "/app/tasks/{id}", "/app/{unknown}"] {
        let state = Arc::clone(&state);
        router.get(path, move |request| {
            if request.query_params().get("delay") == Some("yes") {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let Ok(state) = state.lock() else {
                return super::failure(StatusCode::InternalServerError);
            };
            let Ok(page) = descriptor(&state, request) else {
                return super::failure(StatusCode::BadRequest);
            };
            if request.header("accept") == Some(ruvane::NAVIGATION_MEDIA_TYPE) {
                match request.query_params().get("fixture") {
                    Some("mime") => Response::text("unexpected"),
                    Some("large") => Response::bytes(vec![0; 65537], MediaType::Navigation),
                    Some("redirect") => Response::see_other(InternalUrl::new("/app").unwrap()),
                    Some("broken") => Response::bytes(b"RVP1".to_vec(), MediaType::Navigation),
                    Some("status") => {
                        Response::navigation(&page).with_status(StatusCode::BadRequest)
                    }
                    _ => Response::navigation(&page),
                }
            } else {
                render(
                    &state,
                    &page,
                    request.query_params().get("native") == Some("yes"),
                )
            }
        })?;
    }
    router.get("/navigation-tests", |_| {
        Response::interactive_html(
            Document::new(
                "Navigation tests",
                element(Tag::Main)
                    .child(element(Tag::H1).child("Ruvane / Navigation tests"))
                    .child(
                        element(Tag::P)
                            .attr(Attribute::Id, "status")
                            .child("Running…"),
                    )
                    .child(element(Tag::Pre).attr(Attribute::Id, "report"))
                    .child(
                        element(Tag::Section)
                            .attr(Attribute::Id, "navigation-scope")
                            .child(element(Tag::Div).attr(Attribute::Id, "navigation-layout"))
                            .child(element(Tag::P).attr(Attribute::Id, "navigation-message"))
                            .child(element(Tag::Div).attr(Attribute::Id, "navigation-outlet")),
                    )
                    .child(element(Tag::Pre).child("Scroll fixture\n".repeat(150)))
                    .child(module_script(
                        &InternalUrl::new("/assets/navigation-tests.js").unwrap(),
                    )),
            )
            .render(),
        )
    })?;
    router.asset(
        "/assets/navigation-tests.js",
        include_bytes!("../../web/navigation-tests.mjs"),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/bridge.mjs",
        ruvane::BROWSER_BRIDGE_MODULE.as_bytes(),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/navigation.mjs",
        ruvane::BROWSER_NAVIGATION_MODULE.as_bytes(),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/navigation-demo.js",
        include_bytes!("../../web/navigation-demo.mjs"),
        MediaType::JavaScript,
    )?;
    Ok(())
}
