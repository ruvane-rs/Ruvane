use std::fmt;

/// An invalid attachment display name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadNameError;
impl fmt::Display for DownloadNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid download name")
    }
}
impl std::error::Error for DownloadNameError {}

/// A bounded display name encoded only in a fixed attachment header.
#[derive(Clone, Eq, PartialEq)]
pub struct DownloadName(String);
impl DownloadName {
    /// Accepts 1..=255 UTF-8 bytes without controls, path separators, colon or dot names.
    pub fn new(value: &str) -> Result<Self, DownloadNameError> {
        if value.is_empty()
            || value.len() > crate::MAX_UPLOAD_NAME_BYTES
            || matches!(value, "." | "..")
            || value
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        {
            return Err(DownloadNameError);
        }
        Ok(Self(value.into()))
    }
    /// Returns the untrusted display name, never a filesystem path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(crate) fn header(&self) -> String {
        use std::fmt::Write;
        let mut result = String::from(
            "Content-Disposition: attachment; filename=\"download\"; filename*=UTF-8''",
        );
        for byte in self.0.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
                result.push(char::from(byte));
            } else {
                write!(result, "%{byte:02X}").unwrap();
            }
        }
        result.push_str("\r\n");
        result
    }
}
impl fmt::Debug for DownloadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DownloadName([redacted])")
    }
}

impl crate::Response {
    /// Returns opaque bytes as a no-store attachment with scripts blocked.
    /// This does not authorize access; the handler must check the caller first.
    pub fn download(bytes: impl Into<Vec<u8>>, name: DownloadName) -> Self {
        let mut response = Self::bytes(bytes, crate::MediaType::OctetStream);
        response.download = Some(name);
        response
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_names_are_bounded_and_cannot_inject_header_lines() {
        for name in [
            "",
            ".",
            "..",
            "../secret",
            "a\\b",
            "C:x",
            "x\r\nX: y",
            "\0",
            &"x".repeat(256),
        ] {
            assert!(DownloadName::new(name).is_err());
        }
        let name = DownloadName::new("PRIVATE 🦀 \"'.bin").unwrap();
        let header = name.header();
        assert!(header.contains("%F0%9F%A6%80%20%22%27.bin"));
        assert_eq!(header.matches("\r\n").count(), 1);
        assert!(!format!("{name:?}").contains("PRIVATE"));
        assert!(DownloadName::new(&"x".repeat(255)).is_ok());
    }
    #[test]
    fn downloads_keep_binary_bytes_opaque_and_redact_debug() {
        let response = crate::Response::download(
            b"PRIVATE\0\xff".to_vec(),
            DownloadName::new("PRIVATE.html").unwrap(),
        );
        assert_eq!(response.body(), b"PRIVATE\0\xff");
        assert_eq!(response.content_type(), "application/octet-stream");
        let headers = response.wire_headers("GET, HEAD");
        assert!(headers.contains("Content-Disposition: attachment;"));
        assert!(headers.contains("Cache-Control: no-store\r\n"));
        assert!(headers.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(
            response
                .content_security_policy()
                .contains("script-src 'none'")
        );
        assert!(!format!("{response:?}").contains("PRIVATE"));
        assert!(
            !crate::Response::text("ordinary")
                .wire_headers("GET")
                .contains("Content-Disposition")
        );
    }
}
