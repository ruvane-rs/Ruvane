//! Native multipart forms with a bounded shared memory store.
use ruvane::*;
use std::sync::{Arc, Mutex};
const MAX_FILES: usize = 8;
struct Entry {
    caption: String,
    file: UploadedFile,
}
struct Uploads {
    entries: Mutex<Vec<Entry>>,
    sessions: SessionStore,
}
fn failure() -> Response {
    Response::text("Demo unavailable.").with_status(StatusCode::ServiceUnavailable)
}
impl Uploads {
    fn page(&self, request: &Request, caption: &str, error: Option<(&str, &str)>) -> Response {
        let caption = if caption.chars().any(char::is_control) {
            ""
        } else {
            caption
        };
        let session = match self.sessions.lookup(request) {
            Ok(session) => session,
            Err(_) => return failure(),
        };
        let mut content=element(Tag::Main).child(element(Tag::H1).child("Ruvane native file uploads"))
            .child(element(Tag::P).child("Local demo sessions. Shared memory storage; no real account authentication."))
            .child(element(Tag::P).child("One file up to 32 KiB. Files stay opaque and are never executed or written to disk."));
        if let Some(session) = session {
            let token = match session.csrf_token() {
                Ok(Some(token)) => token,
                _ => return failure(),
            };
            let mut form = MultipartForm::new(InternalUrl::new("/upload").unwrap())
                .unwrap()
                .csrf(token.clone())
                .unwrap()
                .text("caption", "Caption", caption)
                .unwrap()
                .file("attachment", "File (maximum 32 KiB)")
                .unwrap()
                .submit_label("Upload file")
                .unwrap();
            if let Some((field, message)) = error {
                form = form.invalid(field).unwrap();
                content = content.child(element(Tag::P).child(message));
            }
            content = content.child(form.render()).child(
                PostForm::new(InternalUrl::new("/logout").unwrap())
                    .unwrap()
                    .csrf(token)
                    .unwrap()
                    .submit_label("Close demo session")
                    .unwrap()
                    .render(),
            );
            let Ok(entries) = self.entries.lock() else {
                return failure();
            };
            content = content.child(
                element(Tag::H2).child(format!("Stored files: {} / {MAX_FILES}", entries.len())),
            );
            let mut list = element(Tag::Ul);
            for entry in entries.iter() {
                list = list.child(
                    element(Tag::Li)
                        .child(element(Tag::H3).child(entry.caption.as_str()))
                        .child(element(Tag::P).child(format!(
                            "{} — {} bytes — declared type: {}",
                            entry.file.file_name(),
                            entry.file.len(),
                            entry.file.content_type()
                        ))),
                );
            }
            content = content.child(list);
        } else {
            content = content.child(
                PostForm::new(InternalUrl::new("/demo-session").unwrap())
                    .unwrap()
                    .submit_label("Open demo session")
                    .unwrap()
                    .render(),
            );
        }
        Response::html(
            Document::new("Ruvane multipart demo", content)
                .stylesheet(InternalUrl::new("/multipart-demo.css").unwrap())
                .render(),
        )
    }
    fn upload(&self, request: &Request) -> Response {
        let data = request.multipart().unwrap();
        let fields = data.fields();
        let caption = fields.get("caption").unwrap_or("");
        if !fields.has_only(&["caption"])
            || caption.trim().is_empty()
            || caption.len() > 80
            || caption.chars().any(char::is_control)
        {
            return self.page(request,caption,Some(("caption","Enter a nonblank caption of at most 80 bytes. Select the file again before submitting."))).with_status(StatusCode::BadRequest);
        }
        let Some(file) = data
            .file()
            .filter(|file| file.field_name() == "attachment" && !file.file_name().is_empty())
        else {
            return self.page(request,caption,Some(("attachment","Select one file. File selections cannot be restored after a form submission."))).with_status(StatusCode::BadRequest);
        };
        let Ok(mut entries) = self.entries.lock() else {
            return failure();
        };
        if entries.len() == MAX_FILES {
            return failure();
        }
        entries.push(Entry {
            caption: caption.to_owned(),
            file: file.clone(),
        });
        Response::see_other(InternalUrl::new("/").unwrap())
    }
}
pub fn app() -> Result<App, Box<dyn std::error::Error>> {
    let sessions = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("ruvane_multipart_demo")?,
    )?;
    let uploads = Arc::new(Uploads {
        entries: Mutex::new(Vec::new()),
        sessions: sessions.clone(),
    });
    let quota = RateLimiter::new(RateLimitPolicy::new(64, 60)?, RateLimitLimits::default());
    let public = RateLimiter::new(RateLimitPolicy::new(8, 60)?, RateLimitLimits::default());
    let mut app = App::new();
    app.asset(
        "/multipart-demo.css",
        include_bytes!("multipart_demo.css"),
        MediaType::Css,
    )?;
    let mut routes = FileRouteRegistry::new(&mut app);
    let page = uploads.clone();
    routes.endpoint(
        "root/route.rs",
        "/",
        FileEndpoint::new().get(move |request| page.page(request, "", None)),
    )?;
    let open = sessions.clone();
    routes.endpoint(
        "demo-session/route.rs",
        "/demo-session",
        FileEndpoint::new()
            .post_form(move |request| match open.lookup(request) {
                Ok(Some(_)) => Response::see_other(InternalUrl::new("/").unwrap()),
                Ok(None) => match open.create(SessionData::default()) {
                    Ok(cookie) => Response::see_other(InternalUrl::new("/").unwrap())
                        .with_cookie(cookie)
                        .unwrap(),
                    Err(_) => failure(),
                },
                Err(_) => failure(),
            })
            .guard(public.guard()),
    )?;
    routes.endpoint(
        "upload/route.rs",
        "/upload",
        FileEndpoint::new()
            .post_multipart(move |request| uploads.upload(request))
            .guard(sessions.csrf_guard())
            .guard(quota.session_guard(sessions.clone())),
    )?;
    let close = sessions.clone();
    routes.endpoint(
        "logout/route.rs",
        "/logout",
        FileEndpoint::new()
            .post_form(move |request| match close.lookup(request) {
                Ok(Some(session)) if close.revoke(&session).is_ok() => {
                    Response::see_other(InternalUrl::new("/").unwrap())
                        .with_cookie(close.removal_cookie())
                        .unwrap()
                }
                _ => failure(),
            })
            .guard(sessions.csrf_guard())
            .guard(quota.session_guard(sessions)),
    )?;
    routes.finish()?;
    Ok(app)
}
