use ruvane::{
    EntityTag, EntityTagSource, EntityTagSourceError, FileEndpoint, JsonError, JsonValue, Method,
    Request, Response, StatusCode,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const MAX_TASKS: usize = 32;
use crate::task_input::{CreateTask, PatchTask, ReplaceTask};
struct Task {
    id: u64,
    title: String,
    completed: bool,
    tag: EntityTag,
}
#[derive(Default)]
struct State {
    next: u64,
    tasks: Vec<Task>,
}
pub struct Tasks {
    versions: EntityTagSource,
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
impl Tasks {
    pub fn new() -> Result<Self, EntityTagSourceError> {
        Ok(Self {
            versions: EntityTagSource::new()?,
            state: Mutex::new(State::default()),
            calls: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
        })
    }
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
        let input = match request.decode_json::<CreateTask>() {
            Ok(input) => input,
            Err(errors) => return Response::validation_errors(&errors),
        };
        let Ok(mut state) = self.state.lock() else {
            return error(StatusCode::ServiceUnavailable);
        };
        if state.tasks.len() == MAX_TASKS {
            return error(StatusCode::ServiceUnavailable);
        }
        let Some(id) = state.next.checked_add(1) else {
            return error(StatusCode::ServiceUnavailable);
        };
        let Ok(tag) = self.versions.next_tag() else {
            return error(StatusCode::ServiceUnavailable);
        };
        let task = Task {
            id,
            title: input.title,
            completed: false,
            tag,
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
        let (title, completed) = match request.method() {
            Method::Put => match request.decode_json::<ReplaceTask>() {
                Ok(input) => (Some(input.title), Some(input.completed)),
                Err(errors) => return Response::validation_errors(&errors),
            },
            Method::Patch => match request.decode_json::<PatchTask>() {
                Ok(input) => (input.title, input.completed),
                Err(errors) => return Response::validation_errors(&errors),
            },
            _ => (None, None),
        };
        let Ok(mut state) = self.state.lock() else {
            return error(StatusCode::ServiceUnavailable);
        };
        let Some(index) = state.tasks.iter().position(|task| task.id == id) else {
            return error(StatusCode::NotFound);
        };
        let current = &state.tasks[index].tag;
        if request.method().is_mutation() {
            let Some(condition) = request.if_match().filter(|condition| !condition.is_any()) else {
                return error(StatusCode::PreconditionRequired);
            };
            if !condition.matches(Some(current)) {
                return error(StatusCode::PreconditionFailed);
            }
        } else if request
            .if_match()
            .is_some_and(|condition| !condition.matches(Some(current)))
        {
            return error(StatusCode::PreconditionFailed);
        }
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
            title: title.unwrap_or_else(|| old.title.clone()),
            completed: completed.unwrap_or(old.completed),
            tag: if request.method().is_mutation() {
                match self.versions.next_tag() {
                    Ok(tag) => tag,
                    Err(_) => return error(StatusCode::ServiceUnavailable),
                }
            } else {
                old.tag.clone()
            },
        };
        let Ok(value) = value(&task) else {
            return error(StatusCode::ServiceUnavailable);
        };
        let response = Response::json(&value);
        if request.method().is_mutation() {
            state.tasks[index] = task;
            self.writes.fetch_add(1, Ordering::SeqCst);
            response
        } else {
            response.with_etag(task.tag)
        }
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
