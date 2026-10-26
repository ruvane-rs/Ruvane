//! Local development diagnostics and browser reload assets.

use crate::{Html, MediaType, Request, Response, RouteError, Router, StatusCode};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, PathBuf};

pub(crate) const STATUS_PATH: &str = "/_ruvane/dev/status";
pub(crate) const CLIENT_PATH: &str = "/_ruvane/dev/client.mjs";
pub(crate) const STYLE_PATH: &str = "/_ruvane/dev/style.css";
/// Maximum development status response, including compiler diagnostics.
pub const MAX_DEVELOPMENT_STATUS_BYTES: usize = 32 * 1024;

/// Explicit local-only development support. Never attach this to a production app.
///
/// The CLI writes one bounded status file per session. Session identifiers prevent
/// stale tabs from accepting a different server's status; they are not secrets.
/// The generated starter enables this only from compile-time CLI configuration.
#[derive(Clone)]
pub struct Development {
    path: PathBuf,
    session: u64,
}

impl Development {
    /// Opens a CLI-owned status file through real, absolute directories.
    pub fn new(path: impl Into<PathBuf>, session: u64) -> io::Result<Self> {
        let value = Self {
            path: path.into(),
            session,
        };
        if session == 0 {
            return Err(invalid());
        }
        value.read()?;
        Ok(value)
    }

    fn read(&self) -> io::Result<String> {
        if !self.path.is_absolute() {
            return Err(invalid());
        }
        let mut current = PathBuf::new();
        for part in self.path.components() {
            if matches!(part, Component::ParentDir | Component::CurDir) {
                return Err(invalid());
            }
            current.push(part.as_os_str());
            let metadata = fs::symlink_metadata(&current)?;
            if metadata.file_type().is_symlink()
                || if current == self.path {
                    !metadata.is_file()
                } else {
                    !metadata.is_dir()
                }
            {
                return Err(invalid());
            }
        }
        let mut bytes = Vec::new();
        fs::File::open(&self.path)?
            .take(MAX_DEVELOPMENT_STATUS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_DEVELOPMENT_STATUS_BYTES {
            return Err(invalid());
        }
        let text = String::from_utf8(bytes).map_err(|_| invalid())?;
        validate_status(&text, self.session)?;
        Ok(text)
    }

    /// Registers three same-origin, no-store development endpoints.
    /// Cross-origin requests and mismatched sessions cannot read diagnostics.
    pub fn mount(&self, router: &mut Router) -> Result<(), RouteError> {
        // Check all names before changing the router.
        if [STATUS_PATH, CLIENT_PATH, STYLE_PATH]
            .iter()
            .any(|path| router.has_path(path))
        {
            return Err(RouteError::DuplicatePath);
        }
        let value = self.clone();
        router.get(STATUS_PATH, move |request| value.response(request))?;
        router.get(CLIENT_PATH, |_| {
            Response::embedded(
                include_bytes!("../web/development.mjs"),
                MediaType::JavaScript,
            )
        })?;
        router.get(STYLE_PATH, |_| {
            Response::embedded(include_bytes!("../web/development.css"), MediaType::Css)
        })?;
        Ok(())
    }

    fn response(&self, request: &Request) -> Response {
        let origin = request.header("host").map(|host| format!("http://{host}"));
        if request
            .header("sec-fetch-site")
            .is_some_and(|site| !matches!(site, "same-origin" | "none"))
            || request
                .header("origin")
                .is_some_and(|value| Some(value) != origin.as_deref())
            || request.header("accept") != Some("text/plain")
        {
            return Response::error(StatusCode::Forbidden);
        }
        if request.header("ruvane-development") != Some(self.session.to_string().as_str()) {
            return Response::error(StatusCode::Conflict);
        }
        match self.read() {
            Ok(text) => Response::text(text),
            Err(_) => Response::text("Development status unavailable")
                .with_status(StatusCode::InternalServerError),
        }
    }

    pub(crate) fn script(&self, revision: u64) -> String {
        format!(
            "<script id=\"ruvane-development\" type=\"module\" async src=\"{CLIENT_PATH}\" data-session=\"{}\" data-revision=\"{revision}\"></script>",
            self.session
        )
    }

    /// Creates the temporary page shown while the first application build runs.
    /// Unknown GET paths show the same page, so an existing tab can reconnect.
    pub fn waiting_router(&self) -> Result<Router, RouteError> {
        let mut router = Router::new();
        self.mount(&mut router)?;
        let html = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Ruvane development</title></head><body><main><h1>Ruvane development</h1><p>Waiting for the application build…</p></main>{}</body></html>",
            self.script(0)
        );
        let page = Response::interactive_html(Html::rendered(html));
        let native = Response::html(
            crate::Document::new(
                "Ruvane development",
                crate::text("Waiting for the application build. Refresh after compilation."),
            )
            .render(),
        );
        router.get("/", {
            let page = page.clone();
            let native = native.clone();
            move |request| {
                if request.query_params().get("native") == Some("yes") {
                    native.clone()
                } else {
                    page.clone()
                }
            }
        })?;
        router.client_not_found(move |request| {
            if request.query_params().get("native") == Some("yes") {
                native.clone()
            } else {
                page.clone()
            }
        });
        Ok(router)
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid development status")
}

fn decimal(value: &str) -> io::Result<u64> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || value.len() > 1 && value.starts_with('0')
    {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}

fn validate_status(text: &str, session: u64) -> io::Result<()> {
    let (header, message) = text.split_once("\n\n").ok_or_else(invalid)?;
    let lines: Vec<_> = header.split('\n').collect();
    if lines.len() != 5
        || lines[0] != "RVD1"
        || decimal(lines[1])? != session
        || decimal(lines[2])? == 0
        || !matches!(lines[3], "building" | "ready" | "error")
        || lines[3] == "ready" && decimal(lines[4])? == 0
    {
        return Err(invalid());
    }
    decimal(lines[4])?;
    if message
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_schema_is_strict() {
        for state in ["building", "ready", "error"] {
            assert!(
                validate_status(&format!("RVD1\n9\n1\n{state}\n23\n\nerror <script>"), 9).is_ok()
            );
        }
        for text in [
            "RVD1\n8\n1\nready\n23\n\n",
            "RVD1\n9\n0\nready\n23\n\n",
            "RVD1\n9\n1\nready\n0\n\n",
            "RVD1\n9\n1\nother\n1\n\n",
            "RVD1\n9\n1\nready\n01\n\n",
            "RVD1\n9\n1\nerror\n0\n\n\u{1b}[31m",
        ] {
            assert!(validate_status(text, 9).is_err(), "{text:?}");
        }
    }
}
