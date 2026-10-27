use super::{BrowserTag, View, ViewError};
use crate::MAX_BROWSER_TEXT_BYTES;

/// Data carried by a component event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventValue {
    /// A click or form submission.
    None,
    /// Bounded UTF-8 control text.
    Text(String),
    /// A checkbox value.
    Boolean(bool),
    /// A bounded change from one live file picker.
    File(crate::FileSelection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Control {
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub invalid: bool,
    pub epoch: u32,
}

pub(super) fn is_control(tag: BrowserTag) -> bool {
    matches!(
        tag,
        BrowserTag::Input | BrowserTag::Checkbox | BrowserTag::TextArea | BrowserTag::File
    )
}
pub(super) fn valid_value(tag: BrowserTag, value: &str) -> bool {
    value.len() <= MAX_BROWSER_TEXT_BYTES
        && !value
            .chars()
            .any(|c| c.is_control() && !(tag == BrowserTag::TextArea && (c == '\n' || c == '\t')))
}
pub(super) fn accepts(parent: Option<BrowserTag>, tag: BrowserTag) -> bool {
    use BrowserTag::*;
    if parent == Some(List) {
        return tag == ListItem;
    }
    if tag == ListItem {
        return false;
    }
    if parent == Some(Paragraph) {
        return matches!(tag, Span | Output | Button | Link | Download);
    }
    if parent == Some(Label) {
        return matches!(tag, Span | Input | Checkbox | TextArea | File);
    }
    if matches!(tag, Input | Checkbox | TextArea | File) {
        return parent == Some(Label);
    }
    parent.is_none() || matches!(parent, Some(Section | ListItem | Form))
}

impl View {
    /// Adds one labeled file picker. Each view supports at most one selected file.
    pub fn file(
        &mut self,
        parent: u32,
        key: u32,
        event: u32,
        invalid: bool,
    ) -> Result<u32, ViewError> {
        if self.nodes.iter().any(|node| node.tag == BrowserTag::File) {
            return Err(ViewError::Limit);
        }
        self.control(
            parent,
            key,
            BrowserTag::File,
            Control {
                value: None,
                checked: None,
                invalid,
                epoch: 0,
            },
            event,
        )
    }

    /// Adds a client form with a submission event. Forms cannot be nested.
    pub fn form(&mut self, parent: u32, key: u32, event: u32) -> Result<u32, ViewError> {
        if key == 0 || event == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        self.push(parent, key, BrowserTag::Form, None, Some(event), false)
    }
    /// Adds a submit button inside a form.
    pub fn submit(
        &mut self,
        parent: u32,
        key: u32,
        label: &str,
        disabled: bool,
    ) -> Result<u32, ViewError> {
        if key == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        self.push(parent, key, BrowserTag::Submit, Some(label), None, disabled)
    }
    fn control(
        &mut self,
        parent: u32,
        key: u32,
        tag: BrowserTag,
        field: Control,
        event: u32,
    ) -> Result<u32, ViewError> {
        if key == 0 || event == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        if let Some(value) = &field.value {
            if value.len() > MAX_BROWSER_TEXT_BYTES {
                return Err(ViewError::Limit);
            }
            if !valid_value(tag, value) {
                return Err(ViewError::InvalidText);
            }
        }
        let id = self.push(parent, key, tag, None, Some(event), false)?;
        self.nodes[id as usize - 1].field = Some(field);
        Ok(id)
    }
    /// Adds a single-line input to a wrapping label.
    pub fn input(
        &mut self,
        parent: u32,
        key: u32,
        value: &str,
        event: u32,
        invalid: bool,
    ) -> Result<u32, ViewError> {
        if value.len() > MAX_BROWSER_TEXT_BYTES {
            return Err(ViewError::Limit);
        }
        if !valid_value(BrowserTag::Input, value) {
            return Err(ViewError::InvalidText);
        }
        self.control(
            parent,
            key,
            BrowserTag::Input,
            Control {
                value: Some(value.into()),
                checked: None,
                invalid,
                epoch: 0,
            },
            event,
        )
    }
    /// Adds multiline text to a wrapping label.
    pub fn textarea(
        &mut self,
        parent: u32,
        key: u32,
        value: &str,
        event: u32,
        invalid: bool,
    ) -> Result<u32, ViewError> {
        if value.len() > MAX_BROWSER_TEXT_BYTES {
            return Err(ViewError::Limit);
        }
        if !valid_value(BrowserTag::TextArea, value) {
            return Err(ViewError::InvalidText);
        }
        self.control(
            parent,
            key,
            BrowserTag::TextArea,
            Control {
                value: Some(value.into()),
                checked: None,
                invalid,
                epoch: 0,
            },
            event,
        )
    }
    /// Adds a checkbox to a wrapping label.
    pub fn checkbox(
        &mut self,
        parent: u32,
        key: u32,
        checked: bool,
        event: u32,
    ) -> Result<u32, ViewError> {
        self.control(
            parent,
            key,
            BrowserTag::Checkbox,
            Control {
                value: None,
                checked: Some(checked),
                invalid: false,
                epoch: 0,
            },
            event,
        )
    }
    /// Changes the reset token of a control. Increment it to force an explicit reset.
    pub fn field_epoch(&mut self, node: u32, epoch: u32) -> Result<(), ViewError> {
        let field = self
            .nodes
            .get_mut(node.checked_sub(1).ok_or(ViewError::InvalidIdentifier)? as usize)
            .and_then(|n| n.field.as_mut())
            .ok_or(ViewError::InvalidIdentifier)?;
        field.epoch = epoch;
        Ok(())
    }
    /// Enables or disables a field without changing its identity.
    pub fn field_disabled(&mut self, node: u32, disabled: bool) -> Result<(), ViewError> {
        let target = self
            .nodes
            .get_mut(node.checked_sub(1).ok_or(ViewError::InvalidIdentifier)? as usize)
            .ok_or(ViewError::InvalidIdentifier)?;
        if target.field.is_none() {
            return Err(ViewError::InvalidStructure);
        }
        target.disabled = disabled;
        Ok(())
    }
}
