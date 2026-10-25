//! Run with `cargo run --example streaming -- [PORT]`.
use ruvane::{
    Document, Html, HtmlStream, InternalUrl, MediaType, Response, Router, Server, ServerConfig,
    StreamContext, StreamError, StreamLimits, Tag, element, fragment, link, module_script,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args()
        .nth(1)
        .map_or(Ok(3004), |value| value.parse::<u16>())?;
    let released = Arc::new(AtomicBool::new(false));
    let mut router = Router::new();
    router.asset(
        "/assets/stream-tests.mjs",
        include_bytes!("../web/streaming-tests.mjs"),
        MediaType::JavaScript,
    )?;
    router.get("/", |_| {
        Response::html(
            Document::new(
                "Streaming SSR / Ruvane",
                fragment([
                    element(Tag::H1).child("Ruvane / Streaming SSR").render(),
                    link(
                        &InternalUrl::new("/stream").unwrap(),
                        "Open progressive report",
                    ),
                    element(Tag::P)
                        .child(link(&InternalUrl::new("/tests").unwrap(), "Browser checks"))
                        .render(),
                ]),
            )
            .render(),
        )
    })?;
    let gate = Arc::clone(&released);
    router.get("/stream", move |request| {
        let controlled = request.query_params().get("controlled") == Some("yes");
        let fail = request.query_params().get("fail") == Some("yes");
        let label = request
            .query_params()
            .get("q")
            .unwrap_or("Generated safely in Rust")
            .to_owned();
        let gate = Arc::clone(&gate);
        let document = Document::new(
            "Progressive report / Ruvane",
            fragment([
                element(Tag::H1)
                    .attr(ruvane::Attribute::Id, "initial-shell")
                    .child("Report header is ready")
                    .render(),
                element(Tag::P)
                    .child("This HTML was sent before the report was calculated.")
                    .render(),
                link(&InternalUrl::new("/").unwrap(), "Home"),
            ]),
        );
        let plan = HtmlStream::new(document, move |_| {
            let mut sent = false;
            let label = label.clone();
            let gate = Arc::clone(&gate);
            Ok(
                move |context: &StreamContext| -> Result<Option<Html>, StreamError> {
                    if sent {
                        return Ok(None);
                    }
                    sent = true;
                    if controlled {
                        while !gate.load(Ordering::Acquire) {
                            context.wait(Duration::from_millis(10))?;
                        }
                    } else {
                        context.wait(Duration::from_millis(500))?;
                    }
                    if fail {
                        return Err(StreamError::Producer);
                    }
                    Ok(Some(
                        element(Tag::Section)
                            .attr(ruvane::Attribute::Id, "deferred-report")
                            .child(element(Tag::H2).child("Deferred report"))
                            .child(element(Tag::P).child(label.as_str()))
                            .render(),
                    ))
                },
            )
        })
        .limits(StreamLimits {
            duration: Duration::from_secs(60),
            ..StreamLimits::default()
        })
        .unwrap();
        Response::stream_html(plan)
    })?;
    for (path, value) in [("/fixtures/prepare", false), ("/fixtures/release", true)] {
        let gate = Arc::clone(&released);
        router.post_form(path, move |_| {
            gate.store(value, Ordering::Release);
            Response::text("ok")
        })?;
    }
    router.get("/tests", |_| {
        Response::interactive_html(
            Document::new(
                "Streaming browser checks",
                fragment([
                    element(Tag::H1)
                        .child("Ruvane / Progressive response checks")
                        .render(),
                    element(Tag::P)
                        .attr(ruvane::Attribute::Id, "status")
                        .child("Ready.")
                        .render(),
                    element(Tag::Pre)
                        .attr(ruvane::Attribute::Id, "report")
                        .render(),
                    module_script(&InternalUrl::new("/assets/stream-tests.mjs").unwrap()),
                ]),
            )
            .render(),
        )
    })?;
    let server = Server::bind_with_config(
        ([127, 0, 0, 1], port).into(),
        router,
        ServerConfig {
            write_timeout: Duration::from_secs(60),
            ..ServerConfig::default()
        },
    )?;
    let stop = server.shutdown_handle();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        stop.shutdown();
    });
    println!(
        "Streaming report: http://{}/stream — Enter to stop",
        server.local_addr()?
    );
    println!("Browser checks: http://{}/tests", server.local_addr()?);
    server.run()?;
    Ok(())
}
