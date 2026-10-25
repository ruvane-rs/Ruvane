//! Counter runtime for the browser demo.
#![deny(unsafe_code)]
#![deny(missing_docs)]

use ruvane::{BrowserCommands, BrowserTag, ReactiveError, ReactiveRuntime, Scope, Signal};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

// The ABI needs export attributes; all other code keeps unsafe disabled.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[allow(unsafe_code)]
mod abi;

/// Maximum live counters per Wasm instance.
pub const MAX_BROWSER_INSTANCES: usize = 16;
/// Maximum counter value.
pub const MAX_COUNTER_VALUE: u32 = 1000;

/// Browser runtime errors, exposed as numeric ABI codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BrowserError {
    /// A host root is zero or is already registered.
    InvalidRoot = 1,
    /// Instance, identifier, or command budget is exhausted.
    Limit = 2,
    /// The view handle is unknown or disposed.
    InvalidHandle = 3,
    /// The counter does not recognize this event.
    InvalidEvent = 4,
    /// Commands await acknowledgement, or an ABI call is already running.
    Busy = 5,
    /// A reactive computation failed; its view is disposed.
    Reactive = 6,
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
        })
    }
}
impl std::error::Error for BrowserError {}

struct Counter {
    root: u32,
    runtime: ReactiveRuntime,
    _owner: Scope,
    count: Signal<u32>,
}

fn reactive(_: ReactiveError) -> BrowserError {
    BrowserError::Reactive
}

impl Counter {
    fn new(
        view: u32,
        root: u32,
        buffer: Rc<RefCell<BrowserCommands>>,
    ) -> Result<Self, BrowserError> {
        {
            let mut commands = buffer.try_borrow_mut().map_err(|_| BrowserError::Busy)?;
            let result = (|| {
                use BrowserTag::*;
                for (node, parent, tag) in [
                    (1, 0, Section),
                    (2, 1, Heading),
                    (3, 1, Paragraph),
                    (4, 3, Span),
                    (5, 3, Output),
                    (6, 1, Paragraph),
                    (7, 6, Span),
                    (8, 6, Output),
                    (9, 1, Button),
                    (10, 1, Button),
                    (11, 1, Button),
                ] {
                    commands.element(view, node, parent, tag)?;
                }
                for (node, text) in [
                    (2, "Ruvane counter"),
                    (4, "Value: "),
                    (7, "Double: "),
                    (9, "Increment"),
                    (10, "Decrement"),
                    (11, "Reset"),
                ] {
                    commands.text(view, node, text)?;
                }
                for (node, event) in [(9, 1), (10, 2), (11, 3)] {
                    commands.click(view, node, event)?;
                }
                Ok::<_, ruvane::BrowserCommandError>(())
            })();
            result.map_err(|_| BrowserError::Limit)?;
        }
        let runtime = ReactiveRuntime::new();
        let owner = runtime.scope().map_err(reactive)?;
        let count = owner.signal(0u32).map_err(reactive)?;
        let input = count.clone();
        let doubled = owner
            .memo(&[count.source()], move || Ok(input.get()? * 2))
            .map_err(reactive)?;
        let input = count.clone();
        owner
            .effect(&[count.source(), doubled.source()], move || {
                let count = input.get()?;
                let twice = doubled.get()?;
                let mut commands = buffer
                    .try_borrow_mut()
                    .map_err(|_| ReactiveError::Borrowed)?;
                let result = (|| {
                    commands.text(view, 5, &count.to_string())?;
                    commands.text(view, 8, &twice.to_string())?;
                    commands.disabled(view, 9, count == MAX_COUNTER_VALUE)?;
                    commands.disabled(view, 10, count == 0)?;
                    commands.disabled(view, 11, count == 0)
                })();
                result.map_err(|_| ReactiveError::Limit)
            })
            .map_err(reactive)?;
        Ok(Self {
            root,
            runtime,
            _owner: owner,
            count,
        })
    }
}

/// Counter registry and output buffer. The Wasm exports live in `abi`.
pub struct BrowserRuntime {
    instances: BTreeMap<u32, Counter>,
    buffer: Rc<RefCell<BrowserCommands>>,
    next: u32,
}

impl Default for BrowserRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserRuntime {
    /// Creates an empty registry and command buffer.
    pub fn new() -> Self {
        Self {
            instances: BTreeMap::new(),
            buffer: Rc::new(RefCell::new(BrowserCommands::new())),
            next: 0,
        }
    }

    fn ready(&self) -> Result<(), BrowserError> {
        if !self.buffer.borrow().is_empty() {
            Err(BrowserError::Busy)
        } else {
            Ok(())
        }
    }

    /// Mounts a counter on a unique, nonzero root and queues its initial DOM.
    pub fn mount(&mut self, root: u32) -> Result<u32, BrowserError> {
        self.ready()?;
        if root == 0 || self.instances.values().any(|c| c.root == root) {
            return Err(BrowserError::InvalidRoot);
        }
        if self.instances.len() == MAX_BROWSER_INSTANCES {
            return Err(BrowserError::Limit);
        }
        let id = self.next.checked_add(1).ok_or(BrowserError::Limit)?;
        self.next = id;
        match Counter::new(id, root, self.buffer.clone()) {
            Ok(counter) => {
                self.instances.insert(id, counter);
                Ok(id)
            }
            Err(error) => {
                self.buffer.borrow_mut().clear();
                Err(error)
            }
        }
    }

