use crate::InternalUrl;
use std::fmt;

/// Maximum bytes in a request Cookie header.
pub const MAX_COOKIE_HEADER_BYTES: usize = 4096;
/// Maximum cookie pairs accepted in one request.
pub const MAX_REQUEST_COOKIES: usize = 32;
/// Maximum bytes in a cookie name.
pub const MAX_COOKIE_NAME_BYTES: usize = 128;
/// Maximum bytes in a cookie value.
pub const MAX_COOKIE_VALUE_BYTES: usize = 2048;
/// Maximum bytes in a cookie path.
pub const MAX_COOKIE_PATH_BYTES: usize = 1024;
/// Maximum complete Set-Cookie line, including framing.
pub const MAX_SET_COOKIE_BYTES: usize = 4096;
/// Maximum Set-Cookie fields in one response.
pub const MAX_RESPONSE_COOKIES: usize = 8;
/// Maximum combined Set-Cookie bytes in one response.
pub const MAX_RESPONSE_COOKIE_BYTES: usize = 8192;
/// Maximum declared lifetime: one year. Zero requests removal.
pub const MAX_COOKIE_AGE_SECONDS: u32 = 365 * 24 * 60 * 60;

/// Cookie validation errors. Messages never contain cookie data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CookieError {
    /// The name is empty, too long or not an HTTP token.
    InvalidName,
    /// The value contains bytes outside the cookie-octet profile.
    InvalidValue,
    /// A request pair is malformed or a Cookie header is repeated.
    InvalidHeader,
    /// A cookie name repeats in a request, or name/path repeats in a response.
    DuplicateCookie,
    /// The path is not a bounded internal path without a query.
    InvalidPath,
    /// Attributes violate a prefix, SameSite or lifetime requirement.
    InvalidAttributes,
    /// The cookie count exceeds its request or response limit.
    TooManyCookies,
    /// The request header or response cookie bytes exceed a limit.
    TooLarge,
}
impl fmt::Display for CookieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidName => "invalid cookie name",
            Self::InvalidValue => "invalid cookie value",
            Self::InvalidHeader => "invalid Cookie header",
            Self::DuplicateCookie => "duplicate cookie",
            Self::InvalidPath => "invalid cookie path",
            Self::InvalidAttributes => "invalid cookie attributes",
            Self::TooManyCookies => "cookie count limit exceeded",
            Self::TooLarge => "cookie byte limit exceeded",
        })
    }
}
impl std::error::Error for CookieError {}
fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_COOKIE_NAME_BYTES
        && value.bytes().all(crate::http::token)
}
fn value(value: &str) -> bool {
    value.len() <= MAX_COOKIE_VALUE_BYTES
        && value.bytes().all(
            |byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e),
        )
}

/// Validated request cookies. Values are untrusted, case-sensitive and never URL-decoded.
/// Duplicate names are rejected regardless of their browser paths.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct Cookies(Vec<(String, String)>);
impl fmt::Debug for Cookies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookies")
            .field("count", &self.len())
            .finish()
    }
}
impl Cookies {
    /// Parses the bounded server Cookie profile. Outer quotes are removed once.
    /// An empty header is accepted; it still bypasses public caches on a Request.
    pub fn parse(header: &str) -> Result<Self, CookieError> {
        if header.len() > MAX_COOKIE_HEADER_BYTES {
            return Err(CookieError::TooLarge);
        }
        if header.is_empty() {
            return Ok(Self::default());
        }
        let mut result = Self::default();
        for pair in header.split(';') {
            if result.len() == MAX_REQUEST_COOKIES {
                return Err(CookieError::TooManyCookies);
            }
            let (key, raw) = pair
                .trim_matches([' ', '\t'])
                .split_once('=')
                .ok_or(CookieError::InvalidHeader)?;
            if !name(key) {
                return Err(CookieError::InvalidName);
            }
            let content = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                &raw[1..raw.len() - 1]
            } else {
                raw
            };
            if !value(content) {
                return Err(CookieError::InvalidValue);
            }
            if result.get(key).is_some() {
                return Err(CookieError::DuplicateCookie);
            }
            result.0.push((key.into(), content.into()));
        }
        Ok(result)
    }
    /// Looks up one case-sensitive name. Presence does not establish identity.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
    /// Number of validated pairs.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether no pairs were supplied.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Iterates pairs in request order. Values may contain sensitive application data.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// Controls cross-site sending. None requires Secure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SameSite {
    /// Send only in same-site contexts.
    Strict,
    /// Also allow safe top-level cross-site navigation.
    Lax,
    /// Allow cross-site contexts when the browser permits them.
    None,
}
impl SameSite {
    fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
}

