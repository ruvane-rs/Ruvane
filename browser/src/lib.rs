//! Browser registry for application components.
#![deny(unsafe_code)]
#![deny(missing_docs)]

use ruvane::{
    BrowserCommands, Component, ComponentError, ComponentProps, EventValue, View, ViewSnapshot,
};
use std::collections::BTreeMap;
use std::fmt;

// Export attributes are isolated in the Wasm ABI.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[allow(unsafe_code)]
mod abi;

/// Components shared with the example server. Applications supply their own registry.
#[path = "../../examples/support/widgets.rs"]
pub mod demo;

/// Maximum live views per Wasm instance.
pub const MAX_BROWSER_INSTANCES: usize = 16;
/// Maximum live signals or effects reported by one component.
pub const MAX_COMPONENT_RESOURCES: usize = 1024;
/// Application factory used by the browser registry.
pub type ComponentFactory = fn(ComponentProps) -> Result<Box<dyn Component>, ComponentError>;

/// Errors exposed as numeric ABI codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BrowserError {
    /// A root is zero or already registered.
    InvalidRoot = 1,
    /// A resource or identifier limit was reached.
    Limit = 2,
    /// The view is unknown or disposed.
    InvalidHandle = 3,
    /// The view does not declare this event.
    InvalidEvent = 4,
    /// A command batch is awaiting acknowledgement, or the ABI is busy.
    Busy = 5,
    /// A reactive computation failed.
    Reactive = 6,
    /// The application rejected the component props.
    InvalidProps = 7,
    /// The component returned an invalid view or patch.
    InvalidView = 8,
    /// Event data has the wrong type, encoding or size.
    InvalidPayload = 9,
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRoot => "invalid or duplicate browser root",
            Self::Limit => "browser runtime limit exceeded",
            Self::InvalidHandle => "unknown browser view",
            Self::InvalidEvent => "unknown browser event",
            Self::Busy => "browser runtime busy",
            Self::Reactive => "browser reactive computation failed",
            Self::InvalidProps => "invalid component props",
            Self::InvalidView => "invalid component view",
            Self::InvalidPayload => "invalid event payload",
        })
    }
}
impl std::error::Error for BrowserError {}
impl From<ComponentError> for BrowserError {
    fn from(error: ComponentError) -> Self {
        match error {
            ComponentError::InvalidProps => Self::InvalidProps,
            ComponentError::InvalidEvent => Self::InvalidEvent,
            ComponentError::Reactive => Self::Reactive,
            ComponentError::View(_) => Self::InvalidView,
        }
    }
}

struct Instance {
    root: u32,
    component: Box<dyn Component>,
    view: View,
    snapshot: ViewSnapshot,
}

struct PendingText {
    view: u32,
    node: u32,
    event: u32,
    length: usize,
    bytes: Vec<u8>,
}

/// Owns components and encodes their DOM updates. The factory belongs to the application.
pub struct BrowserRuntime {
    instances: BTreeMap<u32, Instance>,
    buffer: BrowserCommands,
    factory: ComponentFactory,
    next: u32,
    pending: Option<PendingText>,
}

impl BrowserRuntime {
    /// Creates an empty registry.
    pub fn new(factory: ComponentFactory) -> Self {
        Self {
            instances: BTreeMap::new(),
            buffer: BrowserCommands::new(),
            factory,
            next: 0,
            pending: None,
        }
    }

    fn ready(&self) -> Result<(), BrowserError> {
        if self.buffer.is_empty() && self.pending.is_none() {
            Ok(())
        } else {
            Err(BrowserError::Busy)
        }
    }

    fn resources(component: &dyn Component) -> Result<(), BrowserError> {
        let stats = component.stats();
        if stats.signals > MAX_COMPONENT_RESOURCES || stats.effects > MAX_COMPONENT_RESOURCES {
            Err(BrowserError::Limit)
        } else {
            Ok(())
        }
    }

