use super::{BrowserCommands, EventValue, Node, View, ViewError, is_control, valid_value};
use crate::BrowserTag;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
struct Entry {
    id: u32,
    position: usize,
    node: Node,
}

/// A live view's handles and order. Removed handles are never reused.
#[derive(Clone, Debug)]
pub struct ViewSnapshot {
    nodes: Vec<Entry>,
    next: u32,
}

impl ViewSnapshot {
    pub(super) fn new(view: &View) -> Result<Self, ViewError> {
        view.check()?;
        Self {
            nodes: Vec::new(),
            next: 0,
        }
        .resolve(view)
    }

    fn resolve(&self, view: &View) -> Result<Self, ViewError> {
        view.check()?;
        let mut nodes: Vec<Entry> = Vec::new();
        let mut next = self.next;
        for source in &view.nodes {
            let parent = if source.parent == 0 {
                0
            } else {
                nodes[source.parent as usize - 1].id
            };
            let position = nodes.iter().filter(|n| n.node.parent == parent).count();
            let old = self.nodes.iter().find(|entry| {
                let node = &entry.node;
                node.parent == parent
                    && node.tag == source.tag
                    && node.event == source.event
                    && if source.key != 0 {
                        node.key == source.key
                    } else {
                        node.key == 0 && entry.position == position
                    }
            });
            let id = if let Some(old) = old {
                old.id
            } else {
                next = next.checked_add(1).ok_or(ViewError::Limit)?;
                next
            };
            let mut node = source.clone();
            node.parent = parent;
            nodes.push(Entry { id, position, node });
        }
        Ok(Self { nodes, next })
    }

    fn emit(entry: &Entry, commands: &mut BrowserCommands, view: u32) -> Result<(), ViewError> {
        let node = &entry.node;
        if node.key == 0 {
            commands.element(view, entry.id, node.parent, node.tag)?;
        } else {
            commands.keyed_element(view, entry.id, node.parent, node.tag, node.key)?;
        }
        if let Some(label) = &node.text {
            commands.text(view, entry.id, label)?;
        }
        if let Some(event) = node.event {
            commands.bind(view, entry.id, event)?;
        }
        if let Some(url) = &node.link {
            commands.link(view, entry.id, url)?;
        }
        if matches!(node.tag, BrowserTag::Button | BrowserTag::Submit) || is_control(node.tag) {
            commands.disabled(view, entry.id, node.disabled)?;
        }
        if let Some(field) = &node.field {
            if let Some(value) = &field.value {
                commands.value(view, entry.id, value, field.epoch)?;
            }
            if let Some(checked) = field.checked {
                commands.checked(view, entry.id, checked, field.epoch)?;
            }
            commands.invalid(view, entry.id, field.invalid)?;
        }
        Ok(())
    }

    /// Encodes this snapshot's complete tree.
    pub fn commands(&self, view: u32) -> Result<BrowserCommands, ViewError> {
        let mut commands = BrowserCommands::new();
        for entry in &self.nodes {
            Self::emit(entry, &mut commands, view)?;
        }
        Ok(commands)
    }

    /// A live node for a declared application event.
    pub fn node_for_event(&self, event: u32) -> Option<u32> {
        self.nodes
            .iter()
            .find(|n| n.node.event == Some(event))
            .map(|n| n.id)
    }

    /// A keyed child's handle, within the supplied parent handle.
    pub fn node_for_key(&self, parent: u32, key: u32) -> Option<u32> {
        if key == 0 {
            return None;
        }
        self.nodes
            .iter()
            .find(|n| n.node.parent == parent && n.node.key == key)
            .map(|n| n.id)
    }

    /// Whether the node still declares this event.
    pub fn has_event(&self, node: u32, event: u32) -> bool {
        self.nodes
            .iter()
            .any(|n| n.id == node && n.node.event == Some(event))
    }

    /// Validates typed event data against the live node binding.
    pub fn accepts_value(&self, node: u32, event: u32, value: &EventValue) -> bool {
        let Some(entry) = self
            .nodes
            .iter()
            .find(|n| n.id == node && n.node.event == Some(event))
        else {
            return false;
        };
        if is_control(entry.node.tag) && entry.node.disabled {
            return false;
        }
        match (entry.node.tag, value) {
            (BrowserTag::Input | BrowserTag::TextArea, EventValue::Text(value)) => {
                valid_value(entry.node.tag, value)
            }
            (BrowserTag::Checkbox, EventValue::Boolean(_)) => true,
            (BrowserTag::Button | BrowserTag::Form, EventValue::None) => true,
            _ => false,
        }
    }

