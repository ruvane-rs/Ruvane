use crate::JsonValue;
use std::fmt;

/// Maximum declared fields in one object decoder.
pub const MAX_VALIDATION_FIELDS: usize = 32;
/// Maximum ASCII bytes in a declared field name.
pub const MAX_VALIDATION_FIELD_BYTES: usize = 64;
/// Maximum distinct issues retained in one validation report.
pub const MAX_VALIDATION_ERRORS: usize = 16;
/// Maximum encoded bytes in a validation response.
pub const MAX_VALIDATION_RESPONSE_BYTES: usize = 4096;

/// A fixed validation category. Reports never contain submitted values or unknown keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationCode {
    /// A required field is absent.
    Required,
    /// An explicit null is not allowed for this field.
    NullNotAllowed,
    /// The root value must be an object.
    ExpectedObject,
    /// The field must be a JSON string.
    ExpectedString,
    /// The field must be a JSON boolean.
    ExpectedBoolean,
    /// The field must be an integer literal, without coercion.
    ExpectedInteger,
    /// The integer cannot be represented or exceeds application bounds.
    OutOfRange,
    /// A string is below its UTF-8 byte minimum.
    TooShort,
    /// A string exceeds its UTF-8 byte maximum.
    TooLong,
    /// A string contains only whitespace.
    Blank,
    /// A string contains forbidden characters.
    InvalidCharacters,
    /// The object has an undeclared key; its name is omitted.
    UnknownField,
    /// An object that requires at least one field is empty.
    EmptyObject,
    /// An application-specific rule failed.
    InvalidValue,
}
impl ValidationCode {
    /// Returns the stable machine-readable code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::NullNotAllowed => "null_not_allowed",
            Self::ExpectedObject => "expected_object",
            Self::ExpectedString => "expected_string",
            Self::ExpectedBoolean => "expected_boolean",
            Self::ExpectedInteger => "expected_integer",
            Self::OutOfRange => "out_of_range",
            Self::TooShort => "too_short",
            Self::TooLong => "too_long",
            Self::Blank => "blank",
            Self::InvalidCharacters => "invalid_characters",
            Self::UnknownField => "unknown_field",
            Self::EmptyObject => "empty_object",
            Self::InvalidValue => "invalid_value",
        }
    }
    /// Returns a fixed English message without input fragments.
    pub fn message(self) -> &'static str {
        match self {
            Self::Required => "This field is required.",
            Self::NullNotAllowed => "Null is not allowed.",
            Self::ExpectedObject => "Expected an object.",
            Self::ExpectedString => "Expected text.",
            Self::ExpectedBoolean => "Expected a boolean.",
            Self::ExpectedInteger => "Expected an integer.",
            Self::OutOfRange => "This number is out of range.",
            Self::TooShort => "This value is too short.",
            Self::TooLong => "This value is too long.",
            Self::Blank => "Enter a nonblank value.",
            Self::InvalidCharacters => "This value contains forbidden characters.",
            Self::UnknownField => "Unexpected field.",
            Self::EmptyObject => "Provide at least one field.",
            Self::InvalidValue => "This value is invalid.",
        }
    }
}
/// A trusted declared field and a fixed error code. None denotes the object itself.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ValidationIssue {
    field: Option<&'static str>,
    code: ValidationCode,
}
impl ValidationIssue {
    /// Returns the application-declared field, never an unknown input key.
    pub fn field(&self) -> Option<&'static str> {
        self.field
    }
    /// Returns the fixed error category.
    pub fn code(&self) -> ValidationCode {
        self.code
    }
}
impl fmt::Debug for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidationIssue([redacted])")
    }
}
/// A bounded validation report. Overflow remains a failure and sets truncated.
/// Field names must be application declarations, never names copied from input objects.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct ValidationErrors {
    issues: Vec<ValidationIssue>,
    truncated: bool,
    definition_error: bool,
}
impl ValidationErrors {
    /// Decodes a bounded remote report. Fields must match trusted static declarations.
    /// Unknown fields, codes and extra members are rejected without retaining their contents.
    pub fn from_json(
        value: &JsonValue,
        fields: &'static [&'static str],
    ) -> Result<Self, crate::JsonError> {
        let invalid = crate::JsonError::InvalidSyntax;
        if fields.len() > MAX_VALIDATION_FIELDS
            || fields
                .iter()
                .enumerate()
                .any(|(i, name)| !valid_name(name) || fields[..i].contains(name))
            || value.encode().len() > MAX_VALIDATION_RESPONSE_BYTES
            || value.as_object().is_none_or(|members| members.len() != 3)
            || value.get("error").and_then(JsonValue::as_str) != Some("Bad Request")
        {
            return Err(invalid);
        }
        let truncated = value
            .get("truncated")
            .and_then(JsonValue::as_bool)
            .ok_or(invalid)?;
        let issues = value
            .get("errors")
            .and_then(JsonValue::as_array)
            .ok_or(invalid)?;
        if issues.len() > MAX_VALIDATION_ERRORS || (issues.is_empty() && !truncated) {
            return Err(invalid);
        }
        let mut report = Self::new();
        for issue in issues {
            let members = issue.as_object().ok_or(invalid)?;
            if members.len() != 1 + usize::from(issue.get("field").is_some()) {
                return Err(invalid);
            }
            let field = match issue.get("field") {
                Some(name) => Some(
                    *fields
                        .iter()
                        .find(|field| Some(**field) == name.as_str())
                        .ok_or(invalid)?,
                ),
                None => None,
            };
            let code = [
                ValidationCode::Required,
                ValidationCode::NullNotAllowed,
                ValidationCode::ExpectedObject,
                ValidationCode::ExpectedString,
                ValidationCode::ExpectedBoolean,
                ValidationCode::ExpectedInteger,
                ValidationCode::OutOfRange,
                ValidationCode::TooShort,
                ValidationCode::TooLong,
                ValidationCode::Blank,
                ValidationCode::InvalidCharacters,
                ValidationCode::UnknownField,
                ValidationCode::EmptyObject,
                ValidationCode::InvalidValue,
            ]
            .into_iter()
            .find(|code| Some(code.as_str()) == issue.get("code").and_then(JsonValue::as_str))
            .ok_or(invalid)?;
            let before = report.len();
            report.add(field, code);
            if report.len() == before {
                return Err(invalid);
            }
        }
        report.truncated = truncated;
        Ok(report)
    }
    /// Creates an empty report.
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a fixed issue for a trusted static field or the object itself.
    /// Invalid names mark a definition failure, which serializes as a generic 500.
    pub fn add(&mut self, field: Option<&'static str>, code: ValidationCode) {
        if field.is_some_and(|name| !valid_name(name)) {
            self.definition_error = true;
            return;
        }
        let issue = ValidationIssue { field, code };
        if self.issues.contains(&issue) {
            return;
        }
        if self.issues.len() == MAX_VALIDATION_ERRORS {
            self.truncated = true;
        } else {
            self.issues.push(issue);
        }
    }
    /// Iterates retained issues in deterministic insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ValidationIssue> {
        self.issues.iter()
    }
    /// Number of retained issues.
    pub fn len(&self) -> usize {
        self.issues.len()
    }
    /// Whether no issue, truncation or definition failure occurred.
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty() && !self.truncated && !self.definition_error
    }
    /// Whether additional distinct issues were omitted.
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    /// Whether the application supplied an invalid decoder definition or error field.
    pub fn is_definition_error(&self) -> bool {
        self.definition_error
    }
    /// Succeeds only for an empty report. Call before constructing application data.
    pub fn finish(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }
    pub(crate) fn status(&self) -> crate::StatusCode {
        if self.is_empty() || self.definition_error {
            crate::StatusCode::InternalServerError
        } else {
            crate::StatusCode::BadRequest
        }
    }
    /// Encodes a bounded report without input values. Invalid definitions and empty reports
    /// produce only a generic internal error; ordinary reports contain fixed codes and declared fields.
    pub fn to_json(&self) -> JsonValue {
        let mut members = vec![(
            "error".into(),
            JsonValue::string(self.status().reason()).unwrap(),
        )];
        if self.status() == crate::StatusCode::BadRequest {
            let issues = self
                .issues
                .iter()
                .map(|issue| {
                    let mut fields = Vec::new();
                    if let Some(field) = issue.field {
                        fields.push(("field".into(), JsonValue::string(field).unwrap()));
                    }
                    fields.push((
                        "code".into(),
                        JsonValue::string(issue.code.as_str()).unwrap(),
                    ));
                    JsonValue::object(fields).unwrap()
                })
                .collect();
            members.push(("errors".into(), JsonValue::array(issues).unwrap()));
            members.push(("truncated".into(), self.truncated.into()));
        }
        JsonValue::object(members).unwrap()
    }
}
impl fmt::Debug for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidationErrors")
            .field("count", &self.len())
            .field("truncated", &self.truncated)
            .field("definition_error", &self.definition_error)
            .finish()
    }
}
impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.definition_error {
            "invalid validation definition"
        } else {
            "validation failed"
        })
    }
}
impl std::error::Error for ValidationErrors {}
fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_VALIDATION_FIELD_BYTES
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Explicit UTF-8 byte and text rules. Text is never trimmed or normalized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct TextRules {
    minimum: usize,
    maximum: usize,
    non_blank: bool,
    no_controls: bool,
}
impl TextRules {
    /// Accepts an ordered byte range within the JSON string budget.
    pub fn new(minimum: usize, maximum: usize) -> Option<Self> {
        (minimum <= maximum && maximum <= crate::MAX_JSON_STRING_BYTES).then_some(Self {
            minimum,
            maximum,
            non_blank: false,
            no_controls: false,
        })
    }
    /// Rejects strings containing only Unicode whitespace.
    pub fn non_blank(mut self) -> Self {
        self.non_blank = true;
        self
    }
    /// Rejects Unicode control characters, including newlines and tabs.
    pub fn no_controls(mut self) -> Self {
        self.no_controls = true;
        self
    }
    fn check(self, value: &str) -> Option<ValidationCode> {
        if value.len() < self.minimum {
            Some(ValidationCode::TooShort)
        } else if value.len() > self.maximum {
            Some(ValidationCode::TooLong)
        } else if self.non_blank && value.trim().is_empty() {
            Some(ValidationCode::Blank)
        } else if self.no_controls && value.chars().any(char::is_control) {
            Some(ValidationCode::InvalidCharacters)
        } else {
            None
        }
    }
}
/// An input field's exact presence. Null and absence are distinct.
#[derive(Clone, Copy)]
pub enum JsonField<'a> {
    /// No member with this declared name exists.
    Missing,
    /// The member is explicitly JSON null.
    Null,
    /// A non-null JSON value, without coercion.
    Value(&'a JsonValue),
}
impl fmt::Debug for JsonField<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "JsonField::Missing",
            Self::Null => "JsonField::Null",
            Self::Value(_) => "JsonField::Value([redacted])",
        })
    }
}