    /// Creates a component on a unique nonzero host root and queues its DOM.
    pub fn mount(&mut self, root: u32, props: ComponentProps) -> Result<u32, BrowserError> {
        self.ready()?;
        if root == 0 || self.instances.values().any(|c| c.root == root) {
            return Err(BrowserError::InvalidRoot);
        }
        if self.instances.len() == MAX_BROWSER_INSTANCES {
            return Err(BrowserError::Limit);
        }
        let id = self.next.checked_add(1).ok_or(BrowserError::Limit)?;
        self.next = id;
        let component = (self.factory)(props)?;
        Self::resources(component.as_ref())?;
        let view = component.view()?;
        let snapshot = view.snapshot().map_err(|_| BrowserError::InvalidView)?;
        let commands = snapshot
            .commands(id)
            .map_err(|_| BrowserError::InvalidView)?;
        self.instances.insert(
            id,
            Instance {
                root,
                component,
                view,
                snapshot,
            },
        );
        self.buffer = commands;
        Ok(id)
    }

    /// Dispatches an application event through one of its live nodes.
    pub fn dispatch(&mut self, view: u32, event: u32) -> Result<(), BrowserError> {
        self.ready()?;
        let instance = self
            .instances
            .get(&view)
            .ok_or(BrowserError::InvalidHandle)?;
        let node = instance
            .snapshot
            .node_for_event(event)
            .ok_or(BrowserError::InvalidEvent)?;
        self.dispatch_node(view, node, event)
    }

    /// Runs a node-bound event. Retired node handles cannot trigger new bindings.
    pub fn dispatch_node(&mut self, view: u32, node: u32, event: u32) -> Result<(), BrowserError> {
        self.dispatch_value(view, node, event, EventValue::None)
    }

    /// Dispatches validated control data to a live binding.
    pub fn dispatch_value(
        &mut self,
        view: u32,
        node: u32,
        event: u32,
        value: EventValue,
    ) -> Result<(), BrowserError> {
        self.ready()?;
        let instance = self
            .instances
            .get_mut(&view)
            .ok_or(BrowserError::InvalidHandle)?;
        if !instance.snapshot.has_event(node, event) {
            return Err(BrowserError::InvalidEvent);
        }
        if !instance.snapshot.accepts_value(node, event, &value) {
            return Err(BrowserError::InvalidPayload);
        }
        let update = (|| {
            instance.component.dispatch_value(event, value)?;
            Self::resources(instance.component.as_ref())?;
            let next = instance.component.view()?;
            let (snapshot, commands) = instance
                .snapshot
                .update(&next, view)
                .map_err(|_| BrowserError::InvalidView)?;
            Ok::<_, BrowserError>((next, snapshot, commands))
        })();
        match update {
            Ok((next, snapshot, commands)) => {
                instance.view = next;
                instance.snapshot = snapshot;
                self.buffer = commands;
                Ok(())
            }
            Err(error) => {
                self.instances.remove(&view);
                self.buffer.clear();
                Err(error)
            }
        }
    }

    /// Starts one bounded text payload. Other mutations wait until commit or clear.
    pub fn begin_text(
        &mut self,
        view: u32,
        node: u32,
        event: u32,
        length: u32,
    ) -> Result<(), BrowserError> {
        self.ready()?;
        if length as usize > ruvane::MAX_BROWSER_TEXT_BYTES {
            return Err(BrowserError::InvalidPayload);
        }
        let instance = self
            .instances
            .get(&view)
            .ok_or(BrowserError::InvalidHandle)?;
        if !instance.snapshot.has_event(node, event) {
            return Err(BrowserError::InvalidEvent);
        }
        if !instance
            .snapshot
            .accepts_value(node, event, &EventValue::Text(String::new()))
        {
            return Err(BrowserError::InvalidPayload);
        }
        self.pending = Some(PendingText {
            view,
            node,
            event,
            length: length as usize,
            bytes: Vec::with_capacity(length as usize),
        });
        Ok(())
    }

