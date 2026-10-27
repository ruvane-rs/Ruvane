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
    pub revision: u64,
    pub input: TicketInput,
}
impl Ticket {
    pub fn json(&self) -> JsonValue {
        let mut fields = self.input.json().as_object().unwrap().to_vec();
        fields.extend([
            ("id".into(), (self.id as u64).into()),
            ("revision".into(), self.revision.into()),
        ]);
        JsonValue::object(fields).unwrap()
    }
    pub fn decode(value: &JsonValue) -> Option<Self> {
        if value.as_object()?.len() != 5 {
            return None;
        }
        let id = value.get("id")?.as_number()?.to_u64().ok()?;
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
