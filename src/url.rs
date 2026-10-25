use std::fmt;
use std::str::FromStr;

/// Maximum UTF-8 bytes in a decoded path parameter, query key, or query value.
pub const MAX_COMPONENT_BYTES: usize = 1024;
/// Maximum number of query pairs, including pairs with empty values.
pub const MAX_QUERY_PAIRS: usize = 32;
/// Maximum number of captures in a dynamic route.
pub const MAX_PATH_PARAMS: usize = 16;
/// Maximum encoded bytes in an internal URL or a raw query string.
pub const MAX_INTERNAL_URL_BYTES: usize = 4096;

/// A URL validation error that never includes untrusted input in its message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum UrlError {
    /// A percent escape or raw URI character is invalid.
    InvalidEncoding,
    /// Decoded bytes are not a valid UTF-8 string.
    InvalidUtf8,
    /// A component is empty where required or contains forbidden characters.
    InvalidComponent,
    /// The path is outside the internal, origin-relative URL profile.
    InvalidPath,
    /// A query key occurs more than once after decoding.
    DuplicateKey,
    /// The query has more than the supported number of pairs.
    TooManyPairs,
    /// Encoded or decoded data exceeds a documented byte budget.
    TooLarge,
    /// A path segment was appended after query construction began.
    QueryAlreadyStarted,
}

impl fmt::Display for UrlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEncoding => "invalid URL encoding",
            Self::InvalidUtf8 => "invalid UTF-8 in URL",
            Self::InvalidComponent => "invalid URL component",
            Self::InvalidPath => "invalid internal URL path",
            Self::DuplicateKey => "duplicate query key",
            Self::TooManyPairs => "too many query pairs",
            Self::TooLarge => "URL size limit exceeded",
            Self::QueryAlreadyStarted => "cannot append a path segment after a query",
        })
    }
}

impl std::error::Error for UrlError {}

/// A missing value or failed conversion through [`FromStr`].
///
/// Conversions use the selected Rust type's syntax. Applications must also
/// validate domain-specific ranges. Neither input nor parser errors are exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParamError {
    /// No value exists for the requested name.
    Missing,
    /// The value could not be parsed into the selected type.
    Invalid,
}

impl fmt::Display for ParamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "missing parameter",
            Self::Invalid => "invalid parameter value",
        })
    }
}

impl std::error::Error for ParamError {}

pub(crate) fn parse_value<T: FromStr>(value: Option<&str>) -> Result<T, ParamError> {
    value
        .ok_or(ParamError::Missing)?
        .parse()
        .map_err(|_| ParamError::Invalid)
}

fn valid_text(value: &str) -> Result<(), UrlError> {
    if value.len() > MAX_COMPONENT_BYTES {
        return Err(UrlError::TooLarge);
    }
    if value.chars().any(char::is_control) {
        return Err(UrlError::InvalidComponent);
    }
    Ok(())
}

pub(crate) fn valid_segment(value: &str) -> Result<(), UrlError> {
    valid_text(value)?;
    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\']) {
        return Err(UrlError::InvalidComponent);
    }
    Ok(())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn decode(value: &str) -> Result<String, UrlError> {
    // At most three encoded bytes can contribute to one decoded byte.
    if value.len() > MAX_COMPONENT_BYTES * 3 {
        return Err(UrlError::TooLarge);
    }
    let mut output = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if output.len() == MAX_COMPONENT_BYTES {
            return Err(UrlError::TooLarge);
        }
        let byte = bytes[index];
        if byte == b'%' {
            let high = bytes
                .get(index + 1)
                .copied()
                .and_then(hex)
                .ok_or(UrlError::InvalidEncoding)?;
            let low = bytes
                .get(index + 2)
                .copied()
                .and_then(hex)
                .ok_or(UrlError::InvalidEncoding)?;
            output.push(high * 16 + low);
            index += 3;
        } else if byte.is_ascii() {
            // '+' is a literal plus, not the space convention used by forms.
            output.push(byte);
            index += 1;
        } else {
            return Err(UrlError::InvalidEncoding);
        }
    }
    let output = String::from_utf8(output).map_err(|_| UrlError::InvalidUtf8)?;
    valid_text(&output)?;
    Ok(output)
}

fn encode(value: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(DIGITS[usize::from(byte >> 4)]));
            output.push(char::from(DIGITS[usize::from(byte & 15)]));
        }
    }
    output
}

/// An ordered, decoded query with unique, case-sensitive keys.
///
/// Parsing splits on `&` and the first `=` before decoding UTF-8 exactly once.
/// A bare key has an empty value. Empty fields/keys and control characters are
/// rejected. `+` stays a plus. No Unicode normalization or case folding occurs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Query {
    pairs: Vec<(String, String)>,
}

