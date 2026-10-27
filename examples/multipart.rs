//! Run with `cargo run --example multipart -- [PORT]`.
#[path = "support/multipart_demo.rs"]
mod demo;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let port = args
        .next()
        .map(|port| port.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);
    if args.next().is_some() {
        return Err("usage: multipart [PORT]".into());
    }
    let server = ruvane::Server::bind(
        format!("127.0.0.1:{port}").parse()?,
        demo::app()?.router(None)?,
    )?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!(
        "Ruvane multipart demo: http://{} — Enter to stop",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
