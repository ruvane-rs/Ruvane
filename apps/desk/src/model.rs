//! Shared ticket validation and wire data.
use ruvane::{ActionData, JsonObject, JsonValue, TextRules, ValidationCode, ValidationErrors};

pub const MAX_TICKETS: usize = 128;
pub const PAGE_SIZE: usize = 3;
pub const MAX_TITLE: usize = 120;
pub const MAX_DESCRIPTION: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Open,
    InProgress,
    Closed,
}
impl State {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "open" => Some(Self::Open),
            "in_progress" => Some(Self::InProgress),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
    pub fn wire(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Closed => "closed",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::InProgress => "In progress",
            Self::Closed => "Closed",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Open => Self::InProgress,
            Self::InProgress => Self::Closed,
            Self::Closed => Self::Open,
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct TicketInput {
    pub title: String,
    pub description: String,
    pub state: State,
}
impl TicketInput {
    pub fn new(title: &str, description: &str, state: &str) -> Result<Self, ValidationErrors> {
        let mut errors = ValidationErrors::new();
        if title.len() > MAX_TITLE {
            errors.add(Some("title"), ValidationCode::TooLong);
        }
        if description.len() > MAX_DESCRIPTION {
            errors.add(Some("description"), ValidationCode::TooLong);
        }
        if state.len() > 11 {
            errors.add(Some("state"), ValidationCode::InvalidValue);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Self::try_from(
            &JsonValue::object(vec![
                ("title".into(), JsonValue::string(title).unwrap()),
                (
                    "description".into(),
                    JsonValue::string(description).unwrap(),
                ),
                ("state".into(), JsonValue::string(state).unwrap()),
            ])
            .unwrap(),
        )
    }
    pub fn json(&self) -> JsonValue {
        JsonValue::object(vec![
            ("title".into(), JsonValue::string(&self.title).unwrap()),
            (
                "description".into(),
                JsonValue::string(&self.description).unwrap(),
            ),
            (
                "state".into(),
                JsonValue::string(self.state.wire()).unwrap(),
            ),
        ])
        .unwrap()
    }
}
impl TryFrom<&JsonValue> for TicketInput {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut input = JsonObject::new(value, &["title", "description", "state"]);
        let title = input.required_string(
            "title",
            TextRules::new(1, MAX_TITLE)
                .unwrap()
                .non_blank()
                .no_controls(),
        );
        let description =
            input.required_string("description", TextRules::new(0, MAX_DESCRIPTION).unwrap());
        if description.is_some_and(|s| s.chars().any(|c| c.is_control() && c != '\n' && c != '\t'))
        {
            input.error(Some("description"), ValidationCode::InvalidValue);
        }
        let state = input
            .required_string("state", TextRules::new(1, 11).unwrap())
            .and_then(State::parse);
        if state.is_none() {
            input.error(Some("state"), ValidationCode::InvalidValue);
        }
        input.finish()?;
        Ok(Self {
            title: title.unwrap().into(),
            description: description.unwrap().into(),
            state: state.unwrap(),
        })
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket {
    pub id: u32,
    pub workspace: u32,
    pub revision: u64,
    pub input: TicketInput,
}
impl Ticket {
    pub fn json(&self) -> JsonValue {
        let mut fields = self.input.json().as_object().unwrap().to_vec();
        fields.extend([
            ("workspace".into(), (self.workspace as u64).into()),
            ("id".into(), (self.id as u64).into()),
            ("revision".into(), self.revision.into()),
        ]);
        JsonValue::object(fields).unwrap()
    }
    pub fn decode(value: &JsonValue) -> Option<Self> {
        if value.as_object()?.len() != 6 {
            return None;
        }
        let id = value.get("id")?.as_number()?.to_u64().ok()?;
        let workspace = u32::try_from(value.get("workspace")?.as_number()?.to_u64().ok()?).ok()?;
        if !(1..=MAX_WORKSPACES).contains(&workspace) {
            return None;
        }
        let revision = value.get("revision")?.as_number()?.to_u64().ok()?;
        if !(1..=MAX_TICKETS as u64).contains(&id) || revision == 0 {
            return None;
        }
        let input = TicketInput::new(
            value.get("title")?.as_str()?,
            value.get("description")?.as_str()?,
            value.get("state")?.as_str()?,
        )
        .ok()?;
        Some(Self {
            id: id as u32,
            workspace,
            revision,
            input,
        })
    }
    pub fn seed(&self, tag: &str) -> ActionData {
        let mut data = ActionData::default();
        for (name, value) in [("id", self.id as u64), ("revision", self.revision)] {
            data.insert(name, ruvane::ActionValue::Number(value))
                .unwrap();
        }
        for (name, value) in [
            ("title", self.input.title.as_str()),
            ("description", self.input.description.as_str()),
            ("state", self.input.state.wire()),
            ("etag", tag),
        ] {
            data.insert(name, ruvane::ActionValue::Text(value.into()))
                .unwrap();
        }
        data
    }
}
pub fn empty_seed() -> ActionData {
    let mut data = ActionData::default();
    for name in ["id", "revision"] {
        data.insert(name, ruvane::ActionValue::Number(0)).unwrap();
    }
    for (name, value) in [
        ("title", ""),
        ("description", ""),
        ("state", "open"),
        ("etag", ""),
    ] {
        data.insert(name, ruvane::ActionValue::Text(value.into()))
            .unwrap();
    }
    data
}
pub fn id(raw: &str) -> Option<u32> {
    let id = raw.parse::<u32>().ok()?;
    (id > 0 && id <= MAX_TICKETS as u32 && id.to_string() == raw).then_some(id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_shared_input() {
        assert!(TicketInput::new("", "", "open").is_err());
        assert!(TicketInput::new("  ", "", "open").is_err());
        assert!(TicketInput::new("a\nb", "", "open").is_err());
        assert!(TicketInput::new("ok", "line\n\ttab", "open").is_ok());
        for state in ["Open", "done", "", "1"] {
            assert!(TicketInput::new("ok", "", state).is_err());
        }
        for text in ["\r", "\0", "\u{85}"] {
            assert!(TicketInput::new("ok", text, "open").is_err());
        }
        assert!(TicketInput::new(&"é".repeat(60), &"é".repeat(512), "closed").is_ok());
        assert!(TicketInput::new(&"é".repeat(61), "", "open").is_err());
        let mut value = TicketInput::new("ok", "", "open")
            .unwrap()
            .json()
            .as_object()
            .unwrap()
            .to_vec();
        value.push(("extra".into(), true.into()));
        assert!(TicketInput::try_from(&JsonValue::object(value).unwrap()).is_err());
        assert!(id("01").is_none());
        assert!(id("129").is_none());
    }
}

pub const MAX_WORKSPACES: u32 = 4;
pub const PROFILE_KEYS: [&str; 4] = ["alex", "blair", "casey", "drew"];
pub const PROFILE_NAMES: [&str; 4] = ["Alex", "Blair", "Casey", "Drew"];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actor(u8);
impl Actor {
    pub fn parse(raw: &str) -> Option<Self> {
        PROFILE_KEYS
            .iter()
            .position(|s| *s == raw)
            .map(|i| Self(i as u8))
    }
    pub fn key(self) -> &'static str {
        PROFILE_KEYS[self.index()]
    }
    pub fn name(self) -> &'static str {
        PROFILE_NAMES[self.index()]
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Viewer,
    Editor,
    Owner,
}
impl Role {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
    pub fn wire(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Owner => "owner",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Viewer => "Viewer",
            Self::Editor => "Editor",
            Self::Owner => "Owner",
        }
    }
    pub fn can_edit(self) -> bool {
        self != Self::Viewer
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct Space {
    pub id: u32,
    pub name: String,
    pub revision: u64,
    pub members: [Option<Role>; 4],
}
impl Space {
    pub fn role(&self, actor: Actor) -> Option<Role> {
        self.members[actor.index()]
    }
}
#[derive(Clone)]
pub struct WorkspaceInput {
    pub name: String,
}
impl TryFrom<&JsonValue> for WorkspaceInput {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut object = JsonObject::new(value, &["name"]);
        let name = object.required_string(
            "name",
            TextRules::new(1, 48).unwrap().non_blank().no_controls(),
        );
        object.finish()?;
        Ok(Self {
            name: name.unwrap().into(),
        })
    }
}
impl WorkspaceInput {
    pub fn new(name: &str) -> Result<Self, ValidationErrors> {
        if name.len() > 48 {
            let mut errors = ValidationErrors::new();
            errors.add(Some("name"), ValidationCode::TooLong);
            return Err(errors);
        }
        Self::try_from(
            &JsonValue::object(vec![("name".into(), JsonValue::string(name).unwrap())]).unwrap(),
        )
    }
}
#[derive(Clone)]
pub struct MembersInput(pub [Option<Role>; 4]);
impl TryFrom<&JsonValue> for MembersInput {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut object = JsonObject::new(value, &PROFILE_KEYS);
        let mut roles = [None; 4];
        for (i, key) in PROFILE_KEYS.iter().enumerate() {
            let raw = object.required_string(key, TextRules::new(1, 6).unwrap());
            if raw != Some("none") {
                roles[i] = raw.and_then(Role::parse);
                if roles[i].is_none() {
                    object.error(Some(key), ValidationCode::InvalidValue);
                }
            }
        }
        object.finish()?;
        Ok(Self(roles))
    }
}
pub fn packed(workspace: u32, id: u32) -> u32 {
    workspace * 256 + id
}
pub fn unpacked(value: u32) -> Option<(u32, u32)> {
    let (workspace, id) = (value / 256, value % 256);
    ((1..=MAX_WORKSPACES).contains(&workspace) && id <= MAX_TICKETS as u32)
        .then_some((workspace, id))
}
pub fn workspace_id(raw: &str) -> Option<u32> {
    let value = raw.parse::<u32>().ok()?;
    ((1..=MAX_WORKSPACES).contains(&value) && value.to_string() == raw).then_some(value)
}
pub fn ticket_path(workspace: u32, id: Option<u32>) -> String {
    match id {
        Some(0) => format!("/w/{workspace}/new-ticket"),
        Some(id) => format!("/w/{workspace}/tickets/{id}"),
        None => format!("/w/{workspace}/tickets"),
    }
}
pub fn scope_seed(mut data: ActionData, space: &Space, role: Role) -> ActionData {
    data.insert("workspace", ruvane::ActionValue::Number(space.id as u64))
        .unwrap();
    data.insert(
        "workspace_name",
        ruvane::ActionValue::Text(space.name.clone()),
    )
    .unwrap();
    data.insert("role", ruvane::ActionValue::Text(role.wire().into()))
        .unwrap();
    data
}
