//! Native profiles, authorization, persistence and HTTP handlers.
mod files;
mod store;
use crate::{
    model::{
        self, Actor, MembersInput, PAGE_SIZE, PROFILE_KEYS, PROFILE_NAMES, Role, Space,
        TicketInput, WorkspaceInput,
    },
    pages::{self, url},
};
use ruvane::*;
use std::sync::Arc;
pub use store::{Access, Snapshot, SpaceSnapshot, Store, StoreError};
#[derive(Clone)]
pub struct Backend {
    pub store: Arc<Store>,
    sessions: SessionStore,
    files: FileStore,
}
impl Backend {
    pub fn open(path: &std::path::Path, recover: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let store = Arc::new(Store::open(path, recover)?);
        let limits = FileStoreLimits::new(64, 2 * 1024 * 1024, 3, 96 * 1024)?;
        let files = if recover {
            FileStore::recover(path.join("attachments"), "desk-attachments", limits)?
        } else {
            FileStore::open(path.join("attachments"), "desk-attachments", limits)?
        };
        Ok(Self {
            store,
            files,
            sessions: SessionStore::new(
                SessionLimits::default(),
                SessionCookie::local_http("ruvane_desk")?,
            )?,
        })
    }
    pub fn application(&self) -> Result<App, Box<dyn std::error::Error>> {
        let b = self.clone();
        let mut app = pages::with_loader(move |c| b.load(c));
        let quota = RateLimiter::new(RateLimitPolicy::new(128, 60)?, RateLimitLimits::default());
        for branch in ["/w", "/api/w", "/workspaces", "/api/workspaces"] {
            app.guard_branch(branch, self.sessions.csrf_guard())?;
            app.guard_branch(branch, quota.session_guard(self.sessions.clone()))?;
        }
        let public = RateLimiter::new(RateLimitPolicy::new(16, 60)?, RateLimitLimits::default());
        let mut routes = FileRouteRegistry::new(&mut app);
        let b = self.clone();
        endpoint(
            &mut routes,
            "/",
            FileEndpoint::new().get(move |r| b.landing(r)),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/session",
            FileEndpoint::new()
                .post_form(move |r| b.open_session(r))
                .guard(public.guard()),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/session/close",
            FileEndpoint::new()
                .post_form(move |r| b.close_session(r))
                .guard(self.sessions.csrf_guard()),
        )?;
        let s = self.sessions.clone();
        endpoint(
            &mut routes,
            "/csrf",
            FileEndpoint::new().get(move |r| s.csrf_response(r)),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/workspaces",
            FileEndpoint::new().post_form(move |r| b.native_workspace(r)),
        )?;
        let read = self.clone();
        let write = self.clone();
        endpoint(
            &mut routes,
            "/api/workspaces",
            FileEndpoint::new()
                .get(move |r| read.json_workspaces(r))
                .post_json(move |r| write.json_workspace(r)),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/w/{workspace}/members",
            FileEndpoint::new().post_form(move |r| b.native_members(r)),
        )?;
        let read = self.clone();
        let write = self.clone();
        endpoint(
            &mut routes,
            "/api/w/{workspace}/members",
            FileEndpoint::new()
                .get(move |r| read.json_members(r, false))
                .patch_json(move |r| write.json_members(r, true)),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/w/{workspace}/tickets",
            FileEndpoint::new().post_form(move |r| b.native_save(r, None)),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/w/{workspace}/tickets/{id}",
            FileEndpoint::new()
                .post_form(move |r| b.native_save(r, r.param("id").and_then(model::id))),
        )?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/api/w/{workspace}/tickets",
            FileEndpoint::new().post_json(move |r| b.json_ticket(r, 0)),
        )?;
        let read = self.clone();
        let write = self.clone();
        endpoint(
            &mut routes,
            "/api/w/{workspace}/tickets/{id}",
            FileEndpoint::new()
                .get(move |r| read.json_ticket(r, 1))
                .patch_json(move |r| write.json_ticket(r, 2)),
        )?;
        self.register_files(&mut routes)?;
        let b = self.clone();
        endpoint(
            &mut routes,
            "/tickets",
            FileEndpoint::new()
                .get(move |r| {
                    let suffix = if r.query_params().get("native") == Some("yes") {
                        "?native=yes"
                    } else {
                        ""
                    };
                    match b
                        .actor(r)
                        .and_then(|a| b.store.space(a, 1, Access::Read).map_err(status))
                    {
                        Ok(_) => Response::see_other(url(&format!("/w/1/tickets{suffix}"))),
                        Err(s) => error(s),
                    }
                })
                .guard(self.sessions.guard()),
        )?;
        routes.finish()?;
        Ok(app)
    }
    fn actor(&self, r: &Request) -> Result<Actor, StatusCode> {
        self.sessions
            .lookup(r)
            .map_err(|_| StatusCode::ServiceUnavailable)?
            .and_then(|s| s.data().ok().flatten())
            .and_then(|d| d.get("profile").and_then(Actor::parse))
            .ok_or(StatusCode::Forbidden)
    }
    fn scope(&self, r: &Request) -> Result<(Actor, u32), StatusCode> {
        Ok((
            self.actor(r)?,
            r.param("workspace")
                .and_then(model::workspace_id)
                .ok_or(StatusCode::NotFound)?,
        ))
    }
    fn ticket_scope(&self, r: &Request) -> Result<(Actor, u32, u32), StatusCode> {
        let (a, w) = self.scope(r)?;
        Ok((
            a,
            w,
            r.param("id")
                .and_then(model::id)
                .ok_or(StatusCode::NotFound)?,
        ))
    }
    fn token(&self, r: &Request) -> Result<CsrfToken, StatusCode> {
        self.sessions
            .lookup(r)
            .ok()
            .flatten()
            .and_then(|s| s.csrf_token().ok().flatten())
            .ok_or(StatusCode::ServiceUnavailable)
    }
    fn load(&self, c: &PageContext<'_>) -> Result<ClientPageData, PageError> {
        let r = c.request().ok_or(PageError::InternalServerError)?;
        let actor = self.actor(r).map_err(page_error)?;
        if r.path() == "/workspaces" {
            query(r, &["native"])?;
            let spaces = self
                .store
                .spaces(actor)
                .map_err(|e| page_error(status(e)))?;
            let seed = workspace_seed(actor, &spaces);
            let token = self.token(r).map_err(page_error)?;
            let mut body = element(Tag::Section).child(element(Tag::H2).child("Native workspaces"));
            for space in &spaces {
                body = body.child(element(Tag::P).child(link(
                    &url(&format!("/w/{}/tickets?native=yes", space.id)),
                    format!(
                        "#{} {} · {}",
                        space.id,
                        space.name,
                        space.role(actor).unwrap().label()
                    ),
                )));
            }
            if spaces.iter().any(|s| s.role(actor) == Some(Role::Owner)) {
                body = body
                    .child(workspace_form("", token).map_err(|_| PageError::InternalServerError)?);
            }
            return Ok(ClientPageData::new("Workspaces / Ruvane Desk", seed)
                .fallback(body)
                .no_store());
        }
        let workspace = c
            .param("workspace")
            .and_then(model::workspace_id)
            .ok_or(PageError::BadRequest)?;
        if r.path().ends_with("/members") {
            query(r, &["native"])?;
            let row = self
                .store
                .members(actor, workspace)
                .map_err(|e| page_error(status(e)))?;
            let token = self.token(r).map_err(page_error)?;
            return Ok(
                ClientPageData::new("Members / Ruvane Desk", members_seed(&row, actor))
                    .fallback(
                        members_form(&row.space, row.tag.as_str(), token)
                            .map_err(|_| PageError::InternalServerError)?,
                    )
                    .no_store(),
            );
        }
        let id = c.param("id").and_then(model::id);
        if r.path().ends_with("/files") {
            return self.files_page(r, actor, workspace, id.ok_or(PageError::BadRequest)?);
        }
        query(r, &["native", "page"])?;
        let access = if r.path().ends_with("/new-ticket") {
            Access::Edit
        } else {
            Access::Read
        };
        let space = self
            .store
            .space(actor, workspace, access)
            .map_err(|e| page_error(status(e)))?
            .space;
        let role = space.role(actor).unwrap();
        if r.path() == model::ticket_path(workspace, None) {
            let raw = r.query_params().get("page").unwrap_or("1");
            let page = raw
                .parse::<usize>()
                .ok()
                .filter(|p| {
                    *p > 0 && *p <= model::MAX_TICKETS.div_ceil(PAGE_SIZE) && p.to_string() == raw
                })
                .ok_or(PageError::BadRequest)?;
            let tickets = self
                .store
                .list(actor, workspace)
                .map_err(|e| page_error(status(e)))?;
            let index = page - 1;
            if index > tickets.len().saturating_sub(1) / PAGE_SIZE {
                return Err(PageError::NotFound);
            }
            let mut seed = ActionData::default();
            for (key, n) in [
                ("page", index),
                ("total", tickets.len()),
                (
                    "count",
                    tickets
                        .len()
                        .saturating_sub(index * PAGE_SIZE)
                        .min(PAGE_SIZE),
                ),
            ] {
                number(&mut seed, key, n as u64);
            }
            let mut fallback =
                element(Tag::Section).child(element(Tag::H2).child("Native tickets"));
            if role.can_edit() {
                fallback = fallback.child(link(
                    &url(&format!("/w/{workspace}/new-ticket?native=yes")),
                    "New ticket",
                ));
            }
            for (i, ticket) in tickets
                .iter()
                .skip(index * PAGE_SIZE)
                .take(PAGE_SIZE)
                .enumerate()
            {
                number(&mut seed, &format!("id{i}"), ticket.id as u64);
                text_field(&mut seed, &format!("title{i}"), &ticket.input.title);
                text_field(&mut seed, &format!("state{i}"), ticket.input.state.wire());
                fallback = fallback.child(element(Tag::P).child(link(
                    &url(&format!("/w/{workspace}/tickets/{}?native=yes", ticket.id)),
                    format!(
                        "#{} {} · {}",
                        ticket.id,
                        ticket.input.title,
                        ticket.input.state.label()
                    ),
                )));
            }
            if index > 0 {
                fallback = fallback.child(link(
                    &url(&format!(
                        "/w/{workspace}/tickets?page={}&native=yes",
                        page - 1
                    )),
                    "Previous page",
                ));
            }
            if page * PAGE_SIZE < tickets.len() {
                fallback = fallback.child(link(
                    &url(&format!(
                        "/w/{workspace}/tickets?page={}&native=yes",
                        page + 1
                    )),
                    "Next page",
                ));
            }
            return Ok(ClientPageData::new(
                "Tickets / Ruvane Desk",
                model::scope_seed(seed, &space, role),
            )
            .fallback(fallback)
            .no_store());
        }
        if r.query_params().get("page").is_some() {
            return Err(PageError::BadRequest);
        }
        let (seed, fallback) = if let Some(id) = id {
            let row = self
                .store
                .get(actor, workspace, id)
                .map_err(|e| page_error(status(e)))?;
            let mut body = element(Tag::Section).child(link(
                &url(&format!("/w/{workspace}/tickets/{id}/files?native=yes")),
                "Attachments",
            ));
            if role.can_edit() {
                body = body.child(
                    native_form(
                        workspace,
                        Some(id),
                        &row.ticket.input.title,
                        &row.ticket.input.description,
                        row.ticket.input.state.wire(),
                        row.tag.as_str(),
                        self.token(r).map_err(page_error)?,
                    )
                    .map_err(|_| PageError::InternalServerError)?,
                );
            }
            (row.ticket.seed(row.tag.as_str()), body.render())
        } else {
            (
                model::empty_seed(),
                native_form(
                    workspace,
                    None,
                    "",
                    "",
                    "open",
                    "",
                    self.token(r).map_err(page_error)?,
                )
                .map_err(|_| PageError::InternalServerError)?,
            )
        };
        Ok(ClientPageData::new(
            "Ticket / Ruvane Desk",
            model::scope_seed(seed, &space, role),
        )
        .fallback(fallback)
        .no_store())
    }
    fn landing(&self, r: &Request) -> Response {
        let mut body=element(Tag::Section).child(element(Tag::H2).child("Local test profiles"))
            .child(element(Tag::P).child("Profiles are selectable by anyone using this local pilot. This does not verify your identity. Tickets, workspaces and memberships persist; sessions end on restart."));
        match self.sessions.lookup(r) {
            Ok(Some(_)) => {
                let actor = match self.actor(r) {
                    Ok(a) => a,
                    Err(s) => return error(s),
                };
                let token = match self.token(r) {
                    Ok(t) => t,
                    Err(s) => return error(s),
                };
                body = body
                    .child(element(Tag::P).child(format!("Current test profile: {}", actor.name())))
                    .child(link(&url("/workspaces"), "Open workspaces"))
                    .child(
                        PostForm::new(url("/session/close"))
                            .unwrap()
                            .csrf(token)
                            .unwrap()
                            .submit_label("Close test session")
                            .unwrap(),
                    );
            }
            Ok(None) => {
                for (key, name) in PROFILE_KEYS.iter().zip(PROFILE_NAMES) {
                    body = body.child(
                        PostForm::new(url("/session"))
                            .unwrap()
                            .hidden("profile", *key)
                            .unwrap()
                            .submit_label(format!("Use {name} test profile"))
                            .unwrap(),
                    );
                }
            }
            Err(_) => return error(StatusCode::ServiceUnavailable),
        }
        document(
            "Local test profiles / Ruvane Desk",
            body.render(),
            StatusCode::Ok,
        )
    }
    fn open_session(&self, r: &Request) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let fields = r.form_data().unwrap();
        if !fields.has_only(&["profile"]) {
            return error(StatusCode::BadRequest);
        }
        let Some(actor) = fields.get("profile").and_then(Actor::parse) else {
            return error(StatusCode::BadRequest);
        };
        match self.sessions.lookup(r) {
            Ok(Some(_)) => return error(StatusCode::Conflict),
            Ok(None) => {}
            Err(_) => return error(StatusCode::ServiceUnavailable),
        }
        let mut data = SessionData::default();
        data.insert("profile", actor.key()).unwrap();
        match self.sessions.create(data) {
            Ok(cookie) => Response::see_other(url("/workspaces"))
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
            Ok(Some(s)) => match self.sessions.revoke(&s) {
                Ok(_) => Response::see_other(url("/"))
                    .with_cookie(self.sessions.removal_cookie())
                    .unwrap(),
                Err(_) => error(StatusCode::ServiceUnavailable),
            },
            _ => error(StatusCode::Forbidden),
        }
    }
    fn json_workspaces(&self, r: &Request) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        match self.actor(r).and_then(|a| {
            self.store
                .spaces(a)
                .map(|s| workspace_value(a, &s))
                .map_err(status)
        }) {
            Ok(v) => Response::json(&v),
            Err(s) => error(s),
        }
    }
    fn json_workspace(&self, r: &Request) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let actor = match self.actor(r) {
            Ok(a) => a,
            Err(s) => return error(s),
        };
        let input = match r.decode_json::<WorkspaceInput>() {
            Ok(v) => v,
            Err(e) => return Response::validation_errors(&e),
        };
        match self.store.create_space(actor, input) {
            Ok(_) => self.json_workspaces(r),
            Err(e) => error(status(e)),
        }
    }
    fn native_workspace(&self, r: &Request) -> Response {
        if query(r, &["native"]).is_err() {
            return error(StatusCode::BadRequest);
        }
        let actor = match self.actor(r) {
            Ok(a) => a,
            Err(s) => return error(s),
        };
        let f = r.form_data().unwrap();
        if !f.has_only(&["name"]) {
            return error(StatusCode::BadRequest);
        }
        match WorkspaceInput::new(f.get("name").unwrap_or(""))
            .map_err(|_| StoreError::InvalidInput)
            .and_then(|input| self.store.create_space(actor, input))
        {
            Ok(space) => Response::see_other(url(&format!("/w/{}/tickets?native=yes", space.id))),
            Err(e) => {
                let token = match self.token(r) {
                    Ok(t) => t,
                    Err(s) => return error(s),
                };
                match workspace_form(f.get("name").unwrap_or(""),token){Ok(form)=>document("Workspace not created",element(Tag::Section).child(element(Tag::P).child("Workspace not created. Use a nonblank name up to 48 bytes that differs from your visible workspaces. Inspect workspaces before retrying an uncertain creation.")).child(form).render(),status(e)),Err(_)=>error(StatusCode::BadRequest)}
            }
        }
    }
    fn json_members(&self, r: &Request, write: bool) -> Response {
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let (a, w) = match self.scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        let result = if write {
            let input = match r.decode_json::<MembersInput>() {
                Ok(i) => i,
                Err(e) => return Response::validation_errors(&e),
            };
            self.store.set_members(a, w, input.0, r.if_match())
        } else {
            self.store.members(a, w)
        };
        match result {
            Ok(row) => Response::json(&members_value(&row.space)).with_etag(row.tag),
            Err(e) => error(status(e)),
        }
    }
    fn native_members(&self, r: &Request) -> Response {
        if query(r, &["native"]).is_err() {
            return error(StatusCode::BadRequest);
        }
        let (a, w) = match self.scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        let f = r.form_data().unwrap();
        if !f.has_only(&["alex", "blair", "casey", "drew", "version"]) {
            return error(StatusCode::BadRequest);
        }
        let current = match self.store.members(a, w) {
            Ok(v) => v,
            Err(e) => return error(status(e)),
        };
        let input = JsonValue::object(
            PROFILE_KEYS
                .iter()
                .map(|key| {
                    (
                        (*key).into(),
                        JsonValue::string(f.get(key).unwrap_or("")).unwrap(),
                    )
                })
                .collect(),
        )
        .unwrap();
        let members = match MembersInput::try_from(&input) {
            Ok(v) => v.0,
            Err(_) => {
                let token = match self.token(r) {
                    Ok(t) => t,
                    Err(s) => return error(s),
                };
                let form = match raw_members_form(
                    w,
                    PROFILE_KEYS.map(|key| f.get(key).unwrap_or("")),
                    f.get("version").unwrap_or(""),
                    token,
                ) {
                    Ok(f) => f,
                    Err(_) => return error(StatusCode::BadRequest),
                };
                return document("Invalid membership draft",element(Tag::Section).child(element(Tag::P).child("Use none, viewer, editor or owner for each profile. Your draft is retained.")).child(form).render(),StatusCode::BadRequest);
            }
        };
        match self
            .store
            .set_members(a, w, members, condition(f.get("version")).as_ref())
        {
            Ok(_) => Response::see_other(url(&format!("/w/{w}/members?native=yes"))),
            Err(e) => {
                let mut draft = current.space;
                draft.members = members;
                let token = match self.token(r) {
                    Ok(t) => t,
                    Err(s) => return error(s),
                };
                let form = match members_form(&draft, f.get("version").unwrap_or(""), token) {
                    Ok(f) => f,
                    Err(_) => return error(StatusCode::BadRequest),
                };
                document("Membership not saved",element(Tag::Section).child(element(Tag::P).child("Membership not saved. Keep at least one owner. A conflict retains this draft; open current members explicitly before replacing it.")).child(form).child(link(&url(&format!("/w/{w}/members?native=yes")),"Open current members (replaces draft)")).render(),status(e))
            }
        }
    }
    fn json_ticket(&self, r: &Request, kind: u8) -> Response {
        let (a, w) = match self.scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        if !r.query_params().is_empty() {
            return error(StatusCode::BadRequest);
        }
        let id = if kind == 0 {
            0
        } else {
            match r.param("id").and_then(model::id) {
                Some(id) => id,
                None => return error(StatusCode::NotFound),
            }
        };
        let result = if kind == 1 {
            self.store.get(a, w, id)
        } else {
            let input = match r.decode_json::<TicketInput>() {
                Ok(i) => i,
                Err(e) => return Response::validation_errors(&e),
            };
            if kind == 0 {
                self.store.create(a, w, input)
            } else {
                self.store.update(a, w, id, input, r.if_match())
            }
        };
        match result {
            Ok(row) => Response::json(&row.ticket.json()).with_etag(row.tag),
            Err(e) => error(status(e)),
        }
    }
    fn native_save(&self, r: &Request, id: Option<u32>) -> Response {
        if query(r, &["native"]).is_err() {
            return error(StatusCode::BadRequest);
        }
        let (a, w) = match self.scope(r) {
            Ok(v) => v,
            Err(s) => return error(s),
        };
        if r.path().ends_with("/tickets") == id.is_some() {
            return error(StatusCode::NotFound);
        }
        if let Err(e) = self.store.space(a, w, Access::Edit) {
            return error(status(e));
        }
        let f = r.form_data().unwrap();
        if !f.has_only(if id.is_some() {
            &["title", "description", "state", "version", "native"]
        } else {
            &["title", "description", "state", "native"]
        }) || f.get("native").is_some_and(|s| s != "yes")
        {
            return error(StatusCode::BadRequest);
        }
        let (title, description, state, version) = (
            f.get("title").unwrap_or(""),
            f.get("description").unwrap_or(""),
            f.get("state").unwrap_or(""),
            f.get("version").unwrap_or(""),
        );
        let result = TicketInput::new(title, description, state)
            .map_err(|_| StoreError::InvalidInput)
            .and_then(|input| {
                if let Some(id) = id {
                    self.store
                        .update(a, w, id, input, condition(Some(version)).as_ref())
                } else {
                    self.store.create(a, w, input)
                }
            });
        match result {
            Ok(row) => {
                Response::see_other(url(&format!("/w/{w}/tickets/{}?native=yes", row.ticket.id)))
            }
            Err(e) => {
                let code = status(e);
                let token = match self.token(r) {
                    Ok(t) => t,
                    Err(s) => return error(s),
                };
                let mut body=element(Tag::Section).child(element(Tag::H2).child("Ticket not saved")).child(element(Tag::P).child(if code==StatusCode::PreconditionFailed{"Conflict. Your draft is retained. Another tab saved a newer version."}else{"Save failed. Check the title, description and state. Inspect saved data before retrying an uncertain creation."}));
                if matches!(
                    e,
                    StoreError::InvalidInput | StoreError::Conflict | StoreError::Precondition
                ) {
                    match native_form(w, id, title, description, state, version, token) {
                        Ok(form) => body = body.child(form),
                        Err(_) => return error(StatusCode::BadRequest),
                    }
                }
                body = body.child(link(
                    &url(&format!("{}?native=yes", model::ticket_path(w, id))),
                    "Open current saved data (replaces draft)",
                ));
                document("Ticket not saved / Ruvane Desk", body.render(), code)
            }
        }
    }
}
fn endpoint(
    routes: &mut FileRouteRegistry<'_>,
    path: &str,
    handler: FileEndpoint,
) -> Result<(), FileRouteError> {
    let source = format!("http{}/route.rs", path.replace(['{', '}'], "_"));
    routes.endpoint(&source, path, handler)
}
fn number(data: &mut ActionData, key: &str, value: u64) {
    data.insert(key, ActionValue::Number(value)).unwrap();
}
fn text_field(data: &mut ActionData, key: &str, value: &str) {
    data.insert(key, ActionValue::Text(value.into())).unwrap();
}
fn workspace_seed(actor: Actor, spaces: &[Space]) -> ActionData {
    let mut d = ActionData::default();
    number(&mut d, "count", spaces.len() as u64);
    text_field(&mut d, "actor", actor.key());
    for (i, s) in spaces.iter().enumerate() {
        number(&mut d, &format!("id{i}"), s.id as u64);
        text_field(&mut d, &format!("name{i}"), &s.name);
        text_field(&mut d, &format!("role{i}"), s.role(actor).unwrap().wire());
    }
    d
}
fn workspace_value(actor: Actor, spaces: &[Space]) -> JsonValue {
    JsonValue::object(vec![
        ("actor".into(), JsonValue::string(actor.key()).unwrap()),
        (
            "spaces".into(),
            JsonValue::array(
                spaces
                    .iter()
                    .map(|s| {
                        JsonValue::object(vec![
                            ("id".into(), (s.id as u64).into()),
                            ("name".into(), JsonValue::string(&s.name).unwrap()),
                            (
                                "role".into(),
                                JsonValue::string(s.role(actor).unwrap().wire()).unwrap(),
                            ),
                        ])
                        .unwrap()
                    })
                    .collect(),
            )
            .unwrap(),
        ),
    ])
    .unwrap()
}
fn members_seed(row: &SpaceSnapshot, actor: Actor) -> ActionData {
    let mut d = ActionData::default();
    text_field(&mut d, "actor", actor.key());
    number(&mut d, "workspace", row.space.id as u64);
    number(&mut d, "revision", row.space.revision);
    text_field(&mut d, "workspace_name", &row.space.name);
    text_field(&mut d, "etag", row.tag.as_str());
    for (key, role) in PROFILE_KEYS.iter().zip(row.space.members) {
        text_field(&mut d, key, role.map_or("none", Role::wire));
    }
    d
}
fn members_value(space: &Space) -> JsonValue {
    JsonValue::object(vec![
        ("workspace".into(), (space.id as u64).into()),
        ("name".into(), JsonValue::string(&space.name).unwrap()),
        ("revision".into(), space.revision.into()),
        (
            "members".into(),
            JsonValue::object(
                PROFILE_KEYS
                    .iter()
                    .zip(space.members)
                    .map(|(key, role)| {
                        (
                            (*key).into(),
                            JsonValue::string(role.map_or("none", Role::wire)).unwrap(),
                        )
                    })
                    .collect(),
            )
            .unwrap(),
        ),
    ])
    .unwrap()
}
fn workspace_form(name: &str, token: CsrfToken) -> Result<Html, FormError> {
    Ok(PostForm::new(url("/workspaces"))?
        .csrf(token)?
        .text("name", "Workspace name (up to 48 bytes)", name)?
        .submit_label("Create workspace")?
        .render())
}
fn members_form(space: &Space, version: &str, token: CsrfToken) -> Result<Html, FormError> {
    raw_members_form(
        space.id,
        space.members.map(|r| r.map_or("none", Role::wire)),
        version,
        token,
    )
}
fn raw_members_form(
    workspace: u32,
    roles: [&str; 4],
    version: &str,
    token: CsrfToken,
) -> Result<Html, FormError> {
    let mut form = PostForm::new(url(&format!("/w/{workspace}/members")))?
        .csrf(token)?
        .hidden("version", version)?;
    for (i, key) in PROFILE_KEYS.iter().enumerate() {
        form = form.text(
            *key,
            format!("{}: none, viewer, editor or owner", PROFILE_NAMES[i]),
            roles[i],
        )?;
    }
    Ok(form.submit_label("Save members")?.render())
}
fn native_form(
    w: u32,
    id: Option<u32>,
    title: &str,
    description: &str,
    state: &str,
    version: &str,
    token: CsrfToken,
) -> Result<Html, FormError> {
    let path = id.map_or_else(
        || format!("/w/{w}/tickets"),
        |id| format!("/w/{w}/tickets/{id}"),
    );
    let mut f = PostForm::new(url(&path))?
        .csrf(token)?
        .hidden("native", "yes")?
        .text("title", "Title (up to 120 bytes)", title)?
        .textarea("description", "Description (up to 1024 bytes)", description)?
        .text("state", "State: open, in_progress or closed", state)?;
    if id.is_some() {
        f = f.hidden("version", version)?;
    }
    Ok(element(Tag::Section)
        .child(element(Tag::H2).child("Native HTML form"))
        .child(f.submit_label(if id.is_some() {
            "Save ticket"
        } else {
            "Create ticket"
        })?)
        .render())
}
fn condition(raw: Option<&str>) -> Option<IfMatch> {
    raw.and_then(|s| EntityTag::parse(s).ok())
        .filter(|t| !t.is_weak())
        .and_then(|t| IfMatch::parse(t.as_str()).ok())
}
fn query(r: &Request, allowed: &[&str]) -> Result<(), PageError> {
    if r.query_params()
        .iter()
        .any(|(k, v)| !allowed.contains(&k) || (k == "native" && v != "yes"))
    {
        return Err(PageError::BadRequest);
    }
    Ok(())
}
fn page_error(s: StatusCode) -> PageError {
    match s {
        StatusCode::BadRequest => PageError::BadRequest,
        StatusCode::NotFound | StatusCode::Forbidden => PageError::NotFound,
        _ => PageError::InternalServerError,
    }
}
fn status(e: StoreError) -> StatusCode {
    match e {
        StoreError::Missing => StatusCode::NotFound,
        StoreError::Forbidden => StatusCode::Forbidden,
        StoreError::InvalidInput => StatusCode::BadRequest,
        StoreError::FileConflict => StatusCode::Conflict,
        StoreError::Conflict => StatusCode::PreconditionFailed,
        StoreError::Precondition => StatusCode::PreconditionRequired,
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