impl Query {
    /// Parses a raw query string without a leading `?`, within the URL budgets.
    ///
    /// ```
    /// let query = ruvane::Query::parse("page=2&search=Rust%20%26%20HTML").unwrap();
    /// assert_eq!(query.get_as::<usize>("page"), Ok(2));
    /// assert_eq!(query.get("search"), Some("Rust & HTML"));
    /// ```
    pub fn parse(raw: &str) -> Result<Self, UrlError> {
        if raw.len() > MAX_INTERNAL_URL_BYTES {
            return Err(UrlError::TooLarge);
        }
        if !crate::http::uri_component(raw.as_bytes(), true) {
            return Err(UrlError::InvalidEncoding);
        }
        let mut query = Self::default();
        if raw.is_empty() {
            return Ok(query);
        }
        for field in raw.split('&') {
            if query.pairs.len() == MAX_QUERY_PAIRS {
                return Err(UrlError::TooManyPairs);
            }
            let (key, value) = field.split_once('=').unwrap_or((field, ""));
            let key = decode(key)?;
            let value = decode(value)?;
            if key.is_empty() {
                return Err(UrlError::InvalidComponent);
            }
            if query.pairs.iter().any(|(existing, _)| *existing == key) {
                return Err(UrlError::DuplicateKey);
            }
            query.pairs.push((key, value));
        }
        Ok(query)
    }

    /// Looks up a decoded value by its exact decoded key.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Converts a decoded value, distinguishing a missing key from invalid data.
    pub fn get_as<T: FromStr>(&self, name: &str) -> Result<T, ParamError> {
        parse_value(self.get(name))
    }

    /// Returns decoded pairs in their original order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns the number of pairs.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Reports whether the query has no pairs.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

/// A validated origin-relative URL for a link to the current site.
///
/// The path begins with one `/`. Schemes, authorities, fragments, queries in
/// the constructor, decoded separators, dot segments, invalid UTF-8, and
/// control characters are rejected. This is not a route-existence check.
/// Append unencoded values using [`Self::segment`] and [`Self::query`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct InternalUrl {
    encoded: String,
    query_keys: Vec<String>,
}

impl InternalUrl {
    /// Validates an already encoded internal path, without decoding it in place.
    ///
    /// Empty leading/trailing root segments are allowed; empty interior
    /// segments are not. Fixed paths retain their original percent encoding.
    pub fn new(path: &str) -> Result<Self, UrlError> {
        if path.len() > MAX_INTERNAL_URL_BYTES {
            return Err(UrlError::TooLarge);
        }
        if !crate::http::valid_path(path) {
            return Err(UrlError::InvalidPath);
        }
        for segment in path[1..].split('/').filter(|segment| !segment.is_empty()) {
            valid_segment(&decode(segment)?)?;
        }
        Ok(Self {
            encoded: path.to_owned(),
            query_keys: Vec::new(),
        })
    }

    /// Appends one unencoded, nonempty path segment before any query pairs.
    ///
    /// ```
    /// let url = ruvane::InternalUrl::new("/articles").unwrap()
    ///     .segment("été & Rust").unwrap();
    /// assert_eq!(url.as_str(), "/articles/%C3%A9t%C3%A9%20%26%20Rust");
    /// ```
    pub fn segment(mut self, value: &str) -> Result<Self, UrlError> {
        if !self.query_keys.is_empty() {
            return Err(UrlError::QueryAlreadyStarted);
        }
        valid_segment(value)?;
        let encoded = encode(value);
        let separator = usize::from(!self.encoded.ends_with('/'));
        if self.encoded.len() + separator + encoded.len() > MAX_INTERNAL_URL_BYTES {
            return Err(UrlError::TooLarge);
        }
        if separator == 1 {
            self.encoded.push('/');
        }
        self.encoded.push_str(&encoded);
        Ok(self)
    }

    /// Appends a unique, unencoded query key and value, encoding their data.
    pub fn query(mut self, key: &str, value: &str) -> Result<Self, UrlError> {
        valid_text(key)?;
        valid_text(value)?;
        if key.is_empty() {
            return Err(UrlError::InvalidComponent);
        }
        if self.query_keys.iter().any(|existing| existing == key) {
            return Err(UrlError::DuplicateKey);
        }
        if self.query_keys.len() == MAX_QUERY_PAIRS {
            return Err(UrlError::TooManyPairs);
        }
        let key_encoded = encode(key);
        let value_encoded = encode(value);
        if self.encoded.len() + key_encoded.len() + value_encoded.len() + 2 > MAX_INTERNAL_URL_BYTES
        {
            return Err(UrlError::TooLarge);
        }
        self.encoded
            .push(if self.query_keys.is_empty() { '?' } else { '&' });
        self.encoded.push_str(&key_encoded);
        self.encoded.push('=');
        self.encoded.push_str(&value_encoded);
        self.query_keys.push(key.to_owned());
        Ok(self)
    }

    /// Borrows the encoded URL. HTML attributes still require HTML escaping.
    pub fn as_str(&self) -> &str {
        &self.encoded
    }
}

impl fmt::Display for InternalUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AsRef<str> for InternalUrl {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
