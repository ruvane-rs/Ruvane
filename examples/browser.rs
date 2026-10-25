//! Build the Wasm crate first; see docs/browser.md.
use ruvane::{
    BROWSER_BRIDGE_MODULE, Document, InternalUrl, MediaType, Response, Router, Server, Tag,
    element, fragment, module_script,
};
use std::fs;
use std::io::Read;
use std::sync::Arc;

fn page(tests: bool) -> ruvane::Html {
    let script = if tests {
        "/assets/browser-tests.js"
    } else {
        "/assets/demo.js"
    };
    Document::new(
        "Ruvane alpha.12 — Rust in the browser",
        fragment([
            element(Tag::H1)
                .child("Ruvane / Rust + WebAssembly")
                .render(),
            element(Tag::P)
                .child("A reactive engine built from scratch, with no third-party dependencies.")
                .render(),
            element(Tag::P)
                .attr(ruvane::Attribute::Id, "status")
                .child("Loading module…")
                .render(),
            element(Tag::Div)
                .attr(ruvane::Attribute::Id, "counter")
                .render(),
            element(Tag::Div)
                .attr(ruvane::Attribute::Id, "controls")
                .render(),
            element(Tag::Pre)
                .attr(ruvane::Attribute::Id, "report")
                .render(),
            module_script(&InternalUrl::new(script).unwrap()),
        ]),
    )
    .stylesheet(InternalUrl::new("/assets/browser.css").unwrap())
    .render()
}

fn wrong_version() -> Vec<u8> {
    // Test module with memory and ABI version 2.
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend([
        1, 5, 1, 96, 0, 1, 127, 3, 2, 1, 0, 5, 3, 1, 0, 1, 7, 31, 2, 6,
    ]);
    bytes.extend(b"memory");
    bytes.extend([2, 0, 18]);
    bytes.extend(b"ruvane_abi_version");
    bytes.extend([0, 0, 10, 6, 1, 4, 0, 65, 2, 11]);
    bytes
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() > 2 {
        return Err("usage: browser [WASM_FILE] [PORT]".into());
    }
    let file = args
        .first()
        .map(String::as_str)
        .unwrap_or("target/wasm32-unknown-unknown/release/ruvane_browser.wasm");
    let port: u16 = args.get(1).map(String::as_str).unwrap_or("3001").parse()?;
    let mut bytes = Vec::new();
    fs::File::open(file)?
        .take((ruvane::MAX_ASSET_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > ruvane::MAX_ASSET_BYTES || !bytes.starts_with(b"\0asm\x01\0\0\0") {
        return Err("invalid or oversized Wasm module".into());
    }
    let bytes: Arc<[u8]> = bytes.into();
    let mut router = Router::new();
    router.get("/", |_| Response::interactive_html(page(false)))?;
    router.get("/tests", |_| Response::interactive_html(page(true)))?;
    router.get("/static", |_| {
        Response::html(Document::new("Static", "Scripts remain blocked here.").render())
    })?;
    router.get("/health", |_| Response::text("ok\n"))?;
    router.get("/assets/counter.wasm", move |_| {
        Response::bytes(bytes.to_vec(), MediaType::WebAssembly)
    })?;
    let wrong = wrong_version();
    router.get("/assets/wrong-version.wasm", move |_| {
        Response::bytes(wrong.clone(), MediaType::WebAssembly)
    })?;
    router.asset(
        "/assets/ruvane.js",
        BROWSER_BRIDGE_MODULE.as_bytes(),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/demo.js",
        include_bytes!("../web/counter-demo.mjs"),
        MediaType::JavaScript,
    )?;
    router.asset(
        "/assets/browser-tests.js",
        include_bytes!("../web/browser-tests.mjs"),
        MediaType::JavaScript,
    )?;
    router.asset("/assets/browser.css", b"body{font:18px system-ui;max-width:760px;margin:48px auto;padding:0 24px;color:#152b25;background:#f4f3ec}section{padding:24px;border:1px solid #a6b5ac;border-radius:16px;background:white}button{font:inherit;padding:10px 16px;margin:6px;border-radius:8px;border:1px solid #768d80;background:#e1eee5}button:disabled{opacity:.45}output{font-weight:700}pre{white-space:pre-wrap;font-size:14px}", MediaType::Css)?;
    let server = Server::bind(format!("127.0.0.1:{port}").parse()?, router)?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!("Ruvane browser demo: http://{}", server.local_addr()?);
    println!(
        "Browser tests: http://{}/tests — Enter to stop",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
