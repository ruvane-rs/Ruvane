//! Application-owned numeric exports.
/// Exports one application runtime from a Wasm cdylib. Invoke once at crate root.
/// The revision must match the server's embedded module revision.
#[macro_export]
macro_rules! export_application {
    ($factory:path, $validate:path, $revision:expr) => {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        #[allow(unsafe_code)]
        mod ruvane_abi {
            use $crate::{BrowserError, BrowserRuntime};
            use std::cell::{Cell, RefCell};

            thread_local! {
                // Each Wasm instance has its own state.
                static STATE: RefCell<BrowserRuntime> = RefCell::new(BrowserRuntime::new($factory).with_navigation($validate));
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

            // SAFETY: This module owns all twenty-two ruvane_ export names.
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_abi_version() -> u32 {
                $crate::ABI_VERSION
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_mount(root: u32, component: u32, kind: u32, value: u32) -> u32 {
                operation(|s| {
                    let state =
                        $crate::InitialState::from_wire(kind, value).map_err(|_| BrowserError::InvalidProps)?;
                    let props = $crate::ExportComponentProps::new(component, state)
                        .map_err(|_| BrowserError::InvalidProps)?;
                    s.mount(root, props)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_dispatch(view: u32, node: u32, event: u32, kind: u32, value: u32) -> u32 {
                operation(|s| {
                    s.dispatch_wire(view, node, event, kind, value)?;
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

            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_event_begin(view: u32, node: u32, event: u32, length: u32) -> u32 {
                operation(|s| {
                    s.begin_text(view, node, event, length)?;
                    Ok(1)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_event_chunk(word: u32, count: u32) -> u32 {
                operation(|s| {
                    s.text_chunk(word, count)?;
                    Ok(1)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_action_begin(view: u32, request: u32, length: u32) -> u32 {
                operation(|s| {
                    s.begin_reply(view, request, length)?;
                    Ok(1)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_action_chunk(word: u32, count: u32) -> u32 {
                operation(|s| {
                    s.reply_chunk(word, count)?;
                    Ok(1)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_action_complete(view: u32, request: u32, kind: u32) -> u32 {
                operation(|s| {
                    Ok(if s.complete_wire(view, request, kind)? {
                        1
                    } else {
                        2
                    })
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_page_begin(length: u32) -> u32 {
                operation(|state| {
                    state.begin_page(length)?;
                    Ok(1)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_page_chunk(word: u32, count: u32) -> u32 {
                operation(|state| {
                    state.page_chunk(word, count)?;
                    Ok(1)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_page_mount(root: u32) -> u32 {
                operation(|state| state.mount_page_wire(root))
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_page_validate() -> u32 {
                operation(|state| { state.validate_page_wire()?; Ok(1) })
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_boundary_mount(root: u32, component: u32, kind: u32, value: u32) -> u32 {
                operation(|state| {
                    let initial=$crate::InitialState::from_wire(kind,value).map_err(|_|BrowserError::InvalidProps)?;
                    let props=$crate::ExportComponentProps::new(component,initial).map_err(|_|BrowserError::InvalidProps)?;
                    state.mount_boundary(root,props)
                })
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn ruvane_application_revision() -> u64 { $revision }
        }
    };
}