    /// Dispatches 1=increment, 2=decrement or 3=reset. Values stay within 0..=1000.
    pub fn dispatch(&mut self, view: u32, event: u32) -> Result<(), BrowserError> {
        self.ready()?;
        let counter = self
            .instances
            .get(&view)
            .ok_or(BrowserError::InvalidHandle)?;
        let current = counter.count.get().map_err(reactive)?;
        let next = match event {
            1 => current.saturating_add(1).min(MAX_COUNTER_VALUE),
            2 => current.saturating_sub(1),
            3 => 0,
            _ => return Err(BrowserError::InvalidEvent),
        };
        if next != current && counter.count.set(next).is_err() {
            self.buffer.borrow_mut().clear();
            self.instances.remove(&view);
            return Err(BrowserError::Reactive);
        }
        Ok(())
    }

    /// Disposes the scope and emits one removal command. Stale handles are errors.
    pub fn dispose(&mut self, view: u32) -> Result<(), BrowserError> {
        self.ready()?;
        if !self.instances.contains_key(&view) {
            return Err(BrowserError::InvalidHandle);
        }
        self.buffer
            .borrow_mut()
            .remove(view)
            .map_err(|_| BrowserError::Limit)?;
        self.instances.remove(&view);
        Ok(())
    }

    /// Copies the pending commands.
    pub fn commands(&self) -> Vec<u8> {
        self.buffer.borrow().as_bytes().to_vec()
    }
    /// Acknowledges the batch; mutation is rejected until this happens.
    pub fn clear_commands(&mut self) {
        self.buffer.borrow_mut().clear();
    }
    /// Number of live counters.
    pub fn live_instances(&self) -> usize {
        self.instances.len()
    }
    /// Live signals, including memo caches.
    pub fn live_signals(&self) -> usize {
        self.instances
            .values()
            .map(|c| c.runtime.stats().signals)
            .sum()
    }
    /// Live effects, including memo computations.
    pub fn live_effects(&self) -> usize {
        self.instances
            .values()
            .map(|c| c.runtime.stats().effects)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifetimes_budgets_and_stale_handles_do_not_alias() {
        let mut runtime = BrowserRuntime::new();
        let mut stale = Vec::new();
        for _ in 0..1000 {
            let id = runtime.mount(1).unwrap();
            assert_eq!(runtime.live_signals(), 2);
            assert_eq!(runtime.live_effects(), 2);
            assert!(runtime.commands().starts_with(b"RVN1"));
            assert_eq!(runtime.dispatch(id, 1), Err(BrowserError::Busy));
            runtime.clear_commands();
            runtime.dispatch(id, 1).unwrap();
            runtime.clear_commands();
            runtime.dispose(id).unwrap();
            assert_eq!(runtime.live_signals(), 0);
            assert_eq!(runtime.live_effects(), 0);
            runtime.clear_commands();
            stale.push(id);
        }
        let current = runtime.mount(1).unwrap();
        runtime.clear_commands();
        for old in stale {
            assert!(old < current);
            assert_eq!(runtime.dispatch(old, 1), Err(BrowserError::InvalidHandle));
            assert_eq!(runtime.dispose(old), Err(BrowserError::InvalidHandle));
        }
        assert_eq!(runtime.live_instances(), 1);
    }

    #[test]
    fn capacity_roots_events_and_counter_arithmetic_are_bounded() {
        let mut runtime = BrowserRuntime::new();
        assert_eq!(runtime.mount(0), Err(BrowserError::InvalidRoot));
        let id = runtime.mount(1).unwrap();
        runtime.clear_commands();
        assert_eq!(runtime.mount(1), Err(BrowserError::InvalidRoot));
        assert_eq!(
            runtime.dispatch(id, u32::MAX),
            Err(BrowserError::InvalidEvent)
        );
        for _ in 0..1100 {
            runtime.dispatch(id, 1).unwrap();
            runtime.clear_commands();
        }
        assert_eq!(runtime.instances[&id].count.get(), Ok(MAX_COUNTER_VALUE));
        for _ in 0..1100 {
            runtime.dispatch(id, 2).unwrap();
            runtime.clear_commands();
        }
        assert_eq!(runtime.instances[&id].count.get(), Ok(0));
        for root in 2..=MAX_BROWSER_INSTANCES as u32 {
            runtime.mount(root).unwrap();
            runtime.clear_commands();
        }
        assert_eq!(runtime.mount(17), Err(BrowserError::Limit));
        runtime.next = u32::MAX;
        runtime.dispose(id).unwrap();
        runtime.clear_commands();
        assert_eq!(runtime.mount(18), Err(BrowserError::Limit));
    }
}
