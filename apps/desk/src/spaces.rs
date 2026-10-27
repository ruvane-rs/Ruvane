//! Workspace selection and membership editing shared by native and Wasm.
use crate::{
    model::{self, Actor, MembersInput, PROFILE_KEYS, PROFILE_NAMES, Role, WorkspaceInput},
    pages::url,
};
use ruvane::{BrowserTag::*, *};
fn text(data: &ActionData, key: &str) -> Result<String, ComponentError> {
    data.text(key)
        .map(str::to_owned)
        .ok_or(ComponentError::InvalidProps)
}
fn view_text(v: &mut View, parent: u32, key: u32, value: &str) -> Result<(), ComponentError> {
    v.keyed_leaf(parent, key, Output, value)?;
    Ok(())
}
struct Workspaces {
    actor: Option<Actor>,
    spaces: Vec<(u32, String, Role)>,
    name: String,
    message: String,
    pending: Option<u32>,
    outgoing: Option<JsonRequest>,
    uncertain: bool,
    epoch: u32,
}
impl Workspaces {
    fn adopt(&mut self, value: &JsonValue) -> Option<()> {
        if value.as_object()?.len() != 2 {
            return None;
        }
        let actor = Actor::parse(value.get("actor")?.as_str()?)?;
        let rows = value.get("spaces")?.as_array()?;
        if rows.len() > model::MAX_WORKSPACES as usize {
            return None;
        }
        let mut spaces = vec![];
        for row in rows {
            if row.as_object()?.len() != 3 {
                return None;
            }
            let id = u32::try_from(row.get("id")?.as_number()?.to_u64().ok()?).ok()?;
            let name = row.get("name")?.as_str()?;
            let role = Role::parse(row.get("role")?.as_str()?)?;
            if !(1..=model::MAX_WORKSPACES).contains(&id)
                || spaces
                    .last()
                    .is_some_and(|(previous, _, _)| *previous >= id)
                || WorkspaceInput::new(name).is_err()
            {
                return None;
            }
            spaces.push((id, name.into(), role));
        }
        if self.actor.is_some_and(|old| old != actor) {
            self.name.clear();
            self.uncertain = false;
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.actor = Some(actor);
        self.spaces = spaces;
        Some(())
    }
    fn can_create(&self) -> bool {
        self.spaces.iter().any(|(_, _, r)| *r == Role::Owner)
    }
    fn send(
        &mut self,
        action: u32,
        method: JsonMethod,
        body: Option<JsonValue>,
    ) -> Result<(), ComponentError> {
        self.outgoing = Some(
            JsonRequest::new(action, method, url("/api/workspaces"), body)
                .map_err(|_| ComponentError::InvalidEvent)?,
        );
        self.pending = Some(action);
        Ok(())
    }
}
impl Component for Workspaces {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        let count = data
            .number("count")
            .filter(|v| *v <= model::MAX_WORKSPACES as u64)
            .ok_or(ComponentError::InvalidProps)? as usize;
        if data.len() != 2 + count * 3 {
            return Err(ComponentError::InvalidProps);
        }
        let mut rows = vec![];
        for i in 0..count {
            rows.push(
                JsonValue::object(vec![
                    (
                        "id".into(),
                        data.number(&format!("id{i}"))
                            .ok_or(ComponentError::InvalidProps)?
                            .into(),
                    ),
                    (
                        "name".into(),
                        JsonValue::string(text(&data, &format!("name{i}"))?).unwrap(),
                    ),
                    (
                        "role".into(),
                        JsonValue::string(text(&data, &format!("role{i}"))?).unwrap(),
                    ),
                ])
                .unwrap(),
            );
        }
        let value = JsonValue::object(vec![
            (
                "actor".into(),
                JsonValue::string(text(&data, "actor")?).unwrap(),
            ),
            ("spaces".into(), JsonValue::array(rows).unwrap()),
        ])
        .unwrap();
        self.adopt(&value).ok_or(ComponentError::InvalidProps)?;
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(root, 1, Heading, "Workspaces")?;
        v.keyed_leaf(
            root,
            2,
            Span,
            &format!(
                "Local test profile: {}",
                self.actor.map_or("unavailable", Actor::name)
            ),
        )?;
        let list = v.keyed_element(root, 3, List)?;
        for (id, name, role) in &self.spaces {
            let row = v.keyed_element(list, *id, ListItem)?;
            v.link(
                row,
                1,
                &url(&format!("/w/{id}/tickets")),
                &format!("#{id} {name}"),
            )?;
            v.keyed_leaf(row, 2, Span, role.label())?;
        }
        if self.can_create() {
            let form = v.form(root, 4, 1)?;
            let label = v.keyed_element(form, 1, Label)?;
            v.keyed_leaf(label, 1, Span, "Workspace name (up to 48 bytes)")?;
            let field = v.input(label, 2, &self.name, 10, false)?;
            v.field_epoch(field, self.epoch)?;
            v.field_disabled(field, self.pending.is_some())?;
            v.submit(
                form,
                2,
                "Create workspace",
                self.pending.is_some() || self.uncertain,
            )?;
        }
        v.keyed_button(root, 5, "Inspect workspaces", 2, self.pending.is_some())?;
        if self.uncertain {
            v.keyed_button(
                root,
                6,
                "Discard uncertain workspace draft",
                3,
                self.pending.is_some(),
            )?;
        }
        view_text(&mut v, root, 7, &self.message)?;
        Ok(v)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending.is_some() {
            return Err(ComponentError::InvalidEvent);
        }
        match event {
            1 if self.can_create() && !self.uncertain => {
                if WorkspaceInput::new(&self.name).is_err() {
                    self.message = "Use a nonblank workspace name up to 48 bytes.".into();
                    return Ok(());
                }
                self.send(
                    1,
                    JsonMethod::Post,
                    Some(
                        JsonValue::object(vec![(
                            "name".into(),
                            JsonValue::string(&self.name).unwrap(),
                        )])
                        .unwrap(),
                    ),
                )
            }
            2 => self.send(2, JsonMethod::Get, None),
            3 if self.uncertain => {
                self.uncertain = false;
                self.name.clear();
                self.epoch = self.epoch.wrapping_add(1);
                self.message =
                    "Draft discarded. Inspect workspaces before creating another.".into();
                Ok(())
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn dispatch_value(&mut self, event: u32, value: EventValue) -> Result<(), ComponentError> {
        match (event, value) {
            (10, EventValue::Text(s)) if self.pending.is_none() => {
                self.name = s;
                Ok(())
            }
            (e, EventValue::None) => self.dispatch(e),
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.outgoing.take()
    }
    fn json_result(&mut self, action: u32, result: JsonOutcome) -> Result<(), ComponentError> {
        if self.pending.take() != Some(action) {
            return Err(ComponentError::InvalidEvent);
        }
        match result {
            Ok(reply) if reply.is_success() => {
                if self.adopt(reply.value()).is_some() {
                    if action == 1 {
                        self.name.clear();
                        self.epoch = self.epoch.wrapping_add(1);
                        self.uncertain = false;
                        self.message = "Workspace created.".into();
                    } else {
                        self.message = "Workspaces inspected. No creation was retried.".into();
                    }
                    return Ok(());
                }
                self.uncertain |= action == 1;
                self.message = "Invalid response. Draft retained; inspect workspaces.".into();
            }
            Ok(reply) => {
                if matches!(reply.status(), 403 | 404) {
                    self.spaces.clear();
                    self.actor = None;
                    self.name.clear();
                    self.epoch = self.epoch.wrapping_add(1);
                }
                self.uncertain |= action == 1 && reply.uncertain();
                self.message = format!(
                    "Request rejected ({}). Inspect current workspaces.",
                    reply.status()
                );
            }
            Err(e) => {
                self.uncertain |= action == 1 && e.uncertain();
                self.message = "Connection failed. Draft retained.".into();
            }
        }
        if self.uncertain {
            self.message="Workspace creation outcome uncertain. Inspect the list, then discard explicitly before another creation.".into();
        }
        Ok(())
    }
}
pub fn workspaces() -> Box<dyn Component> {
    Box::new(Workspaces {
        actor: None,
        spaces: vec![],
        name: String::new(),
        message: String::new(),
        pending: None,
        outgoing: None,
        uncertain: false,
        epoch: 0,
    })
}
struct Members {
    workspace: u32,
    name: String,
    actor: Option<Actor>,
    revision: u64,
    roles: [Option<Role>; 4],
    tag: Option<EntityTag>,
    pending: Option<u32>,
    outgoing: Option<JsonRequest>,
    blocked: bool,
    locked: bool,
    message: String,
}
impl Members {
    fn adopt(&mut self, value: &JsonValue, tag: EntityTag) -> Option<()> {
        if value.as_object()?.len() != 4
            || tag.is_weak()
            || value.get("workspace")?.as_number()?.to_u64().ok()? != self.workspace as u64
        {
            return None;
        }
        let revision = value.get("revision")?.as_number()?.to_u64().ok()?;
        if revision == 0 {
            return None;
        }
        let name = value.get("name")?.as_str()?;
        WorkspaceInput::new(name).ok()?;
        let roles = MembersInput::try_from(value.get("members")?).ok()?.0;
        if !roles.contains(&Some(Role::Owner)) {
            return None;
        }
        self.name = name.into();
        self.revision = revision;
        self.roles = roles;
        self.tag = Some(tag);
        self.blocked = false;
        self.locked = false;
        if self
            .actor
            .is_some_and(|a| self.roles[a.index()] != Some(Role::Owner))
        {
            self.clear();
            self.message = "Membership saved. Owner access ended. Return to the workspace.".into();
        }
        Some(())
    }
    fn clear(&mut self) {
        self.roles = [None; 4];
        self.tag = None;
        self.blocked = true;
        self.locked = true;
    }
}
impl Component for Members {
    fn page_data(&mut self, data: ActionData) -> Result<(), ComponentError> {
        if data.len() != 9 || data.number("workspace") != Some(self.workspace as u64) {
            return Err(ComponentError::InvalidProps);
        }
        self.actor = Some(
            Actor::parse(data.text("actor").ok_or(ComponentError::InvalidProps)?)
                .ok_or(ComponentError::InvalidProps)?,
        );
        let revision = data
            .number("revision")
            .ok_or(ComponentError::InvalidProps)?;
        let mut roles = vec![];
        for key in PROFILE_KEYS {
            roles.push((key.into(), JsonValue::string(text(&data, key)?).unwrap()));
        }
        let value = JsonValue::object(vec![
            ("workspace".into(), (self.workspace as u64).into()),
            ("revision".into(), revision.into()),
            (
                "name".into(),
                JsonValue::string(text(&data, "workspace_name")?).unwrap(),
            ),
            ("members".into(), JsonValue::object(roles).unwrap()),
        ])
        .unwrap();
        let tag = EntityTag::parse(data.text("etag").ok_or(ComponentError::InvalidProps)?)
            .map_err(|_| ComponentError::InvalidProps)?;
        self.adopt(&value, tag)
            .ok_or(ComponentError::InvalidProps)?;
        if self.locked {
            return Err(ComponentError::InvalidProps);
        }
        Ok(())
    }
    fn view(&self) -> Result<View, ComponentError> {
        let mut v = View::new();
        let root = v.keyed_element(0, 1, Section)?;
        v.keyed_leaf(root, 1, Heading, "Workspace members")?;
        v.keyed_leaf(
            root,
            2,
            Span,
            &format!("{} · membership revision {}", self.name, self.revision),
        )?;
        if !self.locked {
            let form = v.form(root, 3, 1)?;
            for (i, role) in self.roles.iter().enumerate() {
                let row = v.keyed_element(form, i as u32 + 1, Paragraph)?;
                v.keyed_leaf(row, 1, Span, PROFILE_NAMES[i])?;
                v.keyed_button(
                    row,
                    2,
                    &format!("{} · change role", role.map_or("None", Role::label)),
                    10 + i as u32,
                    self.pending.is_some(),
                )?;
            }
            v.submit(
                form,
                5,
                "Save members",
                self.pending.is_some() || self.blocked,
            )?;
            v.keyed_button(
                root,
                4,
                "Replace draft with current members",
                2,
                self.pending.is_some(),
            )?;
        }
        view_text(&mut v, root, 5, &self.message)?;
        v.link(
            root,
            6,
            &url(&format!("/w/{}/tickets", self.workspace)),
            "Back to workspace",
        )?;
        Ok(v)
    }
    fn dispatch(&mut self, event: u32) -> Result<(), ComponentError> {
        if self.pending.is_some() || self.locked {
            return Err(ComponentError::InvalidEvent);
        }
        match event {
            10..=13 => {
                let role = &mut self.roles[(event - 10) as usize];
                *role = match role {
                    None => Some(Role::Viewer),
                    Some(Role::Viewer) => Some(Role::Editor),
                    Some(Role::Editor) => Some(Role::Owner),
                    Some(Role::Owner) => None,
                };
                Ok(())
            }
            1 | 2 => {
                if event == 1 && (self.blocked || !self.roles.contains(&Some(Role::Owner))) {
                    self.message =
                        "Keep at least one owner. Reload explicitly after a conflict.".into();
                    return Ok(());
                }
                let body = (event == 1).then(|| {
                    JsonValue::object(
                        PROFILE_KEYS
                            .iter()
                            .zip(self.roles)
                            .map(|(key, r)| {
                                (
                                    (*key).into(),
                                    JsonValue::string(r.map_or("none", Role::wire)).unwrap(),
                                )
                            })
                            .collect(),
                    )
                    .unwrap()
                });
                let mut req = JsonRequest::new(
                    event,
                    if event == 1 {
                        JsonMethod::Patch
                    } else {
                        JsonMethod::Get
                    },
                    url(&format!("/api/w/{}/members", self.workspace)),
                    body,
                )
                .map_err(|_| ComponentError::InvalidEvent)?;
                if event == 1 {
                    req = req
                        .with_if_match(self.tag.clone().ok_or(ComponentError::InvalidEvent)?)
                        .map_err(|_| ComponentError::InvalidEvent)?;
                }
                self.outgoing = Some(req);
                self.pending = Some(event);
                Ok(())
            }
            _ => Err(ComponentError::InvalidEvent),
        }
    }
    fn take_json(&mut self) -> Option<JsonRequest> {
        self.outgoing.take()
    }
    fn json_result(&mut self, action: u32, result: JsonOutcome) -> Result<(), ComponentError> {
        if self.pending.take() != Some(action) {
            return Err(ComponentError::InvalidEvent);
        }
        match result {
            Ok(reply) if reply.is_success() => {
                let accepted = reply
                    .etag()
                    .cloned()
                    .and_then(|tag| self.adopt(reply.value(), tag));
                if accepted.is_none() {
                    self.blocked |= action == 1;
                    self.message =
                        "Invalid response. Draft retained; inspect current members.".into();
                } else if !self.locked {
                    self.message = if action == 1 {
                        "Members saved."
                    } else {
                        "Current members loaded. Previous draft replaced."
                    }
                    .into();
                }
            }
            Ok(reply) => {
                if matches!(reply.status(), 403 | 404) {
                    self.clear();
                    self.message =
                        "Owner access ended or session expired. Return to workspaces.".into();
                } else {
                    self.blocked |= reply.uncertain() || matches!(reply.status(), 412 | 428);
                    self.message = if matches!(reply.status(), 412 | 428) {
                        "Conflict. Your membership draft is retained. Replace it explicitly to load current members.".into()
                    } else {
                        format!("Request rejected ({}). Draft retained.", reply.status())
                    };
                }
            }
            Err(e) => {
                self.blocked |= action == 1 && e.uncertain();
                self.message="Connection failed. Draft retained; inspect current members before another write.".into();
            }
        }
        Ok(())
    }
}
pub fn members(workspace: u32) -> Box<dyn Component> {
    Box::new(Members {
        workspace,
        name: String::new(),
        actor: None,
        revision: 0,
        roles: [None; 4],
        tag: None,
        pending: None,
        outgoing: None,
        blocked: false,
        locked: false,
        message: String::new(),
    })
}

#[cfg(test)]
mod tests;
