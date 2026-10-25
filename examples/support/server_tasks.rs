//! Native forms with shared, bounded server state.
use ruvane::{
    ActionData, ActionError, ActionReply, ActionValue, Attribute, BROWSER_BRIDGE_MODULE, Document,
    FormData, FormError, Html, InternalUrl, MediaType, PostForm, Request, Response, Router,
    StatusCode, Tag, element, link, module_script,
};
use std::sync::{Arc, Mutex};
#[path = "action_fixtures.rs"]
mod action_fixtures;

#[path = "navigation_app.rs"]
mod navigation_app;

const MAX_TASKS: usize = 32;
#[derive(Clone)]
struct Task {
    id: u64,
    title: String,
    notes: String,
    done: bool,
}
#[derive(Default)]
struct State {
    tasks: Vec<Task>,
    next: u64,
}

fn destination() -> InternalUrl {
    InternalUrl::new("/tasks").unwrap()
}
fn failure(status: StatusCode) -> Response {
    Response::text(format!("{}\n", status.reason())).with_status(status)
}

fn page(
    tasks: &[Task],
    title: &str,
    notes: &str,
    error: &str,
    invalid: Option<&str>,
    interactive: bool,
) -> Result<Html, FormError> {
    let mut form = PostForm::new(destination())?
        .text("title", "Task name", title)?
        .textarea("notes", "Notes", notes)?
        .submit_label("Add task")?;
    if let Some(name) = invalid {
        form = form.invalid(name)?;
    }
    let mut list = element(Tag::Ul);
    for task in tasks {
        let base = destination().segment(&task.id.to_string()).unwrap();
        let status = PostForm::new(base.clone().segment("status").unwrap())?
            .checkbox("done", "Completed", task.done)?
            .submit_label("Save status")?;
        let remove = PostForm::new(base.segment("remove").unwrap())?.submit_label("Remove task")?;
        list = list.child(
            element(Tag::Li)
                .child(element(Tag::H3).child(task.title.as_str()))
                .child(element(Tag::P).child(if task.done { "Completed" } else { "Open" }))
                .child(element(Tag::Pre).child(task.notes.as_str()))
                .child(status)
                .child(remove),
        );
    }
    Ok(Document::new(
        "Ruvane alpha.22 — Server tasks",
        element(Tag::Main)
            .child(element(Tag::H1).child("Ruvane / Server tasks"))
            .child(
                element(Tag::P).child("Native forms, validated in Rust. No JavaScript required."),
            )
            .child(element(Tag::Div).attr(Attribute::Id, "remote-root"))
            .child(
                element(Tag::P)
                    .attr(Attribute::Id, "action-status")
                    .child("Native forms are ready."),
            )
            .child(link(&destination(), "Use native forms"))
            .child(
                element(Tag::Div)
                    .attr(Attribute::Id, "native-fallback")
                    .child(
                        element(Tag::Section)
                            .child(element(Tag::H2).child("Add a task"))
                            .child(element(Tag::P).attr(Attribute::Id, "error").child(error))
                            .child(form),
                    )
                    .child(
                        element(Tag::Section)
                            .child(element(Tag::H2).child(format!("Tasks ({})", tasks.len())))
                            .child(if tasks.is_empty() {
                                element(Tag::P).child("No tasks yet.").render()
                            } else {
                                list.render()
                            }),
                    ),
            )
            .child(
                element(Tag::P)
                    .child("Tasks are shared by this server and stay in memory until it stops."),
            )
            .child(if interactive {
                module_script(&InternalUrl::new("/assets/actions-demo.js").unwrap())
            } else {
                Html::default()
            }),
    )
    .stylesheet(InternalUrl::new("/assets/forms.css").unwrap())
    .render())
}

fn render(state: &State, title: &str, notes: &str, error: &str, invalid: Option<&str>) -> Response {
    match page(&state.tasks, title, notes, error, invalid, false) {
        Ok(html) => Response::html(html).with_status(if error.is_empty() {
            StatusCode::Ok
        } else {
            StatusCode::BadRequest
        }),
        Err(_) => failure(StatusCode::InternalServerError),
    }
}

fn create_task(state: &mut State, form: &FormData) -> Result<(), (&'static str, &'static str)> {
    if !form.has_only(&["title", "notes"]) {
        return Err(("Unexpected fields.", ""));
    }
    let title = form.get("title").unwrap_or("").trim();
    let notes = form.get("notes").unwrap_or("");
    if title.chars().any(char::is_control) {
        return Err(("Task names must be a single line.", "title"));
    }
    if title.is_empty() {
        return Err(("Enter a task name.", "title"));
    }
    if title.len() > 80 {
        return Err(("Task name must be at most 80 UTF-8 bytes.", "title"));
    }
    if notes.len() > 160 {
        return Err(("Notes must be at most 160 UTF-8 bytes.", "notes"));
    }
    if state.tasks.len() == MAX_TASKS {
        return Err(("Task limit reached.", ""));
    }
    let id = state
        .next
        .checked_add(1)
        .filter(|id| *id <= u32::MAX as u64)
        .ok_or(("Task ID limit reached.", ""))?;
    state.next = id;
    state.tasks.push(Task {
        id,
        title: title.into(),
        notes: notes.trim().into(),
        done: false,
    });
    Ok(())
}

