//! Native session, persistence and HTTP handlers for the local pilot.
mod store;
use crate::{
    model::{self, PAGE_SIZE, TicketInput},
    pages::{self, url},
};
use ruvane::*;
use std::sync::Arc;
pub use store::{Snapshot, Store, StoreError};

#[derive(Clone)]
pub struct Backend {
    pub store: Arc<Store>,
    sessions: SessionStore,
}
impl Backend {
    pub fn open(path: &std::path::Path, recover: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            store: Arc::new(Store::open(path, recover)?),
            sessions: SessionStore::new(
                SessionLimits::default(),
                SessionCookie::local_http("ruvane_desk")?,
            )?,
        })
    }
    pub fn application(&self) -> Result<App, Box<dyn std::error::Error>> {
        let service = self.clone();
        let mut app = pages::with_loader(move |context| service.load(context));
        let quota = RateLimiter::new(RateLimitPolicy::new(128, 60)?, RateLimitLimits::default());
        for branch in ["/tickets", "/api/tickets"] {
            app.guard_branch(branch, self.sessions.csrf_guard())?;
            app.guard_branch(branch, quota.session_guard(self.sessions.clone()))?;
        }
        let public = RateLimiter::new(RateLimitPolicy::new(16, 60)?, RateLimitLimits::default());
        let mut routes = FileRouteRegistry::new(&mut app);
        let b = self.clone();
        routes.endpoint(
            "session/page.rs",
            "/",
            FileEndpoint::new().get(move |r| b.landing(r)),
        )?;
        let b = self.clone();
        routes.endpoint(
            "session/route.rs",
            "/session",
            FileEndpoint::new()
                .post_form(move |r| b.open_session(r))
                .guard(public.guard()),
        )?;
        let sessions = self.sessions.clone();
        routes.endpoint(
            "csrf/route.rs",
            "/csrf",
            FileEndpoint::new().get(move |r| sessions.csrf_response(r)),
        )?;
        let b = self.clone();
        routes.endpoint(
            "session/close/route.rs",
            "/session/close",
            FileEndpoint::new()
                .post_form(move |r| b.close_session(r))
                .guard(self.sessions.csrf_guard()),
        )?;
        let b = self.clone();
        routes.endpoint(
            "tickets/route.rs",
            "/tickets",
            FileEndpoint::new().post_form(move |r| b.native_save(r, None)),
        )?;
        let b = self.clone();
        routes.endpoint(
            "tickets/id/route.rs",
            "/tickets/{id}",
            FileEndpoint::new().post_form(move |r| match r.param("id").and_then(model::id) {
                Some(id) => b.native_save(r, Some(id)),
                None => error(StatusCode::NotFound),
            }),
        )?;
        let b = self.clone();
        routes.endpoint(
            "api/tickets/route.rs",
            "/api/tickets",
            FileEndpoint::new().post_json(move |r| b.json_create(r)),
        )?;
        let read = self.clone();
        let update = self.clone();
        routes.endpoint(
            "api/tickets/id/route.rs",
            "/api/tickets/{id}",
            FileEndpoint::new()
                .get(move |r| read.json_read(r))
                .patch_json(move |r| update.json_update(r)),
        )?;
        routes.finish()?;
        Ok(app)
    }
    fn token(&self, r: &Request) -> Result<CsrfToken, PageError> {
        self.sessions
            .lookup(r)
            .ok()
            .flatten()
            .and_then(|s| s.csrf_token().ok().flatten())
            .ok_or(PageError::InternalServerError)
    }
    fn load(&self, context: &PageContext<'_>) -> Result<ClientPageData, PageError> {
        let r = context.request().ok_or(PageError::InternalServerError)?;
        let page = page_query(r)?;
        if r.path() == "/tickets" {
            let tickets = self
                .store
                .list()
                .map_err(|_| PageError::InternalServerError)?;
            let index = page - 1;
            if index > tickets.len().saturating_sub(1) / PAGE_SIZE {
                return Err(PageError::NotFound);
            }
            let rows = tickets
                .iter()
                .skip(index * PAGE_SIZE)
                .take(PAGE_SIZE)
                .collect::<Vec<_>>();
            let mut seed = ActionData::default();
            for (key, value) in [
                ("page", index),
                ("total", tickets.len()),
                ("count", rows.len()),
            ] {
                seed.insert(key, ActionValue::Number(value as u64)).unwrap();
            }
            for (i, row) in rows.iter().enumerate() {
                seed.insert(&format!("id{i}"), ActionValue::Number(row.id as u64))
                    .unwrap();
                seed.insert(
                    &format!("title{i}"),
                    ActionValue::Text(row.input.title.clone()),
                )
                .unwrap();
                seed.insert(
                    &format!("state{i}"),
                    ActionValue::Text(row.input.state.wire().into()),
                )
                .unwrap();
            }
            let native = r.query_params().get("native") == Some("yes");
            let suffix = if native { "?native=yes" } else { "" };
            let mut fallback = element(Tag::Section)
                .child(element(Tag::H2).child("Native ticket list"))
                .child(link(&url(&format!("/tickets/new{suffix}")), "New ticket"));
            for row in rows {
                fallback = fallback.child(element(Tag::P).child(link(
                    &url(&format!("/tickets/{}{suffix}", row.id)),
                    format!(
                        "#{} {} · {}",
                        row.id,
                        row.input.title,
                        row.input.state.label()
                    ),
                )));
            }
            if index > 0 {
                fallback = fallback.child(link(
                    &url(&format!("/tickets?page={}&native=yes", page - 1)),
                    "Previous page",
                ));
            }
            if page * PAGE_SIZE < tickets.len() {
                fallback = fallback.child(link(
                    &url(&format!("/tickets?page={}&native=yes", page + 1)),
                    "Next page",
                ));
            }
            return Ok(ClientPageData::new("Tickets / Ruvane Desk", seed)
                .fallback(fallback)
                .no_store());
        }
        if page != 1 || r.query_params().get("page").is_some() {
            return Err(PageError::BadRequest);
        }
        let (seed, input, version, id) = if r.path() == "/tickets/new" {
            (model::empty_seed(), None, String::new(), None)
        } else {
            let id = context
                .param("id")
                .and_then(model::id)
                .ok_or(PageError::NotFound)?;
            let row = self.store.get(id).map_err(|e| {
                if e == StoreError::Missing {
                    PageError::NotFound
                } else {
                    PageError::InternalServerError
                }
            })?;
            (
                row.ticket.seed(row.tag.as_str()),
                Some(row.ticket.input),
                row.tag.as_str().into(),
                Some(id),
            )
        };
        let fallback = native_form(
            id,
            input.as_ref().map(|i| i.title.as_str()).unwrap_or(""),
            input.as_ref().map(|i| i.description.as_str()).unwrap_or(""),
            input.as_ref().map(|i| i.state.wire()).unwrap_or("open"),
            &version,
            self.token(r)?,
        )
        .map_err(|_| PageError::InternalServerError)?;
        Ok(ClientPageData::new(
            if id.is_some() {
                "Edit ticket / Ruvane Desk"
            } else {
                "New ticket / Ruvane Desk"
            },
            seed,
        )
        .fallback(fallback)
        .no_store())
    }
    fn landing(&self, r: &Request) -> Response {
        let mut body=element(Tag::Section).child(element(Tag::H2).child("Local session"))
            .child(element(Tag::P).child("This pilot opens a local session on this computer. It does not verify your identity. Tickets persist; sessions end when the server restarts."));
        match self.sessions.lookup(r) {
            Ok(Some(session)) => {
                let token = match session.csrf_token() {
                    Ok(Some(t)) => t,
                    _ => return error(StatusCode::ServiceUnavailable),
                };
                body = body.child(link(&url("/tickets"), "Open tickets")).child(
                    PostForm::new(url("/session/close"))
                        .unwrap()
                        .csrf(token)
                        .unwrap()
                        .submit_label("Close local session")
                        .unwrap(),
                );
            }
            Ok(None) => {
                body = body.child(
                    PostForm::new(url("/session"))
                        .unwrap()
                        .submit_label("Open local session")
                        .unwrap(),
                )
            }
            Err(_) => return error(StatusCode::ServiceUnavailable),
        }
        document("Local session / Ruvane Desk", body.render(), StatusCode::Ok)
    }
    fn open_session(&self, r: &Request) -> Response {
        if !r.form_data().unwrap().has_only(&[]) {
            return error(StatusCode::BadRequest);
        }
        match self.sessions.lookup(r) {
            Ok(Some(_)) => return Response::see_other(url("/tickets")),
            Ok(None) => {}
            Err(_) => return error(StatusCode::ServiceUnavailable),
        }
        match self.sessions.create(SessionData::default()) {
            Ok(cookie) => Response::see_other(url("/tickets"))
                .with_cookie(cookie)
                .unwrap(),
            Err(_) => error(StatusCode::ServiceUnavailable),
        }
    }
    fn close_session(&self, r: &Request) -> Response {
        if !r.form_data().unwrap().has_only(&[]) {
            return error(StatusCode::BadRequest);
        }
        match self.sessions.lookup(r) {
            Ok(Some(session)) => match self.sessions.revoke(&session) {
                Ok(_) => Response::see_other(url("/"))
                    .with_cookie(self.sessions.removal_cookie())
                    .unwrap(),
                Err(_) => error(StatusCode::ServiceUnavailable),
            },
            _ => error(StatusCode::Forbidden),
        }
    }
    fn json_create(&self, r: &Request) -> Response {
        if r.query_params().iter().next().is_some() {
            return error(StatusCode::BadRequest);
        }
        let input = match r.decode_json::<TicketInput>() {
            Ok(i) => i,
            Err(errors) => return Response::validation_errors(&errors),
        };
        match self.store.create(input) {
            Ok(row) => Response::json(&row.ticket.json()).with_etag(row.tag),
            Err(e) => error(status(e)),
        }
    }
    fn json_read(&self, r: &Request) -> Response {
        if r.query_params().iter().next().is_some() {
            return error(StatusCode::BadRequest);
        }
        let Some(id) = r.param("id").and_then(model::id) else {
            return error(StatusCode::NotFound);
        };
        match self.store.get(id) {
            Ok(row) => Response::json(&row.ticket.json()).with_etag(row.tag),
            Err(e) => error(status(e)),
        }
    }
    fn json_update(&self, r: &Request) -> Response {
        if r.query_params().iter().next().is_some() {
            return error(StatusCode::BadRequest);
        }
        let Some(id) = r.param("id").and_then(model::id) else {
            return error(StatusCode::NotFound);
        };
        let input = match r.decode_json::<TicketInput>() {
            Ok(i) => i,
            Err(errors) => return Response::validation_errors(&errors),
        };
        match self.store.update(id, input, r.if_match()) {
            Ok(row) => Response::json(&row.ticket.json()).with_etag(row.tag),
            Err(e) => error(status(e)),
        }
    }
    fn native_save(&self, r: &Request, id: Option<u32>) -> Response {
        if page_query(r).is_err() || r.query_params().get("page").is_some() {
            return error(StatusCode::BadRequest);
        }
        let fields = r.form_data().unwrap();
        if !fields.has_only(if id.is_some() {
            &["title", "description", "state", "version", "native"]
        } else {
            &["title", "description", "state", "native"]
        }) {
            return error(StatusCode::BadRequest);
        }
        if fields.get("native").is_some_and(|value| value != "yes") {
            return error(StatusCode::BadRequest);
        }
        let (title, description, state) = (
            fields.get("title").unwrap_or(""),
            fields.get("description").unwrap_or(""),
            fields.get("state").unwrap_or(""),
        );
        let version = fields.get("version").unwrap_or("");
        let result = match TicketInput::new(title, description, state) {
            Ok(input) => {
                if let Some(id) = id {
                    let condition = EntityTag::parse(version)
                        .ok()
                        .filter(|t| !t.is_weak())
                        .and_then(|t| IfMatch::parse(t.as_str()).ok());
                    self.store
                        .update(id, input, condition.as_ref())
                        .map_err(status)
                } else {
                    self.store.create(input).map_err(status)
                }
            }
            Err(_) => Err(StatusCode::BadRequest),
        };
        match result {
            Ok(row) => Response::see_other(url(&format!(
                "/tickets/{}{}",
                row.ticket.id,
                if r.query_params().get("native") == Some("yes")
                    || fields.get("native") == Some("yes")
                {
                    "?native=yes"
                } else {
                    ""
                }
            ))),
            Err(code) => {
                let token = match self.token(r) {
                    Ok(t) => t,
                    Err(_) => return error(StatusCode::ServiceUnavailable),
                };
                let message = if code == StatusCode::PreconditionFailed {
                    "Conflict. Your draft is retained below. Another tab saved a newer version. Open the current ticket before trying again."
                } else if code == StatusCode::BadRequest {
                    "Invalid ticket. Title: 1–120 bytes, nonblank. Description: up to 1024 bytes. State: open, in_progress or closed."
                } else {
                    "Save failed. Inspect tickets before another write. Do not resubmit an uncertain creation."
                };
                let mut content = element(Tag::Section)
                    .child(element(Tag::H2).child("Ticket not saved"))
                    .child(element(Tag::P).child(message));
                if matches!(
                    code,
                    StatusCode::BadRequest
                        | StatusCode::PreconditionFailed
                        | StatusCode::PreconditionRequired
                ) {
                    match native_form(id, title, description, state, version, token) {
                        Ok(form) => content = content.child(form),
                        Err(_) => return error(StatusCode::BadRequest),
                    }
                }
                content = content.child(link(
                    &url(&id.map_or_else(
                        || "/tickets?native=yes".into(),
                        |id| format!("/tickets/{id}?native=yes"),
                    )),
                    "Open current saved data (replaces draft)",
                ));
                document("Ticket not saved / Ruvane Desk", content.render(), code)
            }
        }
    }
}
fn page_query(r: &Request) -> Result<usize, PageError> {
    if r.query_params().iter().any(|(key, value)| {
        !matches!(key, "page" | "native") || (key == "native" && value != "yes")
    }) {
        return Err(PageError::BadRequest);
    }
    let raw = r.query_params().get("page").unwrap_or("1");
    let page = raw.parse::<usize>().map_err(|_| PageError::BadRequest)?;
    if page == 0 || page > model::MAX_TICKETS.div_ceil(PAGE_SIZE) || page.to_string() != raw {
        return Err(PageError::BadRequest);
    }
    Ok(page)
}
fn native_form(
    id: Option<u32>,
    title: &str,
    description: &str,
    state: &str,
    version: &str,
    token: CsrfToken,
) -> Result<Html, FormError> {
    let path = id.map_or_else(|| "/tickets".into(), |id| format!("/tickets/{id}"));
    let mut form = PostForm::new(url(&path))?
        .csrf(token)?
        .hidden("native", "yes")?
        .text("title", "Title (up to 120 bytes)", title)?
        .textarea("description", "Description (up to 1024 bytes)", description)?
        .text("state", "State: open, in_progress or closed", state)?;
    if id.is_some() {
        form = form.hidden("version", version)?;
    }
    Ok(element(Tag::Section)
        .child(element(Tag::H2).child("Native HTML form"))
        .child(form.submit_label(if id.is_some() {
            "Save ticket"
        } else {
            "Create ticket"
        })?)
        .render())
}
fn status(e: StoreError) -> StatusCode {
    match e {
        StoreError::Missing => StatusCode::NotFound,
        StoreError::Conflict => StatusCode::PreconditionFailed,
        StoreError::Precondition => StatusCode::PreconditionRequired,
        StoreError::InvalidInput => StatusCode::BadRequest,
        _ => StatusCode::ServiceUnavailable,
    }
}
fn error(code: StatusCode) -> Response {
    Response::json(
        &JsonValue::object(vec![(
            "error".into(),
            JsonValue::string(code.reason()).unwrap(),
        )])
        .unwrap(),
    )
    .with_status(code)
}
fn document(title: &str, body: Html, code: StatusCode) -> Response {
    Response::html(
        Document::new(title, pages::shell(body))
            .stylesheet(url("/assets/site.css"))
            .render(),
    )
    .with_status(code)
}
