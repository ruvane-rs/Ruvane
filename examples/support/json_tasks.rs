use ruvane::{FileEndpoint, JsonError, JsonValue, Method, Request, Response, StatusCode};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const MAX_TASKS: usize = 32;
const MAX_TITLE_BYTES: usize = 256;
struct Task {
    id: u64,
    title: String,
    completed: bool,
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
        ("completed".into(), task.completed.into()),
    ])
}
fn valid_title(title: &str) -> bool {
    !title.trim().is_empty()
        && title.len() <= MAX_TITLE_BYTES
        && !title.chars().any(char::is_control)
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
        if body.as_object().is_none_or(|members| members.len() != 1) || !valid_title(title) {
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
            completed: false,
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
    pub fn resource(&self, request: &Request) -> Response {
        if request.method().is_mutation() {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
        let Some(id) = request.param("id").and_then(|raw| {
            raw.parse::<u64>()
                .ok()
                .filter(|id| *id > 0 && id.to_string() == raw)
        }) else {
            return error(StatusCode::NotFound);
        };
        let mut title = None;
        let mut completed = None;
        if matches!(request.method(), Method::Put | Method::Patch) {
            let Some(body) = request.json().and_then(JsonValue::as_object) else {
                return error(StatusCode::BadRequest);
            };
            for (key, value) in body {
                match key.as_str() {
                    "title" => match value.as_str().filter(|title| valid_title(title)) {
                        Some(value) => title = Some(value),
                        None => return error(StatusCode::BadRequest),
                    },
                    "completed" => match value.as_bool() {
                        Some(value) => completed = Some(value),
                        None => return error(StatusCode::BadRequest),
                    },
                    _ => return error(StatusCode::BadRequest),
                }
            }
            if body.is_empty()
                || (request.method() == Method::Put && (title.is_none() || completed.is_none()))
            {
                return error(StatusCode::BadRequest);
            }
        }
        let Ok(mut state) = self.state.lock() else {
            return error(StatusCode::ServiceUnavailable);
        };
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return error(StatusCode::NotFound);
        };
        if request.method() == Method::Delete {
            let response =
                Response::json(&JsonValue::object(vec![("deleted".into(), id.into())]).unwrap());
            state.tasks.remove(index);
            self.writes.fetch_add(1, Ordering::SeqCst);
            return response;
        }
        let old = &state.tasks[index];
        let task = Task {
            id,
            title: title.unwrap_or(&old.title).into(),
            completed: completed.unwrap_or(old.completed),
        };
        let Ok(value) = value(&task) else {
            return error(StatusCode::ServiceUnavailable);
        };
        let response = Response::json(&value);
        if request.method().is_mutation() {
            state.tasks[index] = task;
            self.writes.fetch_add(1, Ordering::SeqCst);
        }
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
pub fn resource_endpoint(tasks: Arc<Tasks>) -> FileEndpoint {
    let (read, put, patch) = (tasks.clone(), tasks.clone(), tasks.clone());
    FileEndpoint::new()
        .get(move |request| read.resource(request))
        .put_json(move |request| put.resource(request))
        .patch_json(move |request| patch.resource(request))
        .delete(move |request| tasks.resource(request))
}