    /// Appends one to four little-endian bytes. Invalid chunks discard the payload.
    pub fn text_chunk(&mut self, word: u32, count: u32) -> Result<(), BrowserError> {
        let mut pending = self.pending.take().ok_or(BrowserError::InvalidPayload)?;
        if !(1..=4).contains(&count)
            || (count < 4 && word >> (count * 8) != 0)
            || pending.bytes.len() + count as usize > pending.length
        {
            return Err(BrowserError::InvalidPayload);
        }
        pending
            .bytes
            .extend_from_slice(&word.to_le_bytes()[..count as usize]);
        self.pending = Some(pending);
        Ok(())
    }

    /// Decodes the numeric ABI payload: 0=none, 1=staged text, 2=boolean.
    pub fn dispatch_wire(
        &mut self,
        view: u32,
        node: u32,
        event: u32,
        kind: u32,
        scalar: u32,
    ) -> Result<(), BrowserError> {
        let value = match (kind, scalar) {
            (0, 0) => EventValue::None,
            (2, 0) => EventValue::Boolean(false),
            (2, 1) => EventValue::Boolean(true),
            (1, 0) => {
                let pending = self.pending.take().ok_or(BrowserError::InvalidPayload)?;
                if pending.view != view
                    || pending.node != node
                    || pending.event != event
                    || pending.bytes.len() != pending.length
                {
                    return Err(BrowserError::InvalidPayload);
                }
                EventValue::Text(
                    String::from_utf8(pending.bytes).map_err(|_| BrowserError::InvalidPayload)?,
                )
            }
            _ => {
                self.pending = None;
                return Err(BrowserError::InvalidPayload);
            }
        };
        self.dispatch_value(view, node, event, value)
    }

    /// Drops a component and queues removal. Stale handles are errors.
    pub fn dispose(&mut self, view: u32) -> Result<(), BrowserError> {
        self.ready()?;
        if !self.instances.contains_key(&view) {
            return Err(BrowserError::InvalidHandle);
        }
        self.buffer.remove(view).map_err(|_| BrowserError::Limit)?;
        self.instances.remove(&view);
        Ok(())
    }

