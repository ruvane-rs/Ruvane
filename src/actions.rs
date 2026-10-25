//! Typed action requests and RVA1 responses.
use crate::{FormData, InternalUrl, MAX_COMPONENT_BYTES};
use std::fmt;

/// MIME type of an RVA1 action response.
pub const ACTION_MEDIA_TYPE: &str = "application/vnd.ruvane.action";
/// Maximum bytes in an action response.
pub const MAX_ACTION_REPLY_BYTES: usize = 64 * 1024;
/// Maximum fields in an action response.
pub const MAX_ACTION_FIELDS: usize = 160;
/// Maximum encoded bytes in an action request body.
pub const MAX_ACTION_REQUEST_BYTES: usize = 16 * 1024;

/// Invalid action data, without reflected field contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionError {
    /// A field, type, encoding or frame is invalid.
    InvalidData,
    /// A byte or field-count budget was exceeded.
    Limit,
    /// The action ID or internal route is invalid.
    InvalidRoute,
}
impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidData => "invalid action data",
            Self::Limit => "action limit exceeded",
            Self::InvalidRoute => "invalid action route",
        })
    }
}
impl std::error::Error for ActionError {}

/// A value in a typed action response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionValue {
    /// Bounded UTF-8 text, permitting LF and tabs.
    Text(String),
    /// An unsigned integer, encoded without JavaScript number conversion.
    Number(u64),
    /// A boolean.
    Boolean(bool),
}

/// Unique named fields in an action response.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionData {
    fields: Vec<(String, ActionValue)>,
    size: usize,
}
impl ActionData {
    /// Adds one field. Failure leaves the data unchanged.
    pub fn insert(&mut self, name: &str, value: ActionValue) -> Result<(), ActionError> {
        crate::forms::valid_name(name).map_err(|_| ActionError::InvalidData)?;
        if self.fields.iter().any(|(key, _)| key == name) {
            return Err(ActionError::InvalidData);
        }
        let size = 2
            + name.len()
            + match &value {
                ActionValue::Text(text) => {
                    if text.len() > MAX_COMPONENT_BYTES {
                        return Err(ActionError::Limit);
                    }
                    if text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
                    {
                        return Err(ActionError::InvalidData);
                    }
                    4 + text.len()
                }
                ActionValue::Number(_) => 8,
                ActionValue::Boolean(_) => 1,
            };
        if self.fields.len() == MAX_ACTION_FIELDS || self.size + size + 7 > MAX_ACTION_REPLY_BYTES {
            return Err(ActionError::Limit);
        }
        self.fields.push((name.to_owned(), value));
        self.size += size;
        Ok(())
    }
    /// Looks up one typed field.
    pub fn get(&self, name: &str) -> Option<&ActionValue> {
        self.fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }
    /// Looks up a text field without coercion.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            ActionValue::Text(value) => Some(value),
            _ => None,
        }
    }
    /// Looks up an integer field without coercion.
    pub fn number(&self, name: &str) -> Option<u64> {
        match self.get(name)? {
            ActionValue::Number(value) => Some(*value),
            _ => None,
        }
    }
    /// Looks up a boolean field without coercion.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            ActionValue::Boolean(value) => Some(*value),
            _ => None,
        }
    }
    /// Number of fields.
    pub fn len(&self) -> usize {
        self.fields.len()
    }
    /// Whether no fields are present.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// A success or application validation response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionReply {
    data: ActionData,
    validation: bool,
}
impl ActionReply {
    /// Creates a successful reply, served with HTTP 200.
    pub fn success(data: ActionData) -> Self {
        Self {
            data,
            validation: false,
        }
    }
    /// Creates a validation reply, served with HTTP 400.
    pub fn validation(data: ActionData) -> Self {
        Self {
            data,
            validation: true,
        }
    }
    /// Whether the application rejected the submitted values.
    pub fn is_validation(&self) -> bool {
        self.validation
    }
    /// Typed response fields. Applications validate their own schema.
    pub fn data(&self) -> &ActionData {
        &self.data
    }
    /// Encodes one complete RVA1 frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = b"RVA1".to_vec();
        bytes.push(u8::from(self.validation));
        bytes.extend_from_slice(&(self.data.len() as u16).to_le_bytes());
        for (name, value) in &self.data.fields {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
            match value {
                ActionValue::Text(text) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(text.as_bytes());
                }
                ActionValue::Number(value) => {
                    bytes.push(2);
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                ActionValue::Boolean(value) => {
                    bytes.push(3);
                    bytes.push(u8::from(*value));
                }
            }
        }
        bytes
    }
    /// Decodes exactly one bounded frame before exposing any fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, ActionError> {
        if bytes.len() > MAX_ACTION_REPLY_BYTES {
            return Err(ActionError::Limit);
        }
        struct Reader<'a> {
            bytes: &'a [u8],
            position: usize,
        }
        impl<'a> Reader<'a> {
            fn take(&mut self, size: usize) -> Result<&'a [u8], ActionError> {
                let end = self
                    .position
                    .checked_add(size)
                    .ok_or(ActionError::InvalidData)?;
                let slice = self
                    .bytes
                    .get(self.position..end)
                    .ok_or(ActionError::InvalidData)?;
                self.position = end;
                Ok(slice)
            }
            fn byte(&mut self) -> Result<u8, ActionError> {
                Ok(self.take(1)?[0])
            }
        }
        let mut reader = Reader { bytes, position: 0 };
        if reader.take(4)? != b"RVA1" {
            return Err(ActionError::InvalidData);
        }
        let kind = reader.byte()?;
        if kind > 1 {
            return Err(ActionError::InvalidData);
        }
        let count = u16::from_le_bytes(reader.take(2)?.try_into().unwrap()) as usize;
        if count > MAX_ACTION_FIELDS {
            return Err(ActionError::Limit);
        }
        let mut data = ActionData::default();
        for _ in 0..count {
            let size = reader.byte()? as usize;
            if size == 0 || size > 64 {
                return Err(ActionError::InvalidData);
            }
            let name =
                std::str::from_utf8(reader.take(size)?).map_err(|_| ActionError::InvalidData)?;
            let value = match reader.byte()? {
                1 => {
                    let length = u32::from_le_bytes(reader.take(4)?.try_into().unwrap()) as usize;
                    if length > MAX_COMPONENT_BYTES {
                        return Err(ActionError::Limit);
                    }
                    ActionValue::Text(
                        std::str::from_utf8(reader.take(length)?)
                            .map_err(|_| ActionError::InvalidData)?
                            .to_owned(),
                    )
                }
                2 => ActionValue::Number(u64::from_le_bytes(reader.take(8)?.try_into().unwrap())),
                3 => {
                    let value = reader.byte()?;
                    if value > 1 {
                        return Err(ActionError::InvalidData);
                    }
                    ActionValue::Boolean(value == 1)
                }
                _ => return Err(ActionError::InvalidData),
            };
            data.insert(name, value)?;
        }
        if reader.position != bytes.len() {
            return Err(ActionError::InvalidData);
        }
        Ok(Self {
            data,
            validation: kind == 1,
        })
    }
}

