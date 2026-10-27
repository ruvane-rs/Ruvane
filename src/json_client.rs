//! Bounded JSON requests and browser transport results.
use crate::{ActionError, ActionFailure, EntityTag, InternalUrl, JsonValue};
use std::fmt;

/// Maximum encoded bytes in a component JSON request.
pub const MAX_JSON_REQUEST_BYTES: usize = 16 * 1024;
/// Maximum bytes in a JSON transport frame, including metadata.
pub const MAX_JSON_REPLY_BYTES: usize = crate::MAX_JSON_BYTES + 142;

/// Methods supported by the component JSON client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum JsonMethod {
    /// Read a JSON resource, without a body.
    Get = 0,
    /// Create or invoke with a JSON body.
    Post = 1,
    /// Replace with a JSON body.
    Put = 2,
    /// Partially update with a JSON body.
    Patch = 3,
    /// Delete a resource, without a body.
    Delete = 4,
}
impl JsonMethod {
    /// Whether this request can change server state.
    pub fn is_mutation(self) -> bool {
        self != Self::Get
    }
}

/// One explicit request. Headers and credentials are controlled by the bridge.
#[derive(Clone)]
pub struct JsonRequest {
    action: u32,
    method: JsonMethod,
    url: InternalUrl,
    body: String,
    etag: Option<EntityTag>,
}
impl JsonRequest {
    /// Builds a request. GET and DELETE forbid bodies; other methods require JSON.
    pub fn new(
        action: u32,
        method: JsonMethod,
        url: InternalUrl,
        body: Option<JsonValue>,
    ) -> Result<Self, ActionError> {
        if action == 0 || url.as_str().len() > 1024 {
            return Err(ActionError::InvalidRoute);
        }
        if body.is_some()
            != matches!(
                method,
                JsonMethod::Post | JsonMethod::Put | JsonMethod::Patch
            )
        {
            return Err(ActionError::InvalidData);
        }
        let body = body.map(|value| value.encode()).unwrap_or_default();
        if body.len() > MAX_JSON_REQUEST_BYTES {
            return Err(ActionError::Limit);
        }
        Ok(Self {
            action,
            method,
            url,
            body,
            etag: None,
        })
    }
    /// Attaches the strong version retained with the edited snapshot. Never refresh it implicitly.
    pub fn with_if_match(mut self, tag: EntityTag) -> Result<Self, ActionError> {
        if tag.is_weak() {
            return Err(ActionError::InvalidData);
        }
        self.etag = Some(tag);
        Ok(self)
    }
    /// Application request identifier, returned to the component on completion.
    pub fn action(&self) -> u32 {
        self.action
    }
    /// Explicit HTTP method.
    pub fn method(&self) -> JsonMethod {
        self.method
    }
    /// Validated internal destination.
    pub fn url(&self) -> &InternalUrl {
        &self.url
    }
    /// Encoded JSON, or empty for bodyless methods.
    pub fn body(&self) -> &str {
        &self.body
    }
    /// Retained strong version, if supplied.
    pub fn if_match(&self) -> Option<&EntityTag> {
        self.etag.as_ref()
    }
}
impl fmt::Debug for JsonRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonRequest")
            .field("action", &self.action)
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

/// A complete HTTP JSON response. HTTP errors remain distinct from transport failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonReply {
    status: u16,
    value: JsonValue,
    json: bool,
    etag: Option<EntityTag>,
    retry_after: Option<u32>,
    uncertain: bool,
}
impl JsonReply {
    /// HTTP status, including validation 400, conflicts 412/428 and quota 429.
    pub fn status(&self) -> u16 {
        self.status
    }
    /// Whether the HTTP response reports success.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    /// Whether the response body was JSON. Non-JSON HTTP error bodies are discarded.
    pub fn has_json(&self) -> bool {
        self.json
    }
    /// Parsed JSON with exact numeric representations, or null for a discarded error body.
    pub fn value(&self) -> &JsonValue {
        &self.value
    }
    /// Validator supplied with this response. It is never copied to another request automatically.
    pub fn etag(&self) -> Option<&EntityTag> {
        self.etag.as_ref()
    }
    /// A bounded Retry-After delta in seconds. This never schedules a retry.
    pub fn retry_after(&self) -> Option<u32> {
        self.retry_after
    }
    /// Whether a submitted mutation may have committed despite this result.
    pub fn uncertain(&self) -> bool {
        self.uncertain
    }
    /// Converts a 400 report using trusted application field declarations.
    pub fn validation(
        &self,
        fields: &'static [&'static str],
    ) -> Result<crate::ValidationErrors, crate::JsonError> {
        if self.status != 400 || !self.json {
            return Err(crate::JsonError::InvalidSyntax);
        }
        crate::ValidationErrors::from_json(&self.value, fields)
    }
}

