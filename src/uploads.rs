//! Component-owned browser file selections and bounded multipart requests.
use crate::{ActionError, FormData, InternalUrl};
use std::fmt;

/// Maximum bytes in one file-selection event, including its metadata.
pub const MAX_FILE_EVENT_BYTES: usize = 400;

/// A fixed local selection failure, without filenames or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileSelectionError {
    /// The selected file exceeds the upload budget.
    TooLarge,
    /// The selection's count, name or metadata is outside the supported profile.
    InvalidMetadata,
    /// The browser could not provide the selected file.
    Unavailable,
}
impl FileSelectionError {
    /// A fixed message suitable for plain-text rendering.
    pub fn message(self) -> &'static str {
        match self {
            Self::TooLarge => "Select a file of at most 32 KiB.",
            Self::InvalidMetadata => "Select one file with a supported name.",
            Self::Unavailable => "The selected file is unavailable. Select it again.",
        }
    }
}
/// An immutable reference to one browser-selected file, scoped to its live control.
/// It contains metadata, never file bytes or an operating-system path.
#[derive(Clone, Eq, PartialEq)]
pub struct BrowserFile {
    pub(crate) view: u32,
    pub(crate) node: u32,
    pub(crate) epoch: u32,
    pub(crate) token: u32,
    name: String,
    size: u32,
    media_type: String,
}
impl BrowserFile {
    /// Untrusted display name, never a storage path.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Selected payload size, including zero for a selected empty file.
    pub fn size(&self) -> u32 {
        self.size
    }
    /// Untrusted browser MIME hint. Upload transport uses application/octet-stream.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    /// Numeric selection identity used by the first-party runtime.
    #[doc(hidden)]
    pub fn identity(&self) -> u32 {
        self.token
    }
    /// Checks the control that supplied this reference.
    #[doc(hidden)]
    pub fn is_bound_to(&self, view: u32, node: u32, epoch: u32) -> bool {
        self.view == view && self.node == node && self.epoch == epoch
    }
    /// Checks a request against its current live selection and view snapshot.
    #[doc(hidden)]
    pub fn is_current(
        &self,
        view: u32,
        snapshot: &crate::ViewSnapshot,
        current: Option<&Self>,
    ) -> bool {
        current == Some(self)
            && self.view == view
            && snapshot.file_epoch(self.node) == Some(self.epoch)
    }
}
impl fmt::Debug for BrowserFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserFile")
            .field("bytes", &self.size)
            .finish_non_exhaustive()
    }
}
/// A local file-control change. A rejected or cleared selection retires the old reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileSelection {
    /// No file is selected.
    Empty,
    /// One selected file with bounded metadata.
    Selected(BrowserFile),
    /// The browser rejected a selection locally, before any upload.
    Rejected(FileSelectionError),
}
impl FileSelection {
    /// Decodes bounded RUF1 metadata for an already-validated live control.
    #[doc(hidden)]
    pub fn decode(view: u32, node: u32, epoch: u32, bytes: &[u8]) -> Result<Self, ActionError> {
        let invalid = ActionError::InvalidData;
        if view == 0
            || node == 0
            || bytes.len() < 5
            || bytes.len() > MAX_FILE_EVENT_BYTES
            || &bytes[..4] != b"RUF1"
        {
            return Err(invalid);
        }
        if bytes[4] != 1 {
            if bytes.len() != 5 {
                return Err(invalid);
            }
            return match bytes[4] {
                0 => Ok(Self::Empty),
                2 => Ok(Self::Rejected(FileSelectionError::TooLarge)),
                3 => Ok(Self::Rejected(FileSelectionError::InvalidMetadata)),
                4 => Ok(Self::Rejected(FileSelectionError::Unavailable)),
                _ => Err(invalid),
            };
        }
        if bytes.len() < 17 {
            return Err(invalid);
        }
        let token = u32::from_le_bytes(bytes[5..9].try_into().map_err(|_| invalid)?);
        let size = u32::from_le_bytes(bytes[9..13].try_into().map_err(|_| invalid)?);
        let length = u16::from_le_bytes(bytes[13..15].try_into().map_err(|_| invalid)?) as usize;
        if token == 0
            || size as usize > crate::MAX_UPLOAD_BYTES
            || length == 0
            || length > crate::MAX_UPLOAD_NAME_BYTES
            || 17 + length > bytes.len()
        {
            return Err(invalid);
        }
        let name = std::str::from_utf8(&bytes[15..15 + length]).map_err(|_| invalid)?;
        if matches!(name, "." | "..")
            || name
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        {
            return Err(invalid);
        }
        let mime_at = 15 + length;
        let mime_length = u16::from_le_bytes(
            bytes[mime_at..mime_at + 2]
                .try_into()
                .map_err(|_| invalid)?,
        ) as usize;
        if mime_length > 127 || mime_at + 2 + mime_length != bytes.len() {
            return Err(invalid);
        }
        let mime = &bytes[mime_at + 2..];
        if !mime.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
            return Err(invalid);
        }
        Ok(Self::Selected(BrowserFile {
            view,
            node,
            epoch,
            token,
            name: name.into(),
            size,
            media_type: std::str::from_utf8(mime).map_err(|_| invalid)?.into(),
        }))
    }
}
/// One explicit same-origin multipart POST, with a selected file and business fields.
#[derive(Clone)]
pub struct UploadRequest {
    action: u32,
    url: InternalUrl,
    field: String,
    file: BrowserFile,
    fields: String,
}
impl UploadRequest {
    /// Builds a bounded request. The runtime checks the reference's live owner before queuing it.
    pub fn new(
        action: u32,
        url: InternalUrl,
        field: &str,
        file: BrowserFile,
        fields: FormData,
    ) -> Result<Self, ActionError> {
        if action == 0 || url.as_str().len() > 1024 {
            return Err(ActionError::InvalidRoute);
        }
        crate::forms::valid_name(field).map_err(|_| ActionError::InvalidData)?;
        if field == crate::CSRF_FORM_FIELD || fields.get(field).is_some() {
            return Err(ActionError::InvalidData);
        }
        if fields.len() > crate::MAX_MULTIPART_FIELDS {
            return Err(ActionError::Limit);
        }
        let fields = fields.encode().map_err(|_| ActionError::InvalidData)?;
        if fields.len() > crate::MAX_ACTION_REQUEST_BYTES {
            return Err(ActionError::Limit);
        }
        Ok(Self {
            action,
            url,
            field: field.into(),
            file,
            fields,
        })
    }
    /// Application action identifier, returned on completion.
    pub fn action(&self) -> u32 {
        self.action
    }
    /// Validated internal destination.
    pub fn url(&self) -> &InternalUrl {
        &self.url
    }
    /// Declared multipart file field.
    pub fn field(&self) -> &str {
        &self.field
    }
    /// Reference to the selected file, without its bytes.
    pub fn file(&self) -> &BrowserFile {
        &self.file
    }
    /// Encoded bounded business fields, separate from the CSRF header.
    pub fn fields(&self) -> &str {
        &self.fields
    }
}
impl fmt::Debug for UploadRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadRequest")
            .field("action", &self.action)
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}
/// Upload HTTP and transport results use the same bounded JSON response profile.
pub type UploadOutcome = crate::JsonOutcome;
#[cfg(test)]
mod tests;