/// A declared application action using a validated internal POST route.
#[derive(Clone, Debug)]
pub struct ActionRequest {
    action: u32,
    url: InternalUrl,
    body: String,
}
impl ActionRequest {
    /// Validates the route and serializes unique URL-encoded fields.
    pub fn new(action: u32, url: InternalUrl, fields: FormData) -> Result<Self, ActionError> {
        if action == 0 || url.as_str().len() > 1024 || url.as_str().contains('?') {
            return Err(ActionError::InvalidRoute);
        }
        let body = fields.encode().map_err(|_| ActionError::InvalidData)?;
        if body.len() > MAX_ACTION_REQUEST_BYTES {
            return Err(ActionError::Limit);
        }
        Ok(Self { action, url, body })
    }
    /// Application-owned action ID.
    pub fn action(&self) -> u32 {
        self.action
    }
    /// Validated internal destination.
    pub fn url(&self) -> &InternalUrl {
        &self.url
    }
    /// Encoded UTF-8 form fields.
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// A transport failure. A timeout does not roll back a server mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ActionFailure {
    /// The connection failed.
    Network = 1,
    /// The fetch or response read exceeded its deadline.
    Timeout = 2,
    /// The MIME type, frame or application response is invalid.
    Protocol = 3,
    /// The server returned an unexpected HTTP status.
    Rejected = 4,
}
/// The completion delivered to the owning Rust component.
pub type ActionOutcome = Result<ActionReply, ActionFailure>;