/// A transport failure. Cancellation cannot roll back a submitted mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JsonFailure {
    kind: ActionFailure,
    uncertain: bool,
    retry_after: Option<u32>,
}
impl JsonFailure {
    /// Transport category, including deadline, protocol and preflight rejection.
    pub fn kind(self) -> ActionFailure {
        self.kind
    }
    /// Whether the mutation may have committed. Resolve it explicitly before resubmission.
    pub fn uncertain(self) -> bool {
        self.uncertain
    }
    /// Confirmed preflight quota delay, without an automatic retry.
    pub fn retry_after(self) -> Option<u32> {
        self.retry_after
    }
    /// Constructs a protocol failure after an untrusted transport frame.
    pub fn protocol(mutation: bool) -> Self {
        Self {
            kind: ActionFailure::Protocol,
            uncertain: mutation,
            retry_after: None,
        }
    }
}
/// One HTTP response or transport failure, delivered only to its live owner.
pub type JsonOutcome = Result<JsonReply, JsonFailure>;

/// Decodes an RVJ1 transport envelope. The bridge supplies bytes, never parsed JSON numbers.
#[doc(hidden)]
pub fn decode_json_reply(bytes: &[u8], mutation: bool) -> JsonOutcome {
    decode(bytes, mutation).unwrap_or_else(|| Err(JsonFailure::protocol(mutation)))
}
fn decode(bytes: &[u8], mutation: bool) -> Option<JsonOutcome> {
    if bytes.len() < 14 || bytes.len() > MAX_JSON_REPLY_BYTES || &bytes[..4] != b"RVJ1" {
        return None;
    }
    let kind = bytes[4];
    let flags = bytes[5];
    let status = u16::from_le_bytes(bytes[6..8].try_into().ok()?);
    let retry = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let tag_len = u16::from_le_bytes(bytes[12..14].try_into().ok()?) as usize;
    if retry > crate::MAX_RETRY_AFTER_SECONDS
        || tag_len > crate::MAX_ENTITY_TAG_BYTES
        || 14 + tag_len > bytes.len()
    {
        return None;
    }
    let retry_after = (retry != 0).then_some(retry);
    if kind != 0 {
        if flags > 1
            || status != 0
            || tag_len != 0
            || bytes.len() != 14
            || (retry != 0 && kind != 5)
        {
            return None;
        }
        let kind = match kind {
            1 => ActionFailure::Network,
            2 => ActionFailure::Timeout,
            3 => ActionFailure::Protocol,
            4 => ActionFailure::Rejected,
            5 => ActionFailure::RateLimited,
            _ => return None,
        };
        return Some(Err(JsonFailure {
            kind,
            uncertain: mutation && flags == 1,
            retry_after,
        }));
    }
    if flags & !6 != 0
        || !((200..300).contains(&status) || (400..600).contains(&status))
        || (retry != 0 && status != 429)
        || (flags & 2 != 0 && (status != 429 || retry == 0))
        || (flags & 4 != 0 && (status < 400 || tag_len != 0 || bytes.len() != 14))
    {
        return None;
    }
    let etag = if tag_len == 0 {
        None
    } else {
        Some(EntityTag::parse(std::str::from_utf8(&bytes[14..14 + tag_len]).ok()?).ok()?)
    };
    let json = flags & 4 == 0;
    let value = if json {
        JsonValue::parse(&bytes[14 + tag_len..]).ok()?
    } else {
        JsonValue::null()
    };
    let rejected = matches!(
        status,
        400 | 401 | 403 | 404 | 405 | 409 | 412 | 413 | 415 | 422 | 428
    ) || flags & 2 != 0;
    Some(Ok(JsonReply {
        status,
        value,
        json,
        etag,
        retry_after,
        uncertain: mutation && status >= 300 && !rejected,
    }))
}

#[cfg(test)]
mod tests;
