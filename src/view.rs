//! Shared views for HTML rendering and browser updates.
use crate::{
    BrowserCommandError, BrowserCommands, BrowserTag, Html, MAX_BROWSER_TEXT_BYTES, ReactiveError,
    text,
};
use std::fmt;
mod fields;
mod snapshot;
pub use fields::EventValue;
use fields::{Control, accepts, is_control, valid_value};
pub use snapshot::ViewSnapshot;

/// Maximum elements in a shared view.
pub const MAX_VIEW_NODES: usize = 32;
/// Maximum element depth below the host root.
pub const MAX_VIEW_DEPTH: usize = 8;
/// Maximum logical event bindings in a view.
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
    /// Two siblings declare the same nonzero key.
    DuplicateKey,
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
            Self::DuplicateKey => "duplicate sibling key",
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
    key: u32,
    depth: usize,
    tag: BrowserTag,
    text: Option<String>,
    event: Option<u32>,
    disabled: bool,
    field: Option<Control>,
    link: Option<crate::InternalUrl>,
    prefetch: bool,
}

/// A small element tree shared by server rendering and Wasm.
/// Builder IDs follow insertion order. Explicit keys identify siblings across updates.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct View {
    nodes: Vec<Node>,
}

impl View {
    /// Adds an internal attachment link handled directly by the browser.
    pub fn download(
        &mut self,
        parent: u32,
        key: u32,
        url: &crate::InternalUrl,
        label: &str,
    ) -> Result<u32, ViewError> {
        if key == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        let id = self.push(parent, key, BrowserTag::Download, Some(label), None, false)?;
        self.nodes[id as usize - 1].link = Some(url.clone());
        Ok(id)
    }
    /// Adds an internal link with a nonzero sibling key.
    pub fn link(
        &mut self,
        parent: u32,
        key: u32,
        url: &crate::InternalUrl,
        label: &str,
    ) -> Result<u32, ViewError> {
        if key == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        let id = self.push(parent, key, BrowserTag::Link, Some(label), None, false)?;
        self.nodes[id as usize - 1].link = Some(url.clone());
        Ok(id)
    }
    /// Adds a native link eligible for safe, read-only speculative loading.
    pub fn prefetch_link(
        &mut self,
        parent: u32,
        key: u32,
        url: &crate::InternalUrl,
        label: &str,
    ) -> Result<u32, ViewError> {
        let id = self.link(parent, key, url, label)?;
        self.nodes[id as usize - 1].prefetch = true;
        Ok(id)
    }
    /// Number of logical event bindings in this view.
    pub fn event_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.event.is_some())
            .count()
    }
    /// Creates an empty view.
    pub fn new() -> Self {
        Self::default()
    }

    fn push(
        &mut self,
        parent: u32,
        key: u32,
        tag: BrowserTag,
        label: Option<&str>,
        event: Option<u32>,
        disabled: bool,
    ) -> Result<u32, ViewError> {
        if self.nodes.len() == MAX_VIEW_NODES {
            return Err(ViewError::Limit);
        }
        if key != 0
            && self
                .nodes
                .iter()
                .any(|n| n.parent == parent && n.key == key)
        {
            return Err(ViewError::DuplicateKey);
        }
        let parent_tag = if parent == 0 {
            None
        } else {
            Some(
                self.nodes
                    .get(parent as usize - 1)
                    .ok_or(ViewError::InvalidIdentifier)?
                    .tag,
            )
        };
        if !accepts(parent_tag, tag) {
            return Err(ViewError::InvalidStructure);
        }
        let mut ancestor = parent;
        let mut in_form = false;
        while ancestor != 0 {
            let node = &self.nodes[ancestor as usize - 1];
            in_form |= node.tag == BrowserTag::Form;
            ancestor = node.parent;
        }
        if (tag == BrowserTag::Form && in_form) || (tag == BrowserTag::Submit && !in_form) {
            return Err(ViewError::InvalidStructure);
        }
        let depth = if parent == 0 {
            1
        } else {
            self.nodes[parent as usize - 1].depth + 1
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
            key,
            depth,
            tag,
            text: label.map(str::to_owned),
            event,
            disabled,
            field: None,
            link: None,
            prefetch: false,
        });
        Ok(self.nodes.len() as u32)
    }

    /// Adds a section, paragraph, list or list-item container.
    pub fn element(&mut self, parent: u32, tag: BrowserTag) -> Result<u32, ViewError> {
        if !matches!(
            tag,
            BrowserTag::Section
                | BrowserTag::Paragraph
                | BrowserTag::List
                | BrowserTag::ListItem
                | BrowserTag::Label
        ) {
            return Err(ViewError::InvalidStructure);
        }
        self.push(parent, 0, tag, None, None, false)
    }

    /// Adds a heading, span or output containing plain text.
    pub fn leaf(&mut self, parent: u32, tag: BrowserTag, label: &str) -> Result<u32, ViewError> {
        if !matches!(
            tag,
            BrowserTag::Heading | BrowserTag::Span | BrowserTag::Output
        ) {
            return Err(ViewError::InvalidStructure);
        }
        self.push(parent, 0, tag, Some(label), None, false)
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
            0,
            BrowserTag::Button,
            Some(label),
            Some(event),
            disabled,
        )
    }

    /// Adds a container with a nonzero sibling key.
    pub fn keyed_element(
        &mut self,
        parent: u32,
        key: u32,
        tag: BrowserTag,
    ) -> Result<u32, ViewError> {
        if key == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        if !matches!(
            tag,
            BrowserTag::Section
                | BrowserTag::Paragraph
                | BrowserTag::List
                | BrowserTag::ListItem
                | BrowserTag::Label
        ) {
            return Err(ViewError::InvalidStructure);
        }
        self.push(parent, key, tag, None, None, false)
    }

    /// Adds plain text with a nonzero sibling key.
    pub fn keyed_leaf(
        &mut self,
        parent: u32,
        key: u32,
        tag: BrowserTag,
        label: &str,
    ) -> Result<u32, ViewError> {
        if key == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        if !matches!(
            tag,
            BrowserTag::Heading | BrowserTag::Span | BrowserTag::Output
        ) {
            return Err(ViewError::InvalidStructure);
        }
        self.push(parent, key, tag, Some(label), None, false)
    }

    /// Adds a button with a nonzero sibling key and event ID.
    pub fn keyed_button(
        &mut self,
        parent: u32,
        key: u32,
        label: &str,
        event: u32,
        disabled: bool,
    ) -> Result<u32, ViewError> {
        if key == 0 || event == 0 {
            return Err(ViewError::InvalidIdentifier);
        }
        self.push(
            parent,
            key,
            BrowserTag::Button,
            Some(label),
            Some(event),
            disabled,
        )
    }

    /// Assigns initial DOM handles. Keep the returned snapshot for future updates.
    pub fn snapshot(&self) -> Result<ViewSnapshot, ViewError> {
        ViewSnapshot::new(self)
    }

    /// Whether this view declares the event.
    pub fn has_event(&self, event: u32) -> bool {
        self.nodes.iter().any(|n| n.event == Some(event))
    }

    fn check(&self) -> Result<(), ViewError> {
        if self.nodes.is_empty() {
            Err(ViewError::Empty)
        } else {
            for (index, node) in self.nodes.iter().enumerate() {
                if matches!(node.tag, BrowserTag::Link | BrowserTag::Download)
                    != node.link.is_some()
                {
                    return Err(ViewError::InvalidStructure);
                }
                if node.tag == BrowserTag::Label {
                    let children: Vec<_> = self
                        .nodes
                        .iter()
                        .filter(|n| n.parent == index as u32 + 1)
                        .collect();
                    if children.iter().filter(|n| is_control(n.tag)).count() != 1
                        || !children.iter().any(|n| {
                            n.tag == BrowserTag::Span
                                && n.text.as_ref().is_some_and(|s| !s.trim().is_empty())
                        })
                    {
                        return Err(ViewError::InvalidStructure);
                    }
                }
            }
            Ok(())
        }
    }

    /// Encodes the initial DOM and event bindings.
    pub fn commands(&self, view: u32) -> Result<BrowserCommands, ViewError> {
        self.snapshot()?.commands(view)
    }

    /// Encodes one transition from a fresh previous view.
    /// For repeated updates, retain a `ViewSnapshot` so retired handles stay retired.
    pub fn updates(&self, previous: &Self, view: u32) -> Result<BrowserCommands, ViewError> {
        previous
            .snapshot()?
            .update(self, view)
            .map(|(_, commands)| commands)
    }

    fn markup(&self, parent: u32, inert: bool, editable: bool, output: &mut String) {
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
                BrowserTag::Button | BrowserTag::Submit => "button",
                BrowserTag::List => "ul",
                BrowserTag::ListItem => "li",
                BrowserTag::Form => "form",
                BrowserTag::Label => "label",
                BrowserTag::Input | BrowserTag::Checkbox | BrowserTag::File => "input",
                BrowserTag::TextArea => "textarea",
                BrowserTag::Link | BrowserTag::Download => "a",
            };
            write!(output, "<{tag} data-ruvane-node=\"{id}\"").unwrap();
            if node.key != 0 {
                write!(output, " data-ruvane-key=\"{}\"", node.key).unwrap();
            }
            match node.tag {
                BrowserTag::Link | BrowserTag::Download => {
                    output.push_str(if node.tag == BrowserTag::Download {
                        " download=\"\""
                    } else {
                        " data-ruvane-nav=\"\""
                    });
                    if node.prefetch {
                        output.push_str(" data-ruvane-prefetch=\"\"");
                    }
                    output.push_str(" href=\"");
                    output.push_str(text(node.link.as_ref().unwrap().as_str()).as_str());
                    output.push('"');
                }
                BrowserTag::Button => output.push_str(" type=\"button\""),
                BrowserTag::Submit => output.push_str(" type=\"submit\""),
                BrowserTag::Form => output.push_str(" novalidate"),
                BrowserTag::Input => output.push_str(" type=\"text\" maxlength=\"1024\""),
                BrowserTag::Checkbox => output.push_str(" type=\"checkbox\""),
                BrowserTag::File => output.push_str(" type=\"file\""),
                BrowserTag::TextArea => output.push_str(" maxlength=\"1024\""),
                _ => {}
            }
            if let Some(event) = node.event {
                let name = match node.tag {
                    BrowserTag::Form => "submit",
                    BrowserTag::Input | BrowserTag::TextArea => "input",
                    BrowserTag::Checkbox | BrowserTag::File => "change",
                    _ => "click",
                };
                write!(output, " data-ruvane-{name}=\"{event}\"").unwrap();
            }
            if let Some(field) = &node.field {
                write!(
                    output,
                    " aria-invalid=\"{}\" data-ruvane-epoch=\"{}\"",
                    field.invalid, field.epoch
                )
                .unwrap();
                if node.tag == BrowserTag::Input {
                    write!(
                        output,
                        " value=\"{}\"",
                        text(field.value.as_deref().unwrap()).as_str()
                    )
                    .unwrap();
                }
                if field.checked == Some(true) {
                    output.push_str(" checked");
                }
            }
            if (matches!(node.tag, BrowserTag::Button | BrowserTag::Submit) || is_control(node.tag))
                && (node.disabled
                    || inert
                        && !(editable
                            && matches!(node.tag, BrowserTag::Input | BrowserTag::TextArea)))
            {
                output.push_str(" disabled");
            }
            output.push('>');
            if matches!(
                node.tag,
                BrowserTag::Input | BrowserTag::Checkbox | BrowserTag::File
            ) {
                continue;
            }
            if let Some(label) = &node.text {
                output.push_str(text(label).as_str());
            }
            if node.tag == BrowserTag::TextArea {
                let value = node.field.as_ref().unwrap().value.as_deref().unwrap();
                if value.starts_with('\n') {
                    output.push('\n');
                }
                output.push_str(text(value).as_str());
            }
            self.markup(id, inert, editable, output);
            write!(output, "</{tag}>").unwrap();
        }
    }

    /// Renders HTML with stable node markers.
    pub fn render(&self) -> Result<Html, ViewError> {
        self.check()?;
        let mut output = String::new();
        self.markup(0, false, false, &mut output);
        Ok(Html::rendered(output))
    }

    /// Wraps server HTML with versioned props. Controls stay disabled until hydration.
    pub fn hydratable(&self, props: ComponentProps) -> Result<Html, ViewError> {
        self.hydration_markup(props, false)
    }
    /// Marks an independent shell view for opted-in early hydration.
    /// Names must be unique in the application shell. Controls start disabled.
    pub fn shell_island(&self, name: &str, props: ComponentProps) -> Result<Html, ViewError> {
        if name.is_empty()
            || name.len() > 32
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphabetic() || byte == b'_' || index > 0 && byte.is_ascii_digit()
            })
        {
            return Err(ViewError::InvalidIdentifier);
        }
        let view = self.hydratable(props)?;
        Ok(Html::rendered(format!(
            "<div data-ruvane-shell=\"{name}\">{}</div><br hidden data-ruvane-shell-ready=\"{name}\">",
            view.as_str()
        )))
    }
    pub(crate) fn hydratable_page(&self, props: ComponentProps) -> Result<Html, ViewError> {
        self.hydration_markup(props, true)
    }
    fn hydration_markup(&self, props: ComponentProps, editable: bool) -> Result<Html, ViewError> {
        self.check()?;
        let (kind, value) = props.state.to_wire();
        let mut output = format!(
            "<div data-ruvane-version=\"{}\" data-ruvane-component=\"{}\" data-ruvane-state=\"{kind}:{value}\">",
            crate::BROWSER_ABI_VERSION,
            props.component
        );
        self.markup(0, true, editable, &mut output);
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
    /// Takes one queued upload, sharing the pending slot with forms and JSON.
    fn take_upload(&mut self) -> Option<crate::UploadRequest> {
        None
    }
    /// Receives a completion after the live upload owner and request token are checked.
    fn upload_result(
        &mut self,
        _action: u32,
        _outcome: crate::UploadOutcome,
    ) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }

    /// Takes one queued JSON request. Shares the pending slot with forms and uploads.
    fn take_json(&mut self) -> Option<crate::JsonRequest> {
        None
    }
    /// Receives a JSON completion after its live view and request identity are checked.
    fn json_result(
        &mut self,
        _action: u32,
        _outcome: crate::JsonOutcome,
    ) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
    /// Applies a public navigation seed before the first view. Page mounts forbid initial actions.
    fn page_data(&mut self, _data: crate::ActionData) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidProps)
    }
    /// Takes one queued action. Only one action per live component may be pending.
    fn take_action(&mut self) -> Option<crate::ActionRequest> {
        None
    }
    /// Receives an action completion after its view/request identity is checked.
    fn action_result(
        &mut self,
        _action: u32,
        _outcome: crate::ActionOutcome,
    ) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
    /// Describes the current view without changing state.
    fn view(&self) -> Result<View, ComponentError>;
    /// Handles a declared event.
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError>;
    /// Handles typed control data. The default accepts only data-free events.
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match value {
            EventValue::None => self.dispatch(event),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    /// Reports reactive resources. Components without a reactive graph return zero.
    fn stats(&self) -> ComponentStats {
        ComponentStats::default()
    }
}
