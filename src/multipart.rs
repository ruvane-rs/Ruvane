//! A bounded native multipart form profile.
use crate::{FormData, FormError};
use std::{fmt, sync::Arc};

/// Maximum bytes in a complete multipart request, including framing.
pub const MAX_MULTIPART_BYTES: usize = 64 * 1024;
/// Maximum bytes in the single file part.
pub const MAX_UPLOAD_BYTES: usize = 32 * 1024;
/// Maximum business text fields, excluding the reserved CSRF claim.
pub const MAX_MULTIPART_FIELDS: usize = 16;
/// Maximum parts, including a file and a reserved CSRF claim.
pub const MAX_MULTIPART_PARTS: usize = MAX_MULTIPART_FIELDS + 2;
/// Maximum bytes in a boundary value.
pub const MAX_MULTIPART_BOUNDARY_BYTES: usize = 70;
/// Maximum bytes in one part's headers, including the terminating blank line.
pub const MAX_MULTIPART_HEADER_BYTES: usize = 2048;
/// Maximum UTF-8 bytes in a supplied filename.
pub const MAX_UPLOAD_NAME_BYTES: usize = 255;

/// Multipart failures contain no submitted data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MultipartError {
    /// Unsupported media type, boundary or parameter syntax.
    InvalidContentType,
    /// Malformed framing, headers, field names or values.
    InvalidData,
    /// A byte, field, file or part budget was exceeded.
    Limit,
    /// Repeated names or headers, including text/file name collisions.
    Duplicate,
    /// A filename contains a path, control character or unsupported value.
    InvalidFilename,
}
impl fmt::Display for MultipartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidContentType => "invalid multipart content type",
            Self::InvalidData => "invalid multipart data",
            Self::Limit => "multipart limit exceeded",
            Self::Duplicate => "duplicate multipart declaration",
            Self::InvalidFilename => "invalid upload filename",
        })
    }
}
impl std::error::Error for MultipartError {}

/// Opaque untrusted file bytes. The supplied name and MIME type do not authorize storage or execution.
#[derive(Clone, Eq, PartialEq)]
pub struct UploadedFile {
    field: String,
    filename: String,
    content_type: String,
    bytes: Arc<[u8]>,
}
impl UploadedFile {
    /// Exact declared form field name.
    pub fn field_name(&self) -> &str {
        &self.field
    }
    /// Untrusted UTF-8 display name. Never use it as a storage path or response header.
    /// An empty name with empty bytes represents an unselected browser file control.
    pub fn file_name(&self) -> &str {
        &self.filename
    }
    /// Untrusted lowercased MIME declaration, defaulting to application/octet-stream.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }
    /// Borrows the original binary bytes without text conversion or normalization.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// File length, including for a selected empty file.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    /// Whether the supplied file contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}
impl fmt::Debug for UploadedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadedFile")
            .field("bytes", &self.len())
            .finish_non_exhaustive()
    }
}

