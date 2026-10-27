//! Ticket views shared by SSR and Rust/Wasm.
use crate::model::{self, PAGE_SIZE, Role, State, Ticket, TicketInput};
use crate::pages::url;
use ruvane::{BrowserTag::*, *};
fn paragraph(v: &mut View, parent: u32, key: u32, text: &str) -> Result<(), ViewError> {
    let p = v.keyed_element(parent, key, Paragraph)?;
    v.keyed_leaf(p, 1, Span, text)?;
    Ok(())
}
pub const LIST: u32 = 1;
pub const EDITOR: u32 = 2;
pub const ERROR: u32 = 3;
pub const WORKSPACES: u32 = 4;
pub const MEMBERS: u32 = 5;
pub const FILES: u32 = 6;

pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {
    let InitialState::Number(id) = props.state() else {
        return Err(ComponentError::InvalidProps);
    };
    match props.component() {
        LIST if (1..=model::MAX_WORKSPACES).contains(&id) => Ok(Box::new(Listing {
            workspace: id,
            workspace_name: String::new(),
            role: Role::Viewer,
            page: 0,
            total: 0,
            items: Vec::new(),
        })),
        EDITOR if model::unpacked(id).is_some() => {
            let (w, t) = model::unpacked(id).unwrap();
            Ok(Box::new(Editor::new(w, t)))
        }
        WORKSPACES if id == 0 => Ok(crate::spaces::workspaces()),
        MEMBERS if (1..=model::MAX_WORKSPACES).contains(&id) => Ok(crate::spaces::members(id)),
        FILES if model::unpacked(id).is_some_and(|(_, t)| t > 0) => {
            let (w, t) = model::unpacked(id).unwrap();
            Ok(crate::attachments::create(w, t))
        }
        ERROR if matches!(id, 400 | 403 | 404 | 409 | 429 | 500 | 503) => {
            Ok(Box::new(ErrorPage(id)))
        }
        _ => Err(ComponentError::InvalidProps),
    }
}
type ListingSeed = (usize, usize, Vec<(u32, String, State)>);
struct Listing {
    workspace: u32,
    workspace_name: String,
    role: Role,
    page: usize,
    total: usize,
    items: Vec<(u32, String, State)>,
}
impl Component for Listing {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        let (name, role) = scope(&data, self.workspace)?;
        let read = || -> Option<ListingSeed> {
            let page = usize::try_from(data.number("page")?).ok()?;
            let total = usize::try_from(data.number("total")?).ok()?;
            let count = usize::try_from(data.number("count")?).ok()?;
            if total > model::MAX_TICKETS
                || page > total.saturating_sub(1) / PAGE_SIZE
                || count != total.saturating_sub(page * PAGE_SIZE).min(PAGE_SIZE)
                || data.len() != 6 + 3 * count
            {
                return None;
            }
            let mut items = Vec::new();
            for i in 0..count {
                let id = data.number(&format!("id{i}"))?;
                let title = data.text(&format!("title{i}"))?;
                let state = State::parse(data.text(&format!("state{i}"))?)?;
                if id == 0
                    || id > model::MAX_TICKETS as u64
                    || items
                        .last()
                        .is_some_and(|(previous, _, _)| *previous >= id as u32)
                    || TicketInput::new(title, "", state.wire()).is_err()
                {
                    return None;
                }
                items.push((id as u32, title.into(), state));
            }
            Some((page, total, items))
        };
        let (page, total, items) = read().ok_or(ComponentError::InvalidProps)?;
        self.workspace_name = name;
        self.role = role;
        self.page = page;
        self.total = total;
        self.items = items;
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(root, 1, Heading, "Tickets")?;
        paragraph(
            &mut v,
            root,
            2,
            &format!(
                "{} · {} · {} tickets · page {}",
                self.workspace_name,
                self.role.label(),
                self.total,
                self.page + 1
            ),
        )?;
        if self.role.can_edit() {
            v.link(
                root,
                3,
                &url(&model::ticket_path(self.workspace, Some(0))),
                "Create ticket",
            )?;
        }
        if self.role == Role::Owner {
            v.link(
                root,
                8,
                &url(&format!("/w/{}/members", self.workspace)),
                "Manage members",
            )?;
        }
        v.link(root, 9, &url("/workspaces"), "Switch workspace")?;
        if self.items.is_empty() {
            paragraph(&mut v, root, 4, "No tickets in this workspace yet.")?;
        }
        let list = v.keyed_element(root, 5, List)?;
        for (i, (id, title, state)) in self.items.iter().enumerate() {
            let row = v.keyed_element(list, i as u32 + 1, ListItem)?;
            v.link(
                row,
                1,
                &url(&model::ticket_path(self.workspace, Some(*id))),
                &format!("#{id} {title}"),
            )?;
            v.keyed_leaf(row, 2, Span, state.label())?;
        }
        if self.page > 0 {
            v.link(
                root,
                6,
                &url(&format!("/w/{}/tickets?page={}", self.workspace, self.page)),
                "Previous page",
            )?;
        }
        if (self.page + 1) * PAGE_SIZE < self.total {
            v.link(
                root,
                7,
                &url(&format!(
                    "/w/{}/tickets?page={}",
                    self.workspace,
                    self.page + 2
                )),
                "Next page",
            )?;
        }
        Ok(v)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
}
struct Editor {
    workspace: u32,
    workspace_name: String,
    editable: bool,
    route_id: u32,
    id: u32,
    revision: u64,
    tag: Option<EntityTag>,
    title: String,
    description: String,
    state: State,
    pending: bool,
    outgoing: Option<JsonRequest>,
    blocked: bool,
    uncertain: bool,
    message: String,
    epoch: u32,
}
impl Editor {
    fn new(workspace: u32, id: u32) -> Self {
        Self {
            workspace,
            workspace_name: String::new(),
            editable: false,
            route_id: id,
            id,
            revision: 0,
            tag: None,
            title: String::new(),
            description: String::new(),
            state: State::Open,
            pending: false,
            outgoing: None,
            blocked: false,
            uncertain: false,
            message: String::new(),
            epoch: 0,
        }
    }
    fn adopt(&mut self, ticket: Ticket, tag: EntityTag) -> Option<()> {
        if tag.is_weak()
            || ticket.workspace != self.workspace
            || (self.id != 0 && ticket.id != self.id)
        {
            return None;
        }
        self.id = ticket.id;
        self.revision = ticket.revision;
        self.tag = Some(tag);
        self.title = ticket.input.title;
        self.description = ticket.input.description;
        self.state = ticket.input.state;
        self.epoch = self.epoch.wrapping_add(1);
        self.blocked = false;
        self.uncertain = false;
        Some(())
    }
    fn send(
        &mut self,
        action: u32,
        method: JsonMethod,
        path: String,
        body: Option<JsonValue>,
    ) -> Result<(), ComponentError> {
        let mut request = JsonRequest::new(action, method, url(&path), body)
            .map_err(|_| ComponentError::InvalidEvent)?;
        if method == JsonMethod::Patch {
            request = request
                .with_if_match(self.tag.clone().ok_or(ComponentError::InvalidEvent)?)
                .map_err(|_| ComponentError::InvalidEvent)?;
        }
        self.outgoing = Some(request);
        self.pending = true;
        self.message = "Saving or loading…".into();
        Ok(())
    }
}
impl Component for Editor {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        let (name, role) = scope(&data, self.workspace)?;
        let read = || -> Option<(Ticket, Option<EntityTag>)> {
            if data.len() != 9 {
                return None;
            }
            let id = data.number("id")?;
            let revision = data.number("revision")?;
            if id != self.route_id as u64 {
                return None;
            }
            let title = data.text("title")?;
            let description = data.text("description")?;
            let state = data.text("state")?;
            let raw = data.text("etag")?;
            if id == 0 {
                if revision != 0
                    || !title.is_empty()
                    || !description.is_empty()
                    || state != "open"
                    || !raw.is_empty()
                {
                    return None;
                }
                return Some((
                    Ticket {
                        id: 0,
                        workspace: self.workspace,
                        revision: 0,
                        input: TicketInput {
                            title: String::new(),
                            description: String::new(),
                            state: State::Open,
                        },
                    },
                    None,
                ));
            }
            if revision == 0 {
                return None;
            }
            let input = TicketInput::new(title, description, state).ok()?;
            let tag = EntityTag::parse(raw).ok()?;
            if tag.is_weak() {
                return None;
            }
            Some((
                Ticket {
                    id: id as u32,
                    workspace: self.workspace,
                    revision,
                    input,
                },
                Some(tag),
            ))
        };
        let (ticket, tag) = read().ok_or(ComponentError::InvalidProps)?;
        self.workspace_name = name;
        self.editable = role.can_edit();
        if let Some(tag) = tag {
            self.adopt(ticket, tag)
                .ok_or(ComponentError::InvalidProps)?;
        }
        self.message = if self.id == 0 {
            "New ticket. Changes are saved only when you submit.".into()
        } else {
            format!("Ticket #{} · revision {}", self.id, self.revision)
        };
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(
            root,
            1,
            Heading,
            if self.id == 0 {
                "New ticket"
            } else if !self.editable {
                "Ticket details"
            } else {
                "Edit ticket"
            },
        )?;
        paragraph(
            &mut v,
            root,
            2,
            &if self.id == 0 {
                self.workspace_name.clone()
            } else {
                format!(
                    "{} · #{} · revision {}",
                    self.workspace_name, self.id, self.revision
                )
            },
        )?;
        let form = v.form(root, 3, 1)?;
        for (key, name, value, event) in [
            (1, "Title", self.title.as_str(), 10),
            (2, "Description", self.description.as_str(), 11),
        ] {
            let label = v.keyed_element(form, key, Label)?;
            v.keyed_leaf(label, 1, Span, name)?;
            let field = if key == 1 {
                v.input(label, 2, value, event, false)?
            } else {
                v.textarea(label, 2, value, event, false)?
            };
            v.field_epoch(field, self.epoch)?;
            v.field_disabled(field, self.pending || !self.editable)?;
        }
        v.keyed_leaf(form, 3, Output, &format!("State: {}", self.state.label()))?;
        if self.editable {
            v.keyed_button(
                form,
                4,
                &format!("Move to {}", self.state.next().label()),
                2,
                self.pending,
            )?;
            v.submit(
                form,
                5,
                if self.id == 0 {
                    "Create ticket"
                } else {
                    "Save ticket"
                },
                self.pending || self.blocked,
            )?;
        }
        v.keyed_leaf(root, 4, Output, &self.message)?;
        if self.id != 0 {
            v.keyed_button(
                root,
                5,
                "Replace draft with current ticket",
                3,
                self.pending,
            )?;
        }
        if self.route_id == 0 && self.id != 0 {
            v.link(
                root,
                6,
                &url(&model::ticket_path(self.workspace, Some(self.id))),
                "Open saved ticket",
            )?;
        }
        v.link(
            root,
            7,
            &url(&model::ticket_path(self.workspace, None)),
            "Back to tickets",
        )?;
        if self.id > 0 {
            v.link(
                root,
                9,
                &url(&format!("/w/{}/tickets/{}/files", self.workspace, self.id)),
                "Attachments",
            )?;
        }
        if self.uncertain && self.id == 0 {
            v.keyed_button(
                root,
                8,
                "Discard draft after inspecting tickets",
                4,
                self.pending,
            )?;
        }
        Ok(v)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending {
            return Err(ComponentError::InvalidEvent);
        }
        match event {
            1 if !self.blocked && self.editable => {
                let input = match TicketInput::new(
                    &self.title,
                    &self.description,
                    self.state.wire(),
                ) {
                    Ok(i) => i,
                    Err(_) => {
                        self.message="Use a nonblank title up to 120 bytes and a description up to 1024 bytes.".into();
                        return Ok(());
                    }
                };
                let (method, path) = if self.id == 0 {
                    (
                        JsonMethod::Post,
                        format!("/api/w/{}/tickets", self.workspace),
                    )
                } else {
                    (
                        JsonMethod::Patch,
                        format!("/api/w/{}/tickets/{}", self.workspace, self.id),
                    )
                };
                self.send(1, method, path, Some(input.json()))
            }
            2 if self.editable => {
                self.state = self.state.next();
                Ok(())
            }
            3 if self.id != 0 => self.send(
                3,
                JsonMethod::Get,
                format!("/api/w/{}/tickets/{}", self.workspace, self.id),
                None,
            ),
            4 if self.uncertain && self.id == 0 => {
                let epoch = self.epoch.wrapping_add(1);
                let workspace = self.workspace;
                let name = self.workspace_name.clone();
                let editable = self.editable;
                *self = Self::new(workspace, 0);
                self.workspace_name = name;
                self.editable = editable;
                self.epoch = epoch;
                self.message =
                    "Draft discarded. Inspect the list before creating another ticket.".into();
                Ok(())
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        if self.pending || !self.editable {
            return Err(ComponentError::InvalidEvent);
        }
        match (event, value) {
            (10, EventValue::Text(s)) => {
                self.title = s;
                Ok(())
            }
            (11, EventValue::Text(s)) => {
                self.description = s;
                Ok(())
            }
            (e, EventValue::None) => self.dispatch(e),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.outgoing.take()
    }
    fn json_result(&mut self, action: u32, outcome: JsonOutcome) -> Result<(), ComponentError> {
        if !self.pending || !matches!(action, 1 | 3) {
            return Err(ComponentError::InvalidEvent);
        }
        self.pending = false;
        match outcome {
            Ok(reply) if reply.is_success() => {
                let valid = Ticket::decode(reply.value())
                    .and_then(|ticket| self.adopt(ticket, reply.etag()?.clone()));
                if valid.is_some() {
                    self.message = if action == 1 {
                        "Ticket saved."
                    } else {
                        "Current ticket loaded. Previous draft replaced."
                    }
                    .into();
                    return Ok(());
                }
                if action == 1 {
                    self.blocked = true;
                    self.uncertain = true;
                }
                self.message =
                    "Invalid response. Draft retained; inspect the current ticket before saving."
                        .into();
            }
            Ok(reply) => {
                if matches!(reply.status(), 403 | 404) {
                    self.title.clear();
                    self.description.clear();
                    self.tag = None;
                    self.id = 0;
                    self.revision = 0;
                    self.state = State::Open;
                    self.uncertain = false;
                    self.blocked = true;
                    self.editable = false;
                    self.epoch = self.epoch.wrapping_add(1);
                    self.message =
                        "Access changed or session expired. Open workspaces to continue.".into();
                    return Ok(());
                }
                self.uncertain |= action == 1 && (reply.uncertain() || reply.status() >= 500);
                self.blocked |= self.uncertain || matches!(reply.status(), 412 | 428);
                self.message=match reply.status(){412|428=>"Conflict. Another tab changed this ticket. Your draft is retained. Replace it explicitly to load the current ticket.".into(),400=>"Validation failed. Your draft is retained.".into(),429=>"Too many requests. Your draft is retained; retry later.".into(),s=>format!("Request rejected ({s}). Your draft is retained.")};
            }
            Err(error) => {
                self.uncertain |= action == 1 && error.uncertain();
                self.blocked |= self.uncertain;
                self.message = "Connection failed. Your draft is retained.".into();
            }
        }
        if self.uncertain {
            self.message="Save outcome uncertain. Draft retained. Inspect tickets before another write; no automatic retry.".into();
        }
        Ok(())
    }
}
struct ErrorPage(u32);
impl Component for ErrorPage {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::InvalidProps)
        }
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(root, 1, Heading, &format!("Desk · {}", self.0))?;
        paragraph(
            &mut v,
            root,
            2,
            if self.0 == 403 {
                "Open a local session to view tickets."
            } else {
                "This page is unavailable. Your saved tickets are kept on disk."
            },
        )?;
        v.link(root, 3, &url("/"), "Local session")?;
        Ok(v)
    }
    fn dispatch(&mut self, _: u32) -> Result<(), ComponentError> {
        Err(ComponentError::InvalidEvent)
    }
}

#[cfg(test)]
mod tests;

fn scope(data: &ActionData, workspace: u32) -> Result<(String, Role), ComponentError> {
    if data.number("workspace") != Some(workspace as u64) {
        return Err(ComponentError::InvalidProps);
    }
    let name = data
        .text("workspace_name")
        .ok_or(ComponentError::InvalidProps)?;
    model::WorkspaceInput::new(name).map_err(|_| ComponentError::InvalidProps)?;
    let role = data
        .text("role")
        .and_then(Role::parse)
        .ok_or(ComponentError::InvalidProps)?;
    Ok((name.into(), role))
}
