#[path = "support/blog.rs"]
mod blog;

use ruvane::{Server, SiteOrigin, StaticSite};
use std::ffi::{OsStr, OsString};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next();
    if first.as_deref() == Some(OsStr::new("export")) {
        return export(arguments);
    }
    if first.as_deref() == Some(OsStr::new("preview")) {
        return preview(arguments);
    }
    let port = first
        .as_deref()
        .map(parse_port)
        .transpose()?
        .unwrap_or(3000);
    let origin = arguments
        .next()
        .map(|value| parse_origin(&value))
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

fn parse_port(value: &OsStr) -> Result<u16, Box<dyn std::error::Error>> {
    Ok(value.to_str().ok_or("invalid port")?.parse()?)
}

fn preview(
    mut arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: blog preview DIRECTORY [PORT]";
    let directory = arguments.next().ok_or(usage)?;
    let port = arguments
        .next()
        .as_deref()
        .map(parse_port)
        .transpose()?
        .unwrap_or(3001);
    if arguments.next().is_some() {
        return Err(usage.into());
    }
    let router = StaticSite::preview_export(directory)?;
    let server = Server::bind(([127, 0, 0, 1], port).into(), router)?;
    println!("Ruvane static preview at http://{}", server.local_addr()?);
    println!("Open this HTTP address in a browser; do not open index.html as a local file.");
    server.run()?;
    Ok(())
}

fn parse_origin(value: &OsStr) -> Result<SiteOrigin, Box<dyn std::error::Error>> {
    Ok(SiteOrigin::parse(value.to_str().ok_or("invalid origin")?)?)
}

fn export(mut arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: blog export DESTINATION PUBLIC_ORIGIN [--preview PORT]";
    let destination = arguments.next().ok_or(usage)?;
    let origin = parse_origin(&arguments.next().ok_or(usage)?)?;
    let preview = match arguments.next() {
        None => None,
        Some(flag) if flag == OsStr::new("--preview") => {
            Some(parse_port(&arguments.next().ok_or(usage)?)?)
        }
        _ => return Err(usage.into()),
    };
    if arguments.next().is_some() {
        return Err(usage.into());
    }
    let site = blog::static_site(origin)?;
    let router = preview.map(|_| site.preview_router()).transpose()?;
    let report = site.export_to(&destination)?;
    println!(
        "Exported {} files ({} bytes).",
        report.files(),
        report.bytes()
    );
    if let (Some(port), Some(router)) = (preview, router) {
        let server = Server::bind(([127, 0, 0, 1], port).into(), router)?;
        println!("Ruvane static preview at http://{}", server.local_addr()?);
        server.run()?;
    }
    Ok(())
}
