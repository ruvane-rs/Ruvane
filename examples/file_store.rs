//! Run with `cargo run --example file_store -- /absolute/private-directory [PORT] [--recover]`.
#[path = "support/file_store_demo.rs"]
mod demo;
use ruvane::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: file_store /absolute/private-directory [PORT] [--recover]")?;
    let port = args
        .next()
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);
    let recovery = match args.next().as_deref() {
        None => false,
        Some("--recover") => true,
        _ => return Err("unknown option".into()),
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let files = if recovery {
        FileStore::recover(path, "file-demo", FileStoreLimits::default())?
    } else {
        FileStore::open(path, "file-demo", FileStoreLimits::default())?
    };
    let server = Server::bind(
        format!("127.0.0.1:{port}").parse()?,
        demo::app(files)?.router(None)?,
    )?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!(
        "Ruvane private files: http://{}/private-files — Enter to stop",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
