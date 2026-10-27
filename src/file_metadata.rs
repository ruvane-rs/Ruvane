//! Shared bounded file metadata. It contains no owner keys, paths or file bytes.
use crate::{ActionData, ActionValue, DownloadName, JsonValue};
use std::fmt;
#[cfg(test)]
mod tests;
/// Invalid or out-of-profile file metadata, without submitted values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileMetadataError;
impl fmt::Display for FileMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid file metadata")
    }
}
impl std::error::Error for FileMetadataError {}
/// Maximum records in one metadata page or initial seed.
pub const MAX_FILE_LIST_ENTRIES: usize = 3;
/// An OS-random opaque identifier. Possession never grants access to a file.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FileId(pub(crate) [u8; 32]);
impl FileId {
    /// Parses exactly 64 lowercase hexadecimal digits, with no aliases or path syntax.
    pub fn parse(value: &str) -> Result<Self, FileMetadataError> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(FileMetadataError);
        }
        let mut bytes = [0; 32];
        for (index, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| FileMetadataError)?;
        }
        if bytes == [0; 32] {
            return Err(FileMetadataError);
        }
        Ok(Self(bytes))
    }
}
impl std::str::FromStr for FileId {
    type Err = FileMetadataError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}
impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl fmt::Debug for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FileId([redacted])")
    }
}
/// Bounded file metadata. The filename is display text, never a disk path.
/// This type does not authorize a read or mutation.
#[derive(Clone, Eq, PartialEq)]
pub struct FileInfo {
    pub(crate) id: FileId,
    pub(crate) name: DownloadName,
    pub(crate) size: usize,
}
impl FileInfo {
    /// Validates public metadata without accepting an owner, path or payload.
    pub fn new(id: FileId, name: &str, size: usize) -> Result<Self, FileMetadataError> {
        if size > crate::MAX_UPLOAD_BYTES {
            return Err(FileMetadataError);
        }
        Ok(Self {
            id,
            name: DownloadName::new(name).map_err(|_| FileMetadataError)?,
            size,
        })
    }
    /// Decodes exactly id, name and bytes, without type coercion or extra members.
    pub fn from_json(value: &JsonValue) -> Result<Self, FileMetadataError> {
        let mut object = crate::JsonObject::new(value, &["id", "name", "bytes"]);
        let id = object.required_string("id", crate::TextRules::new(64, 64).unwrap());
        let name =
            object.required_string("name", crate::TextRules::new(1, 255).unwrap().no_controls());
        let size = object.required_u64("bytes", 0, crate::MAX_UPLOAD_BYTES as u64);
        object.finish().map_err(|_| FileMetadataError)?;
        Self::new(
            FileId::parse(id.ok_or(FileMetadataError)?)?,
            name.ok_or(FileMetadataError)?,
            size.ok_or(FileMetadataError)? as usize,
        )
    }
    /// Encodes only the bounded public metadata fields.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::object(vec![
            ("id".into(), JsonValue::string(self.id.to_string()).unwrap()),
            ("name".into(), JsonValue::string(self.name()).unwrap()),
            ("bytes".into(), (self.size as u64).into()),
        ])
        .unwrap()
    }
    /// Returns the opaque identifier used by application routes.
    pub fn id(&self) -> FileId {
        self.id
    }
    /// Returns the untrusted original display name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
    /// Returns the exact binary payload length.
    pub fn len(&self) -> usize {
        self.size
    }
    /// Whether this is a selected zero-byte file.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

