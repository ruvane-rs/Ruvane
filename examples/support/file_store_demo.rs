//! Native uploads and private attachments with two public demo identities.
use ruvane::*;
use std::sync::Arc;
const BASE: &str = "/private-files";
struct Demo {
    files: FileStore,
    sessions: SessionStore,
}
fn fail(status: StatusCode, message: &str) -> Response {
    Response::text(message).with_status(status)
}
fn unavailable() -> Response {
    fail(
        StatusCode::ServiceUnavailable,
        "Storage unavailable. Inspect before retrying a write.",
    )
}
fn missing() -> Response {
    fail(StatusCode::NotFound, "File not found.")
}
fn url(path: &str) -> InternalUrl {
    InternalUrl::new(&format!("{BASE}{path}")).unwrap()
}
fn json(info: &FileInfo) -> JsonValue {
    JsonValue::object(vec![
        (
            "id".into(),
            JsonValue::string(info.id().to_string()).unwrap(),
        ),
        ("name".into(), JsonValue::string(info.name()).unwrap()),
        ("bytes".into(), (info.len() as u64).into()),
    ])
    .unwrap()
}
impl Demo {
    fn owner(&self, request: &Request) -> Result<FileOwner, StatusCode> {
        let session = self
            .sessions
            .lookup(request)
            .map_err(|_| StatusCode::ServiceUnavailable)?
            .ok_or(StatusCode::Forbidden)?;
        let data = session
            .data()
            .map_err(|_| StatusCode::ServiceUnavailable)?
            .ok_or(StatusCode::Forbidden)?;
        match data.get("file_owner") {
            Some(value @ ("alpha" | "beta")) => Ok(FileOwner::new(value).unwrap()),
            _ => Err(StatusCode::Forbidden),
        }
    }
    fn page(&self, request: &Request) -> Response {
        let mut body = element(Tag::Main).child(element(Tag::H1).child("Ruvane private file storage"))
            .child(element(Tag::P).child("Two public demo identities; no real account authentication. Files persist locally. One opaque file up to 32 KiB."));
        let session = match self.sessions.lookup(request) {
            Ok(session) => session,
            Err(_) => return unavailable(),
        };
        if let Some(session) = session {
            let token = match session.csrf_token() {
                Ok(Some(token)) => token,
                _ => return unavailable(),
            };
            let owner = match self.owner(request) {
                Ok(owner) => owner,
                Err(status) => return fail(status, "Owner authorization unavailable or denied."),
            };
            let name = session
                .data()
                .ok()
                .flatten()
                .and_then(|data| data.get("file_owner").map(str::to_owned))
                .unwrap_or_default();
            body = body
                .child(element(Tag::H2).child(format!("Owner: {name}")))
                .child(
                    MultipartForm::new(url("/files"))
                        .unwrap()
                        .csrf(token.clone())
                        .unwrap()
                        .file("attachment", "File (maximum 32 KiB)")
                        .unwrap()
                        .submit_label("Store file")
                        .unwrap()
                        .render(),
                );
            let files = match self.files.list(&owner) {
                Ok(files) => files,
                Err(_) => return unavailable(),
            };
            body = body.child(element(Tag::H2).child(format!("Your files: {} / 8", files.len())));
            let mut list = element(Tag::Ul);
            for file in files {
                list = list.child(
                    element(Tag::Li)
                        .child(element(Tag::H3).child(file.name()))
                        .child(element(Tag::P).child(format!("{} bytes", file.len())))
                        .child(link(
                            &url(&format!("/files/{}", file.id())),
                            "Download file",
                        ))
                        .child(
                            PostForm::new(url(&format!("/files/{}/remove", file.id())))
                                .unwrap()
                                .csrf(token.clone())
                                .unwrap()
                                .submit_label("Delete file")
                                .unwrap()
                                .render(),
                        ),
                );
            }
            body = body.child(list).child(
                PostForm::new(url("/logout"))
                    .unwrap()
                    .csrf(token)
                    .unwrap()
                    .submit_label("Close demo session")
                    .unwrap()
                    .render(),
            );
        } else {
            for account in ["alpha", "beta"] {
                body = body.child(
                    PostForm::new(url("/session"))
                        .unwrap()
                        .hidden("account", account)
                        .unwrap()
                        .submit_label(format!("Open {account} demo session"))
                        .unwrap()
                        .render(),
                );
            }
        }
        Response::html(
            Document::new("Ruvane private files", body)
                .stylesheet(InternalUrl::new("/file-store.css").unwrap())
                .render(),
        )
    }
    fn save(&self, request: &Request) -> Response {
        let owner = match self.owner(request) {
            Ok(owner) => owner,
            Err(status) => return fail(status, "Owner authorization unavailable or denied."),
        };
        let form = request.multipart().unwrap();
        let Some(file) = form
            .file()
            .filter(|file| file.field_name() == "attachment" && !file.file_name().is_empty())
        else {
            let mut errors = ValidationErrors::new();
            errors.add(Some("attachment"), ValidationCode::Required);
            return Response::validation_errors(&errors);
        };
        if !form.fields().has_only(&[]) {
            return fail(StatusCode::BadRequest, "Unexpected business fields.");
        }
        match self.files.save(&owner, file) {
            Ok(info) if request.header("accept") == Some(JSON_MEDIA_TYPE) => {
                Response::json(&json(&info))
            }
            Ok(_) => Response::see_other(url("")),
            Err(FileStoreError::Limit) => fail(StatusCode::Conflict, "File storage quota reached."),
            Err(_) => unavailable(),
        }
    }
    fn files(&self, request: &Request) -> Response {
        let owner = match self.owner(request) {
            Ok(owner) => owner,
            Err(status) => return fail(status, "Owner authorization unavailable or denied."),
        };
        match self.files.list(&owner) {
            Ok(files) => Response::json(
                &JsonValue::object(vec![(
                    "files".into(),
                    JsonValue::array(files.iter().map(json).collect()).unwrap(),
                )])
                .unwrap(),
            ),
            Err(_) => unavailable(),
        }
    }
    fn download(&self, request: &Request) -> Response {
        let owner = match self.owner(request) {
            Ok(owner) => owner,
            Err(status) => return fail(status, "Owner authorization unavailable or denied."),
        };
        let Ok(id) = request.param_as::<FileId>("id") else {
            return missing();
        };
        match self.files.read(&owner, id) {
            Ok(Some(file)) => file.into_response(),
            Ok(None) => missing(),
            Err(_) => unavailable(),
        }
    }
    fn remove(&self, request: &Request) -> Response {
        let owner = match self.owner(request) {
            Ok(owner) => owner,
            Err(status) => return fail(status, "Owner authorization unavailable or denied."),
        };
        if request.form_data().is_some_and(|form| !form.has_only(&[])) {
            return fail(StatusCode::BadRequest, "Unexpected business fields.");
        }
        let Ok(id) = request.param_as::<FileId>("id") else {
            return missing();
        };
        match self.files.remove(&owner, id) {
            Ok(true) if request.method() == Method::Delete => Response::json(&true.into()),
            Ok(true) => Response::see_other(url("")),
            Ok(false) => missing(),
            Err(_) => unavailable(),
        }
    }
}
pub fn register(app: &mut App, files: FileStore) -> Result<(), Box<dyn std::error::Error>> {
    let sessions = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("ruvane_files_demo")?,
    )?;
    let demo = Arc::new(Demo {
        files,
        sessions: sessions.clone(),
    });
    let quota = RateLimiter::new(RateLimitPolicy::new(128, 60)?, RateLimitLimits::default());
    let public = RateLimiter::new(RateLimitPolicy::new(16, 60)?, RateLimitLimits::default());
    app.asset(
        "/file-store.css",
        include_bytes!("multipart_demo.css"),
        MediaType::Css,
    )?;
    let mut routes = FileRouteRegistry::new(app);
    let page = demo.clone();
    routes.endpoint(
        "private-files/route.rs",
        BASE,
        FileEndpoint::new().get(move |request| page.page(request)),
    )?;
    let open = sessions.clone();
    routes.endpoint(
        "private-files/session/route.rs",
        &format!("{BASE}/session"),
        FileEndpoint::new()
            .post_form(move |request| {
                if !request.form_data().unwrap().has_only(&["account"]) {
                    return fail(StatusCode::BadRequest, "Invalid demo account.");
                }
                let account = match request.form_data().unwrap().get("account") {
                    Some(account @ ("alpha" | "beta")) => account,
                    _ => return fail(StatusCode::BadRequest, "Invalid demo account."),
                };
                match open.lookup(request) {
                    Ok(None) => {}
                    Ok(Some(_)) => {
                        return fail(
                            StatusCode::Conflict,
                            "Close the current demo session first.",
                        );
                    }
                    Err(_) => return unavailable(),
                }
                let mut data = SessionData::default();
                data.insert("file_owner", account).unwrap();
                match open.create(data) {
                    Ok(cookie) => Response::see_other(url("")).with_cookie(cookie).unwrap(),
                    Err(_) => unavailable(),
                }
            })
            .guard(public.guard()),
    )?;
    let csrf = sessions.clone();
    routes.endpoint(
        "private-files/csrf/route.rs",
        &format!("{BASE}/csrf"),
        FileEndpoint::new().get(move |request| csrf.csrf_response(request)),
    )?;
    let close = sessions.clone();
    routes.endpoint(
        "private-files/logout/route.rs",
        &format!("{BASE}/logout"),
        FileEndpoint::new()
            .post_form(move |request| {
                if !request.form_data().unwrap().has_only(&[]) {
                    return fail(StatusCode::BadRequest, "Unexpected business fields.");
                }
                match close.lookup(request) {
                    Ok(Some(session)) => match close.revoke(&session) {
                        Ok(_) => Response::see_other(url(""))
                            .with_cookie(close.removal_cookie())
                            .unwrap(),
                        Err(_) => unavailable(),
                    },
                    _ => fail(StatusCode::Forbidden, "Open a demo session."),
                }
            })
            .guard(sessions.csrf_guard())
            .guard(quota.session_guard(sessions.clone())),
    )?;
    let list = demo.clone();
    let save = demo.clone();
    routes.endpoint(
        "private-files/files/route.rs",
        &format!("{BASE}/files"),
        FileEndpoint::new()
            .get(move |request| list.files(request))
            .post_multipart(move |request| save.save(request))
            .guard(sessions.csrf_guard())
            .guard(quota.session_guard(sessions.clone())),
    )?;
    let download = demo.clone();
    let delete = demo.clone();
    routes.endpoint(
        "private-files/files/[id]/route.rs",
        &format!("{BASE}/files/{{id}}"),
        FileEndpoint::new()
            .get(move |request| download.download(request))
            .delete(move |request| delete.remove(request))
            .guard(sessions.csrf_guard())
            .guard(quota.session_guard(sessions.clone())),
    )?;
    routes.endpoint(
        "private-files/files/[id]/remove/route.rs",
        &format!("{BASE}/files/{{id}}/remove"),
        FileEndpoint::new()
            .post_form(move |request| demo.remove(request))
            .guard(sessions.csrf_guard())
            .guard(quota.session_guard(sessions)),
    )?;
    routes.finish()?;
    Ok(())
}
pub fn app(files: FileStore) -> Result<App, Box<dyn std::error::Error>> {
    let mut app = App::new();
    register(&mut app, files)?;
    Ok(app)
}