    /// Validates and encodes a transition. Commit the returned snapshot with its batch.
    pub fn update(&self, view: &View, view_id: u32) -> Result<(Self, BrowserCommands), ViewError> {
        if view_id == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        let next = self.resolve(view)?;
        let live: BTreeSet<_> = next.nodes.iter().map(|n| n.id).collect();
        let old: BTreeMap<_, _> = self.nodes.iter().map(|n| (n.id, n)).collect();
        let removed: BTreeSet<_> = self
            .nodes
            .iter()
            .filter(|n| !live.contains(&n.id))
            .map(|n| n.id)
            .collect();
        let mut commands = BrowserCommands::new();
        for entry in &self.nodes {
            if removed.contains(&entry.id) && !removed.contains(&entry.node.parent) {
                commands.remove_node(view_id, entry.id)?;
            }
        }
        let mut order: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for entry in &self.nodes {
            if live.contains(&entry.id) {
                order.entry(entry.node.parent).or_default().push(entry.id);
            }
        }
        for entry in &next.nodes {
            if !old.contains_key(&entry.id) {
                Self::emit(entry, &mut commands, view_id)?;
                order.entry(entry.node.parent).or_default().push(entry.id);
            } else {
                let previous = &old[&entry.id].node;
                if entry.node.link != previous.link {
                    commands.link(
                        view_id,
                        entry.id,
                        entry
                            .node
                            .link
                            .as_ref()
                            .ok_or(ViewError::InvalidStructure)?,
                    )?;
                }
                if entry.node.text != previous.text {
                    commands.text(
                        view_id,
                        entry.id,
                        entry
                            .node
                            .text
                            .as_deref()
                            .ok_or(ViewError::InvalidStructure)?,
                    )?;
                }
                if let (Some(field), Some(old)) = (&entry.node.field, &previous.field) {
                    if field.epoch < old.epoch {
                        return Err(ViewError::InvalidState);
                    }
                    if field.value != old.value || field.epoch != old.epoch {
                        if let Some(value) = &field.value {
                            commands.value(view_id, entry.id, value, field.epoch)?;
                        }
                    }
                    if field.checked != old.checked || field.epoch != old.epoch {
                        if let Some(checked) = field.checked {
                            commands.checked(view_id, entry.id, checked, field.epoch)?;
                        }
                    }
                    if field.invalid != old.invalid {
                        commands.invalid(view_id, entry.id, field.invalid)?;
                    }
                }
                if entry.node.disabled != previous.disabled {
                    commands.disabled(view_id, entry.id, entry.node.disabled)?;
                }
            }
        }
        let parents = std::iter::once(0).chain(next.nodes.iter().map(|n| n.id));
        for parent in parents {
            let desired: Vec<_> = next
                .nodes
                .iter()
                .filter(|n| n.node.parent == parent)
                .map(|n| n.id)
                .collect();
            let children = order.entry(parent).or_default();
            let mut before = 0;
            for id in desired.into_iter().rev() {
                let position = children
                    .iter()
                    .position(|n| *n == id)
                    .ok_or(ViewError::InvalidStructure)?;
                if children.get(position + 1).copied().unwrap_or(0) != before {
                    commands.place(view_id, id, before)?;
                    children.remove(position);
                    let index = if before == 0 {
                        children.len()
                    } else {
                        children
                            .iter()
                            .position(|n| *n == before)
                            .ok_or(ViewError::InvalidStructure)?
                    };
                    children.insert(index, id);
                }
                before = id;
            }
        }
        Ok((next, commands))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_handles_do_not_wrap() {
        let mut view = View::new();
        view.keyed_button(0, 1, "Run", 1, false).unwrap();
        let mut snapshot = view.snapshot().unwrap();
        snapshot.next = u32::MAX;
        let mut changed = View::new();
        changed.keyed_button(0, 2, "Other", 1, false).unwrap();
        assert!(matches!(
            snapshot.update(&changed, 1),
            Err(ViewError::Limit)
        ));
        assert_eq!(snapshot.node_for_key(0, 1), Some(1));
    }
}
