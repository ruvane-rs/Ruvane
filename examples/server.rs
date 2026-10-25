use ruvane::{Document, Response, Router, Server, Tag, element};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut router = Router::new();
    router.get("/", |_| {
        let body = element(Tag::Main)
            .child(element(Tag::H1).child("Ruvane"))
            .child(element(Tag::P).child("HTML served directly from Rust."));
        Response::html(Document::new("Ruvane", body).render())
    })?;
    router.get("/health", |_| Response::text("ok\n"))?;

    let server = Server::bind("127.0.0.1:3000".parse()?, router)?;
    println!("Ruvane listening at http://{}", server.local_addr()?);
    server.run()?;
    Ok(())
}
