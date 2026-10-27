//! Run after building demo Wasm: `file_manager /absolute/private-directory [PORT] [WASM_FILE]`.
#[allow(dead_code)]
#[path = "support/file_store_demo.rs"]
mod demo;
#[path = "support/file_manager_app.rs"]
mod file_manager_app;
#[path = "support/file_manager_component.rs"]
mod file_manager_component;
use ruvane::*;
use std::io::Read;
fn shell(children: Html) -> Html {
    element(Tag::Main)
        .child(element(Tag::H1).child("Ruvane Rust/Wasm private files"))
        .child(link(
            &InternalUrl::new("/private-files").unwrap(),
            "Native file forms and demo sessions",
        ))
        .child(navigation_link(
            &InternalUrl::new("/file-manager").unwrap(),
            "File manager",
        ))
        .child(navigation_link(
            &InternalUrl::new("/file-home").unwrap(),
            "Home",
        ))
        .child(children)
        .render()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = args
        .next()
        .ok_or("usage: file_manager /absolute/private-directory [PORT] [WASM_FILE]")?;
    let port = args
        .next()
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);
    let path = args
        .next()
        .unwrap_or_else(|| "target/wasm32-unknown-unknown/release/ruvane_browser.wasm".into());
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_ASSET_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let files = FileStore::open(directory, "file-demo", FileStoreLimits::default())?;
    let mut app = file_manager_app::application();
    let service = demo::register_service(&mut app, files)?;
    service.register_manager(&mut app)?;
    app.client_runtime(
        ClientRuntime::new(Box::leak(bytes.into_boxed_slice()), 0)?
            .csrf_endpoint(InternalUrl::new("/private-files/csrf")?)?
            .shell(shell)
            .stylesheet(InternalUrl::new("/file-store.css")?)?,
    )?;
    let server = Server::bind(format!("127.0.0.1:{port}").parse()?, app.router(None)?)?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!(
        "Ruvane file manager: http://{}/private-files — Enter to stop",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
