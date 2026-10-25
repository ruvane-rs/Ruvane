//! Test-only module that emits a valid update followed by an invalid removal.
use ruvane::{BrowserCommands, BrowserTag};

fn leb(mut value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        bytes.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return bytes;
        }
    }
}
fn constant(value: u32) -> Vec<u8> {
    let mut bytes = leb(value);
    if bytes.last().unwrap() & 64 != 0 {
        *bytes.last_mut().unwrap() |= 128;
        bytes.push(0);
    }
    let mut output = vec![0x41];
    output.extend(bytes);
    output
}
fn section(output: &mut Vec<u8>, id: u8, bytes: Vec<u8>) {
    output.push(id);
    output.extend(leb(bytes.len() as u32));
    output.extend(bytes);
}
fn choose(initial: u32, dispatched: u32, disposed: u32) -> Vec<u8> {
    let mut code = vec![0x23, 0, 0x41, 1, 0x46, 0x04, 0x7f];
    code.extend(constant(dispatched));
    code.extend([0x05, 0x23, 0, 0x41, 2, 0x46, 0x04, 0x7f]);
    code.extend(constant(disposed));
    code.push(0x05);
    code.extend(constant(initial));
    code.extend([0x0b, 0x0b]);
    code
}

pub fn invalid_patch() -> Vec<u8> {
    let mut initial = BrowserCommands::new();
    initial.element(1, 1, 0, BrowserTag::Button).unwrap();
    initial.text(1, 1, "Before").unwrap();
    initial.click(1, 1, 1).unwrap();
    initial.disabled(1, 1, false).unwrap();
    let mut patch = BrowserCommands::new();
    patch.text(1, 1, "After").unwrap();
    patch.remove_node(1, 999).unwrap();
    let mut removed = BrowserCommands::new();
    removed.remove(1).unwrap();
    let frames = [initial.as_bytes(), patch.as_bytes(), removed.as_bytes()];
    let mut output = b"\0asm\x01\0\0\0".to_vec();
    section(
        &mut output,
        1,
        vec![
            6, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f, 0x60, 5, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 1,
            0x7f, 0x60, 1, 0x7f, 1, 0x7f, 0x60, 0, 1, 0x7f, 0x60, 2, 0x7f, 0x7f, 1, 0x7f, 0x60, 3,
            0x7f, 0x7f, 0x7f, 1, 0x7f,
        ],
    );
    section(
        &mut output,
        3,
        vec![19, 3, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 0, 4, 5, 4, 5, 2, 4, 2],
    );
    section(&mut output, 5, vec![1, 0, 1]);
    section(&mut output, 6, vec![1, 0x7f, 1, 0x41, 0, 0x0b]);
    let names = [
        "ruvane_abi_version",
        "ruvane_mount",
        "ruvane_dispatch",
        "ruvane_dispose",
        "ruvane_commands_ptr",
        "ruvane_commands_len",
        "ruvane_commands_clear",
        "ruvane_last_error",
        "ruvane_live_instances",
        "ruvane_live_signals",
        "ruvane_live_effects",
        "ruvane_event_begin",
        "ruvane_event_chunk",
        "ruvane_action_begin",
        "ruvane_action_chunk",
        "ruvane_action_complete",
        "ruvane_page_begin",
        "ruvane_page_chunk",
        "ruvane_page_mount",
    ];
    let mut exports = vec![20, 6];
    exports.extend(b"memory");
    exports.extend([2, 0]);
    for (index, name) in names.iter().enumerate() {
        exports.extend(leb(name.len() as u32));
        exports.extend(name.as_bytes());
        exports.extend([0, index as u8]);
    }
    section(&mut output, 7, exports);
    let mut dispatch = vec![0x41, 1, 0x24, 0];
    dispatch.extend(constant(1));
    let mut dispose = vec![0x41, 2, 0x24, 0];
    dispose.extend(constant(1));
    let bodies = [
        constant(ruvane::BROWSER_ABI_VERSION),
        constant(1),
        dispatch,
        dispose,
        choose(0, 256, 512),
        choose(
            frames[0].len() as u32,
            frames[1].len() as u32,
            frames[2].len() as u32,
        ),
        constant(1),
        constant(0),
        choose(1, 1, 0),
        constant(0),
        constant(0),
        constant(1),
        constant(1),
        constant(1),
        constant(1),
        constant(1),
        constant(1),
        constant(1),
        constant(1),
    ];
    let mut code = vec![19];
    for body in bodies {
        code.extend(leb(body.len() as u32 + 2));
        code.push(0);
        code.extend(body);
        code.push(0x0b);
    }
    section(&mut output, 10, code);
    let mut data = vec![3];
    for (index, frame) in frames.iter().enumerate() {
        data.push(0);
        data.extend(constant(index as u32 * 256));
        data.push(0x0b);
        data.extend(leb(frame.len() as u32));
        data.extend(*frame);
    }
    section(&mut output, 11, data);
    output
}
