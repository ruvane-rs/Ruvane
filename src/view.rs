//! Shared views for HTML rendering and browser updates.
use crate::{
    BrowserCommandError, BrowserCommands, BrowserTag, Html, MAX_BROWSER_TEXT_BYTES, ReactiveError,
    text,
};
use std::fmt;

/// Maximum elements in a shared view.
pub const MAX_VIEW_NODES: usize = 32;
/// Maximum element depth below the host root.
pub const MAX_VIEW_DEPTH: usize = 8;
/// Maximum click handlers in a view.
pub const MAX_VIEW_EVENTS: usize = 16;

/// Initial state supported by the hydration format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialState {
    /// An unsigned 32-bit number.
    Number(u32),
    /// A boolean.
    Boolean(bool),
}

impl InitialState {
    /// Decodes numeric ABI fields. Kinds are 1=number and 2=boolean.
    pub fn from_wire(kind: u32, value: u32) -> Result<Self, ViewError> {
        match (kind, value) {
            (1, value) => Ok(Self::Number(value)),
            (2, 0) => Ok(Self::Boolean(false)),
            (2, 1) => Ok(Self::Boolean(true)),
            _ => Err(ViewError::InvalidState),
        }
    }

    /// Encodes numeric ABI fields.
    pub fn to_wire(self) -> (u32, u32) {
        match self {
            Self::Number(value) => (1, value),
            Self::Boolean(value) => (2, u32::from(value)),
        }
    }
}

/// A component ID and its initial state. IDs belong to the application's registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentProps {
    component: u32,
    state: InitialState,
}

impl ComponentProps {
    /// Creates props for a nonzero component ID.
    pub fn new(component: u32, state: InitialState) -> Result<Self, ViewError> {
        if component == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        Ok(Self { component, state })
    }

    /// Application component ID.
    pub fn component(self) -> u32 {
        self.component
    }

    /// Initial state, visible to anyone who receives the page.
    pub fn state(self) -> InitialState {
        self.state
    }
}

/// Shared-view validation errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewError {
    /// An ID is zero or refers to an unknown parent.
    InvalidIdentifier,
    /// The tag or parent is outside the shared-view HTML profile.
    InvalidStructure,
    /// A node, depth, event, text or command limit was exceeded.
    Limit,
    /// State kind or boolean encoding is invalid.
    InvalidState,
    /// Text contains controls that cannot round-trip through HTML.
    InvalidText,
    /// A view must contain at least one element.
    Empty,
    /// An update changed tags, parents or event bindings.
    ShapeChanged,
}

impl fmt::Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentifier => "invalid view identifier",
            Self::InvalidStructure => "invalid view structure",
            Self::Limit => "view limit exceeded",
            Self::InvalidState => "invalid initial state",
            Self::InvalidText => "invalid view text",
            Self::Empty => "empty view",
            Self::ShapeChanged => "view structure changed",
        })
    }
}
impl std::error::Error for ViewError {}
impl From<BrowserCommandError> for ViewError {
    fn from(error: BrowserCommandError) -> Self {
        match error {
            BrowserCommandError::InvalidIdentifier => Self::InvalidIdentifier,
            BrowserCommandError::Limit => Self::Limit,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Node {
    parent: u32,
    depth: usize,
    tag: BrowserTag,
    text: Option<String>,
    event: Option<u32>,
    disabled: bool,
}

/// A small element tree shared by server rendering and Wasm.
/// Node IDs follow insertion order. Parent zero is the host root.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct View {
    nodes: Vec<Node>,
}

impl View {
    /// Creates an empty view.
    pub fn new() -> Self {
        Self::default()
    }

    fn push(
        &mut self,
        parent: u32,
        tag: BrowserTag,
        label: Option<&str>,
        event: Option<u32>,
        disabled: bool,
    ) -> Result<u32, ViewError> {
        if self.nodes.len() == MAX_VIEW_NODES {
            return Err(ViewError::Limit);
        }
        let depth = if parent == 0 {
            1
        } else {
            let owner = self
                .nodes
                .get(parent as usize - 1)
                .ok_or(ViewError::InvalidIdentifier)?;
            if owner.tag != BrowserTag::Section && owner.tag != BrowserTag::Paragraph {
                return Err(ViewError::InvalidStructure);
            }
            if owner.tag == BrowserTag::Paragraph
                && matches!(
                    tag,
                    BrowserTag::Section | BrowserTag::Paragraph | BrowserTag::Heading
                )
            {
                return Err(ViewError::InvalidStructure);
            }
            owner.depth + 1
        };
        if depth > MAX_VIEW_DEPTH
            || (event.is_some()
                && self.nodes.iter().filter(|n| n.event.is_some()).count() == MAX_VIEW_EVENTS)
        {
            return Err(ViewError::Limit);
        }
        if let Some(label) = label {
            if label.len() > MAX_BROWSER_TEXT_BYTES {
                return Err(ViewError::Limit);
            }
            if label
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err(ViewError::InvalidText);
            }
        }
        self.nodes.push(Node {
            parent,
            depth,
            tag,
            text: label.map(str::to_owned),
            event,
            disabled,
        });
        Ok(self.nodes.len() as u32)
    }

    /// Adds a section or paragraph container.
    pub fn element(&mut self, parent: u32, tag: BrowserTag) -> Result<u32, ViewError> {
        if !matches!(tag, BrowserTag::Section | BrowserTag::Paragraph) {
            return Err(ViewError::InvalidStructure);
        }
        self.push(parent, tag, None, None, false)
    }

    /// Adds a heading, span or output containing plain text.
    pub fn leaf(&mut self, parent: u32, tag: BrowserTag, label: &str) -> Result<u32, ViewError> {
        if !matches!(
            tag,
            BrowserTag::Heading | BrowserTag::Span | BrowserTag::Output
        ) {
            return Err(ViewError::InvalidStructure);
        }
        self.push(parent, tag, Some(label), None, false)
    }

    /// Adds a button with a nonzero event ID.
    pub fn button(
        &mut self,
        parent: u32,
        label: &str,
        event: u32,
        disabled: bool,
    ) -> Result<u32, ViewError> {
        if event == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        self.push(
            parent,
            BrowserTag::Button,
            Some(label),
            Some(event),
            disabled,
        )
    }

    /// Whether this view declares the event.
    pub fn has_event(&self, event: u32) -> bool {
        self.nodes.iter().any(|n| n.event == Some(event))
    }

    fn check(&self) -> Result<(), ViewError> {
        if self.nodes.is_empty() {
            Err(ViewError::Empty)
        } else {
            Ok(())
        }
    }

    /// Encodes the initial DOM and event bindings.
    pub fn commands(&self, view: u32) -> Result<BrowserCommands, ViewError> {
        self.check()?;
        let mut commands = BrowserCommands::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let id = index as u32 + 1;
            commands.element(view, id, node.parent, node.tag)?;
            if let Some(label) = &node.text {
                commands.text(view, id, label)?;
            }
            if let Some(event) = node.event {
                commands.click(view, id, event)?;
            }
            if node.tag == BrowserTag::Button {
                commands.disabled(view, id, node.disabled)?;
            }
        }
        Ok(commands)
    }

