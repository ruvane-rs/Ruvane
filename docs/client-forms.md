# Client forms

Alpha.15 adds typed controls to shared Rust views. This is a client-side form
profile: submit events run application code in Wasm. No HTTP mutation route,
session or server action is involved. Native [GET forms](get-forms.md) handle
server navigation; alpha.16's [POST forms](post-forms.md) handle server mutations.

## Build a form

```rust
use ruvane::{BrowserTag, Component, ComponentError, EventValue, View};

struct NameForm {
    name: String,
    invalid: bool,
}

impl Component for NameForm {
    fn view(&self) -> Result<View, ComponentError> {
        let mut view = View::new();
        let form = view.form(0, 1, 1)?;
        let label = view.keyed_element(form, 1, BrowserTag::Label)?;
        view.keyed_leaf(label, 1, BrowserTag::Span, "Name")?;
        view.input(label, 2, &self.name, 2, self.invalid)?;
        view.submit(form, 2, "Save", false)?;
        view.keyed_leaf(form, 3, BrowserTag::Output, if self.invalid {
            "Enter a name."
        } else {
            ""
        })?;
        Ok(view)
    }

    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if event != 1 { return Err(ComponentError::InvalidEvent); }
        self.invalid = self.name.trim().is_empty();
        Ok(())
    }

    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (2, EventValue::Text(value)) => { self.name = value; Ok(()) }
            (event, EventValue::None) => self.dispatch(event),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
}
```

The application factory constructs the component. The browser registry validates
the event binding, calls the component, requests its new view and reconciles it.
Ordinary Rust state and scoped reactive signals both work. There is no implicit
subscription or field-to-signal binding.

`textarea` accepts multiline text; `checkbox` carries `EventValue::Boolean`.
Each control needs one wrapping label with a nonblank span. Forms cannot nest.
Text fields are single line, and submit buttons belong to a form. The profile
exposes fixed control types and attributes, without arbitrary names, actions,
URLs or event handlers. `aria-invalid` reflects the Rust model.

## Validation and submit

The transport accepts at most 1024 UTF-8 bytes per text value. Text inputs reject
control characters; textareas permit LF and tabs. Rust validates the payload
before calling application code. DOM `maxlength` is 1024 UTF-16 code units and
does not replace the byte check. The bridge rejects invalid text and restores
the field's last accepted value. A malformed payload leaves the component live.

Application rules belong in Rust. A normal validation failure updates errors and
returns `Ok(())`; returning a component error drops that failed native instance.
Client validation is not authorization. Server POST handlers must independently
validate their own input.

The browser prevents the native submit navigation and flushes dirty live fields
before invoking the form event. This includes programmatic edits or autofill
that did not produce an input event. Public `view.dispatch(event, value)` takes a
string for text fields, a boolean for checkboxes and no value for submissions or
clicks. A submission during composition waits for a later submit event; it is
not automatically replayed.

The task example accepts up to three tasks, names of 80 UTF-8 bytes and notes of
160 bytes. It reports domain errors, clears fields after success, filters and
reorders tasks, and drops each task's signal scope when removed. State lasts for
the lifetime of that component and is not persisted across reloads.

## Hydration and field lifetime

Hydratable server fields start disabled. Defaults are escaped as input value,
checkbox checked or textarea text. Leading textarea newlines round-trip through
the HTML parser. Hydration checks these defaults and the full tree before
attaching handlers. Valid live edits are then adopted into Rust, including all
dirty fields in the same view. Invalid edits return to the model value. Host code
may have enabled a model-enabled field before hydration; its focus and selection
are preserved when possible.

After hydration the initial HTML defaults stay fixed for each field's lifetime.
Updates use live `value` and `checked` properties. Dirty properties are expected;
changes to owned elements, attributes or child nodes are protocol failures.

The bridge avoids assigning an equal value and preserves selection in UTF-16
coordinates. Keyed reorder moves siblings around the active field's subtree so
that subtree is not detached. Composition input stays in the DOM until
compositionend, then sends one committed value to Rust. A final duplicate input
event does not dispatch the same value again.

To force a reset, increment a checked u32 token and apply `field_epoch(field,
token)` to each affected field. Epochs cannot decrease while a field is retained.
A changed value or epoch from Rust during composition is deferred; it wins when
composition ends. Use a regular button to dispatch a Rust reset event. Native
reset buttons are outside this profile.

Removing a field detaches its input and composition listeners. A returning key
receives a fresh DOM handle and the current Rust model value. Disposal releases
all form handlers and component resources. The browser harness covers synthetic
composition events in Chromium; operating-system IMEs and other browsers still
need broader compatibility testing.