    /// Borrows commands until the next mutation.
    pub fn command_bytes(&self) -> &[u8] {
        self.buffer.as_bytes()
    }
    /// Copies the pending commands for native tests.
    pub fn commands(&self) -> Vec<u8> {
        self.buffer.as_bytes().to_vec()
    }
    /// Acknowledges the pending batch and aborts staged text.
    pub fn clear_commands(&mut self) {
        self.buffer.clear();
        self.pending = None;
    }
    /// Number of live components.
    pub fn live_instances(&self) -> usize {
        self.instances.len()
    }
    /// Live signals, including memo caches.
    pub fn live_signals(&self) -> usize {
        self.instances
            .values()
            .map(|c| c.component.stats().signals)
            .sum()
    }
    /// Live effects, including memo computations.
    pub fn live_effects(&self) -> usize {
        self.instances
            .values()
            .map(|c| c.component.stats().effects)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruvane::InitialState;

    fn counter(value: u32) -> ComponentProps {
        ComponentProps::new(demo::COUNTER, InitialState::Number(value)).unwrap()
    }
    fn toggle(value: bool) -> ComponentProps {
        ComponentProps::new(demo::TOGGLE, InitialState::Boolean(value)).unwrap()
    }

    #[test]
    fn lifecycle_budgets_and_stale_handles() {
        let mut runtime = BrowserRuntime::new(demo::create);
        let mut stale = Vec::new();
        for _ in 0..1000 {
            let id = runtime.mount(1, counter(5)).unwrap();
            assert_eq!((runtime.live_signals(), runtime.live_effects()), (2, 1));
            assert_eq!(runtime.dispatch(id, 1), Err(BrowserError::Busy));
            runtime.clear_commands();
            runtime.dispatch(id, 1).unwrap();
            runtime.clear_commands();
            runtime.dispose(id).unwrap();
            runtime.clear_commands();
            assert_eq!((runtime.live_signals(), runtime.live_effects()), (0, 0));
            stale.push(id);
        }
        let current = runtime.mount(1, counter(0)).unwrap();
        runtime.clear_commands();
        for id in stale {
            assert!(id < current);
            assert_eq!(runtime.dispatch(id, 1), Err(BrowserError::InvalidHandle));
            assert_eq!(runtime.dispose(id), Err(BrowserError::InvalidHandle));
        }
        assert_eq!(runtime.mount(1, counter(0)), Err(BrowserError::InvalidRoot));
        for root in 2..=MAX_BROWSER_INSTANCES as u32 {
            runtime.mount(root, toggle(true)).unwrap();
            runtime.clear_commands();
        }
        assert_eq!(runtime.mount(17, counter(0)), Err(BrowserError::Limit));
        runtime.dispose(current).unwrap();
        runtime.clear_commands();
        runtime.next = u32::MAX;
        assert_eq!(runtime.mount(18, counter(0)), Err(BrowserError::Limit));
    }

    #[test]
    fn props_events_and_two_component_types() {
        let mut runtime = BrowserRuntime::new(demo::create);
        assert_eq!(runtime.mount(0, counter(0)), Err(BrowserError::InvalidRoot));
        for props in [
            counter(1001),
            ComponentProps::new(1, InitialState::Boolean(false)).unwrap(),
            ComponentProps::new(99, InitialState::Number(0)).unwrap(),
        ] {
            assert_eq!(runtime.mount(1, props), Err(BrowserError::InvalidProps));
            assert_eq!(runtime.live_instances(), 0);
        }
        let id = runtime.mount(1, counter(5)).unwrap();
        runtime.clear_commands();
        assert_eq!(runtime.dispatch(id, 999), Err(BrowserError::InvalidEvent));
        for _ in 0..1100 {
            runtime.dispatch(id, 1).unwrap();
            runtime.clear_commands();
        }
        assert!(
            runtime.instances[&id]
                .view
                .render()
                .unwrap()
                .as_str()
                .contains(">2000</output>")
        );
        for _ in 0..1100 {
            runtime.dispatch(id, 2).unwrap();
            runtime.clear_commands();
        }
        let other = runtime.mount(2, toggle(true)).unwrap();
        runtime.clear_commands();
        runtime.dispatch(other, 7).unwrap();
        runtime.clear_commands();
        assert!(
            runtime.instances[&other]
                .view
                .render()
                .unwrap()
                .as_str()
                .contains(">Disabled</output>")
        );
        assert!(
            runtime.instances[&id]
                .view
                .render()
                .unwrap()
                .as_str()
                .contains(">0</output>")
        );
    }

    struct Custom(bool);
    impl Component for Custom {
        fn view(&self) -> Result<View, ComponentError> {
            let mut view = View::new();
            view.button(0, if self.0 { "On" } else { "Off" }, 42, false)?;
            Ok(view)
        }
        fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
            self.0 = !self.0;
            Ok(())
        }
    }

    #[test]
    fn registry_accepts_application_components_without_counter_logic() {
        let mut runtime = BrowserRuntime::new(|_| Ok(Box::new(Custom(false))));
        let id = runtime
            .mount(
                1,
                ComponentProps::new(99, InitialState::Boolean(false)).unwrap(),
            )
            .unwrap();
        runtime.clear_commands();
        runtime.dispatch(id, 42).unwrap();
        assert!(
            runtime.instances[&id]
                .view
                .render()
                .unwrap()
                .as_str()
                .contains(">On</button>")
        );
    }