/// An ordered page of public file metadata and an optional continuation cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileListing {
    files: Vec<FileInfo>,
    next: Option<FileId>,
}
impl FileListing {
    /// Accepts at most three distinct ascending IDs. A continuation must equal the last ID of a full page.
    pub fn new(files: Vec<FileInfo>, next: Option<FileId>) -> Result<Self, FileMetadataError> {
        if files.len() > MAX_FILE_LIST_ENTRIES
            || files.windows(2).any(|pair| pair[0].id >= pair[1].id)
            || next.is_some_and(|cursor| {
                files.len() != MAX_FILE_LIST_ENTRIES
                    || files.last().map(FileInfo::id) != Some(cursor)
            })
        {
            return Err(FileMetadataError);
        }
        Ok(Self { files, next })
    }
    /// The current bounded page, without owner keys or storage paths.
    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }
    /// Cursor for an explicit subsequent GET.
    pub fn next(&self) -> Option<FileId> {
        self.next
    }
    /// Decodes exactly files and next, validating every record before returning any data.
    pub fn from_json(value: &JsonValue) -> Result<Self, FileMetadataError> {
        let object = value.as_object().ok_or(FileMetadataError)?;
        if object.len() != 2
            || object
                .iter()
                .any(|(key, _)| !matches!(key.as_str(), "files" | "next"))
        {
            return Err(FileMetadataError);
        }
        let array = value
            .get("files")
            .and_then(JsonValue::as_array)
            .filter(|array| array.len() <= MAX_FILE_LIST_ENTRIES)
            .ok_or(FileMetadataError)?;
        let files = array
            .iter()
            .map(FileInfo::from_json)
            .collect::<Result<Vec<_>, _>>()?;
        let next = value.get("next").ok_or(FileMetadataError)?;
        let next = if next.is_null() {
            None
        } else {
            Some(FileId::parse(next.as_str().ok_or(FileMetadataError)?)?)
        };
        Self::new(files, next)
    }
    /// Encodes a bounded metadata page, never file bytes.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::object(vec![
            (
                "files".into(),
                JsonValue::array(self.files.iter().map(FileInfo::to_json).collect()).unwrap(),
            ),
            (
                "next".into(),
                self.next.map_or(JsonValue::null(), |id| {
                    JsonValue::string(id.to_string()).unwrap()
                }),
            ),
        ])
        .unwrap()
    }
    /// Encodes flat typed fields for private initial page hydration.
    pub fn to_seed(&self) -> Result<ActionData, crate::ActionError> {
        let mut data = ActionData::default();
        data.insert("count", ActionValue::Number(self.files.len() as u64))?;
        data.insert(
            "next",
            ActionValue::Text(self.next.map_or(String::new(), |id| id.to_string())),
        )?;
        for (index, file) in self.files.iter().enumerate() {
            data.insert(
                &format!("id{index}"),
                ActionValue::Text(file.id.to_string()),
            )?;
            data.insert(
                &format!("name{index}"),
                ActionValue::Text(file.name().into()),
            )?;
            data.insert(
                &format!("bytes{index}"),
                ActionValue::Number(file.size as u64),
            )?;
        }
        Ok(data)
    }
    /// Validates the complete initial seed, including field types and unknown members.
    pub fn from_seed(data: &ActionData) -> Result<Self, FileMetadataError> {
        let count = data
            .number("count")
            .filter(|count| *count <= MAX_FILE_LIST_ENTRIES as u64)
            .ok_or(FileMetadataError)? as usize;
        if data.len() != 2 + count * 3 {
            return Err(FileMetadataError);
        }
        let next = match data.text("next").ok_or(FileMetadataError)? {
            "" => None,
            value => Some(FileId::parse(value)?),
        };
        let mut files = Vec::new();
        for index in 0..count {
            files.push(FileInfo::new(
                FileId::parse(data.text(&format!("id{index}")).ok_or(FileMetadataError)?)?,
                data.text(&format!("name{index}"))
                    .ok_or(FileMetadataError)?,
                data.number(&format!("bytes{index}"))
                    .filter(|size| *size <= crate::MAX_UPLOAD_BYTES as u64)
                    .ok_or(FileMetadataError)? as usize,
            )?);
        }
        Self::new(files, next)
    }
}
impl fmt::Debug for FileInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileInfo")
            .field("bytes", &self.size)
            .finish_non_exhaustive()
    }
}
