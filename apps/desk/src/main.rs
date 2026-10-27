#![forbid(unsafe_code)]
use ruvane::{ClientRuntime, InternalUrl, MediaType, Server};
use ruvane_application::{REVISION, native::Backend, pages};
use std::io::{Read, Write};
fn prerender_bytes() -> &'static [u8] {
    include!(concat!(env!("OUT_DIR"), "/ruvane-prerender.rs"))
}
fn application(backend: Option<&Backend>) -> Result<ruvane::App, Box<dyn std::error::Error>> {
    let mut app = match backend {
        Some(b) => b.application()?,
        None => pages::application(),
    };
    app.asset(
        "/assets/site.css",
        include_bytes!("../assets/site.css"),
        MediaType::Css,
    )?;
    let mut runtime = ClientRuntime::new(include_bytes!(env!("RUVANE_WASM_FILE")), REVISION)?
        .shell(pages::shell)
        .csrf_endpoint(InternalUrl::new("/csrf")?)?
        .stylesheet(InternalUrl::new("/assets/site.css")?)?;
    if let (Some(path), Some(session)) = (
        option_env!("RUVANE_DEV_STATUS"),
        option_env!("RUVANE_DEV_SESSION"),
    ) {
        runtime = runtime.development(
            ruvane::Development::new(path, session.parse()?)?
                .stylesheet("/assets/site.css", "assets/site.css")?,
        );
    }
    app.client_runtime(runtime)?;
    if !prerender_bytes().is_empty() {
        app.client_prerendered(prerender_bytes())?;
    }
    if !cfg!(debug_assertions)
        && app.has_client_prerender()
        && prerender_bytes().is_empty()
        && option_env!("RUVANE_PRERENDER_BOOTSTRAP").is_none()
    {
        return Err(
            "missing embedded pre-render artifact; enable the Ruvane.prerender build capability"
                .into(),
        );
    }
    Ok(app)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "--ruvane-prerender" {
        application(None)?
            .client_prerender_plan()?
            .write_new(&args[1])?;
        return Ok(());
    }
    if args.len() == 2 && args[0] == "--ruvane-prerender-verify" {
        let _ = application(None)?;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?
            .write_all(prerender_bytes())?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "--ruvane-client-assets" {
        application(None)?.client_assets()?.export_to(&args[1])?;
        let mut identity = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        writeln!(
            identity,
            "ruvane-runtime\t1\t{}\t{REVISION}",
            ruvane::BROWSER_ABI_VERSION
        )?;
        return Ok(());
    }
    if args.len() == 2 && args[0] == "--recover" {
        let backend = Backend::open(std::path::Path::new(&args[1]), true)?;
        println!(
            "Recovered {} tickets. The server was not started.",
            backend.store.count()?
        );
        return Ok(());
    }
    let managed = args.first().is_some_and(|arg| arg == "--ruvane-serve");
    let port = if (managed && args.len() == 3) || (args.len() == 2 && args[0] == "--port") {
        args[1].to_str().ok_or("invalid port")?.parse::<u16>()?
    } else if args.is_empty() {
        3000
    } else {
        return Err("usage: ruvane-app [--port PORT] | --recover ABSOLUTE_DATA_DIRECTORY".into());
    };
    let directory = std::env::var_os("RUVANE_DESK_DATA")
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join("desk-data"));
    let backend = Backend::open(&directory, false)?;
    let server = Server::bind(
        ([127, 0, 0, 1], port).into(),
        application(Some(&backend))?.router(None)?,
    )?;
    if managed {
        if option_env!("RUVANE_DEV_STATUS").is_some() {
            let mut path = args[2].clone();
            path.push(".styles");
            let record = application(None)?.development_styles()?;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
                .write_all(record.as_bytes())?;
        }
        let shutdown = server.shutdown_handle();
        std::thread::spawn(move || {
            let _ = std::io::stdin().read_exact(&mut [0]);
            shutdown.shutdown();
        });
        let mut ready = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        writeln!(ready, "ruvane-ready\t1\t{}", server.local_addr()?)?;
    }
    println!(
        "Ruvane Desk at http://{} (revision {REVISION})",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
