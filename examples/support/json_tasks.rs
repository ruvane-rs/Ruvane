use ruvane::{FileEndpoint, JsonError, JsonValue, Request, Response, StatusCode};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const MAX_TASKS: usize = 32;
const MAX_TITLE_BYTES: usize = 256;
struct Task {
    id: u64,
    title: String,
}
#[derive(Default)]
struct State {
    next: u64,
    tasks: Vec<Task>,
}
#[derive(Default)]
pub struct Tasks {
    state: Mutex<State>,
    calls: AtomicUsize,
    writes: AtomicUsize,
}

pub fn error(status: StatusCode) -> Response {
    let value = JsonValue::object(vec![(
        "error".into(),
        JsonValue::string(status.reason()).unwrap(),
    )])
    .unwrap();
    Response::json(&value).with_status(status)
}
fn value(task: &Task) -> Result<JsonValue, JsonError> {
    JsonValue::object(vec![
        ("id".into(), task.id.into()),
        ("title".into(), JsonValue::string(task.title.clone())?),
    ])
}
impl Tasks {
    pub fn read(&self) -> Response {
        let Ok(state) = self.state.lock() else {
            return error(StatusCode::ServiceUnavailable);
        };
        let result = state
            .tasks
            .iter()
            .map(value)
            .collect::<Result<Vec<_>, _>>()
            .and_then(JsonValue::array)
            .and_then(|tasks| JsonValue::object(vec![("tasks".into(), tasks)]));
        match result {
            Ok(value) => Response::json(&value),
            Err(_) => error(StatusCode::ServiceUnavailable),
        }
    }
    pub fn write(&self, request: &Request) -> Response {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let Some(body) = request.json() else {
            return error(StatusCode::BadRequest);
        };
        let Some(title) = body.get("title").and_then(JsonValue::as_str) else {
            return error(StatusCode::BadRequest);
        };
        if body.as_object().is_none_or(|members| members.len() != 1)
            || title.trim().is_empty()
            || title.len() > MAX_TITLE_BYTES
            || title.chars().any(char::is_control)
        {
            return error(StatusCode::BadRequest);
        }
        let Ok(mut state) = self.state.lock() else {
            return error(StatusCode::ServiceUnavailable);
        };
        if state.tasks.len() == MAX_TASKS {
            return error(StatusCode::ServiceUnavailable);
        }
        let Some(id) = state.next.checked_add(1) else {
            return error(StatusCode::ServiceUnavailable);
        };
        let task = Task {
            id,
            title: title.into(),
        };
        let Ok(value) = value(&task) else {
            return error(StatusCode::ServiceUnavailable);
        };
        let response = Response::json(&value);
        state.tasks.push(task);
        state.next = id;
        self.writes.fetch_add(1, Ordering::SeqCst);
        response
    }
    #[allow(dead_code)]
    pub fn counts(&self) -> Option<(usize, usize, usize)> {
        Some((
            self.calls.load(Ordering::SeqCst),
            self.writes.load(Ordering::SeqCst),
            self.state.lock().ok()?.tasks.len(),
        ))
    }
}
pub fn endpoint(tasks: Arc<Tasks>) -> FileEndpoint {
    let read = tasks.clone();
    FileEndpoint::new()
        .get(move |_| read.read())
        .post_json(move |request| tasks.write(request))
}
