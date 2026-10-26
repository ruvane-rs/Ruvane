use super::{PROJECT_MARKER, Result, project};
use std::fs;
use std::path::Path;

pub(super) fn valid_name(value: &str) -> bool {
    let Some(first) = value.as_bytes().first() else {
        return false;
    };
    value.len() <= 48
        && first.is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
        && !matches!(
            value,
            "as" | "break"
                | "const"
                | "continue"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "static"
                | "struct"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
                | "async"
                | "await"
                | "dyn"
                | "gen"
        )
        && !matches!(
            value,
            "con"
                | "prn"
                | "aux"
                | "nul"
                | "self"
                | "super"
                | "crate"
                | "std"
                | "core"
                | "alloc"
                | "test"
        )
        && !(value.len() == 4
            && (value.starts_with("com") || value.starts_with("lpt"))
            && value.as_bytes()[3].is_ascii_digit())
}

fn quoted(value: &str) -> Result<String> {
    if value.chars().any(char::is_control) {
        return Err("control character in framework path".into());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub(super) fn create(
    name: &str,
    framework: Option<&Path>,
    interactive: bool,
    file_routes: bool,
) -> Result<()> {
    if interactive {
        return create_interactive(name, framework, file_routes);
    }
    if !valid_name(name) {
        return Err(
            "project name must be a portable lowercase identifier, at most 48 bytes".into(),
        );
    }
    let root = std::env::current_dir()?;
    project::checked_dir(&root)?;
    let dependency = if let Some(path) = framework {
        project::checked_dir(path)?;
        let path = fs::canonicalize(path)?;
        project::regular(&path.join("Cargo.toml"))?;
        format!(
            "{{ path = {} }}",
            quoted(path.to_str().ok_or("framework path must be UTF-8")?)?
        )
    } else {
        quoted(&format!("={}", env!("CARGO_PKG_VERSION")))?
    };
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.85\"\npublish = false\n\n[workspace]\n\n[[bin]]\nname = \"ruvane-app\"\npath = \"src/main.rs\"\n\n[dependencies]\nruvane = {dependency}\n\n[build-dependencies]\nruvane = {dependency}\n"
    );
    let destination = root.join(name);
    fs::create_dir(&destination).map_err(|_| "project destination must be new")?;
    fs::create_dir(destination.join("src"))?;
    fs::create_dir(destination.join("assets"))?;
    fs::create_dir(destination.join("content"))?;
    for (path, body) in [
        ("Cargo.toml", manifest.as_str()),
        ("Ruvane.project", PROJECT_MARKER),
        (".gitignore", "/target/\n/.ruvane/\n/dist/\n.DS_Store\n"),
        ("README.md", include_str!("templates/README.md")),
        ("build.rs", include_str!("templates/build.rs.txt")),
        ("src/content.rs", include_str!("templates/content.rs.txt")),
        ("content/welcome.md", include_str!("templates/welcome.md")),
        ("src/main.rs", include_str!("templates/main.rs.txt")),
        ("src/pages.rs", include_str!("templates/pages.rs.txt")),
        (
            "src/components.rs",
            include_str!("templates/components.rs.txt"),
        ),
        ("assets/site.css", include_str!("templates/site.css")),
    ] {
        project::write_new(&destination.join(path), body.as_bytes())?;
    }
    Ok(())
}

fn create_interactive(name: &str, framework: Option<&Path>, file_routes: bool) -> Result<()> {
    if matches!(
        name,
        "ruvane" | "ruvane-browser" | "ruvane-application-browser"
    ) {
        return Err("project name conflicts with a framework package".into());
    }
    if !valid_name(name) {
        return Err(
            "project name must be a portable lowercase identifier, at most 48 bytes".into(),
        );
    }
    let root = std::env::current_dir()?;
    project::checked_dir(&root)?;
    let framework = framework
        .ok_or("interactive projects currently require --framework-path to a Ruvane checkout")?;
    project::checked_dir(framework)?;
    let framework = fs::canonicalize(framework)?;
    project::regular(&framework.join("Cargo.toml"))?;
    project::checked_dir(&framework.join("browser"))?;
    project::regular(&framework.join("browser/Cargo.toml"))?;
    let core = quoted(framework.to_str().ok_or("framework path must be UTF-8")?)?;
    let adapter = quoted(
        framework
            .join("browser")
            .to_str()
            .ok_or("framework path must be UTF-8")?,
    )?;
    let mut manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.85\"\npublish = false\n\n[workspace]\nmembers = [\"browser\"]\ndefault-members = [\".\"]\nresolver = \"2\"\n\n[lib]\nname = \"ruvane_application\"\npath = \"src/lib.rs\"\n\n[[bin]]\nname = \"ruvane-app\"\npath = \"src/main.rs\"\n\n[dependencies]\nruvane = {{ path = {core} }}\n"
    );
    if file_routes {
        manifest.push_str(&format!(
            "\n[build-dependencies]\nruvane = {{ path = {core} }}\n"
        ));
    }
    let browser = format!(
        "[package]\nname = \"ruvane-application-browser\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.85\"\npublish = false\n\n[lib]\nname = \"ruvane_application_browser\"\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nruvane-application = {{ package = \"{name}\", path = \"..\" }}\nruvane-browser = {{ path = {adapter}, default-features = false }}\n"
    );
    let destination = root.join(name);
    fs::create_dir(&destination).map_err(|_| "project destination must be new")?;
    for directory in ["src", "assets", "browser", "browser/src"] {
        fs::create_dir(destination.join(directory))?;
    }
    for (path, body) in [
        ("Cargo.toml", manifest.as_str()),
        ("browser/Cargo.toml", browser.as_str()),
        ("Ruvane.project", super::INTERACTIVE_MARKER),
        (".gitignore", "/target/\n/.ruvane/\n/dist/\n.DS_Store\n"),
        (
            "src/lib.rs",
            include_str!("templates/interactive/lib.rs.txt"),
        ),
        (
            "src/main.rs",
            include_str!("templates/interactive/main.rs.txt"),
        ),
        (
            "src/pages.rs",
            if file_routes {
                include_str!("templates/file_routes/pages.rs.txt")
            } else {
                include_str!("templates/interactive/pages.rs.txt")
            },
        ),
        (
            "src/components.rs",
            include_str!("templates/interactive/components.rs.txt"),
        ),
        (
            "src/remote_tasks.rs",
            include_str!("templates/interactive/remote_tasks.rs.txt"),
        ),
        (
            "src/store.rs",
            include_str!("templates/interactive/store.rs.txt"),
        ),
        (
            "browser/src/lib.rs",
            include_str!("templates/interactive/browser.rs.txt"),
        ),
        (
            "README.md",
            if file_routes {
                include_str!("templates/file_routes/README.md")
            } else {
                include_str!("templates/interactive/README.md")
            },
        ),
        ("assets/site.css", include_str!("../../../web/browser.css")),
    ] {
        project::write_new(&destination.join(path), body.as_bytes())?;
    }
    if file_routes {
        for directory in [
            "src/routes",
            "src/routes/tasks",
            "src/routes/tasks/[id]",
            "src/routes/tasks/settings",
        ] {
            fs::create_dir(destination.join(directory))?;
        }
        for (path, source) in [
            (
                "build.rs",
                include_str!("templates/file_routes/build.rs.txt"),
            ),
            (
                "src/routes/page.rs",
                include_str!("templates/file_routes/home.rs.txt"),
            ),
            (
                "src/routes/not_found.rs",
                include_str!("templates/file_routes/root_not_found.rs.txt"),
            ),
            (
                "src/routes/tasks/not_found.rs",
                include_str!("templates/file_routes/task_not_found.rs.txt"),
            ),
            (
                "src/routes/tasks/page.rs",
                include_str!("templates/file_routes/tasks.rs.txt"),
            ),
            (
                "src/routes/tasks/layout.rs",
                include_str!("templates/file_routes/workspace.rs.txt"),
            ),
            (
                "src/routes/tasks/loading.rs",
                include_str!("templates/file_routes/loading.rs.txt"),
            ),
            (
                "src/routes/tasks/error.rs",
                include_str!("templates/file_routes/error.rs.txt"),
            ),
            (
                "src/routes/tasks/[id]/page.rs",
                include_str!("templates/file_routes/task.rs.txt"),
            ),
            (
                "src/routes/tasks/[id]/layout.rs",
                include_str!("templates/file_routes/context.rs.txt"),
            ),
            (
                "src/routes/tasks/settings/page.rs",
                include_str!("templates/file_routes/settings.rs.txt"),
            ),
        ] {
            project::write_new(&destination.join(path), source.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_paths_are_toml_strings_not_configuration_fragments() {
        assert_eq!(
            quoted("/a/quoted\"path\\name").unwrap(),
            "\"/a/quoted\\\"path\\\\name\""
        );
        for value in ["a\nb", "a\rb", "a\0b", "a\tb"] {
            assert!(quoted(value).is_err());
        }
    }
}
