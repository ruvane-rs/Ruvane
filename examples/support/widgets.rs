//! Components compiled for both the server and Wasm demo.
use ruvane::{
    BrowserTag::*, Component, ComponentError, ComponentProps, ComponentStats, InitialState, Memo,
    ReactiveRuntime, Scope, Signal, View,
};

/// Counter registry ID.
pub const COUNTER: u32 = 1;
/// Toggle registry ID.
pub const TOGGLE: u32 = 2;
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
        _ => Err(ComponentError::InvalidProps),
    }
}
