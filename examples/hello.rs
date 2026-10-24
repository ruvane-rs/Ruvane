use ruvane::{Attribute, Document, Tag, element};

fn main() {
    let message = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("Hello from Rust & Ruvane!"));

    let body = element(Tag::Main)
        .attr(Attribute::Class, "page")
        .child(element(Tag::H1).child("Ruvane"))
        .child(element(Tag::P).child(message));

    let page = Document::new("Ruvane — HTML from Rust", body)
        .language("en")
        .render();

    println!("{page}");
}
