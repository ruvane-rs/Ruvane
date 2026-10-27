//! Task editor shared by native tests and Wasm.
use super::task_input::{CreateTask, PatchTask, ReplaceTask};
use ruvane::{BrowserTag::*, *};

#[derive(Clone)]
struct Task {
    id: u64,
    title: String,
    completed: bool,
}
impl Task {
    fn decode(value: &JsonValue) -> Option<Self> {
        let input = ReplaceTask::try_from(
            &JsonValue::object(vec![
                ("title".into(), value.get("title")?.clone()),
                ("completed".into(), value.get("completed")?.clone()),
            ])
            .ok()?,
        )
        .ok()?;
        let id = value.get("id")?.as_number()?.to_u64().ok()?;
        if id == 0 || value.as_object()?.len() != 3 {
            return None;
        }
        Some(Self {
            id,
            title: input.title,
            completed: input.completed,
        })
    }
}
struct Tasks {
    items: Vec<Task>,
    page: usize,
    selected: Option<u64>,
    tag: Option<EntityTag>,
    title: String,
    completed: bool,
    message: String,
    errors: ValidationErrors,
    pending: bool,
    outgoing: Option<JsonRequest>,
    uncertain: bool,
    epoch: u32,
}
impl Tasks {
    fn send(
        &mut self,
        action: u32,
        method: JsonMethod,
        path: &str,
        body: Option<JsonValue>,
        conditional: bool,
    ) -> Result<(), ComponentError> {
        if self.pending {
            return Err(ComponentError::InvalidEvent);
        }
        let mut request = JsonRequest::new(
            action,
            method,
            InternalUrl::new(path).map_err(|_| ComponentError::InvalidEvent)?,
            body,
        )
        .map_err(|_| ComponentError::InvalidEvent)?;
        if conditional {
            request = request
                .with_if_match(self.tag.clone().ok_or(ComponentError::InvalidEvent)?)
                .map_err(|_| ComponentError::InvalidEvent)?;
        }
        self.pending = true;
        self.outgoing = Some(request);
        self.errors = ValidationErrors::new();
        self.message = "Loading…".into();
        Ok(())
    }
    fn body(&self, title: bool, completed: bool) -> JsonValue {
        let mut fields = Vec::new();
        if title {
            fields.push((
                "title".into(),
                JsonValue::string(self.title.clone()).unwrap(),
            ));
        }
        if completed {
            fields.push(("completed".into(), self.completed.into()));
        }
        JsonValue::object(fields).unwrap()
    }
    fn read_selected(&mut self, action: u32) -> Result<(), ComponentError> {
        self.send(
            action,
            JsonMethod::Get,
            &format!(
                "/api/tasks/{}",
                self.selected.ok_or(ComponentError::InvalidEvent)?
            ),
            None,
            false,
        )
    }
    fn reset(&mut self) {
        self.selected = None;
        self.tag = None;
        self.title.clear();
        self.completed = false;
        self.errors = ValidationErrors::new();
        self.epoch = self.epoch.wrapping_add(1);
    }
    fn accept(&mut self, action: u32, reply: JsonReply) -> Option<()> {
        match action {
            1 => {
                self.send(2, JsonMethod::Get, "/api/tasks", None, false)
                    .ok()?;
            }
            2 | 21 => {
                let values = reply.value().get("tasks")?.as_array()?;
                if values.len() > 32 {
                    return None;
                }
                let items = values
                    .iter()
                    .map(Task::decode)
                    .collect::<Option<Vec<_>>>()?;
                for (i, task) in items.iter().enumerate() {
                    if items[..i].iter().any(|other| other.id == task.id) {
                        return None;
                    }
                }
                self.page = self.page.min(items.len().saturating_sub(1) / 2);
                self.items = items;
                if action == 2 {
                    self.uncertain = false;
                }
                self.message = "Tasks loaded. Select a task to retain its version.".into();
            }
            3 => {
                Task::decode(reply.value())?;
                self.reset();
                self.page = usize::MAX;
                self.send(21, JsonMethod::Get, "/api/tasks", None, false)
                    .ok()?;
            }
            4..=6 => {
                Task::decode(reply.value())?;
                self.tag = None;
                self.read_selected(20).ok()?;
            }
            7 => {
                if reply.value().get("deleted")?.as_number()?.to_u64().ok()? != self.selected? {
                    return None;
                }
                self.reset();
                self.send(21, JsonMethod::Get, "/api/tasks", None, false)
                    .ok()?;
            }
            8 | 20 => {
                let task = Task::decode(reply.value())?;
                if Some(task.id) != self.selected {
                    return None;
                }
                self.tag = Some(reply.etag()?.clone());
                if self.tag.as_ref()?.is_weak() {
                    return None;
                }
                self.title = task.title;
                self.completed = task.completed;
                self.epoch = self.epoch.wrapping_add(1);
                self.uncertain = false;
                self.message = "Snapshot loaded. Edits use this exact version.".into();
                if action == 20 {
                    self.send(21, JsonMethod::Get, "/api/tasks", None, false)
                        .ok()?;
                }
            }
            10 => {
                if reply.value().as_bool() != Some(false) {
                    return None;
                }
                self.reset();
                self.items.clear();
                self.message = "Demo session closed.".into();
            }
            _ => return None,
        }
        Some(())
    }
}
impl Component for Tasks {
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(root, 1, Heading, "JSON tasks / Rust + Wasm")?;
        v.keyed_leaf(
            root,
            2,
            Span,
            "Local demo sessions. No real user authentication.",
        )?;
        v.keyed_button(root, 3, "Open demo session", 1, self.pending)?;
        v.keyed_button(root, 4, "Refresh list", 2, self.pending)?;
        v.keyed_button(root, 5, "New draft", 9, self.pending || self.uncertain)?;
        v.keyed_button(root, 6, "Log out", 10, self.pending || self.uncertain)?;
        let form = v.form(root, 7, 3)?;
        let label = v.keyed_element(form, 1, Label)?;
        v.keyed_leaf(label, 1, Span, "Title")?;
        let invalid = self
            .errors
            .iter()
            .any(|issue| issue.field() == Some("title"));
        let field = v.input(label, 2, &self.title, 11, invalid)?;
        v.field_epoch(field, self.epoch)?;
        v.field_disabled(field, self.pending)?;
        let label = v.keyed_element(form, 2, Label)?;
        v.keyed_leaf(label, 1, Span, "Completed")?;
        let check = v.checkbox(label, 2, self.completed, 12)?;
        v.field_disabled(check, self.pending)?;
        v.submit(
            form,
            3,
            "Create task",
            self.pending || self.selected.is_some() || self.uncertain,
        )?;
        let blocked = self.pending || self.tag.is_none() || self.uncertain;
        v.keyed_button(root, 8, "Replace task (PUT)", 4, blocked)?;
        v.keyed_button(root, 9, "Save title (PATCH)", 5, blocked)?;
        v.keyed_button(root, 10, "Save state (PATCH)", 6, blocked)?;
        v.keyed_button(root, 11, "Delete task", 7, blocked)?;
        v.keyed_button(
            root,
            12,
            "Reload selected snapshot",
            8,
            self.pending || self.selected.is_none(),
        )?;
        let messages = self
            .errors
            .iter()
            .map(|issue| {
                format!(
                    "{}: {}",
                    issue.field().unwrap_or("Request"),
                    issue.code().message()
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        v.keyed_leaf(root, 13, Output, &messages)?;
        v.keyed_leaf(root, 14, Output, &self.message)?;
        let list = v.keyed_element(root, 15, List)?;
        for (i, task) in self.items.iter().enumerate().skip(self.page * 2).take(2) {
            let row = v.keyed_element(list, i as u32 + 1, ListItem)?;
            v.keyed_leaf(
                row,
                1,
                Span,
                &format!(
                    "#{} {} [{}]",
                    task.id,
                    task.title,
                    if task.completed { "done" } else { "open" }
                ),
            )?;
            v.keyed_button(row, 2, "Edit snapshot", 100 + i as u32, self.pending)?;
        }
        v.keyed_button(
            root,
            16,
            "Previous tasks",
            13,
            self.pending || self.page == 0,
        )?;
        v.keyed_button(
            root,
            17,
            "Next tasks",
            14,
            self.pending || (self.page + 1) * 2 >= self.items.len(),
        )?;
        Ok(v)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending {
            return Err(ComponentError::InvalidEvent);
        }
        if self.uncertain && matches!(event, 1 | 3..=7 | 9 | 10) {
            return Err(ComponentError::InvalidEvent);
        }
        match event {
            1 => self.send(
                1,
                JsonMethod::Post,
                "/demo-session",
                Some(JsonValue::object(vec![]).unwrap()),
                false,
            ),
            2 => self.send(2, JsonMethod::Get, "/api/tasks", None, false),
            3 => {
                if self.selected.is_some() {
                    return Err(ComponentError::InvalidEvent);
                }
                let body = self.body(true, false);
                if let Err(errors) = CreateTask::try_from(&body) {
                    self.errors = errors;
                    self.message = "Correct the marked fields.".into();
                    return Ok(());
                }
                self.send(3, JsonMethod::Post, "/api/tasks", Some(body), false)
            }
            4..=7 => {
                let path = format!(
                    "/api/tasks/{}",
                    self.selected.ok_or(ComponentError::InvalidEvent)?
                );
                let method = if event == 4 {
                    JsonMethod::Put
                } else if event == 7 {
                    JsonMethod::Delete
                } else {
                    JsonMethod::Patch
                };
                let body = (event != 7).then(|| self.body(event != 6, event != 5));
                let errors = body.as_ref().and_then(|body| {
                    if event == 4 {
                        ReplaceTask::try_from(body).err()
                    } else {
                        PatchTask::try_from(body).err()
                    }
                });
                if let Some(errors) = errors {
                    self.errors = errors;
                    self.message = "Correct the marked fields.".into();
                    return Ok(());
                }
                self.send(event, method, &path, body, true)
            }
            8 => self.read_selected(8),
            9 => {
                self.reset();
                self.message = "New draft.".into();
                Ok(())
            }
            10 => self.send(
                10,
                JsonMethod::Post,
                "/api/logout",
                Some(JsonValue::object(vec![]).unwrap()),
                false,
            ),
            13 if self.page > 0 => {
                self.page -= 1;
                Ok(())
            }
            14 if (self.page + 1) * 2 < self.items.len() => {
                self.page += 1;
                Ok(())
            }
            100..=131 => {
                self.selected = Some(
                    self.items
                        .get((event - 100) as usize)
                        .ok_or(ComponentError::InvalidEvent)?
                        .id,
                );
                self.tag = None;
                self.read_selected(8)
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        if self.pending {
            return Err(ComponentError::InvalidEvent);
        }
        match (event, value) {
            (11, EventValue::Text(text)) => {
                self.title = text;
                self.errors = ValidationErrors::new();
                Ok(())
            }
            (12, EventValue::Boolean(value)) => {
                self.completed = value;
                Ok(())
            }
            (event, EventValue::None) => self.dispatch(event),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.outgoing.take()
    }
    fn json_result(&mut self, action: u32, outcome: JsonOutcome) -> Result<(), ComponentError> {
        self.pending = false;
        match outcome {
            Ok(reply) if reply.is_success() => {
                if self.accept(action, reply).is_none() {
                    self.tag = None;
                    self.uncertain = matches!(action, 1 | 3..=7 | 10);
                    self.message =
                        "Invalid response. Refresh explicitly before another mutation.".into();
                }
            }
            Ok(reply) => {
                self.uncertain |= reply.uncertain();
                self.message = match reply.status() {
                    400 => {
                        if let Ok(errors) = reply.validation(&["title", "completed"]) {
                            self.errors = errors;
                        }
                        "Validation failed. Your draft is retained.".into()
                    }
                    412 | 428 => {
                        self.tag = None;
                        "Conflict. Draft retained; reload the snapshot explicitly.".into()
                    }
                    429 => format!(
                        "Too many requests. Retry explicitly after {} seconds.",
                        reply
                            .retry_after()
                            .map_or("the indicated delay".into(), |n| n.to_string())
                    ),
                    status => format!("Request rejected ({status}). Draft retained."),
                };
                if self.uncertain {
                    self.tag = None;
                    self.message = "Mutation result uncertain. Inspect the server state explicitly; nothing was retried.".into();
                }
            }
            Err(error) => {
                self.uncertain |= error.uncertain();
                if self.uncertain {
                    self.tag = None;
                }
                self.message = if self.uncertain {
                    "Mutation result uncertain. Inspect the server state explicitly; nothing was retried.".into()
                } else {
                    format!("Transport {:?}. Retry explicitly.", error.kind())
                };
            }
        }
        Ok(())
    }
}

pub(super) fn create() -> Box<dyn Component> {
    Box::new(Tasks {
        items: Vec::new(),
        page: 0,
        selected: None,
        tag: None,
        title: String::new(),
        completed: false,
        message: "Open a demo session to begin.".into(),
        errors: ValidationErrors::new(),
        pending: false,
        outgoing: None,
        uncertain: false,
        epoch: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_views_fit_the_budget_with_full_storage_and_validation_errors() {
        let mut component = create();
        component.view().unwrap().snapshot().unwrap();
        let tasks = JsonValue::array(
            (1..=32u64)
                .map(|id| {
                    JsonValue::object(vec![
                        ("id".into(), id.into()),
                        (
                            "title".into(),
                            JsonValue::string(format!("Task {id}")).unwrap(),
                        ),
                        ("completed".into(), false.into()),
                    ])
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let value = JsonValue::object(vec![("tasks".into(), tasks)]).unwrap();
        let mut frame = b"RVJ1\0\0\xc8\0\0\0\0\0\0\0".to_vec();
        frame.extend(value.encode().as_bytes());
        component
            .json_result(2, decode_json_reply(&frame, false))
            .unwrap();
        for index in 0..16 {
            let view = component.view().unwrap();
            view.snapshot().unwrap();
            assert!(view.event_count() <= ruvane::MAX_VIEW_EVENTS);
            assert!(
                view.render()
                    .unwrap()
                    .as_str()
                    .contains(&format!("Task {}", index * 2 + 1))
            );
            if index != 15 {
                component.dispatch(14).unwrap();
            }
        }
        component.dispatch(3).unwrap();
        assert!(
            component
                .view()
                .unwrap()
                .render()
                .unwrap()
                .as_str()
                .contains("This value is too short.")
        );
        component.view().unwrap().snapshot().unwrap();
    }
}