/// A strict flat-object decoder with trusted static field declarations.
/// Always call finish before using the returned values to mutate application state.
/// Nested values remain available through field for application-defined validation.
///
/// ```
/// use ruvane::{JsonObject, JsonValue, TextRules};
/// let value = JsonValue::parse(br#"{"title":"Rust"}"#).unwrap();
/// let mut input = JsonObject::new(&value, &["title"]);
/// let title = input.required_string("title", TextRules::new(1, 256).unwrap().non_blank());
/// input.finish().unwrap();
/// assert_eq!(title, Some("Rust"));
/// ```
#[must_use]
pub struct JsonObject<'a> {
    object: Option<&'a [(String, JsonValue)]>,
    fields: &'static [&'static str],
    errors: ValidationErrors,
}
impl<'a> JsonObject<'a> {
    /// Declares allowed fields. Unknown input keys add one object-level issue without their names.
    /// Duplicate/invalid declarations or more than 32 fields fail as definition errors.
    pub fn new(value: &'a JsonValue, fields: &'static [&'static str]) -> Self {
        let mut errors = ValidationErrors::new();
        if fields.len() > MAX_VALIDATION_FIELDS
            || fields
                .iter()
                .enumerate()
                .any(|(i, name)| !valid_name(name) || fields[..i].contains(name))
        {
            errors.definition_error = true;
            return Self {
                object: None,
                fields: &[],
                errors,
            };
        }
        let object = value.as_object();
        match object {
            None => errors.add(None, ValidationCode::ExpectedObject),
            Some(members)
                if members
                    .iter()
                    .any(|(name, _)| !fields.contains(&name.as_str())) =>
            {
                errors.add(None, ValidationCode::UnknownField)
            }
            _ => {}
        }
        Self {
            object,
            fields,
            errors,
        }
    }
    /// Borrows a declared field without treating null as absent. Undeclared access fails closed.
    pub fn field(&mut self, name: &'static str) -> JsonField<'a> {
        if !self.fields.contains(&name) {
            self.errors.definition_error = true;
            return JsonField::Missing;
        }
        match self
            .object
            .and_then(|members| members.iter().find(|(key, _)| key == name))
            .map(|(_, value)| value)
        {
            None => JsonField::Missing,
            Some(value) if value.is_null() => JsonField::Null,
            Some(value) => JsonField::Value(value),
        }
    }
    /// Adds a fixed domain issue for a declared field or the object. No arbitrary message is accepted.
    pub fn error(&mut self, field: Option<&'static str>, code: ValidationCode) {
        if field.is_some_and(|name| !self.fields.contains(&name)) {
            self.errors.definition_error = true;
            return;
        }
        self.errors.add(field, code);
    }
    /// Requires an object with at least one member, useful for partial updates.
    pub fn require_nonempty(&mut self) {
        if self.object.is_some_and(<[_]>::is_empty) {
            self.error(None, ValidationCode::EmptyObject);
        }
    }
    fn value(&mut self, name: &'static str, required: bool) -> Option<&'a JsonValue> {
        if !self.fields.contains(&name) {
            self.errors.definition_error = true;
            return None;
        }
        self.object?;
        match self.field(name) {
            JsonField::Missing => {
                if required {
                    self.error(Some(name), ValidationCode::Required);
                }
                None
            }
            JsonField::Null => {
                self.error(Some(name), ValidationCode::NullNotAllowed);
                None
            }
            JsonField::Value(value) => Some(value),
        }
    }
    fn string(&mut self, name: &'static str, required: bool, rules: TextRules) -> Option<&'a str> {
        let value = self.value(name, required)?;
        let Some(text) = value.as_str() else {
            self.error(Some(name), ValidationCode::ExpectedString);
            return None;
        };
        if let Some(code) = rules.check(text) {
            self.error(Some(name), code);
            None
        } else {
            Some(text)
        }
    }
    /// Reads a required string, enforcing explicit text rules without coercion.
    pub fn required_string(&mut self, name: &'static str, rules: TextRules) -> Option<&'a str> {
        self.string(name, true, rules)
    }
    /// Reads an optional string. Absence is allowed; explicit null and invalid values add issues.
    pub fn optional_string(&mut self, name: &'static str, rules: TextRules) -> Option<&'a str> {
        self.string(name, false, rules)
    }
    fn boolean(&mut self, name: &'static str, required: bool) -> Option<bool> {
        let value = self.value(name, required)?;
        match value.as_bool() {
            Some(value) => Some(value),
            None => {
                self.error(Some(name), ValidationCode::ExpectedBoolean);
                None
            }
        }
    }
    /// Reads a required JSON boolean. Strings and numeric values are rejected.
    pub fn required_bool(&mut self, name: &'static str) -> Option<bool> {
        self.boolean(name, true)
    }
    /// Reads an optional boolean. Missing and null never silently become false.
    pub fn optional_bool(&mut self, name: &'static str) -> Option<bool> {
        self.boolean(name, false)
    }
    fn number(&mut self, name: &'static str, required: bool) -> Option<&'a crate::JsonNumber> {
        let value = self.value(name, required)?;
        match value.as_number() {
            Some(number)
                if !number
                    .as_str()
                    .bytes()
                    .any(|b| matches!(b, b'.' | b'e' | b'E')) =>
            {
                Some(number)
            }
            _ => {
                self.error(Some(name), ValidationCode::ExpectedInteger);
                None
            }
        }
    }
    fn unsigned(
        &mut self,
        name: &'static str,
        required: bool,
        minimum: u64,
        maximum: u64,
    ) -> Option<u64> {
        if minimum > maximum {
            self.errors.definition_error = true;
            return None;
        }
        let value = self
            .number(name, required)?
            .to_u64()
            .ok()
            .filter(|n| minimum <= *n && *n <= maximum);
        if value.is_none() {
            self.error(Some(name), ValidationCode::OutOfRange);
        }
        value
    }
    /// Reads a required exact unsigned integer within inclusive bounds.
    pub fn required_u64(&mut self, name: &'static str, minimum: u64, maximum: u64) -> Option<u64> {
        self.unsigned(name, true, minimum, maximum)
    }
    /// Reads an optional exact unsigned integer within inclusive bounds.
    pub fn optional_u64(&mut self, name: &'static str, minimum: u64, maximum: u64) -> Option<u64> {
        self.unsigned(name, false, minimum, maximum)
    }
    fn signed(
        &mut self,
        name: &'static str,
        required: bool,
        minimum: i64,
        maximum: i64,
    ) -> Option<i64> {
        if minimum > maximum {
            self.errors.definition_error = true;
            return None;
        }
        let value = self
            .number(name, required)?
            .to_i64()
            .ok()
            .filter(|n| minimum <= *n && *n <= maximum);
        if value.is_none() {
            self.error(Some(name), ValidationCode::OutOfRange);
        }
        value
    }
    /// Reads a required exact signed integer within inclusive bounds.
    pub fn required_i64(&mut self, name: &'static str, minimum: i64, maximum: i64) -> Option<i64> {
        self.signed(name, true, minimum, maximum)
    }
    /// Reads an optional exact signed integer within inclusive bounds.
    pub fn optional_i64(&mut self, name: &'static str, minimum: i64, maximum: i64) -> Option<i64> {
        self.signed(name, false, minimum, maximum)
    }
    /// Completes decoding. Required values may be unwrapped only after this succeeds.
    pub fn finish(self) -> Result<(), ValidationErrors> {
        self.errors.finish()
    }
}
impl fmt::Debug for JsonObject<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonObject")
            .field("errors", &self.errors)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