    #[test]
    fn failed_components_are_dropped_without_pending_commands() {
        use ruvane::{ComponentStats, ViewError};
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct Fault {
            mode: u32,
            fired: bool,
        }
        impl Drop for Fault {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl Component for Fault {
            fn view(&self) -> Result<View, ComponentError> {
                if self.mode == 4 {
                    return Err(ViewError::Empty.into());
                }
                if self.fired && self.mode == 2 {
                    return Err(ComponentError::Reactive);
                }
                let mut view = View::new();
                view.button(0, "Run", 42, false)?;
                if self.fired {
                    view.keyed_button(0, 1, "Extra", 43, false)?;
                    view.keyed_button(0, 1, "Duplicate", 44, false)?;
                }
                Ok(view)
            }
            fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
                if self.mode == 3 {
                    return Err(ComponentError::Reactive);
                }
                self.fired = true;
                Ok(())
            }
            fn stats(&self) -> ComponentStats {
                ComponentStats {
                    signals: if self.mode == 1 {
                        MAX_COMPONENT_RESOURCES + 1
                    } else {
                        0
                    },
                    effects: 0,
                }
            }
        }
        let mut runtime = BrowserRuntime::new(|props| {
            let InitialState::Number(mode) = props.state() else {
                return Err(ComponentError::InvalidProps);
            };
            Ok(Box::new(Fault { mode, fired: false }))
        });
        for mode in [0, 2, 3] {
            let id = runtime.mount(1, counter(mode)).unwrap();
            runtime.clear_commands();
            let expected = if mode == 0 {
                BrowserError::InvalidView
            } else {
                BrowserError::Reactive
            };
            assert_eq!(runtime.dispatch(id, 42), Err(expected));
            assert_eq!(runtime.live_instances(), 0);
            assert_eq!(runtime.command_bytes(), b"RVN3\0\0\0\0");
            assert_eq!(runtime.dispatch(id, 42), Err(BrowserError::InvalidHandle));
        }
        assert_eq!(runtime.mount(1, counter(1)), Err(BrowserError::Limit));
        assert_eq!(runtime.mount(1, counter(4)), Err(BrowserError::InvalidView));
        assert_eq!(runtime.live_instances(), 0);
        assert_eq!(DROPS.load(Ordering::SeqCst), 5);
    }
    #[test]
    fn typed_payload_staging_is_bounded_exact_and_can_be_aborted() {
        let mut runtime = BrowserRuntime::new(demo::create);
        let id = runtime
            .mount(
                1,
                ComponentProps::new(demo::TASK_FORM, InitialState::Number(0)).unwrap(),
            )
            .unwrap();
        runtime.clear_commands();
        let node = runtime.instances[&id].snapshot.node_for_event(5).unwrap();
        assert_eq!(
            runtime.begin_text(id, node, 5, 1025),
            Err(BrowserError::InvalidPayload)
        );
        assert_eq!(
            runtime.dispatch_wire(id, node, 5, 2, 2),
            Err(BrowserError::InvalidPayload)
        );
        runtime.begin_text(id, node, 5, 2).unwrap();
        assert_eq!(runtime.dispose(id), Err(BrowserError::Busy));
        assert_eq!(runtime.dispatch(id, 1), Err(BrowserError::Busy));
        runtime.text_chunk(0xf0, 1).unwrap();
        assert_eq!(
            runtime.dispatch_wire(id, node, 5, 1, 0),
            Err(BrowserError::InvalidPayload)
        );
        runtime.begin_text(id, node, 5, 1).unwrap();
        assert_eq!(
            runtime.text_chunk(256, 1),
            Err(BrowserError::InvalidPayload)
        );
        assert!(runtime.pending.is_none());
        runtime.begin_text(id, node, 5, 1).unwrap();
        runtime.clear_commands();
        assert!(runtime.pending.is_none());
        let value = "Rust 🦀";
        runtime.begin_text(id, node, 5, value.len() as u32).unwrap();
        for chunk in value.as_bytes().chunks(4) {
            let mut bytes = [0; 4];
            bytes[..chunk.len()].copy_from_slice(chunk);
            runtime
                .text_chunk(u32::from_le_bytes(bytes), chunk.len() as u32)
                .unwrap();
        }
        runtime.dispatch_wire(id, node, 5, 1, 0).unwrap();
        runtime.clear_commands();
        runtime.dispatch(id, 1).unwrap();
        runtime.clear_commands();
        assert_eq!(runtime.live_signals(), 1);
        assert!(
            runtime.instances[&id]
                .view
                .render()
                .unwrap()
                .as_str()
                .contains("Rust 🦀")
        );
        runtime.dispose(id).unwrap();
        runtime.clear_commands();
        assert_eq!(runtime.live_signals(), 0);
    }
}