/// Unique UTF-8 text fields and at most one binary file, with a separate CSRF claim.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct MultipartData {
    fields: FormData,
    file: Option<UploadedFile>,
}
impl fmt::Debug for MultipartData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultipartData")
            .field("fields", &self.fields.len())
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}
impl MultipartData {
    /// Business text fields. CRLF becomes LF; percent escapes and plus signs stay literal.
    /// The reserved CSRF claim is omitted from business accessors.
    pub fn fields(&self) -> &FormData {
        &self.fields
    }
    /// The single file part, if present. An unselected browser control may have an empty name.
    pub fn file(&self) -> Option<&UploadedFile> {
        self.file.as_ref()
    }
    /// Parses a complete body under the fixed native multipart limits.
    /// No callbacks, filesystem operations, MIME sniffing or file decoding occur here.
    pub fn parse(content_type: &str, bytes: &[u8]) -> Result<Self, MultipartError> {
        if bytes.len() > MAX_MULTIPART_BYTES {
            return Err(MultipartError::Limit);
        }
        let boundary = boundary(content_type)?;
        let opening = format!("--{boundary}");
        let marker = format!("\r\n--{boundary}");
        let mut rest = bytes
            .strip_prefix(opening.as_bytes())
            .ok_or(MultipartError::InvalidData)?;
        let mut data = Self::default();
        let mut names = Vec::new();
        loop {
            if let Some(tail) = rest.strip_prefix(b"--") {
                return if tail.is_empty() || tail == b"\r\n" {
                    Ok(data)
                } else {
                    Err(MultipartError::InvalidData)
                };
            }
            rest = rest
                .strip_prefix(b"\r\n")
                .ok_or(MultipartError::InvalidData)?;
            if names.len() == MAX_MULTIPART_PARTS {
                return Err(MultipartError::Limit);
            }
            let header_end = rest
                .windows(4)
                .take(MAX_MULTIPART_HEADER_BYTES)
                .position(|part| part == b"\r\n\r\n")
                .ok_or({
                    if rest.len() >= MAX_MULTIPART_HEADER_BYTES {
                        MultipartError::Limit
                    } else {
                        MultipartError::InvalidData
                    }
                })?;
            if header_end + 4 > MAX_MULTIPART_HEADER_BYTES {
                return Err(MultipartError::Limit);
            }
            let (name, filename, mime) = headers(&rest[..header_end])?;
            if names.contains(&name) {
                return Err(MultipartError::Duplicate);
            }
            names.push(name.clone());
            rest = &rest[header_end + 4..];
            let end = rest
                .windows(marker.len())
                .enumerate()
                .find_map(|(index, candidate)| {
                    if candidate != marker.as_bytes() {
                        return None;
                    }
                    let suffix = &rest[index + marker.len()..];
                    (suffix.starts_with(b"\r\n")
                        || suffix == b"--"
                        || suffix.starts_with(b"--\r\n"))
                    .then_some(index)
                })
                .ok_or(MultipartError::InvalidData)?;
            let body = &rest[..end];
            match filename {
                Some(filename) => {
                    if data.file.is_some() {
                        return Err(MultipartError::Limit);
                    }
                    if name == crate::CSRF_FORM_FIELD {
                        return Err(MultipartError::InvalidData);
                    }
                    if body.len() > MAX_UPLOAD_BYTES {
                        return Err(MultipartError::Limit);
                    }
                    if filename.is_empty() && !body.is_empty() {
                        return Err(MultipartError::InvalidFilename);
                    }
                    data.file = Some(UploadedFile {
                        field: name,
                        filename,
                        content_type: file_type(mime.as_deref())?,
                        bytes: Arc::from(body),
                    });
                }
                None => {
                    if name != crate::CSRF_FORM_FIELD && data.fields.len() == MAX_MULTIPART_FIELDS {
                        return Err(MultipartError::Limit);
                    }
                    if let Some(mime) = mime {
                        text_type(&mime)?;
                    }
                    data.fields
                        .insert_multipart(&name, body)
                        .map_err(form_error)?;
                }
            }
            rest = &rest[end + marker.len()..];
        }
    }
}
fn form_error(error: FormError) -> MultipartError {
    match error {
        FormError::TooLarge | FormError::TooManyFields => MultipartError::Limit,
        FormError::DuplicateName => MultipartError::Duplicate,
        _ => MultipartError::InvalidData,
    }
}
fn token(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(b))
}
pub(crate) fn boundary(content_type: &str) -> Result<&str, MultipartError> {
    let invalid = MultipartError::InvalidContentType;
    if content_type.len() > 256 {
        return Err(invalid);
    }
    let mut parts = content_type.split(';');
    if !parts
        .next()
        .unwrap_or("")
        .trim_matches([' ', '\t'])
        .eq_ignore_ascii_case("multipart/form-data")
    {
        return Err(invalid);
    }
    let (name, raw) = parts
        .next()
        .and_then(|part| part.trim_matches([' ', '\t']).split_once('='))
        .ok_or(invalid)?;
    if parts.next().is_some()
        || !name
            .trim_matches([' ', '\t'])
            .eq_ignore_ascii_case("boundary")
    {
        return Err(invalid);
    }
    let raw = raw.trim_matches([' ', '\t']);
    let value = if raw.starts_with('"') {
        raw.strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(invalid)?
    } else {
        if !token(raw.as_bytes()) {
            return Err(invalid);
        }
        raw
    };
    if value.is_empty()
        || value.len() > MAX_MULTIPART_BOUNDARY_BYTES
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"'()+_,-./:=?".contains(&b))
    {
        return Err(invalid);
    }
    Ok(value)
}
fn headers(bytes: &[u8]) -> Result<(String, Option<String>, Option<String>), MultipartError> {
    let raw = std::str::from_utf8(bytes).map_err(|_| MultipartError::InvalidData)?;
    let mut disposition = None;
    let mut mime = None;
    for line in raw.split("\r\n") {
        if line.starts_with([' ', '\t']) || line.chars().any(|c| c.is_control() && c != '\t') {
            return Err(MultipartError::InvalidData);
        }
        let (name, value) = line.split_once(':').ok_or(MultipartError::InvalidData)?;
        if !token(name.as_bytes()) {
            return Err(MultipartError::InvalidData);
        }
        let slot = if name.eq_ignore_ascii_case("content-disposition") {
            &mut disposition
        } else if name.eq_ignore_ascii_case("content-type") {
            &mut mime
        } else {
            return Err(MultipartError::InvalidData);
        };
        if slot.is_some() {
            return Err(MultipartError::Duplicate);
        }
        *slot = Some(value.trim_matches([' ', '\t']).to_owned());
    }
    let (name, filename) =
        disposition_value(disposition.as_deref().ok_or(MultipartError::InvalidData)?)?;
    crate::forms::valid_name(&name).map_err(form_error)?;
    if let Some(filename) = &filename {
        if filename.len() > MAX_UPLOAD_NAME_BYTES {
            return Err(MultipartError::Limit);
        }
        if matches!(filename.as_str(), "." | "..")
            || filename
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        {
            return Err(MultipartError::InvalidFilename);
        }
    }
    Ok((name, filename, mime))
}
fn disposition_value(value: &str) -> Result<(String, Option<String>), MultipartError> {
    let invalid = MultipartError::InvalidData;
    let (kind, mut rest) = value.split_once(';').ok_or(invalid)?;
    if !kind
        .trim_matches([' ', '\t'])
        .eq_ignore_ascii_case("form-data")
    {
        return Err(invalid);
    }
    let mut name = None;
    let mut filename = None;
    loop {
        let (key, tail) = rest
            .trim_start_matches([' ', '\t'])
            .split_once('=')
            .ok_or(invalid)?;
        let key = key.trim_matches([' ', '\t']);
        if !token(key.as_bytes()) {
            return Err(invalid);
        }
        let tail = tail.trim_start_matches([' ', '\t']);
        let (decoded, after) = if let Some(tail) = tail.strip_prefix('"') {
            let mut output = Vec::new();
            let mut input = tail.bytes().enumerate();
            let end = loop {
                let (i, b) = input.next().ok_or(invalid)?;
                if b == b'"' {
                    break i + 1;
                }
                if b == b'\\' {
                    let (_, escaped) = input.next().ok_or(invalid)?;
                    if !matches!(escaped, b'"' | b'\\') {
                        return Err(invalid);
                    }
                    output.push(escaped);
                } else {
                    output.push(b);
                }
            };
            (
                String::from_utf8(output).map_err(|_| invalid)?,
                &tail[end..],
            )
        } else {
            let end = tail.find(';').unwrap_or(tail.len());
            let value = tail[..end].trim_end_matches([' ', '\t']);
            let after = &tail[end..];
            if !token(value.as_bytes()) {
                return Err(invalid);
            }
            (value.to_owned(), after)
        };
        if decoded.chars().any(char::is_control) {
            return Err(invalid);
        }
        let slot = if key.eq_ignore_ascii_case("name") {
            &mut name
        } else if key.eq_ignore_ascii_case("filename") {
            &mut filename
        } else {
            return Err(invalid);
        };
        if slot.is_some() {
            return Err(MultipartError::Duplicate);
        }
        *slot = Some(decoded);
        let after = after.trim_start_matches([' ', '\t']);
        if after.is_empty() {
            break;
        }
        rest = after.strip_prefix(';').ok_or(invalid)?;
    }
    Ok((name.ok_or(invalid)?, filename))
}
fn file_type(mime: Option<&str>) -> Result<String, MultipartError> {
    let mime = mime.unwrap_or("application/octet-stream");
    let (kind, subtype) = mime.split_once('/').ok_or(MultipartError::InvalidData)?;
    if mime.len() > 127
        || !token(kind.as_bytes())
        || !token(subtype.as_bytes())
        || kind.eq_ignore_ascii_case("multipart")
    {
        return Err(MultipartError::InvalidData);
    }
    Ok(mime.to_ascii_lowercase())
}
fn text_type(mime: &str) -> Result<(), MultipartError> {
    let mut parts = mime.split(';');
    if !parts
        .next()
        .unwrap_or("")
        .trim_matches([' ', '\t'])
        .eq_ignore_ascii_case("text/plain")
    {
        return Err(MultipartError::InvalidData);
    }
    if let Some(charset) = parts.next() {
        let (name, value) = charset
            .trim_matches([' ', '\t'])
            .split_once('=')
            .ok_or(MultipartError::InvalidData)?;
        if !name
            .trim_matches([' ', '\t'])
            .eq_ignore_ascii_case("charset")
            || !(["utf-8", "\"utf-8\""].iter().any(|expected| {
                value
                    .trim_matches([' ', '\t'])
                    .eq_ignore_ascii_case(expected)
            }))
            || parts.next().is_some()
        {
            return Err(MultipartError::InvalidData);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
