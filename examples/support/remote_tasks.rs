//! Task model shared by native tests and the Wasm adapter.
use ruvane::{
    ActionData, ActionFailure, ActionOutcome, ActionRequest, BrowserTag::*, Component,
    ComponentError, EventValue, FormData, InternalUrl, View,
};
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct Task {
    pub id: u32,
    pub title: String,
    pub notes: String,
    pub done: bool,
}
struct Tasks {
    navigation: bool,
    items: Vec<Task>,
    title: String,
    notes: String,
    message: String,
    invalid: Option<&'static str>,
    epoch: u32,
    page: usize,
    pending: Option<u32>,
    outgoing: Option<ActionRequest>,
}
impl Tasks {
    fn page_size(&self) -> usize {
        if self.navigation { 2 } else { 3 }
    }
    fn request(&mut self, action: u32, fields: FormData) -> Result<(), ComponentError> {
        let path = match action {
            1 => "/actions/tasks/list",
            2 => "/actions/tasks/create",
            3 => "/actions/tasks/status",
            4 => "/actions/tasks/remove",
            _ => return Err(ComponentError::InvalidEvent),
        };
        self.outgoing = Some(
            ActionRequest::new(action, InternalUrl::new(path).unwrap(), fields)
                .map_err(|_| ComponentError::InvalidProps)?,
        );
        self.pending = Some(action);
        self.message = "Sending…".into();
        self.invalid = None;
        Ok(())
    }
    pub(super) fn snapshot(data: &ActionData) -> Option<Vec<Task>> {
        let count = usize::try_from(data.number("count")?).ok()?;
        if count > 32 || data.len() != 1 + count * 4 {
            return None;
        }
        let mut items = Vec::new();
        let mut ids = BTreeSet::new();
        for index in 0..count {
            let id = u32::try_from(data.number(&format!("id_{index}"))?).ok()?;
            let title = data.text(&format!("title_{index}"))?;
            let notes = data.text(&format!("notes_{index}"))?;
            let done = data.boolean(&format!("done_{index}"))?;
            if id == 0
                || !ids.insert(id)
                || title.trim().is_empty()
                || title.len() > 80
                || title.chars().any(char::is_control)
                || notes.len() > 160
            {
                return None;
            }
            items.push(Task {
                id,
                title: title.into(),
                notes: notes.into(),
                done,
            });
        }
        Some(items)
    }
}
impl Component for Tasks {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        self.items = Self::snapshot(&data).ok_or(ComponentError::InvalidProps)?;
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let busy = self.pending.is_some();
        let mut view = View::new();
        let section = view.keyed_element(0, 1, Section)?;
        view.keyed_leaf(section, 1, Heading, "Server tasks / Rust + Wasm")?;
        let form = view.form(section, 2, 1)?;
        let label = view.keyed_element(form, 1, Label)?;
        view.keyed_leaf(label, 1, Span, "Task name")?;
        let title = view.input(label, 2, &self.title, 5, self.invalid == Some("title"))?;
        view.field_disabled(title, busy)?;
        view.field_epoch(title, self.epoch)?;
        let label = view.keyed_element(form, 2, Label)?;
        view.keyed_leaf(label, 1, Span, "Notes")?;
        let notes = view.textarea(label, 2, &self.notes, 6, self.invalid == Some("notes"))?;
        view.field_disabled(notes, busy)?;
        view.field_epoch(notes, self.epoch)?;
        view.submit(form, 3, if busy { "Sending…" } else { "Add task" }, busy)?;
        view.keyed_leaf(section, 3, Output, &self.message)?;
        let list = view.keyed_element(section, 4, List)?;
        for (slot, task) in self
            .items
            .iter()
            .skip(self.page * self.page_size())
            .take(self.page_size())
            .enumerate()
        {
            let row = view.keyed_element(list, task.id, ListItem)?;
            let label = view.keyed_element(row, 1, Label)?;
            let text = if task.notes.is_empty() {
                task.title.clone()
            } else {
                format!("{} — {}", task.title, task.notes)
            };
            view.keyed_leaf(label, 1, Span, &text)?;
            let check = view.checkbox(label, 2, task.done, 100 + slot as u32 * 2)?;
            view.field_disabled(check, busy)?;
            view.keyed_button(row, 2, "Remove task", 101 + slot as u32 * 2, busy)?;
            if self.navigation {
                let url = super::navigation_pages::routes()
                    .url("task", &[("id", &task.id.to_string())])
                    .unwrap();
                view.link(row, 3, &url, "Open task")?;
            }
        }
        view.keyed_button(section, 5, "Previous", 2, busy || self.page == 0)?;
        view.keyed_button(
            section,
            6,
            "Next",
            3,
            busy || (self.page + 1) * self.page_size() >= self.items.len(),
        )?;
        view.keyed_leaf(
            section,
            7,
            Output,
            &format!("{} tasks / page {}", self.items.len(), self.page + 1),
        )?;
        view.keyed_button(section, 8, "Refresh tasks", 4, busy)?;
        if self.navigation {
            view.link(section, 9, &InternalUrl::new("/app").unwrap(), "Home")?;
        }
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending.is_some() {
            return Ok(());
        }
        match event {
            1 => {
                let fields = FormData::default()
                    .field("title", &self.title)
                    .and_then(|f| f.field("notes", &self.notes))
                    .map_err(|_| ComponentError::InvalidProps)?;
                self.request(2, fields)
            }
            2 => {
                self.page = self.page.saturating_sub(1);
                Ok(())
            }
            3 => {
                if (self.page + 1) * self.page_size() < self.items.len() {
                    self.page += 1;
                }
                Ok(())
            }
            4 => self.request(1, FormData::default()),
            101 | 103 | 105 => {
                let index = self.page * self.page_size() + (event as usize - 101) / 2;
                let task = self.items.get(index).ok_or(ComponentError::InvalidEvent)?;
                let fields = FormData::default()
                    .field("id", &task.id.to_string())
                    .map_err(|_| ComponentError::InvalidProps)?;
                self.request(4, fields)
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        if self.pending.is_some() {
            return Ok(());
        }
        match (event, value) {
            (5, EventValue::Text(value)) => {
                self.title = value;
                Ok(())
            }
            (6, EventValue::Text(value)) => {
                self.notes = value;
                Ok(())
            }
            (event @ (100 | 102 | 104), EventValue::Boolean(value)) => {
                let index = self.page * self.page_size() + (event as usize - 100) / 2;
                let task = self.items.get(index).ok_or(ComponentError::InvalidEvent)?;
                let mut fields = FormData::default()
                    .field("id", &task.id.to_string())
                    .map_err(|_| ComponentError::InvalidProps)?;
                if value {
                    fields = fields
                        .field("done", "on")
                        .map_err(|_| ComponentError::InvalidProps)?;
                }
                self.request(3, fields)
            }
            (event, EventValue::None) => self.dispatch(event),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_action(&mut self) -> Option<ActionRequest> {
        self.outgoing.take()
    }
    fn action_result(&mut self, action: u32, outcome: ActionOutcome) -> Result<(), ComponentError> {
        if self.pending.take() != Some(action) {
            return Err(ComponentError::InvalidEvent);
        }
        self.message = match outcome {
            Ok(reply) if reply.is_validation() => {
                let data = reply.data();
                match (data.text("message"), data.text("field")) {
                    (Some(message), Some(field)) if data.len() == 2 && matches!(field, "" | "title" | "notes") => {
                        self.invalid = match field { "title" => Some("title"), "notes" => Some("notes"), _ => None }; message.to_owned()
                    }
                    _ => "Invalid server response.".into(),
                }
            }
            Ok(reply) => if let Some(items) = Self::snapshot(reply.data()) {
                self.items = items; self.page = self.page.min(self.items.len().saturating_sub(1) / self.page_size());
                if action == 2 { self.title.clear(); self.notes.clear(); self.epoch = self.epoch.checked_add(1).ok_or(ComponentError::InvalidProps)?; }
                    if action == 1 { "Loaded.".into() } else { "Saved.".into() }
            } else { "Invalid server response.".into() },
            Err(ActionFailure::Timeout) => "Request timed out. The server may have applied the change. Refresh before retrying.".into(),
            Err(ActionFailure::Network) => "Network error. The server may have applied the change. Refresh before retrying.".into(),
            Err(ActionFailure::Protocol) => "Invalid server response. Refresh before retrying.".into(),
            Err(ActionFailure::Rejected) => "Server returned an unexpected response. Refresh before retrying.".into(),
            Err(ActionFailure::RateLimited) => "Too many requests. Wait before trying again.".into(),
        };
        Ok(())
    }
}
pub(super) fn create() -> Box<dyn Component> {
    let mut tasks = Tasks {
        navigation: false,
        items: Vec::new(),
        title: String::new(),
        notes: String::new(),
        message: String::new(),
        invalid: None,
        epoch: 0,
        page: 0,
        pending: None,
        outgoing: None,
    };
    tasks.request(1, FormData::default()).unwrap();
    Box::new(tasks)
}

pub(super) fn decode(data: &ActionData) -> Option<Vec<Task>> {
    Tasks::snapshot(data)
}
pub(super) fn create_page() -> Box<dyn Component> {
    Box::new(Tasks {
        navigation: true,
        items: Vec::new(),
        title: String::new(),
        notes: String::new(),
        message: String::new(),
        invalid: None,
        epoch: 0,
        page: 0,
        pending: None,
        outgoing: None,
    })
}
