//! Run with `cargo run --example server_forms -- [PORT]`.
#[path = "support/server_tasks.rs"]
mod tasks;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let port = args.next().map_or(Ok(3002), |value| value.parse::<u16>())?;
    if args.next().is_some() {
        return Err("usage: server_forms [PORT]".into());
    }
    let server = ruvane::Server::bind(format!("127.0.0.1:{port}").parse()?, tasks::router()?)?;
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
    server.run()?;
    Ok(())
}
