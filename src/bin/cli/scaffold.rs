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

pub(super) fn create(name: &str, framework: Option<&Path>) -> Result<()> {
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
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.85\"\npublish = false\n\n[workspace]\n\n[[bin]]\nname = \"ruvane-app\"\npath = \"src/main.rs\"\n\n[dependencies]\nruvane = {dependency}\n"
    );
    let destination = root.join(name);
    fs::create_dir(&destination).map_err(|_| "project destination must be new")?;
    fs::create_dir(destination.join("src"))?;
    fs::create_dir(destination.join("assets"))?;
    for (path, body) in [
        ("Cargo.toml", manifest.as_str()),
        ("Ruvane.project", PROJECT_MARKER),
        (".gitignore", "/target/\n/.ruvane/\n/dist/\n.DS_Store\n"),
        ("README.md", include_str!("templates/README.md")),
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
