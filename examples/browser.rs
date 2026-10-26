//! Shared SSR and Wasm components. See docs/browser.md.
use ruvane::{
    BROWSER_BRIDGE_MODULE, Document, InternalUrl, MediaType, Response, Router, Server, Tag,
    element, fragment, module_script,
};
#[path = "support/browser_fixtures.rs"]
mod fixtures;
#[path = "support/widgets.rs"]
mod widgets;

use std::fs;
use std::io::Read;
use std::sync::Arc;

fn island(component: u32, state: ruvane::InitialState) -> ruvane::Html {
    let props = ruvane::ComponentProps::new(component, state).unwrap();
    widgets::create(props)
        .unwrap()
        .view()
        .unwrap()
        .hydratable(props)
        .unwrap()
}

fn page(tests: bool, scripts: bool) -> ruvane::Html {
    use ruvane::{Attribute, InitialState};
    let widgets = element(Tag::Div)
        .attr(Attribute::Id, if tests { "fixtures" } else { "widgets" })
        .child(
            element(Tag::Div)
                .attr(Attribute::Id, "counter")
                .child(island(widgets::COUNTER, InitialState::Number(5))),
        )
        .child(
            element(Tag::Div)
                .attr(Attribute::Id, "toggle")
                .child(island(widgets::TOGGLE, InitialState::Boolean(true))),
        )
        .child(
            element(Tag::Div)
                .attr(Attribute::Id, "item-list")
                .child(island(widgets::ITEM_LIST, InitialState::Number(3))),
        )
        .child(if tests {
            element(Tag::Div)
                .attr(Attribute::Id, "empty-list")
                .child(island(widgets::ITEM_LIST, InitialState::Number(0)))
                .render()
        } else {
            ruvane::Html::default()
        })
        .child(
            element(Tag::Div)
                .attr(Attribute::Id, "tasks")
                .child(island(widgets::TASK_FORM, InitialState::Number(0))),
        )
        .child(if tests {
            element(Tag::Div)
                .attr(Attribute::Id, "field-probe")
                .child(island(widgets::FIELDS_PROBE, InitialState::Number(0)))
                .render()
        } else {
            ruvane::Html::default()
        })
        .render();
    let script = if scripts {
        module_script(
            &InternalUrl::new(if tests {
                "/assets/browser-tests.js"
            } else {
                "/assets/demo.js"
            })
            .unwrap(),
        )
    } else {
        ruvane::Html::default()
    };
    Document::new(
        "Ruvane alpha.31 — Typed controls and forms",
        fragment([
            element(Tag::H1)
                .child("Ruvane / Server HTML + Rust/Wasm")
                .render(),
            element(Tag::P)
                .child("Shared Rust components, with no third-party dependencies.")
                .render(),
            element(Tag::P)
                .attr(Attribute::Id, "status")
                .child("Server HTML is ready. Controls activate after hydration.")
                .render(),
            widgets,
            element(Tag::Div).attr(Attribute::Id, "controls").render(),
            element(Tag::Pre).attr(Attribute::Id, "report").render(),
            script,
        ]),
    )
    .stylesheet(InternalUrl::new("/assets/browser.css").unwrap())
    .render()
}

fn wrong_version() -> Vec<u8> {
    // Test module using the previous ABI version.
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend([
        1, 5, 1, 96, 0, 1, 127, 3, 2, 1, 0, 5, 3, 1, 0, 1, 7, 31, 2, 6,
    ]);
    bytes.extend(b"memory");
    bytes.extend([2, 0, 18]);
    bytes.extend(b"ruvane_abi_version");
    bytes.extend([0, 0, 10, 6, 1, 4, 0, 65, 7, 11]);
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
    router.get("/", |_| Response::interactive_html(page(false, true)))?;
    router.get("/tests", |_| Response::interactive_html(page(true, true)))?;
    router.get("/ssr", |_| Response::html(page(false, false)))?;
    router.get("/static", |_| {
        Response::html(Document::new("Static", "Scripts remain blocked here.").render())
    })?;
    router.get("/health", |_| Response::text("ok\n"))?;
    router.get("/assets/app.wasm", move |_| {
        Response::bytes(bytes.to_vec(), MediaType::WebAssembly)
    })?;
    let invalid_patch = fixtures::invalid_patch();
    router.get("/assets/invalid-patch.wasm", move |_| {
        Response::bytes(invalid_patch.clone(), MediaType::WebAssembly)
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
    router.asset(
        "/assets/browser.css",
        include_bytes!("../web/browser.css"),
        MediaType::Css,
    )?;
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
