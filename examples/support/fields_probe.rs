//! Field lifecycle fixture used by the browser tests.
use ruvane::{BrowserTag::*, Component, ComponentError, EventValue, View};
struct Fields {
    title: String,
    notes: String,
    checked: bool,
    reversed: bool,
    show_notes: bool,
    epoch: u32,
    submits: u32,
}
impl Component for Fields {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let form = view.form(0, 1, 3)?;
        for key in if self.reversed { [2, 1] } else { [1, 2] } {
            if key == 2 && !self.show_notes {
                continue;
            }
            let label = view.keyed_element(form, key, Label)?;
            view.keyed_leaf(
                label,
                1,
                Span,
                if key == 1 {
                    "Probe title"
                } else {
                    "Probe notes"
                },
            )?;
            let field = if key == 1 {
                view.input(label, 2, &self.title, 10, false)?
            } else {
                view.textarea(label, 2, &self.notes, 11, false)?
            };
            view.field_epoch(field, self.epoch)?;
        }
        let label = view.keyed_element(form, 3, Label)?;
        view.keyed_leaf(label, 1, Span, "Probe flag")?;
        view.checkbox(label, 2, self.checked, 12)?;
        view.keyed_button(form, 4, "Reverse fields", 1, false)?;
        view.keyed_button(form, 5, "Reset fields", 2, false)?;
        view.submit(form, 6, "Submit fields", false)?;
        view.keyed_leaf(form, 7, Output, &self.submits.to_string())?;
        view.keyed_button(form, 8, "Toggle notes", 4, false)?;
        Ok(view)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        match event {
            1 => self.reversed = !self.reversed,
            2 => {
                self.title.clear();
                self.notes.clear();
                self.checked = false;
                self.epoch = self
                    .epoch
                    .checked_add(1)
                    .ok_or(ComponentError::InvalidProps)?;
            }
            3 => self.submits = self.submits.saturating_add(1),
            4 => self.show_notes = !self.show_notes,
            _ => return Err(ComponentError::InvalidEvent),
        }
        Ok(())
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (10, EventValue::Text(value)) => self.title = value,
            (11, EventValue::Text(value)) => self.notes = value,
            (12, EventValue::Boolean(value)) => self.checked = value,
            (event, EventValue::None) => return self.dispatch(event),
            _ => return Err(ComponentError::InvalidEvent),
        }
        Ok(())
    }
}
pub(super) fn create() -> Box<dyn Component> {
    Box::new(Fields {
        title: "Rust 🦀 <tag>".into(),
        notes: "\nNotes </textarea> & 🦀\n".into(),
        checked: true,
        reversed: false,
        show_notes: true,
        epoch: 0,
        submits: 0,
    })
}
