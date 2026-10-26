mod development;
mod prerender;
mod process;
mod project;
mod scaffold;
#[cfg(test)]
mod tests;
mod watch;

use ruvane::{Server, SiteOrigin, StaticSite};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const INTERACTIVE_MARKER: &str = "ruvane-project\t2\tinteractive\n";
const APP_BUILDS_MARKER: &str = "ruvane-cli-app-builds\t1\n";
const PROJECT_MARKER: &str = "ruvane-project\t1\n";
const WORK_MARKER: &str = "ruvane-cli-work\t1\n";
const BUILDS_MARKER: &str = "ruvane-cli-builds\t1\n";

const HELP: &str = "Ruvane — zero third-party dependencies

  ruvane new NAME [--interactive [--file-routes]] [--framework-path PATH]
  ruvane dev [--port PORT] [--offline]
  ruvane build [--origin ORIGIN] [--out NAME] [--offline]
  ruvane preview DIRECTORY [--port PORT]

Run dev/build inside a generated trusted project. Preview accepts an existing
flat export or a versioned build directory. Open the printed HTTP URL.
Interactive starters reload after successful builds when no input or action is at risk.
";

pub(super) fn run(arguments: impl Iterator<Item = OsString>) -> Result<()> {
    let args: Vec<_> = arguments.take(34).collect();
    if args.len() > 32 {
        return Err("too many arguments".into());
    }
    if args.iter().any(|arg| arg.len() > 4096) {
        return Err("argument too large".into());
    }
    let Some(command) = args.first().and_then(|value| value.to_str()) else {
        print!("{HELP}");
        return Ok(());
    };
    if matches!(command, "--version" | "-V") {
        println!("ruvane {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if matches!(command, "--help" | "-h")
        || args[1..].iter().any(|arg| arg == "--help" || arg == "-h")
    {
        print!("{HELP}");
        return Ok(());
    }
    match command {
        "new" => new(&args[1..]),
        "dev" => dev(&args[1..]),
        "build" => build(&args[1..]),
        "preview" => preview(&args[1..]),
        _ => Err("unknown command; use ruvane --help".into()),
    }
}

fn option<'a>(args: &'a [OsString], index: &mut usize) -> Result<&'a OsStr> {
    *index += 1;
    args.get(*index)
        .map(OsString::as_os_str)
        .ok_or_else(|| "missing option value".into())
}

fn port(value: &OsStr) -> Result<u16> {
    let value = value.to_str().ok_or("invalid port")?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("invalid port".into());
    }
    Ok(value.parse()?)
}

fn new(args: &[OsString]) -> Result<()> {
    let name = args
        .first()
        .and_then(|name| name.to_str())
        .ok_or("new requires a project name")?;
    let mut framework = None;
    let mut interactive = false;
    let mut file_routes = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].to_str() {
            Some("--framework-path") if framework.is_none() => {
                framework = Some(PathBuf::from(option(args, &mut index)?))
            }
            Some("--interactive") if !interactive => interactive = true,
            Some("--file-routes") if !file_routes => file_routes = true,
            _ => return Err("invalid new option".into()),
        }
        index += 1;
    }
    if file_routes && !interactive {
        return Err("--file-routes requires --interactive".into());
    }
    scaffold::create(name, framework.as_deref(), interactive, file_routes)?;
    println!("Created {name}.\nNext: cd {name}\n      ruvane dev");
    Ok(())
}

