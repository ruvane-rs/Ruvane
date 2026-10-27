//! Run after building the demo Wasm, with `cargo run --example browser_uploads -- [PORT] [WASM_FILE]`.
#[path = "support/multipart_demo.rs"]
mod demo;
#[path = "support/upload_component.rs"]
mod upload_component;
use ruvane::*;
use std::io::Read;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let port = args
        .next()
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);
    let path = args
        .next()
        .unwrap_or_else(|| "target/wasm32-unknown-unknown/release/ruvane_browser.wasm".into());
    if args.next().is_some() {
        return Err("usage: browser_uploads [PORT] [WASM_FILE]".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_ASSET_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_ASSET_BYTES || !bytes.starts_with(b"\0asm\x01\0\0\0") {
        return Err("invalid or oversized Wasm module".into());
    }
    let mut app = demo::app()?;
    app.asset(
        "/assets/bridge.mjs",
        BROWSER_BRIDGE_MODULE.as_bytes(),
        MediaType::JavaScript,
    )?;
    app.asset(
        "/upload-demo.mjs",
        include_bytes!("support/upload_demo.mjs"),
        MediaType::JavaScript,
    )?;
    app.asset(
        "/upload-tests.mjs",
        include_bytes!("../web/upload-tests.mjs"),
        MediaType::JavaScript,
    )?;
    let mut routes = FileRouteRegistry::new(&mut app);
    routes.endpoint(
        "upload-tests/route.rs",
        "/upload-tests",
        FileEndpoint::new().get(|_| {
            Response::interactive_html(
                Document::new(
                    "Upload lifecycle checks",
                    fragment([
                        element(Tag::H1)
                            .child("Ruvane alpha.57 — Browser uploads")
                            .render(),
                        element(Tag::P)
                            .attr(Attribute::Id, "status")
                            .child("Running…")
                            .render(),
                        element(Tag::Ol).attr(Attribute::Id, "results").render(),
                        element(Tag::Pre).attr(Attribute::Id, "report").render(),
                        module_script(&InternalUrl::new("/upload-tests.mjs").unwrap()),
                    ]),
                )
                .stylesheet(InternalUrl::new("/multipart-demo.css").unwrap())
                .render(),
            )
        }),
    )?;
    routes.endpoint(
        "interactive/route.rs",
        "/interactive",
        FileEndpoint::new().get(|_| {
            let view = upload_component::create().view().unwrap();
            let root = view
                .hydratable(ComponentProps::new(29, InitialState::Number(0)).unwrap())
                .unwrap();
            Response::interactive_html(
                Document::new(
                    "Ruvane Rust uploads",
                    fragment([
                        element(Tag::H1)
                            .child("Interactive uploads / Rust + Wasm")
                            .render(),
                        link(&InternalUrl::new("/").unwrap(), "Native form"),
                        element(Tag::Main)
                            .attr(Attribute::Id, "upload-root")
                            .child(root)
                            .render(),
                        module_script(&InternalUrl::new("/upload-demo.mjs").unwrap()),
                    ]),
                )
                .stylesheet(InternalUrl::new("/multipart-demo.css").unwrap())
                .render(),
            )
        }),
    )?;
    routes.finish()?;
    let mut router = app.router(None)?;
    router.get("/assets/app.wasm", move |_| {
        Response::bytes(bytes.clone(), MediaType::WebAssembly)
    })?;
    let server = Server::bind(format!("127.0.0.1:{port}").parse()?, router)?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!(
        "Ruvane browser uploads: http://{}/interactive — Enter to stop",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
