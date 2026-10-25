#[path = "support/blog.rs"]
mod blog;

use ruvane::Server;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(3000);
    let server = Server::bind(([127, 0, 0, 1], port).into(), blog::router()?)?;
    println!("Ruvane blog at http://{}", server.local_addr()?);
    server.run()?;
    Ok(())
}
