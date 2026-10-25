//! Client-side task form shared by SSR and Wasm.
use ruvane::{
    BrowserTag::*, Component, ComponentError, ComponentStats, EventValue, ReactiveRuntime, Scope,
    Signal, View,
};

const MAX_TASKS: usize = 3;
struct Task {
    id: u32,
    title: String,
    notes: String,
    _scope: Scope,
    done: Signal<bool>,
}
struct Tasks {
    runtime: ReactiveRuntime,
    scope: Scope,
    items: Vec<Task>,
    next: u32,
    title: String,
    notes: String,
    error: &'static str,
    title_invalid: bool,
    notes_invalid: bool,
    epoch: u32,
    show_done: bool,
}

impl Tasks {
    fn clear(&mut self) -> Result<(), ComponentError> {
        self.title.clear();
        self.notes.clear();
        self.error = "";
        self.title_invalid = false;
        self.notes_invalid = false;
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(ComponentError::InvalidProps)?;
        Ok(())
    }
    fn add(&mut self, title: String, notes: String) -> Result<(), ComponentError> {
        let id = self
            .next
            .checked_add(1)
            .filter(|n| n.checked_mul(4).and_then(|n| n.checked_add(17)).is_some())
            .ok_or(ComponentError::InvalidProps)?;
        let scope = self.scope.child()?;
        let done = scope.signal(false)?;
        self.items.push(Task {
            id,
            title,
            notes,
            _scope: scope,
            done,
        });
        self.next = id;
        Ok(())
    }
    fn submit(&mut self) -> Result<(), ComponentError> {
        self.title_invalid = false;
        self.notes_invalid = false;
        self.error = "";
        let title = self.title.trim();
        if title.is_empty() {
            self.title_invalid = true;
            self.error = "Enter a task name.";
        } else if title.len() > 80 {
            self.title_invalid = true;
            self.error = "Task name must be at most 80 UTF-8 bytes.";
        } else if self.notes.len() > 160 {
            self.notes_invalid = true;
            self.error = "Notes must be at most 160 UTF-8 bytes.";
        } else if self.items.len() == MAX_TASKS {
            self.error = "Task limit reached.";
        } else {
            self.add(title.to_owned(), self.notes.trim().to_owned())?;
            self.clear()?;
        }
        Ok(())
    }
}
impl Component for Tasks {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let section = view.keyed_element(0, 1, Section)?;
        view.keyed_leaf(section, 1, Heading, "Ruvane tasks")?;
        let form = view.form(section, 2, 1)?;
        let label = view.keyed_element(form, 1, Label)?;
        view.keyed_leaf(label, 1, Span, "Task name")?;
        let input = view.input(label, 2, &self.title, 5, self.title_invalid)?;
        view.field_epoch(input, self.epoch)?;
        let label = view.keyed_element(form, 2, Label)?;
        view.keyed_leaf(label, 1, Span, "Notes")?;
        let notes = view.textarea(label, 2, &self.notes, 6, self.notes_invalid)?;
        view.field_epoch(notes, self.epoch)?;
        view.submit(form, 3, "Add task", self.items.len() == MAX_TASKS)?;
        view.keyed_button(form, 4, "Reset form", 4, false)?;
        view.keyed_leaf(form, 5, Output, self.error)?;
        let list = view.keyed_element(section, 3, List)?;
        let mut visible = 0;
        for item in &self.items {
            if !self.show_done && item.done.get()? {
                continue;
            }
            visible += 1;
            let row = view.keyed_element(list, item.id, ListItem)?;
            let label = view.keyed_element(row, 1, Label)?;
            let text = if item.notes.is_empty() {
                item.title.clone()
            } else {
                format!("{} — {}", item.title, item.notes)
            };
            view.keyed_leaf(label, 1, Span, &text)?;
            view.checkbox(label, 2, item.done.get()?, 16 + item.id * 4)?;
            view.keyed_button(row, 2, "Remove task", 17 + item.id * 4, false)?;
        }
        if visible == 0 {
            let message = view.keyed_element(section, 20, Paragraph)?;
            view.keyed_leaf(message, 1, Span, "No visible tasks.")?;
        }
        view.keyed_button(section, 4, "Reverse tasks", 2, self.items.len() < 2)?;
        view.keyed_button(
            section,
            5,
            if self.show_done {
                "Hide completed"
            } else {
                "Show completed"
            },
            3,
            false,
        )?;
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        match event {
            1 => self.submit(),
            2 => {
                self.items.reverse();
                Ok(())
            }
            3 => {
                self.show_done = !self.show_done;
                Ok(())
            }
            4 => self.clear(),
            _ => {
                let value = event
                    .checked_sub(17)
                    .filter(|n| n % 4 == 0)
                    .ok_or(ComponentError::InvalidEvent)?;
                let index = self
                    .items
                    .iter()
                    .position(|item| item.id == value / 4)
                    .ok_or(ComponentError::InvalidEvent)?;
                self.items.remove(index);
                Ok(())
            }
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (5, EventValue::Text(value)) => {
                self.title = value;
                Ok(())
            }
            (6, EventValue::Text(value)) => {
                self.notes = value;
                Ok(())
            }
            (event, EventValue::None) => self.dispatch(event),
            (event, EventValue::Boolean(value)) => {
                let id = event
                    .checked_sub(16)
                    .filter(|n| n % 4 == 0)
                    .ok_or(ComponentError::InvalidEvent)?
                    / 4;
                let item = self
                    .items
                    .iter()
                    .find(|n| n.id == id)
                    .ok_or(ComponentError::InvalidEvent)?;
                item.done.set(value)?;
                Ok(())
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn stats(&self) -> ComponentStats {
        let stats = self.runtime.stats();
        ComponentStats {
            signals: stats.signals,
            effects: stats.effects,
        }
    }
}
pub(super) fn create(count: u32) -> Result<Box<dyn Component>, ComponentError> {
    if count > MAX_TASKS as u32 {
        return Err(ComponentError::InvalidProps);
    }
    let runtime = ReactiveRuntime::new();
    let scope = runtime.scope()?;
    let mut tasks = Tasks {
        runtime,
        scope,
        items: Vec::new(),
        next: 0,
        title: String::new(),
        notes: String::new(),
        error: "",
        title_invalid: false,
        notes_invalid: false,
        epoch: 0,
        show_done: true,
    };
    for index in 0..count {
        tasks.add(format!("Task {}", index + 1), String::new())?;
    }
    Ok(Box::new(tasks))
}
