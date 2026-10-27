use ruvane::{JsonObject, JsonValue, TextRules, ValidationErrors};

pub const MAX_TITLE_BYTES: usize = 256;
fn title_rules() -> TextRules {
    TextRules::new(1, MAX_TITLE_BYTES)
        .unwrap()
        .non_blank()
        .no_controls()
}
#[derive(Eq, PartialEq)]
pub struct CreateTask {
    pub title: String,
}
#[derive(Eq, PartialEq)]
pub struct ReplaceTask {
    pub title: String,
    pub completed: bool,
}
#[derive(Eq, PartialEq)]
pub struct PatchTask {
    pub title: Option<String>,
    pub completed: Option<bool>,
}
impl TryFrom<&JsonValue> for CreateTask {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut input = JsonObject::new(value, &["title"]);
        let title = input.required_string("title", title_rules());
        input.finish()?;
        Ok(Self {
            title: title.unwrap().into(),
        })
    }
}
impl TryFrom<&JsonValue> for ReplaceTask {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut input = JsonObject::new(value, &["title", "completed"]);
        let title = input.required_string("title", title_rules());
        let completed = input.required_bool("completed");
        input.finish()?;
        Ok(Self {
            title: title.unwrap().into(),
            completed: completed.unwrap(),
        })
    }
}
impl TryFrom<&JsonValue> for PatchTask {
    type Error = ValidationErrors;
    fn try_from(value: &JsonValue) -> Result<Self, Self::Error> {
        let mut input = JsonObject::new(value, &["title", "completed"]);
        input.require_nonempty();
        let title = input.optional_string("title", title_rules());
        let completed = input.optional_bool("completed");
        input.finish()?;
        Ok(Self {
            title: title.map(str::to_owned),
            completed,
        })
    }
}

impl std::fmt::Debug for CreateTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CreateTask([redacted])")
    }
}
impl std::fmt::Debug for ReplaceTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReplaceTask([redacted])")
    }
}
impl std::fmt::Debug for PatchTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PatchTask([redacted])")
    }
}
