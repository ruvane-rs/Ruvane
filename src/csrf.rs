use std::fmt;

/// Random bytes in a session CSRF token.
pub const CSRF_TOKEN_BYTES: usize = 32;
/// ASCII bytes in the canonical token representation.
pub const CSRF_TOKEN_LENGTH: usize = CSRF_TOKEN_BYTES * 2;
/// Reserved native form field, separate from application fields.
pub const CSRF_FORM_FIELD: &str = "_ruvane_csrf";
/// Same-origin action and token-response header.
pub const CSRF_HEADER: &str = "Ruvane-Csrf-Token";
/// MIME type of an empty CSRF token response.
pub const CSRF_MEDIA_TYPE: &str = "application/vnd.ruvane.csrf";

/// A malformed or ambiguous submission. Errors never contain token values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CsrfError {
    /// The token is not 64 lowercase hexadecimal characters.
    Invalid,
    /// Both a header and a form field supplied a token.
    Ambiguous,
}
impl fmt::Display for CsrfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid CSRF token",
            Self::Ambiguous => "ambiguous CSRF token",
        })
    }
}
impl std::error::Error for CsrfError {}

/// Opaque CSRF claim. Parsing validates syntax, never session ownership.
/// Debug redacts the value; use `as_str` only for intentional transport.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct CsrfToken(String);
impl fmt::Debug for CsrfToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CsrfToken([redacted])")
    }
}
impl CsrfToken {
    /// Parses the canonical wire representation without authorizing it.
    pub fn parse(value: &str) -> Result<Self, CsrfError> {
        if value.len() != CSRF_TOKEN_LENGTH
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(CsrfError::Invalid);
        }
        Ok(Self(value.into()))
    }
    /// Exposes the sensitive value for a header or typed native form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn from_bytes(bytes: &[u8; CSRF_TOKEN_BYTES]) -> Self {
        use fmt::Write;
        let mut value = String::with_capacity(CSRF_TOKEN_LENGTH);
        for byte in bytes {
            write!(value, "{byte:02x}").unwrap();
        }
        Self(value)
    }
}
