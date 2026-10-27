//! Bounded upload identities and private operation results.
use crate::{FileInfo, FileMetadataError, JsonValue};
use std::fmt;
/// Lifetime of one upload identity and its retained result, in seconds.
pub const FILE_UPLOAD_RETENTION_SECONDS: u64 = 3600;
/// Maximum retained upload operations in one store.
pub const MAX_FILE_UPLOAD_OPERATIONS: usize = 256;
/// Maximum retained upload operations for one owner.
pub const MAX_OWNER_FILE_UPLOAD_OPERATIONS: usize = 32;
/// One upload attempt. It is scoped to an authorized owner and grants no access.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FileUploadId {
    pub(crate) issued: u64,
    pub(crate) random: [u8; 32],
}
impl FileUploadId {
    /// Parses exactly 80 lowercase hexadecimal digits: issue time and random identity.
    pub fn parse(value: &str) -> Result<Self, FileMetadataError> {
        if value.len() != 80
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(FileMetadataError);
        }
        let issued = u64::from_str_radix(&value[..16], 16).map_err(|_| FileMetadataError)?;
        let random = crate::FileId::parse(&value[16..])?.0;
        if issued == 0 || issued.checked_add(FILE_UPLOAD_RETENTION_SECONDS).is_none() {
            return Err(FileMetadataError);
        }
        Ok(Self { issued, random })
    }
    /// Fixed Unix-second expiry. This is a lifetime bound, not an authentication claim.
    pub fn expires_at(self) -> u64 {
        self.issued + FILE_UPLOAD_RETENTION_SECONDS
    }
}
impl std::str::FromStr for FileUploadId {
    type Err = FileMetadataError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl fmt::Display for FileUploadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.issued)?;
        for byte in self.random {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl fmt::Debug for FileUploadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FileUploadId([redacted])")
    }
}
/// What the server can currently confirm about an owner's upload attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileUploadState {
    /// No committed result is recorded. An earlier request may still be running.
    Unconfirmed,
    /// The file and this operation were published together.
    Stored(FileInfo),
    /// The operation committed, then its file was deleted. Replays cannot recreate it.
    Removed,
    /// The identity expired, is unavailable or belongs to another owner. It cannot be submitted.
    Expired,
}
/// An exact bounded operation result, without owner keys, paths or payload bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileUploadStatus {
    operation: FileUploadId,
    state: FileUploadState,
}
impl FileUploadStatus {
    /// Constructs result metadata. Authorization belongs to the native handler.
    pub fn new(operation: FileUploadId, state: FileUploadState) -> Self {
        Self { operation, state }
    }
    /// Identity of the operation being inspected.
    pub fn operation(&self) -> FileUploadId {
        self.operation
    }
    /// Current recorded state. Unconfirmed does not prove non-execution.
    pub fn state(&self) -> &FileUploadState {
        &self.state
    }
    /// Encodes exactly operation/state/file.
    pub fn to_json(&self) -> JsonValue {
        let state = match self.state {
            FileUploadState::Unconfirmed => "unconfirmed",
            FileUploadState::Stored(_) => "stored",
            FileUploadState::Removed => "removed",
            FileUploadState::Expired => "expired",
        };
        JsonValue::object(vec![
            (
                "operation".into(),
                JsonValue::string(self.operation.to_string()).unwrap(),
            ),
            ("state".into(), JsonValue::string(state).unwrap()),
            (
                "file".into(),
                match &self.state {
                    FileUploadState::Stored(info) => info.to_json(),
                    _ => JsonValue::null(),
                },
            ),
        ])
        .unwrap()
    }
    /// Validates the complete response without coercion or unknown members.
    pub fn from_json(value: &JsonValue) -> Result<Self, FileMetadataError> {
        let object = value.as_object().ok_or(FileMetadataError)?;
        if object.len() != 3
            || object
                .iter()
                .any(|(name, _)| !matches!(name.as_str(), "operation" | "state" | "file"))
        {
            return Err(FileMetadataError);
        }
        let operation = FileUploadId::parse(
            value
                .get("operation")
                .and_then(JsonValue::as_str)
                .ok_or(FileMetadataError)?,
        )?;
        let file = value.get("file").ok_or(FileMetadataError)?;
        let state = match value.get("state").and_then(JsonValue::as_str) {
            Some("stored") => FileUploadState::Stored(FileInfo::from_json(file)?),
            Some(state) if file.is_null() => match state {
                "unconfirmed" => FileUploadState::Unconfirmed,
                "removed" => FileUploadState::Removed,
                "expired" => FileUploadState::Expired,
                _ => return Err(FileMetadataError),
            },
            _ => return Err(FileMetadataError),
        };
        Ok(Self { operation, state })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_ids_and_statuses_are_exact_bounded_and_redacted() {
        let text = format!("{:016x}{}", 2_000_000_000u64, "a1".repeat(32));
        let operation = FileUploadId::parse(&text).unwrap();
        assert_eq!(operation.to_string(), text);
        assert_eq!(operation.expires_at(), 2_000_003_600);
        assert!(!format!("{operation:?}").contains("a1"));
        for invalid in [
            text.to_uppercase(),
            "0".repeat(80),
            format!("ffffffffffffffff{}", "a1".repeat(32)),
            format!("{:016x}{}", 1, "0".repeat(64)),
            text[..79].into(),
        ] {
            assert!(FileUploadId::parse(&invalid).is_err());
        }
        let info = FileInfo::new(
            crate::FileId::parse(&"01".repeat(32)).unwrap(),
            "Rust <&.bin",
            3,
        )
        .unwrap();
        for state in [
            FileUploadState::Unconfirmed,
            FileUploadState::Stored(info),
            FileUploadState::Removed,
            FileUploadState::Expired,
        ] {
            let status = FileUploadStatus::new(operation, state);
            assert_eq!(
                FileUploadStatus::from_json(&status.to_json()).unwrap(),
                status
            );
        }
        for body in [
            format!(r#"{{"operation":"{text}","state":"stored","file":null}}"#),
            format!(r#"{{"operation":"{text}","state":"unknown","file":null}}"#),
            format!(
                r#"{{"operation":"{text}","state":"unconfirmed","file":null,"owner":"PRIVATE"}}"#
            ),
        ] {
            assert!(
                FileUploadStatus::from_json(&JsonValue::parse(body.as_bytes()).unwrap()).is_err()
            );
        }
    }
}
