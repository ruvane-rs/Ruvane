//! Components compiled for both the server and Wasm demo.
use ruvane::{
    BrowserTag::*, Component, ComponentError, ComponentProps, ComponentStats, InitialState, Memo,
    ReactiveRuntime, Scope, Signal, View,
};

/// Shared page routes and models.
#[path = "navigation_pages.rs"]
#[allow(dead_code)]
pub mod navigation_pages;
/// Checks the demo page registry before mounting.
#[allow(dead_code)]
pub fn validate_page(page: &ruvane::NavigationPage) -> bool {
    if !page.layouts().is_empty() {
        return false;
    }
    navigation_pages::validate(page)
}

#[path = "action_probe.rs"]
mod action_probe;
#[path = "fields_probe.rs"]
mod fields_probe;
#[path = "remote_tasks.rs"]
mod remote_tasks;
/// Action transport fixture registry ID.
pub const ACTION_PROBE: u32 = 7;
#[path = "tasks.rs"]
mod tasks;
/// Server-backed task component registry ID.
pub const REMOTE_TASKS: u32 = 6;
/// Field lifecycle fixture registry ID.
pub const FIELDS_PROBE: u32 = 5;

/// Task form registry ID.
pub const TASK_FORM: u32 = 4;

/// Counter registry ID.
pub const COUNTER: u32 = 1;
/// Toggle registry ID.
pub const TOGGLE: u32 = 2;
/// Dynamic list registry ID.
pub const ITEM_LIST: u32 = 3;
/// Maximum logical items in the demo list.
pub const MAX_ITEMS: usize = 4;
/// Maximum counter value.
pub const MAX_COUNT: u32 = 1000;

struct Counter {
    runtime: ReactiveRuntime,
    _scope: Scope,
    count: Signal<u32>,
    doubled: Memo<u32>,
}

impl Component for Counter {
    fn view(&self) -> Result<View, ComponentError> {
        let count = self.count.get()?;
        let mut view = View::new();
        let section = view.element(0, Section)?;
        view.leaf(section, Heading, "Ruvane counter")?;
        let row = view.element(section, Paragraph)?;
        view.leaf(row, Span, "Value: ")?;
        view.leaf(row, Output, &count.to_string())?;
        let row = view.element(section, Paragraph)?;
        view.leaf(row, Span, "Double: ")?;
        view.leaf(row, Output, &self.doubled.get()?.to_string())?;
        view.button(section, "Increment", 1, count == MAX_COUNT)?;
        view.button(section, "Decrement", 2, count == 0)?;
        view.button(section, "Reset", 3, count == 0)?;
        Ok(view)
    }

    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        let value = self.count.get()?;
        let next = match event {
            1 => value.saturating_add(1).min(MAX_COUNT),
            2 => value.saturating_sub(1),
            3 => 0,
            _ => return Err(ComponentError::InvalidEvent),
        };
        if next != value {
            self.count.set(next)?;
        }
        Ok(())
    }

    fn stats(&self) -> ComponentStats {
        let stats = self.runtime.stats();
        ComponentStats {
            signals: stats.signals,
            effects: stats.effects,
        }
    }
}

struct Toggle {
    runtime: ReactiveRuntime,
    _scope: Scope,
    enabled: Signal<bool>,
}

impl Component for Toggle {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let section = view.element(0, Section)?;
        view.leaf(section, Heading, "Ruvane toggle")?;
        let row = view.element(section, Paragraph)?;
        view.leaf(row, Span, "Status: ")?;
        view.leaf(
            row,
            Output,
            if self.enabled.get()? {
                "Enabled"
            } else {
                "Disabled"
            },
        )?;
        view.button(section, "Toggle", 7, false)?;
        Ok(view)
    }

    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if event != 7 {
            return Err(ComponentError::InvalidEvent);
        }
        self.enabled.set(!self.enabled.get()?)?;
        Ok(())
    }

    fn stats(&self) -> ComponentStats {
        let stats = self.runtime.stats();
        ComponentStats {
            signals: stats.signals,
            effects: stats.effects,
        }
    }
}

struct Item {
    id: u32,
    _scope: Scope,
    count: Signal<u32>,
    done: Signal<bool>,
}

struct ItemList {
    runtime: ReactiveRuntime,
    scope: Scope,
    items: Vec<Item>,
    next: u32,
    show_done: bool,
}

impl ItemList {
    fn add(&mut self) -> Result<(), ComponentError> {
        if self.items.len() == MAX_ITEMS {
            return Ok(());
        }
        let id = self
            .next
            .checked_add(1)
            .filter(|id| id.checked_mul(4).and_then(|n| n.checked_add(18)).is_some())
            .ok_or(ComponentError::InvalidProps)?;
        let scope = self.scope.child()?;
        let count = scope.signal(0)?;
        let done = scope.signal(false)?;
        self.items.push(Item {
            id,
            _scope: scope,
            count,
            done,
        });
        self.next = id;
        Ok(())
    }
}

