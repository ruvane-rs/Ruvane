use crate::html::escape_into;
use crate::{InternalUrl, MAX_INTERNAL_URL_BYTES};
mod wire;
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Maximum input bytes accepted by the restricted site-origin parser.
pub const MAX_SITE_ORIGIN_BYTES: usize = 320;
/// Maximum UTF-8 bytes in a page description.
pub const MAX_METADATA_TEXT_BYTES: usize = 1024;

/// An invalid site origin, without reflecting configuration text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OriginError;

impl fmt::Display for OriginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid site origin")
    }
}

impl std::error::Error for OriginError {}

/// A configured HTTP(S) origin used to construct canonical URLs.
///
/// This is a restricted parser, not a general browser URL implementation.
/// It accepts lowercase `http://` or `https://`, ASCII DNS names, canonical
/// dotted IPv4, or bracketed IPv6, and an optional nonzero decimal port.
/// One trailing `/` is allowed. Credentials, other paths, queries, fragments,
/// escapes, zone IDs, trailing DNS dots, non-ASCII and `xn--` IDNA hosts are rejected.
/// DNS labels are at most 63 bytes; the last label must start with a letter
/// to exclude browser numeric-host aliases. No DNS lookup or TLS is performed.
///
/// Configure this value from trusted application settings, never request
/// headers. It declares a public URL; it does not change the local listener.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SiteOrigin {
    serialized: String,
}

impl SiteOrigin {
    /// Validates and normalizes an origin, lowercasing DNS names, serializing
    /// IP literals, and omitting default HTTP/HTTPS ports.
    ///
    /// ```
    /// use ruvane::{InternalUrl, SiteOrigin};
    /// let origin = SiteOrigin::parse("https://EXAMPLE.COM:443/").unwrap();
    /// let url = origin.canonical(&InternalUrl::new("/articles").unwrap());
    /// assert_eq!(url.as_str(), "https://example.com/articles");
    /// ```
    pub fn parse(value: &str) -> Result<Self, OriginError> {
        if value.len() > MAX_SITE_ORIGIN_BYTES || !value.is_ascii() {
            return Err(OriginError);
        }
        let (scheme, authority, default_port) = if let Some(rest) = value.strip_prefix("https://") {
            ("https", rest, 443)
        } else if let Some(rest) = value.strip_prefix("http://") {
            ("http", rest, 80)
        } else {
            return Err(OriginError);
        };
        let authority = authority.strip_suffix('/').unwrap_or(authority);
        if authority.is_empty()
            || authority
                .bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && !b".-:[]".contains(&byte))
        {
            return Err(OriginError);
        }
        let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
            let (host, suffix) = rest.split_once(']').ok_or(OriginError)?;
            let address: Ipv6Addr = host.parse().map_err(|_| OriginError)?;
            let port = if suffix.is_empty() {
                None
            } else {
                Some(suffix.strip_prefix(':').ok_or(OriginError)?)
            };
            (format!("[{address}]"), port)
        } else {
            let (host, port) = authority
                .split_once(':')
                .map_or((authority, None), |(host, port)| (host, Some(port)));
            let host = if let Ok(address) = host.parse::<Ipv4Addr>() {
                if address.to_string() != host {
                    return Err(OriginError);
                }
                address.to_string()
            } else {
                valid_dns(host)?;
                host.to_ascii_lowercase()
            };
            (host, port)
        };
        let mut serialized = format!("{scheme}://{host}");
        if let Some(port) = port {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(OriginError);
            }
            let port: u16 = port.parse().map_err(|_| OriginError)?;
            if port == 0 {
                return Err(OriginError);
            }
            if port != default_port {
                serialized.push(':');
                serialized.push_str(&port.to_string());
            }
        }
        Ok(Self { serialized })
    }

    /// Returns the normalized origin, without a trailing slash.
    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// Combines this configured origin with a validated internal path/query.
    ///
    /// No request headers, DNS lookup, network request, or additional decoding
    /// is involved. The result is bounded by `MAX_SITE_ORIGIN_BYTES` plus
    /// `MAX_INTERNAL_URL_BYTES`. Route existence remains application-defined.
    pub fn canonical(&self, path: &InternalUrl) -> CanonicalUrl {
        let mut serialized = String::with_capacity(self.serialized.len() + path.as_str().len());
        serialized.push_str(&self.serialized);
        serialized.push_str(path.as_str());
        debug_assert!(serialized.len() <= MAX_SITE_ORIGIN_BYTES + MAX_INTERNAL_URL_BYTES);
        CanonicalUrl { serialized }
    }
}

fn valid_dns(host: &str) -> Result<(), OriginError> {
    if host.is_empty() || host.len() > 253 {
        return Err(OriginError);
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label
                .get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("xn--"))
            || !label.as_bytes()[0].is_ascii_alphanumeric()
            || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(OriginError);
        }
    }
    if !host.rsplit('.').next().unwrap_or("").as_bytes()[0].is_ascii_alphabetic() {
        return Err(OriginError);
    }
    Ok(())
}

/// An absolute canonical URL constructed only by [`SiteOrigin::canonical`].
///
/// URL validation is separate from HTML attribute escaping. This type does
/// not claim that a resource exists or that search engines will select it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalUrl {
    serialized: String,
}

