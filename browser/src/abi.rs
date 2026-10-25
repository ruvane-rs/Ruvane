//! Numeric exports for browser ABI v2.
use super::{BrowserError, BrowserRuntime};
use std::cell::{Cell, RefCell};

thread_local! {
    // Each Wasm instance has its own state.
    static STATE: RefCell<BrowserRuntime> = RefCell::new(BrowserRuntime::new(super::demo::create));
    static ERROR: Cell<u32> = const { Cell::new(0) };
}

fn operation(f: impl FnOnce(&mut BrowserRuntime) -> Result<u32, BrowserError>) -> u32 {
    let result = STATE.with(|state| {
        let mut state = state.try_borrow_mut().map_err(|_| BrowserError::Busy)?;
        f(&mut state)
    });
    match result {
        Ok(value) => {
            ERROR.with(|e| e.set(0));
            value
        }
        Err(error) => {
            ERROR.with(|e| e.set(error as u32));
            0
        }
    }
}

fn read(f: impl FnOnce(&BrowserRuntime) -> u32) -> u32 {
    STATE.with(|state| match state.try_borrow() {
        Ok(state) => f(&state),
        Err(_) => u32::MAX,
    })
}

// SAFETY: This module is the sole definition of all eleven ruvane_ export names.
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_abi_version() -> u32 {
    ruvane::BROWSER_ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_mount(root: u32, component: u32, kind: u32, value: u32) -> u32 {
    operation(|s| {
        let state =
            ruvane::InitialState::from_wire(kind, value).map_err(|_| BrowserError::InvalidProps)?;
        let props = ruvane::ComponentProps::new(component, state)
            .map_err(|_| BrowserError::InvalidProps)?;
        s.mount(root, props)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_dispatch(view: u32, event: u32) -> u32 {
    operation(|s| {
        s.dispatch(view, event)?;
        Ok(1)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_dispose(view: u32) -> u32 {
    operation(|s| {
        s.dispose(view)?;
        Ok(1)
    })
}
// Copy the output before the next mutating ABI call.
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_commands_ptr() -> u32 {
    read(|s| s.command_bytes().as_ptr() as usize as u32)
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_commands_len() -> u32 {
    read(|s| s.command_bytes().len() as u32)
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_commands_clear() -> u32 {
    operation(|s| {
        s.clear_commands();
        Ok(1)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_last_error() -> u32 {
    ERROR.with(Cell::get)
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_live_instances() -> u32 {
    read(|s| s.live_instances() as u32)
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_live_signals() -> u32 {
    read(|s| s.live_signals() as u32)
}
#[unsafe(no_mangle)]
pub extern "C" fn ruvane_live_effects() -> u32 {
    read(|s| s.live_effects() as u32)
}
