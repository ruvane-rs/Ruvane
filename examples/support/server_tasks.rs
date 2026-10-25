//! Native forms with shared, bounded server state.
use ruvane::{
    Attribute, Document, FormError, Html, InternalUrl, MediaType, PostForm, Request, Response,
    Router, StatusCode, Tag, element,
};
use std::sync::{Arc, Mutex};

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
        "Ruvane alpha.16 — Server tasks",
        element(Tag::Main)
            .child(element(Tag::H1).child("Ruvane / Server tasks"))
            .child(
                element(Tag::P).child("Native forms, validated in Rust. No JavaScript required."),
            )
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
            )
            .child(
                element(Tag::P)
                    .child("Tasks are shared by this server and stay in memory until it stops."),
            ),
    )
    .stylesheet(InternalUrl::new("/assets/forms.css").unwrap())
    .render())
}

fn render(state: &State, title: &str, notes: &str, error: &str, invalid: Option<&str>) -> Response {
    match page(&state.tasks, title, notes, error, invalid) {
        Ok(html) => Response::html(html).with_status(if error.is_empty() {
            StatusCode::Ok
        } else {
            StatusCode::BadRequest
        }),
        Err(_) => failure(StatusCode::InternalServerError),
    }
}

fn add(state: &Mutex<State>, request: &Request) -> Response {
    let form = request.form_data().unwrap();
    if !form.has_only(&["title", "notes"]) {
        return failure(StatusCode::BadRequest);
    }
    let title = form.get("title").unwrap_or("").trim();
    let notes = form.get("notes").unwrap_or("");
    let Ok(mut state) = state.lock() else {
        return failure(StatusCode::InternalServerError);
    };
    if title.chars().any(char::is_control) {
        return render(
            &state,
            "",
            notes,
            "Task names must be a single line.",
            Some("title"),
        );
    }
    let error = if title.is_empty() {
        Some(("Enter a task name.", Some("title")))
    } else if title.len() > 80 {
        Some(("Task name must be at most 80 UTF-8 bytes.", Some("title")))
    } else if notes.len() > 160 {
        Some(("Notes must be at most 160 UTF-8 bytes.", Some("notes")))
    } else if state.tasks.len() == MAX_TASKS {
        Some(("Task limit reached.", None))
    } else {
        None
    };
    if let Some((error, invalid)) = error {
        return render(&state, title, notes, error, invalid);
    }
    let Some(id) = state.next.checked_add(1) else {
        return failure(StatusCode::InternalServerError);
    };
    state.next = id;
    state.tasks.push(Task {
        id,
        title: title.to_owned(),
        notes: notes.trim().to_owned(),
        done: false,
    });
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
    router.post_form("/tasks/{id}/remove", move |request| {
        update(&state, request, true)
    })?;
    router.asset(
        "/assets/forms.css",
        include_bytes!("../../web/browser.css"),
        MediaType::Css,
    )?;
    Ok(router)
}