impl CanonicalUrl {
    /// Returns the encoded absolute URL, before HTML attribute escaping.
    pub fn as_str(&self) -> &str {
        &self.serialized
    }
}

impl fmt::Display for SiteOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Display for CanonicalUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A consistent indexing/following hint for cooperative search crawlers.
///
/// These hints do not restrict access to a page or authenticate a crawler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Robots {
    /// Allow indexing and following links.
    IndexFollow,
    /// Allow indexing, but request that links not be followed.
    IndexNoFollow,
    /// Request no indexing, while allowing links to be followed.
    NoIndexFollow,
    /// Request neither indexing nor link following.
    NoIndexNoFollow,
}

impl Robots {
    fn as_str(self) -> &'static str {
        match self {
            Self::IndexFollow => "index, follow",
            Self::IndexNoFollow => "index, nofollow",
            Self::NoIndexFollow => "noindex, follow",
            Self::NoIndexNoFollow => "noindex, nofollow",
        }
    }
}

/// A page-description construction error, without reflected input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataError {
    /// The description contains a control character.
    InvalidText,
    /// The description has no non-whitespace text.
    EmptyDescription,
    /// The description exceeds `MAX_METADATA_TEXT_BYTES` UTF-8 bytes.
    TooLarge,
}

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidText => "control character in page description",
            Self::EmptyDescription => "page description must not be empty",
            Self::TooLarge => "page description byte limit exceeded",
        })
    }
}

impl std::error::Error for MetadataError {}

/// Typed page metadata with one slot per supported field.
///
/// Repeated setters replace the previous value. Rendering order is fixed:
/// responsive viewport, description, robots, canonical URL, PNG favicon.
/// No raw metadata names, `http-equiv`, HTML, or event attributes are accepted.
/// Title and language remain properties of [`crate::Document`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct Metadata {
    description: Option<String>,
    robots: Option<Robots>,
    canonical: Option<CanonicalUrl>,
    favicon_png: Option<InternalUrl>,
}

impl Metadata {
    pub(crate) fn for_error(mut self) -> Self {
        self.canonical = None;
        self.robots = Some(match self.robots {
            Some(Robots::IndexNoFollow | Robots::NoIndexNoFollow) => Robots::NoIndexNoFollow,
            _ => Robots::NoIndexFollow,
        });
        self
    }

    /// Creates metadata containing only a fixed responsive viewport.
    ///
    /// The viewport permits zoom and has `width=device-width, initial-scale=1`.
    /// Description, robots hints, canonical URL, and favicon are opt-in.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a nonempty, control-free description within the UTF-8 byte budget.
    /// Text is HTML-escaped when the document is rendered.
    pub fn description(mut self, value: impl Into<String>) -> Result<Self, MetadataError> {
        let value = value.into();
        if value.len() > MAX_METADATA_TEXT_BYTES {
            return Err(MetadataError::TooLarge);
        }
        if value.chars().any(char::is_control) {
            return Err(MetadataError::InvalidText);
        }
        if value.trim().is_empty() {
            return Err(MetadataError::EmptyDescription);
        }
        self.description = Some(value);
        Ok(self)
    }

    /// Sets a typed indexing/following hint; this is not access control.
    pub fn robots(mut self, value: Robots) -> Self {
        self.robots = Some(value);
        self
    }

    /// Sets the canonical URL, built from a configured origin and internal URL.
    pub fn canonical(mut self, value: CanonicalUrl) -> Self {
        self.canonical = Some(value);
        self
    }

    /// Sets an internal PNG favicon URL. The type is declared, not inferred
    /// from a filename or verified by decoding the asset bytes.
    pub fn favicon_png(mut self, source: InternalUrl) -> Self {
        self.favicon_png = Some(source);
        self
    }

    /// Current description, before HTML escaping.
    pub fn description_text(&self) -> Option<&str> {
        self.description.as_deref()
    }
    /// Current typed robots policy.
    pub fn robots_policy(&self) -> Option<Robots> {
        self.robots
    }
    /// Current canonical URL.
    pub fn canonical_url(&self) -> Option<&CanonicalUrl> {
        self.canonical.as_ref()
    }
    /// Current internal PNG favicon URL.
    pub fn favicon_url(&self) -> Option<&InternalUrl> {
        self.favicon_png.as_ref()
    }

    pub(crate) fn render_into(self, markup: &mut String) {
        markup.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
        if let Some(description) = self.description {
            markup.push_str("<meta name=\"description\" content=\"");
            escape_into(markup, &description);
            markup.push_str("\">");
        }
        if let Some(robots) = self.robots {
            markup.push_str("<meta name=\"robots\" content=\"");
            markup.push_str(robots.as_str());
            markup.push_str("\">");
        }
        if let Some(canonical) = self.canonical {
            markup.push_str("<link rel=\"canonical\" href=\"");
            escape_into(markup, canonical.as_str());
            markup.push_str("\">");
        }
        if let Some(source) = self.favicon_png {
            markup.push_str("<link rel=\"icon\" type=\"image/png\" href=\"");
            escape_into(markup, source.as_str());
            markup.push_str("\">");
        }
    }
}