fn dev(args: &[OsString]) -> Result<()> {
    let mut selected_port = 3000;
    let mut has_port = false;
    let mut offline = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--port") if !has_port => {
                selected_port = port(option(args, &mut index)?)?;
                has_port = true;
            }
            Some("--offline") if !offline => offline = true,
            _ => return Err("invalid dev option".into()),
        }
        index += 1;
    }
    let project = project::Project::open()?;
    let _lock = project.lock()?;
    let quit = Arc::new(AtomicBool::new(false));
    let input_quit = Arc::clone(&quit);
    std::thread::spawn(move || {
        use std::io::Read;
        let mut stdin = std::io::stdin().lock();
        let mut byte = [0];
        loop {
            match stdin.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) if matches!(byte[0], b'q' | b'Q') => break,
                Ok(_) => {}
            }
        }
        input_quit.store(true, Ordering::SeqCst);
    });
    println!(
        "Watching src/, assets/, content/, build.rs, Cargo.toml and Cargo.lock. Type q then Enter to stop."
    );
    let mut observed = watch::snapshot(&project.root)?;
    let mut pending = true;
    let mut changed_at = Instant::now() - Duration::from_secs(1);
    let mut running: Option<process::App> = None;
    let mut session = if project.interactive {
        Some(development::Session::new(&project)?)
    } else {
        None
    };
    let mut waiting = if let Some(session) = &session {
        Some(session.waiting(selected_port)?)
    } else {
        None
    };
    if let Some(server) = &waiting {
        selected_port = server.address.port();
        println!(
            "Ruvane dev waiting at http://{} — waiting for the first build.",
            server.address
        );
    }
    while !quit.load(Ordering::SeqCst) {
        let current = match watch::snapshot(&project.root) {
            Ok(snapshot) => snapshot,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                // Editors may remove files while saving. Keep serving until the scan succeeds.
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
            Err(error) => return Err(error),
        };
        if current != observed {
            observed = current;
            pending = true;
            changed_at = Instant::now();
        }
        if pending && changed_at.elapsed() >= Duration::from_millis(200) {
            pending = false;
            if let (Some(session), Some(app)) = (&mut session, &mut running) {
                match session.reload_styles(&project, app, &observed) {
                    Ok(true) => {
                        println!("Updated styles without compilation or restart.");
                        continue;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        session.publish(
                            "error",
                            app.revision(),
                            &format!("Stylesheet update failed: {error}"),
                        )?;
                        eprintln!("Stylesheet update failed: {error}; keeping previous styles.");
                        continue;
                    }
                }
            }
            println!("Compiling application...");
            let active = running.as_ref().map_or(0, process::App::revision);
            if let Some(session) = &mut session {
                session.publish("building", active, "")?;
            }
            match process::compile(&project, offline, &quit, session.as_ref()) {
                Ok(binary) => {
                    if quit.load(Ordering::SeqCst) {
                        break;
                    }
                    // Keep the old executable until the new one starts.
                    let mut previous = running.take();
                    if let Some(app) = &mut previous {
                        app.stop();
                    }
                    drop(waiting.take());
                    match process::App::start(binary, selected_port, &quit) {
                        Ok(app) => {
                            selected_port = app.address.port();
                            if let Some(session) = &mut session {
                                if let Some(sources) = app.sources() {
                                    if let Err(error) =
                                        session.capture_styles(&project, &app, sources)
                                    {
                                        eprintln!("Could not prepare development styles: {error}");
                                    }
                                }
                                session.publish("ready", app.revision(), "")?;
                            }
                            println!("Ruvane dev at http://{} — application ready.", app.address);
                            running = Some(app);
                        }
                        Err(error) => {
                            eprintln!("Application startup failed: {error}");
                            let message = format!("Application startup failed: {error}");
                            if let Some(mut app) = previous {
                                match app.restart(selected_port, &quit) {
                                    Ok(()) => {
                                        println!(
                                            "Restored the previous server at http://{}",
                                            app.address
                                        );
                                        running = Some(app);
                                    }
                                    Err(error) => {
                                        eprintln!("Could not restore previous server: {error}")
                                    }
                                }
                            }
                            if let Some(session) = &mut session {
                                session.publish(
                                    "error",
                                    running.as_ref().map_or(0, process::App::revision),
                                    &message,
                                )?;
                                if running.is_none() {
                                    waiting = Some(session.waiting(selected_port)?);
                                }
                            }
                        }
                    }
                }
                Err(error) if !quit.load(Ordering::SeqCst) => {
                    if let Some(session) = &mut session {
                        session.publish("error", active, &process::diagnostics(error.as_ref()))?;
                    }
                    eprintln!("{error}; keeping the previous server when available.");
                }
                Err(_) => break,
            }
            // Ignore Cargo's lockfile write, but keep edits made during compilation.
            watch::refresh_lock(&project.root, &mut observed)?;
        }
        if let Some(app) = &mut running {
            if app.exited()? {
                eprintln!("Application exited; waiting for a source change.");
                running = None;
                if let Some(session) = &mut session {
                    session.publish(
                        "error",
                        0,
                        "Application exited. Save a source file to rebuild.",
                    )?;
                    waiting = Some(session.waiting(selected_port)?);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    drop(running);
    drop(waiting);
    println!("Development server stopped.");
    Ok(())
}

fn build(args: &[OsString]) -> Result<()> {
    let mut origin = None;
    let mut output = OsString::from("dist");
    let mut has_output = false;
    let mut offline = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--origin") if origin.is_none() => {
                origin = Some(SiteOrigin::parse(
                    option(args, &mut index)?.to_str().ok_or("invalid origin")?,
                )?)
            }
            Some("--out") if !has_output => {
                output = option(args, &mut index)?.to_owned();
                has_output = true;
            }
            Some("--offline") if !offline => offline = true,
            _ => return Err("invalid build option".into()),
        }
        index += 1;
    }

    let output = output.to_str().ok_or("invalid output name")?;
    if !scaffold::valid_name(output)
        || matches!(
            output,
            "src" | "assets" | "target" | "tests" | "examples" | "browser" | "content"
        )
    {
        return Err("output must be a simple project directory name".into());
    }
    let project = project::Project::open()?;
    let _lock = project.lock()?;
    if project.interactive {
        if origin.is_some() {
            return Err("interactive builds do not use --origin in this profile".into());
        }
        return process::build_application(&project, output, offline);
    }
    let origin = origin.ok_or("build requires --origin")?;
    let container = project.root.join(output);
    project::managed(&container, ".ruvane-builds", BUILDS_MARKER)?;
    let version = project::next_build(&container)?;
    let destination = container.join(&version);
    let quit = AtomicBool::new(false);
    let binary = process::compile(&project, offline, &quit, None)?;
    process::export(&binary, &destination, &origin, &quit)?;
    StaticSite::preview_export(&destination)?;
    project::publish(&container, &version)?;
    println!(
        "Built {}\nPreview: ruvane preview {output}",
        destination.display()
    );
    Ok(())
}

fn preview(args: &[OsString]) -> Result<()> {
    let directory = args.first().ok_or("preview requires a directory")?;
    let mut selected_port = 3001;
    let mut index = 1;
    if index < args.len() {
        if args[index] != "--port" {
            return Err("invalid preview option".into());
        }
        selected_port = port(option(args, &mut index)?)?;
        index += 1;
    }
    if index != args.len() {
        return Err("invalid preview option".into());
    }
    let root = Path::new(directory);
    let export = if fs::symlink_metadata(root.join(".ruvane-builds")).is_ok() {
        project::check_managed(root, ".ruvane-builds", BUILDS_MARKER)?;
        root.join(project::current(root)?)
    } else {
        root.to_owned()
    };
    let router = StaticSite::preview_export(export)?;
    let server = Server::bind(([127, 0, 0, 1], selected_port).into(), router)?;
    println!(
        "Ruvane preview at http://{} — open this HTTP address, not index.html.",
        server.local_addr()?
    );
    server.run()?;
    Ok(())
}