/// One host-only response cookie, with no Domain, Expires or extension attributes.
/// Defaults to Secure, HttpOnly, SameSite=Lax, Path=/ and no explicit lifetime.
/// Values are not encrypted, signed or encoded by this type. Debug omits cookie data.
#[derive(Clone, Eq, PartialEq)]
pub struct Cookie {
    name: String,
    value: String,
    path: String,
    secure: bool,
    http_only: bool,
    same_site: SameSite,
    max_age: Option<u32>,
}
impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookie")
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("max_age", &self.max_age)
            .finish_non_exhaustive()
    }
}
impl Cookie {
    /// Constructs a host-only Secure cookie. HTTP transport is not upgraded to HTTPS.
    ///
    /// ```
    /// use ruvane::{Cookie, Response, SameSite};
    /// let cookie = Cookie::new("preference", "dark")?.same_site(SameSite::Strict)?.max_age(3600)?;
    /// let response = Response::text("saved").with_cookie(cookie)?;
    /// assert_eq!(response.cookies().len(), 1);
    /// assert_eq!(response.cache_control(), "no-store");
    /// # Ok::<(), ruvane::CookieError>(())
    /// ```
    pub fn new(name: &str, value: &str) -> Result<Self, CookieError> {
        Self::create(name, value, true)
    }
    /// Explicitly omits Secure for local loopback HTTP. Prefix and SameSite checks still apply.
    /// Ruvane's server remains loopback-only; this constructor adds no HTTPS or proxy support.
    pub fn local_http(name: &str, value: &str) -> Result<Self, CookieError> {
        Self::create(name, value, false)
    }
    fn create(key: &str, content: &str, secure: bool) -> Result<Self, CookieError> {
        if !name(key) {
            return Err(CookieError::InvalidName);
        }
        if !value(content) {
            return Err(CookieError::InvalidValue);
        }
        let cookie = Self {
            name: key.into(),
            value: content.into(),
            path: "/".into(),
            secure,
            http_only: true,
            same_site: SameSite::Lax,
            max_age: None,
        };
        cookie.validate()?;
        Ok(cookie)
    }
    /// Sets a bounded internal path. Query strings and fragments are not supported.
    pub fn path(mut self, path: &str) -> Result<Self, CookieError> {
        if path.len() > MAX_COOKIE_PATH_BYTES
            || path
                .bytes()
                .any(|byte| matches!(byte, b';' | b',' | b'"' | b'\\'))
            || InternalUrl::new(path).is_err()
        {
            return Err(CookieError::InvalidPath);
        }
        self.path = path.into();
        self.validate()?;
        Ok(self)
    }
    /// Sets JavaScript visibility. HttpOnly is enabled by default.
    pub fn http_only(mut self, enabled: bool) -> Result<Self, CookieError> {
        self.http_only = enabled;
        self.validate()?;
        Ok(self)
    }
    /// Sets cross-site policy. None is rejected for a local HTTP cookie.
    pub fn same_site(mut self, policy: SameSite) -> Result<Self, CookieError> {
        self.same_site = policy;
        self.validate()?;
        Ok(self)
    }
    /// Sets lifetime in seconds, bounded to MAX_COOKIE_AGE_SECONDS. Zero removes the cookie.
    pub fn max_age(mut self, seconds: u32) -> Result<Self, CookieError> {
        if seconds > MAX_COOKIE_AGE_SECONDS {
            return Err(CookieError::InvalidAttributes);
        }
        self.max_age = Some(seconds);
        self.validate()?;
        Ok(self)
    }
    /// Clears the value and requests removal with Max-Age=0, preserving scope and flags.
    ///
    /// ```
    /// let cookie = ruvane::Cookie::local_http("preference", "dark")?.path("/settings")?;
    /// let removal = cookie.removal();
    /// assert_eq!(removal.cookie_path(), "/settings");
    /// assert_eq!(removal.age_seconds(), Some(0));
    /// # Ok::<(), ruvane::CookieError>(())
    /// ```
    pub fn removal(&self) -> Self {
        let mut result = self.clone();
        result.value.clear();
        result.max_age = Some(0);
        result
    }
    /// Cookie name, preserved as supplied.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Cookie value. Treat this as potentially sensitive untrusted application data.
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Exact path used for browser scope and removal.
    pub fn cookie_path(&self) -> &str {
        &self.path
    }
    /// Whether Secure is emitted.
    pub fn is_secure(&self) -> bool {
        self.secure
    }
    /// Whether HttpOnly is emitted.
    pub fn is_http_only(&self) -> bool {
        self.http_only
    }
    /// Declared cross-site policy.
    pub fn same_site_policy(&self) -> SameSite {
        self.same_site
    }
    /// Declared lifetime, or None for a browser session cookie.
    pub fn age_seconds(&self) -> Option<u32> {
        self.max_age
    }
    fn validate(&self) -> Result<(), CookieError> {
        let host = self.name.starts_with("__Host-");
        let http = self.name.starts_with("__Http-") || self.name.starts_with("__Host-Http-");
        if ((!self.secure)
            && (host
                || http
                || self.name.starts_with("__Secure-")
                || self.same_site == SameSite::None))
            || (host && self.path != "/")
            || (http && !self.http_only)
        {
            return Err(CookieError::InvalidAttributes);
        }
        if self.wire_line().len() > MAX_SET_COOKIE_BYTES {
            return Err(CookieError::TooLarge);
        }
        Ok(())
    }
    pub(crate) fn wire_line(&self) -> String {
        let mut line = format!(
            "Set-Cookie: {}={}; Path={}; SameSite={}",
            self.name,
            self.value,
            self.path,
            self.same_site.as_str()
        );
        if self.secure {
            line.push_str("; Secure");
        }
        if self.http_only {
            line.push_str("; HttpOnly");
        }
        if let Some(seconds) = self.max_age {
            line.push_str(&format!("; Max-Age={seconds}"));
        }
        line.push_str("\r\n");
        line
    }
}

#[cfg(test)]
mod tests;