impl Component for ItemList {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let section = view.keyed_element(0, 1, Section)?;
        view.keyed_leaf(section, 1, Heading, "Ruvane item list")?;
        let mut visible = 0;
        let list = view.keyed_element(section, 3, List)?;
        for item in &self.items {
            let done = item.done.get()?;
            if !self.show_done && done {
                continue;
            }
            visible += 1;
            let row = view.keyed_element(list, item.id, ListItem)?;
            let label = if item.id <= 26 {
                format!("Item {}: ", char::from(b'A' + item.id as u8 - 1))
            } else {
                format!("Item {}: ", item.id)
            };
            view.keyed_leaf(row, 1, Span, &label)?;
            let count = item.count.get()?;
            view.keyed_leaf(row, 2, Output, &count.to_string())?;
            let event = 16 + item.id * 4;
            view.keyed_button(row, 3, "Increase", event, count == MAX_COUNT)?;
            view.keyed_button(row, 4, if done { "Undo" } else { "Done" }, event + 1, false)?;
            view.keyed_button(row, 5, "Remove", event + 2, false)?;
        }
        view.keyed_leaf(
            section,
            2,
            Span,
            &format!("{} items / {visible} visible", self.items.len()),
        )?;
        if visible == 0 {
            let message = view.keyed_element(section, 20, Paragraph)?;
            view.keyed_leaf(message, 1, Span, "No visible items.")?;
        }
        view.keyed_button(section, 4, "Add item", 1, self.items.len() == MAX_ITEMS)?;
        view.keyed_button(section, 5, "Reverse order", 2, self.items.len() < 2)?;
        view.keyed_button(
            section,
            6,
            if self.show_done {
                "Hide done"
            } else {
                "Show all"
            },
            3,
            false,
        )?;
        Ok(view)
    }

    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        match event {
            1 => return self.add(),
            2 => {
                self.items.reverse();
                return Ok(());
            }
            3 => {
                self.show_done = !self.show_done;
                return Ok(());
            }
            _ => {}
        }
        let value = event.checked_sub(16).ok_or(ComponentError::InvalidEvent)?;
        let index = self
            .items
            .iter()
            .position(|item| item.id == value / 4)
            .ok_or(ComponentError::InvalidEvent)?;
        match value % 4 {
            0 => {
                let count = self.items[index].count.get()?;
                if count < MAX_COUNT {
                    self.items[index].count.set(count + 1)?;
                }
            }
            1 => {
                let done = self.items[index].done.get()?;
                self.items[index].done.set(!done)?;
            }
            2 => {
                self.items.remove(index);
            }
            _ => return Err(ComponentError::InvalidEvent),
        }
        Ok(())
    }

    fn stats(&self) -> ComponentStats {
        let stats = self.runtime.stats();
        ComponentStats {
            signals: stats.signals,
            effects: stats.effects,
        }
    }
}

/// Creates a component from the demo's registry.
pub fn create(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    match (props.component(), props.state()) {
        (COUNTER, InitialState::Number(value)) if value <= MAX_COUNT => {
            let runtime = ReactiveRuntime::new();
            let scope = runtime.scope()?;
            let count = scope.signal(value)?;
            let input = count.clone();
            let doubled = scope.memo(&[count.source()], move || Ok(input.get()? * 2))?;
            Ok(Box::new(Counter {
                runtime,
                _scope: scope,
                count,
                doubled,
            }))
        }
        (TOGGLE, InitialState::Boolean(enabled)) => {
            let runtime = ReactiveRuntime::new();
            let scope = runtime.scope()?;
            let enabled = scope.signal(enabled)?;
            Ok(Box::new(Toggle {
                runtime,
                _scope: scope,
                enabled,
            }))
        }
        (ITEM_LIST, InitialState::Number(count)) if count <= MAX_ITEMS as u32 => {
            let runtime = ReactiveRuntime::new();
            let scope = runtime.scope()?;
            let mut list = ItemList {
                runtime,
                scope,
                items: Vec::new(),
                next: 0,
                show_done: true,
            };
            for _ in 0..count {
                list.add()?;
            }
            Ok(Box::new(list))
        }
        (TASK_FORM, InitialState::Number(count)) => tasks::create(count),
        (FIELDS_PROBE, InitialState::Number(0)) => Ok(fields_probe::create()),
        (REMOTE_TASKS, InitialState::Number(0)) => Ok(remote_tasks::create()),
        (ACTION_PROBE, InitialState::Number(mode)) if mode <= 9 => Ok(action_probe::create(mode)),
        _ => navigation_pages::create(props),
    }
}
