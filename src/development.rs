//! Local development diagnostics and browser reload assets.

use crate::{Html, MediaType, Request, Response, RouteError, Router, StaticPath, StatusCode};
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
    pub(crate) styles: Vec<(String, String)>,
}

impl Development {
    /// Opens a CLI-owned status file through real, absolute directories.
    pub fn new(path: impl Into<PathBuf>, session: u64) -> io::Result<Self> {
        let value = Self {
            path: path.into(),
            session,
            styles: Vec::new(),
        };
        if session == 0 {
            return Err(invalid());
        }
        value.read()?;
        Ok(value)
    }

    /// Declares a development stylesheet and its project-relative source in assets/.
    /// The URL must also name an embedded CSS asset in the application.
    pub fn stylesheet(mut self, url: &str, source: &str) -> io::Result<Self> {
        if self.styles.len() == crate::MAX_PAGE_STYLESHEETS
            || !style_url(url)
            || !source.starts_with("assets/")
            || !style_url(&format!("/{source}"))
            || self
                .styles
                .iter()
                .any(|(old_url, old_source)| old_url == url || old_source == source)
        {
            return Err(invalid());
        }
        self.styles.push((url.into(), source.into()));
        Ok(self)
    }

    fn read(&self) -> io::Result<String> {
        let bytes = read_file(&self.path, MAX_DEVELOPMENT_STATUS_BYTES)?;
        let text = String::from_utf8(bytes).map_err(|_| invalid())?;
        validate_status(&text, self.session)?;
        Ok(text)
    }

    pub(crate) fn css(
        &self,
        request: &Request,
        revision: u64,
        index: usize,
        embedded: &'static [u8],
    ) -> Response {
        if !same_origin(request) {
            return Response::error(StatusCode::Forbidden);
        }
        let generation = if let Some(value) = request.query_params().get("ruvane-css") {
            if request.query_params().len() != 1 {
                return Response::error(StatusCode::BadRequest);
            }
            let parts: Vec<_> = value.split('-').collect();
            if parts.len() != 3
                || decimal(parts[0]).ok() != Some(self.session)
                || decimal(parts[1]).ok() != Some(revision)
                || decimal(parts[2]).ok().filter(|value| *value > 0).is_none()
            {
                return Response::error(StatusCode::Conflict);
            }
            decimal(parts[2]).unwrap()
        } else {
            let status = self
                .read()
                .and_then(|text| parse_status(&text, self.session));
            match status {
                Ok(status)
                    if status.revision == revision
                        && status
                            .styles
                            .iter()
                            .map(String::as_str)
                            .eq(self.styles.iter().map(|(url, _)| url.as_str()))
                        && status.css > 0 =>
                {
                    status.css
                }
                _ => return Response::embedded(embedded, MediaType::Css),
            }
        };
        let path = self
            .path
            .parent()
            .unwrap()
            .join(format!("css-{revision}-{generation}-{index}"));
        match read_file(&path, crate::MAX_ASSET_BYTES) {
            Ok(bytes) => Response::bytes(bytes, MediaType::Css),
            Err(_) => Response::error(StatusCode::NotFound),
        }
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
        if !same_origin(request) || request.header("accept") != Some("text/plain") {
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

fn read_file(path: &std::path::Path, limit: usize) -> io::Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir | Component::CurDir) {
            return Err(invalid());
        }
        current.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink()
            || if current == path {
                !metadata.is_file()
            } else {
                !metadata.is_dir()
            }
        {
            return Err(invalid());
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid development status")
}

fn same_origin(request: &Request) -> bool {
    let origin = request.header("host").map(|host| format!("http://{host}"));
    !request
        .header("sec-fetch-site")
        .is_some_and(|site| !matches!(site, "same-origin" | "none"))
        && request
            .header("origin")
            .is_none_or(|value| Some(value) == origin.as_deref())
}

fn style_url(url: &str) -> bool {
    url.len() <= 256
        && url.ends_with(".css")
        && !url.starts_with("/_ruvane/")
        && StaticPath::new(url).is_ok()
}

struct Status {
    revision: u64,
    css: u64,
    styles: Vec<String>,
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
    parse_status(text, session).map(|_| ())
}

fn parse_status(text: &str, session: u64) -> io::Result<Status> {
    let (header, message) = text.split_once("\n\n").ok_or_else(invalid)?;
    let lines: Vec<_> = header.split('\n').collect();
    if lines.len() < 5
        || !matches!(lines[0], "RVD1" | "RVD2")
        || decimal(lines[1])? != session
        || decimal(lines[2])? == 0
        || !matches!(lines[3], "building" | "ready" | "error")
        || lines[3] == "ready" && decimal(lines[4])? == 0
    {
        return Err(invalid());
    }
    let revision = decimal(lines[4])?;
    let (css, styles) = if lines[0] == "RVD2" {
        if lines.len() < 7 {
            return Err(invalid());
        }
        let css = decimal(lines[5])?;
        let count = usize::try_from(decimal(lines[6])?).map_err(|_| invalid())?;
        if count == 0
            || count > crate::MAX_PAGE_STYLESHEETS
            || css == 0
            || revision == 0
            || lines.len() != 7 + count
        {
            return Err(invalid());
        }
        let mut styles = Vec::new();
        for value in &lines[7..] {
            if !style_url(value) || styles.iter().any(|old| old == value) {
                return Err(invalid());
            }
            styles.push((*value).to_owned());
        }
        (css, styles)
    } else {
        if lines.len() != 5 {
            return Err(invalid());
        }
        (0, Vec::new())
    };
    if message
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(invalid());
    }
    Ok(Status {
        revision,
        css,
        styles,
    })
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
    #[test]
    fn css_status_rejects_invalid_counts_generations_and_urls() {
        assert!(validate_status("RVD2\n9\n1\nready\n123\n1\n1\n/assets/site.css\n\n", 9).is_ok());
        for text in [
            "RVD2\n9\n1\nready\n123\n0\n1\n/assets/site.css\n\n",
            "RVD2\n9\n1\nready\n123\n1\n2\n/assets/site.css\n\n",
            "RVD2\n9\n1\nready\n123\n1\n2\n/assets/site.css\n/assets/site.css\n\n",
            "RVD2\n9\n1\nready\n123\n1\n1\n/assets/../secret.css\n\n",
            "RVD2\n9\n1\nready\n123\n1\n1\nhttps://example.com/style.css\n\n",
        ] {
            assert!(validate_status(text, 9).is_err(), "{text}");
        }
    }
}
