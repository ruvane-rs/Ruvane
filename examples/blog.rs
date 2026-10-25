#[path = "support/blog.rs"]
mod blog;

use ruvane::{Server, SiteOrigin};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let port = arguments
        .next()
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(3000);
    let origin = arguments
        .next()
        .map(|value| SiteOrigin::parse(&value))
        .transpose()?;
    if arguments.next().is_some() {
        return Err("usage: blog [port] [public-origin]".into());
    }
    let router = match origin {
        Some(origin) => blog::router_with_origin(origin)?,
        None => blog::router()?,
    };
    let server = Server::bind(([127, 0, 0, 1], port).into(), router)?;
    println!("Ruvane blog at http://{}", server.local_addr()?);
    server.run()?;
    Ok(())
}
