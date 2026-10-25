//! Run with `cargo run --example server_forms -- [PORT]`.
#[path = "support/server_tasks.rs"]
mod tasks;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let port = args.next().map_or(Ok(3002), |value| value.parse::<u16>())?;
    let file = args.next();
    if args.next().is_some() {
        return Err("usage: server_forms [PORT] [WASM_FILE]".into());
    }
    let mut router = tasks::router()?;
    let path = file
        .as_deref()
        .unwrap_or("target/wasm32-unknown-unknown/release/ruvane_browser.wasm");
    if file.is_some() || std::path::Path::new(path).exists() {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((ruvane::MAX_ASSET_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > ruvane::MAX_ASSET_BYTES || !bytes.starts_with(b"\0asm\x01\0\0\0") {
            return Err("invalid or oversized Wasm module".into());
        }
        router.get("/assets/app.wasm", move |_| {
            ruvane::Response::bytes(bytes.clone(), ruvane::MediaType::WebAssembly)
        })?;
    }
    let server = ruvane::Server::bind(format!("127.0.0.1:{port}").parse()?, router)?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!(
        "Ruvane server forms: http://{}/tasks — Enter to stop",
        server.local_addr()?
    );
    println!(
        "Interactive tasks: http://{}/interactive",
        server.local_addr()?
    );
    println!("Navigation: http://{}/app", server.local_addr()?);
    server.run()?;
    Ok(())
}