    /// Encodes changed text and button state. Structure and event IDs stay fixed.
    pub fn updates(&self, previous: &Self, view: u32) -> Result<BrowserCommands, ViewError> {
        self.check()?;
        if view == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        if self.nodes.len() != previous.nodes.len() {
            return Err(ViewError::ShapeChanged);
        }
        let mut commands = BrowserCommands::new();
        for (index, (next, old)) in self.nodes.iter().zip(&previous.nodes).enumerate() {
            if next.parent != old.parent || next.tag != old.tag || next.event != old.event {
                return Err(ViewError::ShapeChanged);
            }
            let id = index as u32 + 1;
            if next.text != old.text {
                commands
                    .text(
                        view,
                        id,
                        next.text.as_deref().ok_or(ViewError::ShapeChanged)?,
                    )
                    .map_err(|_| ViewError::Limit)?;
            }
            if next.disabled != old.disabled {
                commands
                    .disabled(view, id, next.disabled)
                    .map_err(|_| ViewError::Limit)?;
            }
        }
        Ok(commands)
    }

    fn markup(&self, parent: u32, inert: bool, output: &mut String) {
        use std::fmt::Write;
        for (index, node) in self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.parent == parent)
        {
            let id = index as u32 + 1;
            let tag = match node.tag {
                BrowserTag::Section => "section",
                BrowserTag::Heading => "h2",
                BrowserTag::Paragraph => "p",
                BrowserTag::Span => "span",
                BrowserTag::Output => "output",
                BrowserTag::Button => "button",
            };
            write!(output, "<{tag} data-ruvane-node=\"{id}\"").unwrap();
            if let Some(event) = node.event {
                write!(output, " type=\"button\" data-ruvane-click=\"{event}\"").unwrap();
            }
            if node.tag == BrowserTag::Button && (inert || node.disabled) {
                output.push_str(" disabled");
            }
            output.push('>');
            if let Some(label) = &node.text {
                output.push_str(text(label).as_str());
            }
            self.markup(id, inert, output);
            write!(output, "</{tag}>").unwrap();
        }
    }

    /// Renders HTML with stable node markers.
    pub fn render(&self) -> Result<Html, ViewError> {
        self.check()?;
        let mut output = String::new();
        self.markup(0, false, &mut output);
        Ok(Html::rendered(output))
    }

    /// Wraps server HTML with versioned props. Buttons stay disabled until hydration.
    pub fn hydratable(&self, props: ComponentProps) -> Result<Html, ViewError> {
        self.check()?;
        let (kind, value) = props.state.to_wire();
        let mut output = format!(
            "<div data-ruvane-version=\"{}\" data-ruvane-component=\"{}\" data-ruvane-state=\"{kind}:{value}\">",
            crate::BROWSER_ABI_VERSION,
            props.component
        );
        self.markup(0, true, &mut output);
        output.push_str("</div>");
        Ok(Html::rendered(output))
    }
}

/// Reactive resources owned by a component.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComponentStats {
    /// Live signals, including memo caches.
    pub signals: usize,
    /// Live effects, including memo computations.
    pub effects: usize,
}

/// Application component errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentError {
    /// The registry does not accept these props.
    InvalidProps,
    /// The component does not accept this event.
    InvalidEvent,
    /// A reactive computation failed.
    Reactive,
    /// Building the view failed.
    View(ViewError),
}
impl fmt::Display for ComponentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidProps => "invalid component props",
            Self::InvalidEvent => "invalid component event",
            Self::Reactive => "component reactive failure",
            Self::View(_) => "invalid component view",
        })
    }
}
impl std::error::Error for ComponentError {}
impl From<ReactiveError> for ComponentError {
    fn from(_: ReactiveError) -> Self {
        Self::Reactive
    }
}
impl From<ViewError> for ComponentError {
    fn from(error: ViewError) -> Self {
        Self::View(error)
    }
}

/// A reusable component. Dropping it must release its owned resources.
pub trait Component {
    /// Describes the current view without changing state.
    fn view(&self) -> Result<View, ComponentError>;
    /// Handles a declared event.
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError>;
    /// Reports reactive resources. Components without a reactive graph return zero.
    fn stats(&self) -> ComponentStats {
        ComponentStats::default()
    }
}