fn snapshot(state: &State) -> Result<ActionReply, ActionError> {
    let mut data = ActionData::default();
    data.insert("count", ActionValue::Number(state.tasks.len() as u64))?;
    for (index, task) in state.tasks.iter().enumerate() {
        data.insert(&format!("id_{index}"), ActionValue::Number(task.id))?;
        data.insert(
            &format!("title_{index}"),
            ActionValue::Text(task.title.clone()),
        )?;
        data.insert(
            &format!("notes_{index}"),
            ActionValue::Text(task.notes.clone()),
        )?;
        data.insert(&format!("done_{index}"), ActionValue::Boolean(task.done))?;
    }
    Ok(ActionReply::success(data))
}
fn validation(message: &str, field: &str) -> Result<ActionReply, ActionError> {
    let mut data = ActionData::default();
    data.insert("message", ActionValue::Text(message.into()))?;
    data.insert("field", ActionValue::Text(field.into()))?;
    Ok(ActionReply::validation(data))
}
fn action(state: &Mutex<State>, request: &Request, kind: u32) -> Result<ActionReply, ActionError> {
    let fields = request.form_data().unwrap();
    let mut state = state.lock().map_err(|_| ActionError::InvalidData)?;
    match kind {
        1 if !fields.is_empty() => return validation("Unexpected fields.", ""),
        2 => {
            if let Err((message, field)) = create_task(&mut state, fields) {
                return validation(message, field);
            }
        }
        3 | 4 => {
            if !fields.has_only(if kind == 3 { &["id", "done"] } else { &["id"] }) {
                return validation("Unexpected fields.", "");
            }
            let Some(raw) = fields.get("id") else {
                return validation("Invalid task ID.", "");
            };
            let Ok(id) = raw.parse::<u64>() else {
                return validation("Invalid task ID.", "");
            };
            if raw != id.to_string() {
                return validation("Invalid task ID.", "");
            }
            let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
                return validation("Task not found.", "");
            };
            let Ok(done) = fields.checkbox("done") else {
                return validation("Invalid checkbox.", "");
            };
            if kind == 4 {
                state.tasks.remove(index);
            } else {
                state.tasks[index].done = done;
            }
        }
        _ => {}
    }
    snapshot(&state)
}

fn add(state: &Mutex<State>, request: &Request) -> Response {
    let form = request.form_data().unwrap();
    if !form.has_only(&["title", "notes"]) {
        return failure(StatusCode::BadRequest);
    }
    let Ok(mut state) = state.lock() else {
        return failure(StatusCode::InternalServerError);
    };
    if let Err((message, field)) = create_task(&mut state, form) {
        let title = form.get("title").unwrap_or("").trim();
        let title = if title.chars().any(char::is_control) {
            ""
        } else {
            title
        };
        return render(
            &state,
            title,
            form.get("notes").unwrap_or(""),
            message,
            if field.is_empty() { None } else { Some(field) },
        );
    }
    Response::see_other(destination())
}

fn update(state: &Mutex<State>, request: &Request, remove: bool) -> Response {
    let form = request.form_data().unwrap();
    let Ok(id) = request.param_as::<u64>("id") else {
        return failure(StatusCode::BadRequest);
    };
    if request.param("id") != Some(id.to_string().as_str())
        || !form.has_only(if remove { &[] } else { &["done"] })
    {
        return failure(StatusCode::BadRequest);
    }
    let Ok(done) = form.checkbox("done") else {
        return failure(StatusCode::BadRequest);
    };
    let Ok(mut state) = state.lock() else {
        return failure(StatusCode::InternalServerError);
    };
    let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
        return failure(StatusCode::NotFound);
    };
    if remove {
        state.tasks.remove(index);
    } else {
        state.tasks[index].done = done;
    }
    Response::see_other(destination())
}

/// Builds a fresh task application with no disk persistence.
pub fn router() -> Result<Router, Box<dyn std::error::Error>> {
    let state = Arc::new(Mutex::new(State::default()));
    let mut router = Router::new();
    navigation_app::register(&mut router, Arc::clone(&state))?;
    action_fixtures::register(&mut router)?;
    router.get("/", |_| Response::see_other(destination()))?;
    let current = Arc::clone(&state);
    router.get("/tasks", move |_| match current.lock() {
        Ok(state) => render(&state, "", "", "", None),
        Err(_) => failure(StatusCode::InternalServerError),
    })?;
    let current = Arc::clone(&state);
    router.post_form("/tasks", move |request| add(&current, request))?;
    let current = Arc::clone(&state);
    router.post_form("/tasks/{id}/status", move |request| {
        update(&current, request, false)
    })?;
    let current = Arc::clone(&state);
    router.post_form("/tasks/{id}/remove", move |request| {
        update(&current, request, true)
    })?;
    let current = Arc::clone(&state);
    router.get("/interactive", move |_| match current.lock() {
        Ok(state) => match page(&state.tasks, "", "", "", None, true) {
            Ok(html) => Response::interactive_html(html),
            Err(_) => failure(StatusCode::InternalServerError),
        },
        Err(_) => failure(StatusCode::InternalServerError),
    })?;
    for (path, kind) in [("list", 1), ("create", 2), ("status", 3), ("remove", 4)] {
        let current = Arc::clone(&state);
        router.post_action(format!("/actions/tasks/{path}"), move |request| {
            action(&current, request, kind)
        })?;
    }
    router.asset(
        "/assets/ruvane.js",
        BROWSER_BRIDGE_MODULE.as_bytes(),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/actions-demo.js",
        include_bytes!("../../web/actions-demo.mjs"),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/forms.css",
        include_bytes!("../../web/browser.css"),
        MediaType::Css,
    )?;
    Ok(router)
}
